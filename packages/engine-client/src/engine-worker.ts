/// <reference lib="webworker" />
// Runs the WASM engine. Started from the main thread. Messages come first, then calculation in
// slices of 8 ms: a slice returns to the event loop, so a command or a tile never waits for more
// than one of them.
import { expose, transfer } from 'comlink';
import initWasm, { blank_workbook, Engine, version, write_pdf } from './pkg/cgs_wasm';
import type {
  Applied,
  BookSummary,
  CalcStatus,
  CellArea,
  Command,
  ChartView,
  DrawingBox,
  PivotBox,
  PivotInfo,
  EngineApi,
  EngineTilePort,
  ErpBinding,
  ErpPageInfo,
  ErpRefreshSummary,
  FileSource,
  LoadEvent,
  CellFormat,
  ColFit,
  FormatLists,
  FoundCell,
  RowFit,
  Recoverable,
  SaveProgress,
  SaveReport,
} from './types';

/** How long one slice of reading holds up other messages. */
const SLICE_MS = 10;
/** How long one slice of calculation holds up other messages. */
const CALC_SLICE_MS = 8;
/** How long one slice of a save holds up tiles and reads. */
const SAVE_SLICE_MS = 8;
/** How often a save reports its progress. */
const SAVE_REPORT_MS = 100;
/** How long reads must be quiet before the first edit's work starts, as it can't be divided. */
const QUIET_MS = 500;
/** How often reading reports its progress. */
const REPORT_MS = 80;
/** How often calculation says which tiles to draw again. */
const CHANGES_MS = 32;
/** How much a working copy takes from the file at a time. */
const COPY_CHUNK = 8 * 1024 * 1024;
const WORKING_FOLDER = 'cgs-working';
const WORKBOOK_TYPE = 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet';
const JOURNAL_SUFFIX = '.journal';

let engine: Promise<Engine> | undefined;
let memory: WebAssembly.Memory | undefined;
let generation = 0;
let lastRead = 0;
/** What the open workbook reads from, unless a working copy has taken over. */
let source: FileSource | undefined;
let working: WorkingCopy | undefined;
/** The name the workbook goes by, for offering it back after a crash. */
let bookName = 'Workbook';
let onChanges: ((numbers: Uint32Array) => void) | undefined;
let onCalc: ((status: CalcStatus) => void) | undefined;
let calculating = false;
let viewport: { sheet: number; area: CellArea; fresh: boolean } | undefined;
let lastCalcStatus = '';
/** What a crash finds at the next launch. Left over from an earlier page, so not swept yet. */
let recoverables: Recoverable[] | undefined;
/** Cells a CSV brought, which a recovery has to put back before any edit. */
let loadedCsv: string | undefined;
/** The open workbook holds ERP data, which no working copy or journal may keep yet. */
let erpBound = false;
/**
 * A save being written in slices. Tiles and reads go on meanwhile, and everything that would
 * change the workbook waits for it (see `guarded`), so what is saved is what was there when it
 * began.
 */
let saving: Promise<unknown> | undefined;

/** Waits until no save is being written. */
async function afterSave() {
  while (saving) await saving.catch(() => undefined);
}

function ensureEngine() {
  engine ??= initWasm({ module_or_path: new URL('./pkg/cgs_wasm_bg.wasm', import.meta.url) }).then(
    (exports) => {
      memory = exports.memory;
      const wasm = new Engine();
      wasm.set_today(todaySerial());
      // Not awaited: a slow disk mustn't hold up the first file.
      void sweepWorkingCopies();
      return wasm;
    },
  );
  return engine;
}

/** Today's date as Excel counts it, in local time. */
function todaySerial() {
  const now = new Date();
  return (
    Math.floor(Date.UTC(now.getFullYear(), now.getMonth(), now.getDate()) / 86_400_000) + 25569
  );
}

/** The sync file API, which isn't in every lib.d.ts this builds against. */
interface SyncAccess {
  read(into: Uint8Array, options: { at: number }): number;
  write(from: Uint8Array, options: { at: number }): number;
  flush(): void;
  close(): void;
  getSize(): number;
}

interface SyncFileHandle {
  createSyncAccessHandle(): Promise<SyncAccess>;
}

/**
 * A private copy of the source in OPFS, held open so that nothing else can change it, with a journal
 * of the commands made since. After a crash the two give the edits back.
 */
interface WorkingCopy {
  folder: FileSystemDirectoryHandle;
  name: string;
  access: SyncAccess;
  size: number;
  journal: SyncAccess | undefined;
  journalAt: number;
  /** The workbook reads from a copy on the native side, and this one only serves recovery. */
  onlyForRecovery: boolean;
}

async function workingFolder() {
  const root = await navigator.storage.getDirectory();
  return root.getDirectoryHandle(WORKING_FOLDER, { create: true });
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();

function journalLines(access: SyncAccess) {
  const size = access.getSize();
  const bytes = new Uint8Array(size);
  access.read(bytes, { at: 0 });
  return decoder
    .decode(bytes)
    .split('\n')
    .filter((line) => line.length > 0);
}

function appendJournal(entry: unknown) {
  const copy = working;
  if (!copy?.journal) return;
  try {
    const bytes = encoder.encode(JSON.stringify(entry) + '\n');
    copy.journal.write(bytes, { at: copy.journalAt });
    copy.journal.flush();
    copy.journalAt += bytes.length;
  } catch (error) {
    // No room or no disk: editing carries on, without being able to come back from a crash.
    console.warn('The recovery journal stopped:', error);
    try {
      copy.journal.close();
    } catch {
      // Closed already.
    }
    copy.journal = undefined;
  }
}

/**
 * Finds what closed or crashed pages left. A journal with edits in it is kept and offered back.
 * Anything else goes. One that another tab still holds is locked, which is how it stays.
 */
async function sweepWorkingCopies() {
  const found: Recoverable[] = [];
  try {
    const folder = await workingFolder();
    const names: string[] = [];
    const entries = (folder as unknown as { keys(): AsyncIterable<string> }).keys();
    for await (const name of entries) names.push(name);
    for (const name of names) {
      if (name === working?.name || name === working?.name + JOURNAL_SUFFIX) continue;
      if (name.endsWith(JOURNAL_SUFFIX)) continue;
      const journalName = name + JOURNAL_SUFFIX;
      let offered = false;
      if (names.includes(journalName)) {
        offered = await readRecoverable(folder, name, journalName, found);
      }
      if (!offered) {
        await folder.removeEntry(name).catch(() => undefined);
        await folder.removeEntry(journalName).catch(() => undefined);
      }
    }
  } catch {
    // No OPFS here, so nothing can have been left in it.
  }
  recoverables = found;
}

async function readRecoverable(
  folder: FileSystemDirectoryHandle,
  name: string,
  journalName: string,
  found: Recoverable[],
) {
  let access: SyncAccess | undefined;
  try {
    const handle = await folder.getFileHandle(journalName);
    // Locked means another tab has it open, and it isn't ours to offer.
    access = await (handle as unknown as SyncFileHandle).createSyncAccessHandle();
    const lines = journalLines(access);
    const edits = lines.length - 1;
    if (edits <= 0) return false;
    const head = JSON.parse(lines[0]!) as { name: string };
    const file = await handle.getFile();
    found.push({ id: name, name: head.name, edits, time: file.lastModified });
    return true;
  } catch {
    return false;
  } finally {
    access?.close();
  }
}

function releaseWorkingCopy() {
  const copy = working;
  working = undefined;
  if (!copy) return;
  for (const access of [copy.access, copy.journal]) {
    try {
      access?.close();
    } catch {
      // Closed already, so only the entry is left.
    }
  }
  void copy.folder.removeEntry(copy.name).catch(() => undefined);
  void copy.folder.removeEntry(copy.name + JOURNAL_SUFFIX).catch(() => undefined);
}

async function openJournal(copy: WorkingCopy, existing: boolean) {
  try {
    const handle = await copy.folder.getFileHandle(copy.name + JOURNAL_SUFFIX, {
      create: !existing,
    });
    copy.journal = await (handle as unknown as SyncFileHandle).createSyncAccessHandle();
    copy.journalAt = copy.journal.getSize();
    if (!existing) {
      const head = encoder.encode(JSON.stringify({ name: bookName, today: todaySerial() }) + '\n');
      copy.journal.write(head, { at: 0 });
      copy.journal.flush();
      copy.journalAt = head.length;
    }
  } catch {
    copy.journal = undefined;
  }
}

/**
 * Copies a file into OPFS. Reading a `File` whose file has changed since it was opened throws, so
 * this is also what finds that out, before anyone has edits to lose.
 */
async function copyToWorking(from: FileSource): Promise<WorkingCopy | undefined> {
  let folder: FileSystemDirectoryHandle;
  let handle: FileSystemFileHandle;
  let access: SyncAccess;
  const name = crypto.randomUUID() + '.xlsx';
  try {
    folder = await workingFolder();
    handle = await folder.getFileHandle(name, { create: true });
    access = await (handle as unknown as SyncFileHandle).createSyncAccessHandle();
  } catch {
    return undefined;
  }
  const { read, size } = reader(from);
  try {
    for (let at = 0; at < size; at += COPY_CHUNK) {
      const bytes = read(at, COPY_CHUNK);
      if (access.write(bytes, { at }) !== bytes.length) throw new Error('short');
      await nextTask();
    }
    access.flush();
    return {
      folder,
      name,
      access,
      size,
      journal: undefined,
      journalAt: 0,
      onlyForRecovery: false,
    };
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

const TOO_LARGE = 'This workbook is too large to open here: it ran out of memory.';
const RAN_OUT =
  'The engine ran out of memory: a workbook here can use up to 4 GB. A new engine is starting, ' +
  'and your edits since the last save can be recovered.';

/** The open workbook has finished opening, so running out of memory is no longer about opening. */
let opened = false;

/**
 * A trap means WASM's memory ran out or a bug fired, and this module can't be used again.
 * Anything else that was asked of it gets the same plain answer, and the shell starts a new one.
 * Both answers say "ran out of memory", which is what the shell looks for.
 */
let trapped = false;
let OUT_OF_MEMORY = TOO_LARGE;
function friendly(error: unknown) {
  // After a trap, wasm-bindgen still holds the borrow of the call that trapped, so every later
  // call says this instead. Seeing it means a trap went by somewhere nothing caught it.
  const broken = error instanceof Error && /recursive use of an object/.test(error.message);
  if (
    !trapped &&
    (error instanceof WebAssembly.RuntimeError || error instanceof RangeError || broken)
  ) {
    trapped = true;
    OUT_OF_MEMORY = opened ? RAN_OUT : TOO_LARGE;
  }
  return trapped ? new Error(OUT_OF_MEMORY) : error;
}

function reportCalc(wasm: Engine) {
  const status: CalcStatus = {
    calculating: calculating || wasm.is_calculating(),
    manual: wasm.is_manual(),
  };
  const text = JSON.stringify(status);
  if (text !== lastCalcStatus) {
    lastCalcStatus = text;
    onCalc?.(status);
  }
}

function flushChanges(wasm: Engine) {
  const numbers = wasm.take_dirty();
  // Sheets, layouts, the book and tiles are each counted, so four zeros say nothing changed.
  if (numbers.length === 4 && numbers.every((number) => number === 0)) return;
  onChanges?.(transfer(numbers, [numbers.buffer as ArrayBuffer]));
}

/** Calculates in slices until nothing is due, and says what changed as it goes. */
async function runCalculation(wasm: Engine) {
  if (calculating) return;
  calculating = true;
  const mine = generation;
  let flushed = performance.now();
  try {
    for (;;) {
      await nextTask();
      if (mine !== generation) return;
      const focus = viewport?.fresh ? viewport : undefined;
      if (focus) focus.fresh = false;
      const more = focus
        ? wasm.calc_step(
            CALC_SLICE_MS,
            focus.sheet,
            focus.area.top,
            focus.area.left,
            focus.area.bottom,
            focus.area.right,
          )
        : wasm.calc_step(CALC_SLICE_MS, -1, 0, 0, 0, 0);
      const now = performance.now();
      if (!more || now - flushed >= CHANGES_MS) {
        flushChanges(wasm);
        flushed = now;
      }
      if (!more) break;
    }
  } finally {
    calculating = false;
    // A module that trapped can't be asked anything.
    if (mine === generation && !trapped) reportCalc(wasm);
  }
}

/** After anything that can leave calculation to do. */
function afterChange(wasm: Engine) {
  flushChanges(wasm);
  if (wasm.is_calculating() && !wasm.is_manual()) {
    void runCalculation(wasm).catch((error) => {
      const answer = friendly(error);
      // Running out of memory in a slice leaves the engine unusable: the shell has to know now,
      // not at the next command.
      if (trapped) onCalc?.({ calculating: false, manual: false, failure: message(answer) });
      else console.error('Calculation stopped:', error);
    });
  }
  reportCalc(wasm);
}

/** Brings calculation to an end, in slices, before a save. */
async function finishCalculation(wasm: Engine) {
  while (calculating) await nextTask();
  while (wasm.is_calculating()) {
    wasm.calculate_now(false);
    if (!wasm.calc_step(CALC_SLICE_MS, -1, 0, 0, 0, 0)) break;
    flushChanges(wasm);
    await nextTask();
  }
}

async function pump(
  wasm: Engine,
  mine: number,
  onEvent: (event: LoadEvent) => void,
  afterReading?: () => void,
) {
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
    afterReading?.();
    onEvent({ type: 'loaded' });
    for (;;) {
      await new Promise((resolve) => setTimeout(resolve, 100));
      if (mine !== generation) return;
      // The first edit's work can't be divided, so it waits for reads to go quiet and for a save.
      if (saving || performance.now() - lastRead < QUIET_MS) continue;
      if (!wasm.prepare_step()) break;
    }
    if (mine === generation) {
      opened = true;
      onEvent({ type: 'prepared' });
      // What a saved value doesn't cover: volatile functions, or a file that asks for it.
      afterChange(wasm);
    }
  } catch (error) {
    if (mine !== generation) return;
    onEvent({ type: 'error', message: message(friendly(error)) });
  }
}

/**
 * Writes the workbook to a blob a slice at a time, then reads it back to check it, also in
 * slices. Between slices the event loop serves tiles and reads.
 */
async function writeSave(
  wasm: Engine,
  onProgress?: (progress: SaveProgress) => void,
): Promise<{ blob: Blob; report: SaveReport }> {
  let reported = 0;
  const progress = (stage: SaveProgress['stage'], force = false) => {
    const now = performance.now();
    if (!onProgress || (!force && now - reported < SAVE_REPORT_MS)) return;
    reported = now;
    const [done, total] = wasm.save_progress();
    onProgress({ stage, fraction: total > 0 ? Math.min(1, done / total) : 0 });
  };
  // Blobs live outside WASM's memory, and the browser can move large ones to its disk.
  const parts: Blob[] = [];
  try {
    await finishCalculation(wasm);
    wasm.save_begin();
    progress('writing', true);
    let report: SaveReport | undefined;
    while (!report) {
      const done = wasm.save_step(SAVE_SLICE_MS, (chunk: Uint8Array) =>
        parts.push(new Blob([chunk as BlobPart])),
      );
      if (done) report = JSON.parse(done) as SaveReport;
      else {
        progress('writing');
        await nextTask();
      }
    }
    const blob = new Blob(parts, { type: WORKBOOK_TYPE });
    const { read, size } = reader({ kind: 'blob', blob });
    wasm.verify_begin(read, size);
    progress('checking', true);
    while (!wasm.verify_step(SAVE_SLICE_MS)) {
      progress('checking');
      await nextTask();
    }
    return { blob, report };
  } catch (error) {
    // A module that trapped can't be asked anything.
    if (!trapped) {
      try {
        wasm.save_cancel();
      } catch {
        // It is broken, and `friendly` below says so.
      }
    }
    throw friendly(error);
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
    expose(guarded(tilePort), port);
  },
  viewBlock: (...args) => tilePort.viewBlock(...args),
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
  async open(next, onEvent, name) {
    loadedCsv = undefined;
    erpBound = false;
    opened = false;
    const wasm = await ensureEngine();
    const mine = ++generation;
    const { read, size } = reader(next);
    releaseWorkingCopy();
    source = next;
    if (name) bookName = name;
    lastCalcStatus = '';
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
  async blankWorkbook(rightToLeft = false) {
    await ensureEngine();
    const bytes = blank_workbook(rightToLeft);
    return transfer(bytes, [bytes.buffer as ArrayBuffer]);
  },
  async writePdf(document, blob) {
    await ensureEngine();
    const bytes = write_pdf(document, blob);
    return transfer(bytes, [bytes.buffer as ArrayBuffer]);
  },
  async protectSource(given) {
    const wasm = await ensureEngine();
    if (erpBound) return false;
    if (given) {
      const { read, size } = reader(given);
      wasm.use_source(read, size);
      source = given;
      releaseWorkingCopy();
      // The native side holds what the workbook reads from. This copy is for after a crash.
      const copy = await copyToWorking(given).catch(() => undefined);
      if (copy) {
        copy.onlyForRecovery = true;
        working = copy;
        await openJournal(copy, false);
      }
      return true;
    }
    if (working) return true;
    if (source?.kind !== 'blob') return false;
    const copy = await copyToWorking(source);
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
    await openJournal(copy, false);
    if (loadedCsv !== undefined) appendJournal({ csv: loadedCsv });
    return true;
  },
  async command(command: Command) {
    const wasm = await ensureEngine();
    wasm.set_today(todaySerial());
    const json = JSON.stringify(command);
    let applied: Applied;
    try {
      applied = JSON.parse(wasm.apply_command(json)) as Applied;
    } catch (error) {
      throw friendly(error);
    }
    appendJournal({ c: command });
    afterChange(wasm);
    return applied;
  },
  async takeRowFits() {
    return JSON.parse((await ensureEngine()).take_row_fits()) as RowFit[];
  },
  async openingRowFits(sheet) {
    return JSON.parse((await ensureEngine()).opening_row_fits(sheet)) as RowFit[];
  },
  async colFits(sheet, first, last, keep = 0) {
    return JSON.parse((await ensureEngine()).col_fits(sheet, first, last, keep)) as ColFit[];
  },
  async mergeDiscards(sheet, areas, across) {
    return (await ensureEngine()).merge_discards(sheet, JSON.stringify(areas), across);
  },
  async cellFormat(sheet, row, col) {
    return JSON.parse((await ensureEngine()).cell_format(sheet, row, col)) as CellFormat | null;
  },
  async formatLists() {
    return JSON.parse((await ensureEngine()).format_lists()) as FormatLists | null;
  },
  async formatSample(sheet, row, col, code) {
    return (await ensureEngine()).format_sample(sheet, row, col, code) ?? null;
  },
  async findNext(query, row, col, back) {
    const wasm = await ensureEngine();
    return JSON.parse(wasm.find_next(JSON.stringify(query), row, col, back)) as FoundCell | null;
  },
  async findAll(query) {
    return JSON.parse((await ensureEngine()).find_all(JSON.stringify(query))) as FoundCell[];
  },
  async watchChanges(changes, calc) {
    onChanges = changes;
    onCalc = calc;
  },
  setViewport(sheet, area) {
    viewport = { sheet, area, fresh: true };
  },
  async setManualCalc(manual) {
    const wasm = await ensureEngine();
    wasm.set_manual(manual);
    appendJournal({ m: manual });
    afterChange(wasm);
  },
  async calculateNow(all) {
    const wasm = await ensureEngine();
    if (trapped) throw new Error(OUT_OF_MEMORY);
    try {
      wasm.calculate_now(all);
      // A manual workbook only calculates when asked, so this drives it itself.
      while (calculating) await nextTask();
      for (;;) {
        if (!wasm.calc_step(CALC_SLICE_MS, -1, 0, 0, 0, 0)) break;
        flushChanges(wasm);
        await nextTask();
        wasm.calculate_now(false);
      }
      flushChanges(wasm);
      reportCalc(wasm);
    } catch (error) {
      throw friendly(error);
    }
  },
  async calculateSheet(sheet) {
    const wasm = await ensureEngine();
    wasm.calculate_sheet(sheet);
    flushChanges(wasm);
    reportCalc(wasm);
  },
  async copyText(sheet, area) {
    const wasm = await ensureEngine();
    const [text, html] = wasm.copy_text(sheet, area.top, area.left, area.bottom, area.right);
    return { text: text ?? '', html: html ?? '' };
  },
  async loadCsv(text) {
    const wasm = await ensureEngine();
    wasm.load_csv(text);
    loadedCsv = text;
    afterChange(wasm);
  },
  async csvText(sheet) {
    return (await ensureEngine()).csv_text(sheet);
  },
  async jumpTarget(sheet, row, col, rows, cols) {
    const wasm = await ensureEngine();
    const [toRow, toCol] = wasm.jump_target(sheet, row, col, rows, cols);
    return [toRow ?? row, toCol ?? col];
  },
  async resolveName(text, sheet) {
    const found = (await ensureEngine()).resolve_name(text, sheet);
    if (found.length < 5) return null;
    return {
      sheet: found[0]!,
      top: found[1]!,
      left: found[2]!,
      bottom: found[3]!,
      right: found[4]!,
    };
  },
  async functionNames() {
    return (await ensureEngine()).function_names();
  },
  async query(question) {
    const wasm = await ensureEngine();
    try {
      // The answer's text, which the main thread parses: a large answer, such as ERP data's
      // changeset, then crosses as one string rather than as a structured clone of its objects.
      return wasm.query(JSON.stringify(question));
    } catch (error) {
      throw friendly(error);
    }
  },
  async definedNames() {
    return (await ensureEngine()).defined_names();
  },
  async bookSummary() {
    return JSON.parse((await ensureEngine()).book_json()) as BookSummary;
  },
  async recoverable() {
    await ensureEngine();
    while (!recoverables) await new Promise((resolve) => setTimeout(resolve, 20));
    return recoverables;
  },
  async openRecovered(id, onEvent) {
    opened = false;
    const wasm = await ensureEngine();
    const mine = ++generation;
    releaseWorkingCopy();
    const folder = await workingFolder();
    const handle = await folder.getFileHandle(id);
    const access = await (handle as unknown as SyncFileHandle).createSyncAccessHandle();
    const copy: WorkingCopy = {
      folder,
      name: id,
      access,
      size: access.getSize(),
      journal: undefined,
      journalAt: 0,
      onlyForRecovery: false,
    };
    working = copy;
    await openJournal(copy, true);
    const lines = copy.journal ? journalLines(copy.journal) : [];
    const head = JSON.parse(lines[0] ?? '{}') as { name?: string; today?: number };
    if (head.name) bookName = head.name;
    const { read, size } = readerOfWorking(copy);
    source = undefined;
    lastCalcStatus = '';
    try {
      const summary = JSON.parse(wasm.open(read, size)) as BookSummary;
      if (head.today) wasm.set_today(head.today);
      void pump(wasm, mine, onEvent, () => {
        // Commands run as they ran, and any one that fails now failed then too.
        for (const line of lines.slice(1)) {
          const entry = JSON.parse(line) as { c?: Command; m?: boolean; csv?: string };
          try {
            if (entry.csv !== undefined) wasm.load_csv(entry.csv);
            else if (entry.c) wasm.apply_command(JSON.stringify(entry.c));
            else if (entry.m !== undefined) wasm.set_manual(entry.m);
          } catch (error) {
            console.warn('A recovered edit could not be applied again:', error);
          }
        }
        wasm.set_today(todaySerial());
        afterChange(wasm);
        onChanges?.(new Uint32Array([0, 0, 1, 0]));
      });
      recoverables = (recoverables ?? []).filter((entry) => entry.id !== id);
      return summary;
    } catch (error) {
      throw friendly(error);
    }
  },
  async discardRecovered(id) {
    const folder = await workingFolder();
    await folder.removeEntry(id).catch(() => undefined);
    await folder.removeEntry(id + JOURNAL_SUFFIX).catch(() => undefined);
    recoverables = (recoverables ?? []).filter((entry) => entry.id !== id);
  },
  async hasChanges() {
    return (await ensureEngine()).has_changes();
  },
  async save(onProgress) {
    const wasm = await ensureEngine();
    const job = writeSave(wasm, onProgress);
    saving = job;
    try {
      return await job;
    } finally {
      if (saving === job) saving = undefined;
      // Calculation turned away during the save carries on.
      if (!trapped) afterChange(wasm);
    }
  },
  async commitSaved(saved) {
    const wasm = await ensureEngine();
    const { read, size } = reader(saved);
    wasm.commit_saved(read, size);
    source = saved;
    loadedCsv = undefined;
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
  async pivots(sheet) {
    return JSON.parse((await ensureEngine()).pivot_list(sheet)) as PivotBox[];
  },
  async pivotInfo(sheet, row, col) {
    const json = (await ensureEngine()).pivot_info(sheet, row, col);
    return json ? (JSON.parse(json) as PivotInfo) : null;
  },
  async chart(sheet, drawing, screen) {
    const json = (await ensureEngine()).chart_json(
      sheet,
      drawing,
      screen.dark,
      screen.background,
      screen.text,
    );
    return JSON.parse(json) as ChartView | null;
  },
  async addMedia(bytes) {
    return (await ensureEngine()).add_media(bytes) ?? null;
  },
  async media(part) {
    const bytes = (await ensureEngine()).media(part);
    return transfer(bytes, [bytes.buffer as ArrayBuffer]);
  },
  async cellText(sheet, row, col) {
    const wasm = await ensureEngine();
    return { display: wasm.display_text(sheet, row, col), edit: wasm.edit_text(sheet, row, col) };
  },
  async memoryBytes() {
    await ensureEngine();
    return memory?.buffer.byteLength ?? 0;
  },
  async erpBegin(schema) {
    (await ensureEngine()).erp_begin(schema);
  },
  async erpBeginRefresh() {
    (await ensureEngine()).erp_begin_refresh();
  },
  async erpPage(bytes) {
    return JSON.parse((await ensureEngine()).erp_page(bytes)) as ErpPageInfo;
  },
  async erpCancel() {
    (await ensureEngine()).erp_cancel();
  },
  async erpBind(session) {
    const wasm = await ensureEngine();
    const binding = JSON.parse(wasm.erp_bind(session)) as ErpBinding;
    // ERP data stays off the device until Phase 18's capped recovery draft: no working copy,
    // so no journal.
    erpBound = true;
    afterChange(wasm);
    return binding;
  },
  async erpRefresh() {
    const wasm = await ensureEngine();
    const summary = JSON.parse(wasm.erp_refresh()) as ErpRefreshSummary;
    afterChange(wasm);
    return summary;
  },
  async erpSettle() {
    (await ensureEngine()).erp_settle();
  },
  async binding() {
    return JSON.parse((await ensureEngine()).binding_json()) as ErpBinding | null;
  },
  async bindingIds() {
    return Array.from((await ensureEngine()).binding_ids());
  },
};

/**
 * Every call goes through the same door: once the module has trapped, nothing reaches it again,
 * and whatever a call throws is put in words a person can use. A trap can come in any call,
 * including a calculation slice or a tile, and wasm-bindgen then fails every later call.
 */
/** What changes the workbook, or replaces it, and so waits for a save being written. */
const WAITS_FOR_SAVE = new Set([
  'open',
  'openRecovered',
  'close',
  'protectSource',
  'command',
  'mergeDiscards',
  'setManualCalc',
  'calculateNow',
  'calculateSheet',
  'loadCsv',
  'addMedia',
  'erpBind',
  'erpRefresh',
  'save',
  'commitSaved',
]);

function guarded<T extends object>(target: T): T {
  const wrapped: Record<string, unknown> = {};
  for (const [name, value] of Object.entries(target)) {
    if (typeof value !== 'function') {
      wrapped[name] = value;
      continue;
    }
    const waits = WAITS_FOR_SAVE.has(name);
    wrapped[name] = async (...args: unknown[]) => {
      if (waits) await afterSave();
      if (trapped) throw new Error(OUT_OF_MEMORY);
      try {
        return await (value as (...args: unknown[]) => unknown).apply(target, args);
      } catch (error) {
        throw friendly(error);
      }
    };
  }
  return wrapped as T;
}

expose(guarded(api));
