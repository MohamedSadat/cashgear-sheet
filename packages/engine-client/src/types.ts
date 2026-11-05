/**
 * The colours and font a tile is drawn with. Workers can't read CSS, so the main
 * thread builds this from the `--cg-*` tokens.
 */
export interface TilePalette {
  dark: boolean;
  /** `#rrggbb`. */
  background: string;
  gridline: string;
  /** `#rrggbb`. */
  text: string;
  /** A CSS font family list, for when a cell's own font is missing. */
  fontFamily: string;
  /** Draws every cell in this font instead, so that golden images don't depend on the machine. */
  fontOverride?: string;
  /** The sheet's own setting. */
  showGrid?: boolean;
}

export interface TileRequest {
  sheet: number;
  /** The tile's top-left cell, counted from 0. */
  firstRow: number;
  firstCol: number;
  /** Device pixels per CSS pixel at 100%, zoom included. */
  scale: number;
  palette: TilePalette;
}

export interface TileImage {
  bitmap: ImageBitmap;
  /** FNV-1a of the tile's bytes from the engine, for tests. */
  hash: number;
  /** False if the sheet isn't read yet, so the picture is only its outline. */
  ready: boolean;
}

export interface EngineInfo {
  version: string;
}

/** A file to open, which the engine worker reads a slice at a time. */
export type FileSource =
  | { kind: 'blob'; blob: Blob }
  /** The desktop's read protocol, which serves byte ranges. */
  | { kind: 'url'; url: string; size: number };

/** A change to the workbook. See `Command` in `crates/cgs-engine/src/command.rs`. */
export interface CellArea {
  /** 1-based. */
  top: number;
  left: number;
  bottom: number;
  right: number;
}

export type Command =
  | { kind: 'input'; sheet: number; row: number; col: number; text: string; areas?: CellArea[] }
  | { kind: 'arrayInput'; sheet: number; area: CellArea; text: string }
  | { kind: 'clear'; sheet: number; areas: CellArea[] }
  | { kind: 'fill'; sheet: number; from: CellArea; to: CellArea; copy?: boolean }
  | {
      kind: 'paste';
      fromSheet: number;
      from: CellArea;
      sheet: number;
      to: CellArea;
      mode?: 'all' | 'values' | 'formulas' | 'formats';
    }
  | {
      kind: 'move';
      fromSheet: number;
      from: CellArea;
      sheet: number;
      row: number;
      col: number;
    }
  | { kind: 'pasteExternal'; sheet: number; row: number; col: number; text: string; html?: string }
  | { kind: 'axis'; sheet: number; axis: 'row' | 'col'; at: number; count: number; delete: boolean }
  | { kind: 'insertSheet'; position: number; name?: string }
  | { kind: 'deleteSheet'; sheet: number }
  | { kind: 'renameSheet'; sheet: number; name: string }
  | { kind: 'undo' }
  | { kind: 'redo' };

/** What a command did. */
export interface Applied {
  selection: (CellArea & { sheet: number }) | null;
  undo: string | null;
  redo: string | null;
  /** The last change couldn't be kept for undo. */
  lost: boolean;
}

/** What changed for the UI, from `decodeChanges`. */
export interface WorkbookChanges {
  /** Sheets to draw again in full. */
  sheets: number[];
  /** Sheets whose sizes or merges changed. */
  layouts: number[];
  /** The list of sheets changed. */
  book: boolean;
  /** Tiles to draw again. */
  tiles: { sheet: number; firstRow: number; firstCol: number }[];
}

/** Whether calculation is running or due. */
export interface CalcStatus {
  calculating: boolean;
  manual: boolean;
}

/** Edits a crashed session left behind. */
export interface Recoverable {
  id: string;
  name: string;
  edits: number;
  /** When its last edit was made, in ms since 1970. */
  time: number;
}

/** What a save did. See `SaveReport` in `crates/cgs-xlsx/src/save.rs`. */
export interface SaveReport {
  keptParts: number;
  cellsWritten: number;
  stringsAdded: number;
  recalculatesOnOpen: boolean;
  pivotsRefreshOnOpen: boolean;
  resultsLeftToExcel: number;
  tableHeadersFixed: number;
  leftExtensions: string[];
  changedParts: string[];
  removedParts: string[];
  addedParts: string[];
  keptContent: { what: string; parts: number }[];
}

/** A checked save, ready to replace the file. */
export interface SavedFile {
  blob: Blob;
  report: SaveReport;
}

export interface SheetSummary {
  name: string;
  state: 'visible' | 'hidden' | 'veryHidden';
  kind: 'worksheet' | 'chart' | 'other';
  loaded: boolean;
  deleted: boolean;
}

export interface BookSummary {
  active: number;
  sheets: SheetSummary[];
  /** Sheets by index, in the order their tabs show. */
  order: number[];
}

export type LoadEvent =
  /** Bytes read so far, and sheets that became readable since the last event. */
  | { type: 'progress'; done: number; total: number; ready: number[] }
  /** All sheets are readable. */
  | { type: 'loaded' }
  /** The work the first edit would otherwise start with is done too. */
  | { type: 'prepared' }
  | { type: 'error'; message: string };

export interface DrawingBox {
  kind: 'chart' | 'picture' | 'shape' | 'group' | 'other';
  name: string;
  /** In CSS pixels at 100%. */
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Regional conventions, from `Intl`. See `crates/cgs-numfmt/src/locale.rs`. */
export interface LocaleData {
  decimal: string;
  group: string;
  shortDate: string;
  longDate: string;
  shortTime: string;
  longTime: string;
  am: string;
  pm: string;
  months: string[];
  monthsShort: string[];
  days: string[];
  daysShort: string[];
  currency: string;
  dateSeparator: string;
  timeSeparator: string;
}

export interface CellText {
  /** What the cell shows. */
  display: string;
  /** What the formula bar shows. */
  edit: string;
}

/** The engine worker, as the main thread sees it. */
export interface EngineApi {
  init(): Promise<EngineInfo>;
  /** Connects the tile worker, which then reads tiles from this port. */
  attachTileWorker(port: MessagePort): void;
  /** Writes `value` into `buffer`, to prove the page and the worker share memory. */
  touchSharedMemory(buffer: SharedArrayBuffer, value: number): void;
  setLocale(locale: LocaleData): void;
  /** Opens a workbook and starts reading it. Rejects with a message people can read. */
  open(
    source: FileSource,
    onEvent: (event: LoadEvent) => void,
    name?: string,
  ): Promise<BookSummary>;
  close(): void;
  /** The bytes of a new blank workbook. */
  blankWorkbook(): Promise<Uint8Array>;
  /**
   * Makes saves independent of the file on disk before the first edit. With a source, the
   * workbook reads from that copy. With none, the worker copies a browser file to OPFS itself.
   * Rejects if the file has changed since it was opened. Resolves false if there is nowhere to
   * copy it, as saving then still works unless the file changes.
   */
  protectSource(source?: FileSource): Promise<boolean>;
  /** Runs a command. Rejects with a message people can read, changing nothing. */
  command(command: Command): Promise<Applied>;
  /** Tells the main thread what changed and whether calculation is running, from now on. */
  watchChanges(
    onChanges: (numbers: Uint32Array) => void,
    onCalc: (status: CalcStatus) => void,
  ): void;
  /** Where the user is looking, which calculation gets to first. 1-based. */
  setViewport(sheet: number, area: CellArea): void;
  setManualCalc(manual: boolean): Promise<void>;
  /** F9, Ctrl+Alt+F9 with `all`. */
  calculateNow(all: boolean): Promise<void>;
  /** Shift+F9. */
  calculateSheet(sheet: number): Promise<void>;
  /** What the clipboard gets for cells of this workbook. */
  copyText(sheet: number, area: CellArea): Promise<{ text: string; html: string }>;
  /** Opens CSV text as a new workbook's cells. */
  loadCsv(text: string): Promise<void>;
  /** A sheet as CSV text. */
  csvText(sheet: number): Promise<string>;
  /** Where Ctrl+arrow goes. 1-based. */
  jumpTarget(
    sheet: number,
    row: number,
    col: number,
    rows: number,
    cols: number,
  ): Promise<[number, number]>;
  /** What the name box's text refers to, or null. */
  resolveName(text: string, sheet: number): Promise<(CellArea & { sheet: number }) | null>;
  functionNames(): Promise<string[]>;
  definedNames(): Promise<string[]>;
  /** The list of sheets as it now stands. */
  bookSummary(): Promise<BookSummary>;
  /** Edits a crashed session left, for the next launch to offer back. */
  recoverable(): Promise<Recoverable[]>;
  /** Opens what a crash left, with its edits applied again. */
  openRecovered(id: string, onEvent: (event: LoadEvent) => void): Promise<BookSummary>;
  discardRecovered(id: string): Promise<void>;
  hasChanges(): Promise<boolean>;
  /** Writes the workbook to a checked blob, and leaves the file on disk alone. */
  save(): Promise<SavedFile>;
  /** The save now stands in `source`, which the workbook reads from from now on. */
  commitSaved(source: FileSource): Promise<void>;
  prioritize(sheet: number): void;
  /** The numbers `decodeLayout` reads. */
  layout(sheet: number): Promise<Uint32Array>;
  drawings(sheet: number): Promise<DrawingBox[]>;
  /** `row` and `col` are 1-based. */
  cellText(sheet: number, row: number, col: number): Promise<CellText>;
  /** The size of WASM's memory, for the speed checks. */
  memoryBytes(): Promise<number>;
}

/** The engine worker, as the tile worker sees it over their `MessageChannel`. */
export interface EngineTilePort {
  viewBlock(
    sheet: number,
    firstRow: number,
    firstCol: number,
    dark: boolean,
    background: number,
    text: number,
  ): Promise<ArrayBuffer>;
}

/** The tile worker, as the main thread sees it. */
export interface TileWorkerApi {
  attachEngine(port: MessagePort): void;
  draw(request: TileRequest): Promise<TileImage>;
}
