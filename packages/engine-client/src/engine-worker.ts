/// <reference lib="webworker" />
// Runs the WASM engine. Started from the main thread.
import { expose, transfer } from 'comlink';
import initWasm, { Engine, version } from './pkg/cgs_wasm';
import type {
  BookSummary,
  DrawingBox,
  EngineApi,
  EngineTilePort,
  FileSource,
  LoadEvent,
} from './types';

/** How long one slice of reading holds up other messages. */
const SLICE_MS = 10;
/** How long reads must be quiet before the first edit's work starts, as it can't be divided. */
const QUIET_MS = 500;
/** How often reading reports its progress. */
const REPORT_MS = 80;

let engine: Promise<Engine> | undefined;
let memory: WebAssembly.Memory | undefined;
let generation = 0;
let lastRead = 0;

function ensureEngine() {
  engine ??= initWasm({ module_or_path: new URL('./pkg/cgs_wasm_bg.wasm', import.meta.url) }).then(
    (exports) => {
      memory = exports.memory;
      return new Engine();
    },
  );
  return engine;
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

function reader(source: FileSource): {
  read: (offset: number, length: number) => Uint8Array;
  size: number;
} {
  if (source.kind === 'blob') {
    const sync = new FileReaderSync();
    const { blob } = source;
    return {
      size: blob.size,
      read: (offset, length) =>
        new Uint8Array(sync.readAsArrayBuffer(blob.slice(offset, offset + length))),
    };
  }
  const { url, size } = source;
  return {
    size,
    read: (offset, length) => {
      const last = Math.min(offset + length, size) - 1;
      const request = new XMLHttpRequest();
      request.open('GET', url, false);
      request.responseType = 'arraybuffer';
      request.setRequestHeader('Range', 'bytes=' + offset + '-' + last);
      request.send();
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
  async open(source, onEvent) {
    const wasm = await ensureEngine();
    const mine = ++generation;
    const { read, size } = reader(source);
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
