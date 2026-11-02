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

export interface SheetSummary {
  name: string;
  state: 'visible' | 'hidden' | 'veryHidden';
  kind: 'worksheet' | 'chart' | 'other';
  loaded: boolean;
}

export interface BookSummary {
  active: number;
  sheets: SheetSummary[];
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
  open(source: FileSource, onEvent: (event: LoadEvent) => void): Promise<BookSummary>;
  close(): void;
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
