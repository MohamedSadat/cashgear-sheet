import { proxy, releaseProxy, transfer, wrap, type Remote } from 'comlink';
import type {
  BookSummary,
  CellText,
  DrawingBox,
  EngineApi,
  EngineInfo,
  FileSource,
  LoadEvent,
  LocaleData,
  TileImage,
  TileRequest,
  TileWorkerApi,
} from './types';
import { decodeLayout, type SheetLayout } from './layout';

export interface EngineClient {
  /** Starts the engine and connects the tile worker to it. */
  init(): Promise<EngineInfo>;
  /** Proves this page and the engine share memory (needs cross-origin isolation). */
  checkSharedMemory(): Promise<boolean>;
  setLocale(locale: LocaleData): Promise<void>;
  /** Opens a workbook. Its sheets become readable through `onEvent`. */
  open(source: FileSource, onEvent: (event: LoadEvent) => void): Promise<BookSummary>;
  close(): Promise<void>;
  /** Reads this sheet next, if it isn't read yet. */
  prioritize(sheet: number): Promise<void>;
  layout(sheet: number): Promise<SheetLayout>;
  drawings(sheet: number): Promise<DrawingBox[]>;
  /** `row` and `col` are 1-based. */
  cellText(sheet: number, row: number, col: number): Promise<CellText>;
  memoryBytes(): Promise<number>;
  drawTile(request: TileRequest): Promise<TileImage>;
  dispose(): void;
}

function watch(worker: Worker, name: string, onError: (message: string) => void) {
  worker.addEventListener('error', (event) => {
    event.preventDefault();
    onError(event.message || `The ${name} worker stopped unexpectedly.`);
  });
  worker.addEventListener('messageerror', () => onError(`Could not read a ${name} response.`));
}

/**
 * Starts the engine and tile workers. Both start here on the main thread, because
 * WebView2 never loads a worker started by a worker (see CLAUDE.md), and they talk to
 * each other over a `MessageChannel`. `onError` reports worker crashes.
 */
export function createEngineClient(onError: (message: string) => void): EngineClient {
  let disposed = false;
  const report = (message: string) => {
    if (!disposed) onError(message);
  };
  const engineWorker = new Worker(new URL('./engine-worker.ts', import.meta.url), {
    type: 'module',
    name: 'cgs-engine',
  });
  const tileWorker = new Worker(new URL('./tile-worker.ts', import.meta.url), {
    type: 'module',
    name: 'cgs-tiles',
  });
  watch(engineWorker, 'engine', report);
  watch(tileWorker, 'tile', report);
  const engine: Remote<EngineApi> = wrap<EngineApi>(engineWorker);
  const tiles: Remote<TileWorkerApi> = wrap<TileWorkerApi>(tileWorker);
  const channel = new MessageChannel();
  const connected = Promise.all([
    engine.attachTileWorker(transfer(channel.port1, [channel.port1])),
    tiles.attachEngine(transfer(channel.port2, [channel.port2])),
  ]);
  return {
    async init() {
      await connected;
      return engine.init();
    },
    async checkSharedMemory() {
      if (typeof SharedArrayBuffer !== 'function' || !crossOriginIsolated) return false;
      const buffer = new SharedArrayBuffer(4);
      const value = 0x0c95;
      await engine.touchSharedMemory(buffer, value);
      return Atomics.load(new Int32Array(buffer), 0) === value;
    },
    setLocale: (locale) => engine.setLocale(locale),
    open: (source, onEvent) => engine.open(source, proxy(onEvent)),
    close: () => engine.close(),
    prioritize: (sheet) => engine.prioritize(sheet),
    async layout(sheet) {
      return decodeLayout(await engine.layout(sheet));
    },
    drawings: (sheet) => engine.drawings(sheet),
    cellText: (sheet, row, col) => engine.cellText(sheet, row, col),
    memoryBytes: () => engine.memoryBytes(),
    async drawTile(request) {
      await connected;
      return tiles.draw(request);
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      engine[releaseProxy]();
      tiles[releaseProxy]();
      engineWorker.terminate();
      tileWorker.terminate();
    },
  };
}
