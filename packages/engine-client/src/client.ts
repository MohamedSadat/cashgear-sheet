import { proxy, releaseProxy, transfer, wrap, type Remote } from 'comlink';
import type {
  Answers,
  Applied,
  BookSummary,
  CalcStatus,
  CellArea,
  CellFormat,
  CellText,
  Command,
  ChartScreen,
  ChartView,
  DrawingBox,
  PivotBox,
  PivotInfo,
  EngineApi,
  EngineInfo,
  ErpBinding,
  ErpPageInfo,
  ErpRefreshSummary,
  FileSource,
  FindQuery,
  FitRoom,
  FormatLists,
  FoundCell,
  LoadEvent,
  LocaleData,
  Question,
  Recoverable,
  SavedFile,
  SaveProgress,
  TileImage,
  TilePalette,
  TileRequest,
  TileWorkerApi,
  WorkbookChanges,
} from './types';
import { decodeLayout, type SheetLayout } from './layout';
import { decodeViewBlock, type ViewBlock } from './view-block';

export interface EngineClient {
  /** Starts the engine and connects the tile worker to it. */
  init(): Promise<EngineInfo>;
  /** Proves this page and the engine share memory (needs cross-origin isolation). */
  checkSharedMemory(): Promise<boolean>;
  setLocale(locale: LocaleData): Promise<void>;
  /** Opens a workbook. Its sheets become readable through `onEvent`. */
  open(
    source: FileSource,
    onEvent: (event: LoadEvent) => void,
    name?: string,
  ): Promise<BookSummary>;
  close(): Promise<void>;
  /** The bytes of a new blank workbook. */
  /** A new workbook, whose sheet is right to left if the default direction says so. */
  blankWorkbook(rightToLeft?: boolean): Promise<Uint8Array>;
  /** A PDF of recorded pages. See `recordPages` and `cgs_pdf::Document`. */
  writePdf(document: string, blob: Uint8Array): Promise<Uint8Array>;
  /** A tile's cells in the file's own colours, for drawing pages. */
  viewBlock(sheet: number, firstRow: number, firstCol: number): Promise<ViewBlock>;
  /** Makes saves independent of the file on disk, before the first edit. See `EngineApi`. */
  protectSource(source?: FileSource): Promise<boolean>;
  /** Runs a command, and fits any rows it left to measure. Rejects with a message people can read. */
  command(command: Command): Promise<Applied>;
  /** Fits rows of a sheet from another program's file, for how it looks here. */
  fitOpeningRows(sheet: number): Promise<void>;
  /** Fits columns to what they hold. */
  autoFitColumns(
    sheet: number,
    first: number,
    last: number,
    room?: FitRoom,
  ): Promise<Applied | null>;
  mergeDiscards(sheet: number, areas: CellArea[], across: boolean): Promise<boolean>;
  cellFormat(sheet: number, row: number, col: number): Promise<CellFormat | null>;
  formatLists(): Promise<FormatLists | null>;
  formatSample(sheet: number, row: number, col: number, code: string): Promise<string | null>;
  findNext(query: FindQuery, row: number, col: number, back: boolean): Promise<FoundCell | null>;
  findAll(query: FindQuery): Promise<FoundCell[]>;
  /** Hears what changed and whether calculation is running, from now on. */
  watchChanges(
    onChanges: (changes: WorkbookChanges) => void,
    onCalc: (status: CalcStatus) => void,
  ): Promise<void>;
  /** Where the user is looking, which calculation gets to first. 1-based. */
  setViewport(sheet: number, area: CellArea): void;
  setManualCalc(manual: boolean): Promise<void>;
  calculateNow(all: boolean): Promise<void>;
  calculateSheet(sheet: number): Promise<void>;
  copyText(sheet: number, area: CellArea): Promise<{ text: string; html: string }>;
  loadCsv(text: string): Promise<void>;
  csvText(sheet: number): Promise<string>;
  jumpTarget(
    sheet: number,
    row: number,
    col: number,
    rows: number,
    cols: number,
  ): Promise<[number, number]>;
  resolveName(text: string, sheet: number): Promise<(CellArea & { sheet: number }) | null>;
  functionNames(): Promise<string[]>;
  definedNames(): Promise<string[]>;
  /** Asks one of the data tools' questions. */
  query<Q extends Question>(question: Q): Promise<Answers[Q['kind']]>;
  bookSummary(): Promise<BookSummary>;
  recoverable(): Promise<Recoverable[]>;
  openRecovered(id: string, onEvent: (event: LoadEvent) => void): Promise<BookSummary>;
  discardRecovered(id: string): Promise<void>;
  hasChanges(): Promise<boolean>;
  /** Writes the workbook to a checked blob, and leaves the file on disk alone. See `EngineApi`. */
  save(onProgress?: (progress: SaveProgress) => void): Promise<SavedFile>;
  /** The save now stands in `source`, which the workbook reads from from now on. */
  commitSaved(source: FileSource): Promise<void>;
  /** Reads this sheet next, if it isn't read yet. */
  prioritize(sheet: number): Promise<void>;
  layout(sheet: number): Promise<SheetLayout>;
  drawings(sheet: number): Promise<DrawingBox[]>;
  /** The pivot tables on a sheet. */
  pivots(sheet: number): Promise<PivotBox[]>;
  /** The pivot table at a cell, 1-based, or null. */
  pivotInfo(sheet: number, row: number, col: number): Promise<PivotInfo | null>;
  chart(sheet: number, drawing: number, screen: ChartScreen): Promise<ChartView | null>;
  media(part: string): Promise<Uint8Array>;
  addMedia(bytes: Uint8Array): Promise<string | null>;
  /** `row` and `col` are 1-based. */
  cellText(sheet: number, row: number, col: number): Promise<CellText>;
  memoryBytes(): Promise<number>;
  /** Starts staging ERP record pages for a new binding. See `EngineApi`. */
  erpBegin(schema: string): Promise<void>;
  erpBeginRefresh(): Promise<void>;
  /** Hands a Query page's bytes to the engine, transferring them. */
  erpPage(bytes: Uint8Array): Promise<ErpPageInfo>;
  erpCancel(): Promise<void>;
  erpBind(session: string): Promise<ErpBinding>;
  erpRefresh(): Promise<ErpRefreshSummary>;
  erpSettle(): Promise<void>;
  binding(): Promise<ErpBinding | null>;
  bindingIds(): Promise<number[]>;
  drawTile(request: TileRequest): Promise<TileImage>;
  dispose(): void;
}

/** Reads `dirty_numbers` in crates/cgs-engine/src/query.rs. */
export function decodeChanges(numbers: Uint32Array): WorkbookChanges {
  let at = 0;
  const list = () => {
    const count = numbers[at++] ?? 0;
    const out = Array.from(numbers.subarray(at, at + count));
    at += count;
    return out;
  };
  const sheets = list();
  const layouts = list();
  const book = numbers[at++] === 1;
  const count = numbers[at++] ?? 0;
  const tiles: WorkbookChanges['tiles'] = [];
  for (let index = 0; index < count; index++) {
    tiles.push({ sheet: numbers[at]!, firstRow: numbers[at + 1]!, firstCol: numbers[at + 2]! });
    at += 3;
  }
  return { sheets, layouts, book, tiles };
}

function watch(worker: Worker, name: string, onError: (message: string) => void) {
  worker.addEventListener('error', (event) => {
    event.preventDefault();
    onError(event.message || `The ${name} worker stopped unexpectedly.`);
  });
  worker.addEventListener('messageerror', () => onError(`Could not read a ${name} response.`));
}

/** What a call on an engine that has been disposed rejects with. The shell says nothing about it. */
export const ENGINE_STOPPED = 'The engine has stopped.';

/**
 * A remote whose calls also fail once `gone` rejects: a worker that is terminated mid-call never
 * answers, and a caller waiting on it would wait for ever.
 */
function untilGone<T>(remote: Remote<T>, gone: Promise<never>): Remote<T> {
  return new Proxy(remote as object, {
    get(target, key) {
      const value: unknown = Reflect.get(target, key);
      if (typeof key !== 'string' || key === 'then' || typeof value !== 'function') return value;
      return (...args: unknown[]) =>
        Promise.race([(value as (...args: unknown[]) => unknown)(...args), gone]);
    },
  }) as Remote<T>;
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
  let stop: (error: Error) => void = () => undefined;
  const gone = new Promise<never>((_, reject) => {
    stop = reject;
  });
  // Nobody may be waiting when it goes.
  gone.catch(() => undefined);
  const engineRemote = wrap<EngineApi>(engineWorker);
  const tilesRemote = wrap<TileWorkerApi>(tileWorker);
  const engine: Remote<EngineApi> = untilGone(engineRemote, gone);
  const tiles: Remote<TileWorkerApi> = untilGone(tilesRemote, gone);
  /** Text measures as tiles draw it, so with the font they last used. */
  let lastPalette: TilePalette | undefined;
  const measuringPalette = (): TilePalette =>
    lastPalette ?? {
      dark: false,
      background: '#ffffff',
      gridline: '#d4d4d4',
      text: '#000000',
      fontFamily: 'sans-serif',
    };
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
    open: (source, onEvent, name) => engine.open(source, proxy(onEvent), name),
    close: () => engine.close(),
    blankWorkbook: (rightToLeft) => engine.blankWorkbook(rightToLeft),
    writePdf: (document, blob) =>
      engine.writePdf(document, transfer(blob, [blob.buffer as ArrayBuffer])),
    async viewBlock(sheet, firstRow, firstCol) {
      return decodeViewBlock(await engine.viewBlock(sheet, firstRow, firstCol, false, 0xffffff, 0));
    },
    protectSource: (source) => engine.protectSource(source),
    async command(command) {
      const applied = await engine.command(command);
      const rows = await engine.takeRowFits();
      if (rows.length) {
        const fitted = await tiles.fitRows(rows, measuringPalette());
        await engine.command({ kind: 'fitRows', rows: fitted });
      }
      return applied;
    },
    async fitOpeningRows(sheet) {
      const rows = await engine.openingRowFits(sheet);
      if (!rows.length) return;
      await connected;
      const fitted = await tiles.fitRows(rows, measuringPalette());
      await engine.command({ kind: 'fitRows', rows: fitted, quiet: true });
    },
    async autoFitColumns(sheet, first, last, room) {
      const cells = await engine.colFits(sheet, first, last);
      if (!cells.length) return null;
      const cols = await tiles.fitColumns(cells, measuringPalette(), room);
      return engine.command({ kind: 'fitCols', sheet, cols });
    },
    mergeDiscards: (sheet, areas, across) => engine.mergeDiscards(sheet, areas, across),
    cellFormat: (sheet, row, col) => engine.cellFormat(sheet, row, col),
    formatLists: () => engine.formatLists(),
    formatSample: (sheet, row, col, code) => engine.formatSample(sheet, row, col, code),
    findNext: (query, row, col, back) => engine.findNext(query, row, col, back),
    findAll: (query) => engine.findAll(query),
    watchChanges: (onChanges, onCalc) =>
      engine.watchChanges(
        proxy((numbers: Uint32Array) => onChanges(decodeChanges(numbers))),
        proxy(onCalc),
      ),
    setViewport: (sheet, area) => void engine.setViewport(sheet, area),
    setManualCalc: (manual) => engine.setManualCalc(manual),
    calculateNow: (all) => engine.calculateNow(all),
    calculateSheet: (sheet) => engine.calculateSheet(sheet),
    copyText: (sheet, area) => engine.copyText(sheet, area),
    loadCsv: (text) => engine.loadCsv(text),
    csvText: (sheet) => engine.csvText(sheet),
    jumpTarget: (sheet, row, col, rows, cols) => engine.jumpTarget(sheet, row, col, rows, cols),
    resolveName: (text, sheet) => engine.resolveName(text, sheet),
    functionNames: () => engine.functionNames(),
    definedNames: () => engine.definedNames(),
    query: <Q extends Question>(question: Q) =>
      engine.query(question) as Promise<Answers[Q['kind']]>,
    bookSummary: () => engine.bookSummary(),
    recoverable: () => engine.recoverable(),
    openRecovered: (id, onEvent) => engine.openRecovered(id, proxy(onEvent)),
    discardRecovered: (id) => engine.discardRecovered(id),
    hasChanges: () => engine.hasChanges(),
    save: (onProgress) => engine.save(onProgress ? proxy(onProgress) : undefined),
    commitSaved: (source) => engine.commitSaved(source),
    prioritize: (sheet) => engine.prioritize(sheet),
    async layout(sheet) {
      return decodeLayout(await engine.layout(sheet));
    },
    drawings: (sheet) => engine.drawings(sheet),
    pivots: (sheet) => engine.pivots(sheet),
    pivotInfo: (sheet, row, col) => engine.pivotInfo(sheet, row, col),
    chart: (sheet, drawing, screen) => engine.chart(sheet, drawing, screen),
    media: (part) => engine.media(part),
    addMedia: (bytes) => engine.addMedia(transfer(bytes, [bytes.buffer as ArrayBuffer])),
    cellText: (sheet, row, col) => engine.cellText(sheet, row, col),
    memoryBytes: () => engine.memoryBytes(),
    erpBegin: (schema) => engine.erpBegin(schema),
    erpBeginRefresh: () => engine.erpBeginRefresh(),
    erpPage: (bytes) => engine.erpPage(transfer(bytes, [bytes.buffer as ArrayBuffer])),
    erpCancel: () => engine.erpCancel(),
    erpBind: (session) => engine.erpBind(session),
    erpRefresh: () => engine.erpRefresh(),
    erpSettle: () => engine.erpSettle(),
    binding: () => engine.binding(),
    bindingIds: () => engine.bindingIds(),
    async drawTile(request) {
      lastPalette = request.palette;
      await connected;
      return tiles.draw(request);
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      stop(new Error(ENGINE_STOPPED));
      engineRemote[releaseProxy]();
      tilesRemote[releaseProxy]();
      engineWorker.terminate();
      tileWorker.terminate();
    },
  };
}
