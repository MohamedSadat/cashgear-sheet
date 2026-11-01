/**
 * The colours and font a tile is drawn with. Workers can't read CSS, so the main
 * thread builds this from the `--cg-*` tokens.
 */
export interface TilePalette {
  background: string;
  gridline: string;
  text: string;
  /** A CSS font family list. */
  fontFamily: string;
}

export interface TileRequest {
  /** The tile's top-left cell, counted from 0. */
  firstRow: number;
  firstCol: number;
  /** Device pixels per CSS pixel. */
  scale: number;
  palette: TilePalette;
}

export interface TileImage {
  bitmap: ImageBitmap;
  /** FNV-1a of the tile's bytes from the engine, for tests. */
  hash: number;
}

export interface EngineInfo {
  version: string;
}

/** The engine worker, as the main thread sees it. */
export interface EngineApi {
  init(): Promise<EngineInfo>;
  /** Connects the tile worker, which then reads tiles from this port. */
  attachTileWorker(port: MessagePort): void;
  /** Writes `value` into `buffer`, to prove the page and the worker share memory. */
  touchSharedMemory(buffer: SharedArrayBuffer, value: number): void;
}

/** The engine worker, as the tile worker sees it over their `MessageChannel`. */
export interface EngineTilePort {
  viewBlock(firstRow: number, firstCol: number): Promise<ArrayBuffer>;
}

/** The tile worker, as the main thread sees it. */
export interface TileWorkerApi {
  attachEngine(port: MessagePort): void;
  draw(request: TileRequest): Promise<TileImage>;
}
