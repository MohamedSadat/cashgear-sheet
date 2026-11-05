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

/** One cell to measure for a row or column to fit. */
export interface FitCell {
  text: string;
  font: string;
  /** In points. */
  size: number;
  bold: boolean;
  italic: boolean;
  /** The column's width in pixels at 100%, for text that wraps. */
  width: number;
  rotation: number;
  indent: number;
  wrap: boolean;
}

/** What it takes to fit one row: see `Session::fit_rows_json`. */
export interface RowFit {
  sheet: number;
  row: number;
  /** What its fonts need anyway, in pixels at 100%. */
  base: number;
  cells: FitCell[];
}

/** A cell to measure for a column to fit: see `Session::fit_cols_json`. */
export type ColFit = FitCell & { col: number };

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
  | {
      kind: 'format';
      sheet: number;
      areas: CellArea[];
      change: FormatChange;
      /** The cell a relative change, such as more decimals, goes by: [row, col], 1-based. */
      active?: [number, number];
    }
  | {
      kind: 'lines';
      sheet: number;
      axis: 'row' | 'col';
      first: number;
      last: number;
      size?: number;
      hidden?: boolean;
    }
  | { kind: 'autoFitRows'; sheet: number; first: number; last: number }
  | { kind: 'fitCols'; sheet: number; cols: [number, number][] }
  | { kind: 'fitRows'; rows: [number, number, number][]; quiet?: boolean }
  | { kind: 'merge'; sheet: number; areas: CellArea[]; center?: boolean; across?: boolean }
  | { kind: 'unmerge'; sheet: number; areas: CellArea[] }
  | { kind: 'freeze'; sheet: number; rows: number; cols: number; top?: number; left?: number }
  | { kind: 'shiftCells'; sheet: number; area: CellArea; down: boolean; delete?: boolean }
  | {
      kind: 'replaceOne';
      query: FindQuery;
      with: string;
      sheet: number;
      row: number;
      col: number;
    }
  | { kind: 'replaceAll'; query: FindQuery; with: string }
  | { kind: 'viewFlags'; sheet: number; grid?: boolean; headings?: boolean }
  | { kind: 'undo' }
  | { kind: 'redo' };

/** A colour as a format gives it. Theme colours stay references to the theme. */
export type FormatColor =
  { kind: 'auto' } | { kind: 'rgb'; rgb: string } | { kind: 'theme'; theme: number; tint?: number };

export interface FormatLine {
  /** Excel's name for the style, or `none`. */
  style: string;
  color?: FormatColor;
}

/** Lines by where a cell's edge falls in the selection. */
export interface BorderChange {
  top?: FormatLine;
  bottom?: FormatLine;
  left?: FormatLine;
  right?: FormatLine;
  insideHorizontal?: FormatLine;
  insideVertical?: FormatLine;
  diagonal?: FormatLine;
  diagonalUp?: boolean;
  diagonalDown?: boolean;
}

/** What a format does. Anything left out stays as it is in each cell. */
export interface FormatChange {
  clear?: boolean;
  fontName?: string;
  fontSize?: number;
  fontStep?: number;
  bold?: boolean;
  italic?: boolean;
  strike?: boolean;
  underline?: 'none' | 'single' | 'double' | 'singleAccounting' | 'doubleAccounting';
  fontColor?: FormatColor;
  fill?: { pattern: string; color?: FormatColor; patternColor?: FormatColor };
  numberFormat?: string;
  numberFormatId?: number;
  decimals?: number;
  horizontal?: string;
  vertical?: string;
  wrap?: boolean;
  shrink?: boolean;
  indent?: number;
  indentStep?: number;
  rotation?: number;
  readingOrder?: number;
  border?: BorderChange;
}

/** A colour as a cell has it, with how it looks here. */
export type StoredColor =
  | { kind: 'auto' }
  | { kind: 'rgb'; color: string }
  | { kind: 'theme'; theme: number; tint: number; color: string };

export interface StoredLine {
  style: string;
  color: StoredColor;
}

/** How a cell is formatted, for the ribbon. */
export interface CellFormat {
  fontName: string;
  fontSize: number;
  bold: boolean;
  italic: boolean;
  strike: boolean;
  underline: NonNullable<FormatChange['underline']>;
  fontColor: StoredColor;
  fill: { pattern: string; color: StoredColor; patternColor: StoredColor };
  horizontal: string;
  vertical: string;
  wrap: boolean;
  shrink: boolean;
  indent: number;
  rotation: number;
  readingOrder: number;
  numberFormat: string;
  numberFormatId: number;
  merged: boolean;
  border: {
    top: StoredLine;
    bottom: StoredLine;
    left: StoredLine;
    right: StoredLine;
    diagonal: StoredLine;
    diagonalUp: boolean;
    diagonalDown: boolean;
  };
}

/** The workbook's theme colours and its own number formats. */
export interface FormatLists {
  theme: string[];
  codes: string[];
}

/** A search, as the dialog sets it up. See `FindQuery` in `crates/cgs-engine/src/find.rs`. */
export interface FindQuery {
  text: string;
  sheet: number;
  workbook?: boolean;
  byColumns?: boolean;
  formulas?: boolean;
  matchCase?: boolean;
  wholeCell?: boolean;
  within?: CellArea;
}

export interface FoundCell {
  sheet: number;
  row: number;
  col: number;
  text: string;
}

/** What a command did. */
export interface Applied {
  /** How many cells a replace changed. */
  replaced?: number;
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
  /** Rows waiting to be measured. */
  takeRowFits(): Promise<RowFit[]>;
  /** Rows of a sheet from another program's file that wrap and hold no height. */
  openingRowFits(sheet: number): Promise<RowFit[]>;
  /** What to measure to fit columns. */
  colFits(sheet: number, first: number, last: number): Promise<ColFit[]>;
  mergeDiscards(sheet: number, areas: CellArea[], across: boolean): Promise<boolean>;
  cellFormat(sheet: number, row: number, col: number): Promise<CellFormat | null>;
  formatLists(): Promise<FormatLists | null>;
  /** What a cell looks like in a format code, or null if the code can't be used. */
  formatSample(sheet: number, row: number, col: number, code: string): Promise<string | null>;
  findNext(query: FindQuery, row: number, col: number, back: boolean): Promise<FoundCell | null>;
  findAll(query: FindQuery): Promise<FoundCell[]>;
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
  /** Each row's height, as [sheet, row, pixels]. */
  fitRows(rows: RowFit[], palette: TilePalette): Promise<[number, number, number][]>;
  /** Each column's width, as [column, pixels]. */
  fitColumns(cells: ColFit[], palette: TilePalette): Promise<[number, number][]>;
}
