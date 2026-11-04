/// <reference lib="webworker" />
// Runs the WASM engine. Started from the main thread.
import { expose, transfer } from 'comlink';
import initWasm, { blank_workbook, Engine, version } from './pkg/cgs_wasm';
import type {
  BookSummary,
  CellContent,
  DrawingBox,
  EngineApi,
  EngineTilePort,
  FileSource,
  LoadEvent,
  SaveReport,
} from './types';

/** How long one slice of reading holds up other messages. */
const SLICE_MS = 10;
/** How long reads must be quiet before the first edit's work starts, as it can't be divided. */
const QUIET_MS = 500;
/** How often reading reports its progress. */
const REPORT_MS = 80;
/** How much a working copy takes from the file at a time. */
const COPY_CHUNK = 8 * 1024 * 1024;
const WORKING_FOLDER = 'cgs-working';
const WORKBOOK_TYPE = 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet';

let engine: Promise<Engine> | undefined;
let memory: WebAssembly.Memory | undefined;
let generation = 0;
let lastRead = 0;
/** What the open workbook reads from, unless a working copy has taken over. */
let source: FileSource | undefined;
let working: WorkingCopy | undefined;

function ensureEngine() {
  engine ??= initWasm({ module_or_path: new URL('./pkg/cgs_wasm_bg.wasm', import.meta.url) }).then(
    (exports) => {
      memory = exports.memory;
      // Not awaited: a slow disk mustn't hold up the first file.
      void sweepWorkingCopies();
      return new Engine();
    },
  );
  return engine;
}

/** The sync file API, which isn't in every lib.d.ts this builds against. */
interface SyncAccess {
  read(into: Uint8Array, options: { at: number }): number;
  write(from: Uint8Array, options: { at: number }): number;
  flush(): void;
  close(): void;
}

/** A private copy of the source in OPFS, held open so that nothing else can change it. */
interface WorkingCopy {
  folder: FileSystemDirectoryHandle;
  name: string;
  access: SyncAccess;
  size: number;
}

async function workingFolder() {
  const root = await navigator.storage.getDirectory();
  return root.getDirectoryHandle(WORKING_FOLDER, { create: true });
}

/**
 * Deletes copies a closed or crashed page left. One that another tab still holds is locked, so
 * removing it fails, which is how it stays.
 */
async function sweepWorkingCopies() {
  try {
    const folder = await workingFolder();
    const entries = (folder as unknown as { keys(): AsyncIterable<string> }).keys();
    for await (const name of entries) {
      if (name === working?.name) continue;
      await folder.removeEntry(name).catch(() => undefined);
    }
  } catch {
    // No OPFS here, so nothing can have been left in it.
  }
}

function releaseWorkingCopy() {
  const copy = working;
  working = undefined;
  if (!copy) return;
  try {
    copy.access.close();
  } catch {
    // Closed already, so only the entry is left.
  }
  void copy.folder.removeEntry(copy.name).catch(() => undefined);
}

/**
 * Copies a file into OPFS. Reading a `File` whose file has changed since it was opened throws, so
 * this is also what finds that out, before anyone has edits to lose.
 */
async function copyToWorking(blob: Blob): Promise<WorkingCopy | undefined> {
  let folder: FileSystemDirectoryHandle;
  let handle: FileSystemFileHandle;
  let access: SyncAccess;
  const name = crypto.randomUUID() + '.xlsx';
  try {
    folder = await workingFolder();
    handle = await folder.getFileHandle(name, { create: true });
    access = await (
      handle as unknown as { createSyncAccessHandle(): Promise<SyncAccess> }
    ).createSyncAccessHandle();
  } catch {
    return undefined;
  }
  const sync = new FileReaderSync();
  try {
    for (let at = 0; at < blob.size; at += COPY_CHUNK) {
      const bytes = new Uint8Array(sync.readAsArrayBuffer(blob.slice(at, at + COPY_CHUNK)));
      if (access.write(bytes, { at }) !== bytes.length) throw new Error('short');
      await nextTask();
    }
    access.flush();
    return { folder, name, access, size: blob.size };
  } catch (error) {
    access.close();
    await folder.removeEntry(name).catch(() => undefined);
    if (error instanceof DOMException && error.name === 'NotReadableError') {
      throw new Error('This file has changed since it was opened. Open it again to edit it.', {
        cause: error,
      });
    }
    // The disk or the quota: saving still works while the file stays as it is.
    return undefined;
  }
}

function readerOfWorking(copy: WorkingCopy) {
  return {
    size: copy.size,
    read(offset: number, length: number) {
      const bytes = new Uint8Array(Math.min(length, Math.max(copy.size - offset, 0)));
      const count = copy.access.read(bytes, { at: offset });
      return count === bytes.length ? bytes : bytes.subarray(0, count);
    },
  };
}

/** Runs next on the task queue, after any messages already waiting. */
const channel = new MessageChannel();
const waiting: (() => void)[] = [];
channel.port1.onmessage = () => waiting.shift()?.();
function nextTask() {
  return new Promise<void>((resolve) => {
    waiting.push(resolve);
    channel.port2.postMessage(0);
  });
}

function reader(from: FileSource): {
  read: (offset: number, length: number) => Uint8Array;
  size: number;
} {
  if (from.kind === 'blob') {
    const sync = new FileReaderSync();
    const { blob } = from;
    return {
      size: blob.size,
      read: (offset, length) =>
        new Uint8Array(sync.readAsArrayBuffer(blob.slice(offset, offset + length))),
    };
  }
  const { url, size } = from;
  return {
    size,
    read: (offset, length) => {
      const last = Math.min(offset + length, size) - 1;
      const request = new XMLHttpRequest();
      request.open('GET', url, false);
      request.responseType = 'arraybuffer';
      request.setRequestHeader('Range', 'bytes=' + offset + '-' + last);
      request.send();
      if (request.status === 409) {
        throw new Error('This file has changed since it was opened.');
      }
      if (request.status !== 206 && request.status !== 200) {
        throw new Error('The file could not be read (' + request.status + ').');
      }
      return new Uint8Array(request.response as ArrayBuffer);
    },
  };
}

function message(error: unknown) {
  if (error instanceof Error) return error.message;
  return String(error);
}

const OUT_OF_MEMORY = 'This workbook is too large to open here: it ran out of memory.';

/**
 * A trap means WASM's memory ran out or a bug fired, and this module can't be used again.
 * Anything else that was asked of it gets the same plain answer, and the shell starts a new one.
 */
let trapped = false;
function friendly(error: unknown) {
  if (error instanceof WebAssembly.RuntimeError || error instanceof RangeError) trapped = true;
  return trapped ? new Error(OUT_OF_MEMORY) : error;
}

async function pump(wasm: Engine, mine: number, onEvent: (event: LoadEvent) => void) {
  let reported = 0;
  let ready: number[] = [];
  try {
    for (;;) {
      await nextTask();
      if (mine !== generation) return;
      const status = JSON.parse(wasm.load_step(SLICE_MS)) as {
        phase: 'reading' | 'preparing' | 'ready';
        done: number;
        total: number;
        ready: number[];
      };
      ready.push(...status.ready);
      const now = performance.now();
      if (ready.length || status.phase !== 'reading' || now - reported > REPORT_MS) {
        onEvent({ type: 'progress', done: status.done, total: status.total, ready });
        ready = [];
        reported = now;
      }
      if (status.phase !== 'reading') break;
    }
    onEvent({ type: 'loaded' });
    for (;;) {
      await new Promise((resolve) => setTimeout(resolve, 100));
      if (mine !== generation) return;
      if (performance.now() - lastRead < QUIET_MS) continue;
      if (!wasm.prepare_step()) break;
    }
    if (mine === generation) onEvent({ type: 'prepared' });
  } catch (error) {
    if (mine !== generation) return;
    onEvent({ type: 'error', message: message(friendly(error)) });
  }
}

const tilePort: EngineTilePort = {
  async viewBlock(sheet, firstRow, firstCol, dark, background, text) {
    lastRead = performance.now();
    const wasm = await ensureEngine();
    if (trapped) throw new Error(OUT_OF_MEMORY);
    try {
      const { buffer } = wasm.view_block(sheet, firstRow, firstCol, dark, background, text);
      return transfer(buffer as ArrayBuffer, [buffer as ArrayBuffer]);
    } catch (error) {
      throw friendly(error);
    }
  },
};

const api: EngineApi = {
  async init() {
    await ensureEngine();
    return { version: version() };
  },
  attachTileWorker(port) {
    expose(tilePort, port);
  },
  touchSharedMemory(buffer, value) {
    Atomics.store(new Int32Array(buffer), 0, value);
  },
  async setLocale(next) {
    const wasm = await ensureEngine();
    wasm.set_locale(
      next.decimal,
      next.group,
      next.shortDate,
      next.longDate,
      next.shortTime,
      next.longTime,
      next.am,
      next.pm,
      next.months,
      next.monthsShort,
      next.days,
      next.daysShort,
      next.currency,
      next.dateSeparator,
      next.timeSeparator,
    );
  },
  async open(next, onEvent) {
    const wasm = await ensureEngine();
    const mine = ++generation;
    const { read, size } = reader(next);
    releaseWorkingCopy();
    source = next;
    try {
      const summary = JSON.parse(wasm.open(read, size)) as BookSummary;
      void pump(wasm, mine, onEvent);
      return summary;
    } catch (error) {
      throw friendly(error);
    }
  },
  async close() {
    generation++;
    (await ensureEngine()).close();
    releaseWorkingCopy();
    source = undefined;
  },
  async blankWorkbook() {
    await ensureEngine();
    const bytes = blank_workbook();
    return transfer(bytes, [bytes.buffer as ArrayBuffer]);
  },
  async protectSource(given) {
    const wasm = await ensureEngine();
    if (given) {
      const { read, size } = reader(given);
      wasm.use_source(read, size);
      source = given;
      releaseWorkingCopy();
      return true;
    }
    if (working) return true;
    if (source?.kind !== 'blob') return false;
    const copy = await copyToWorking(source.blob);
    if (!copy) return false;
    const { read, size } = readerOfWorking(copy);
    try {
      wasm.use_source(read, size);
    } catch (error) {
      copy.access.close();
      await copy.folder.removeEntry(copy.name).catch(() => undefined);
      throw error;
    }
    working = copy;
    return true;
  },
  async editCell(sheet, row, col, content: CellContent) {
    const wasm = await ensureEngine();
    try {
      if (content.formula !== undefined) wasm.edit_formula(sheet, row, col, content.formula);
      else wasm.edit_value(sheet, row, col, content.number, content.text);
    } catch (error) {
      throw friendly(error);
    }
  },
  async hasChanges() {
    return (await ensureEngine()).has_changes();
  },
  async save() {
    const wasm = await ensureEngine();
    // Blobs live outside WASM's memory, and the browser can move large ones to its disk.
    const parts: Blob[] = [];
    try {
      const report = JSON.parse(
        wasm.save((chunk: Uint8Array) => parts.push(new Blob([chunk as BlobPart]))),
      ) as SaveReport;
      const blob = new Blob(parts, { type: WORKBOOK_TYPE });
      const { read, size } = reader({ kind: 'blob', blob });
      wasm.verify_saved(read, size);
      return { blob, report };
    } catch (error) {
      throw friendly(error);
    }
  },
  async commitSaved(saved) {
    const wasm = await ensureEngine();
    const { read, size } = reader(saved);
    wasm.commit_saved(read, size);
    source = saved;
    // Nothing reads the old source now. The next edit makes a new copy, of this file.
    releaseWorkingCopy();
  },
  async prioritize(sheet) {
    (await ensureEngine()).prioritize(sheet);
  },
  async layout(sheet) {
    const numbers = (await ensureEngine()).layout_numbers(sheet);
    return transfer(numbers, [numbers.buffer as ArrayBuffer]);
  },
  async drawings(sheet) {
    return JSON.parse((await ensureEngine()).drawings_json(sheet)) as DrawingBox[];
  },
  async cellText(sheet, row, col) {
    const wasm = await ensureEngine();
    return { display: wasm.display_text(sheet, row, col), edit: wasm.edit_text(sheet, row, col) };
  },
  async memoryBytes() {
    await ensureEngine();
    return memory?.buffer.byteLength ?? 0;
  },
};

expose(api);
