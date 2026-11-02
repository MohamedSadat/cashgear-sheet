import type { TileImage, TilePalette, TileRequest } from './types';

/**
 * Tiles asked for at once: one being drawn, and one ready to follow it. More would only queue
 * in the workers, where they can't be taken back, and hold up the tiles in view after a jump.
 */
const MAX_IN_FLIGHT = 2;
/** What the cache keeps by default. Tiles in view are kept whatever this says. */
const DEFAULT_LIMIT_BYTES = 512 * 1024 * 1024;

interface Entry {
  sheet: number;
  firstRow: number;
  firstCol: number;
  bitmap: ImageBitmap | null;
  hash: number | null;
  /** The style the bitmap was drawn with. */
  drawn: number;
  /** The style a drawing is on its way for, or 0. */
  pending: number;
  /** The sheet was fully read when this was drawn. */
  ready: boolean;
  bytes: number;
  used: number;
}

export interface TileStyle {
  scale: number;
  palette: TilePalette;
}

export interface TileAddress {
  sheet: number;
  firstRow: number;
  firstCol: number;
}

const keyOf = (sheet: number, firstRow: number, firstCol: number) =>
  sheet + ':' + firstRow + ':' + firstCol;

/**
 * The main thread's drawn tiles, as pictures, never cell data. When the style changes, the
 * old bitmaps stay visible until their replacements arrive. Only what is wanted right now
 * gets drawn, nearest first, so a fast scroll never leaves a queue of tiles it has passed.
 */
export class TileCache {
  private readonly entries = new Map<string, Entry>();
  private style = 1;
  private current: TileStyle | null = null;
  private clock = 0;
  private inFlight = 0;
  private wanted: TileAddress[] = [];
  private held = 0;
  private disposed = false;

  constructor(
    private readonly draw: (request: TileRequest) => Promise<TileImage>,
    /** A tile arrived, so the view should composite again. */
    private readonly onChange: () => void,
    private readonly onError: (error: unknown) => void,
    private readonly limitBytes = DEFAULT_LIMIT_BYTES,
  ) {}

  /** Sets what tiles are drawn with. Tiles drawn with the old style become stale. */
  setStyle(style: TileStyle) {
    this.current = style;
    this.style++;
  }

  /** The tile's picture, stale or not, or null. */
  get(sheet: number, firstRow: number, firstCol: number): ImageBitmap | null {
    const entry = this.entries.get(keyOf(sheet, firstRow, firstCol));
    if (!entry) return null;
    entry.used = ++this.clock;
    return entry.bitmap;
  }

  /** True when the picture was drawn in the current style, from a sheet that was read. */
  isCurrent(sheet: number, firstRow: number, firstCol: number) {
    const entry = this.entries.get(keyOf(sheet, firstRow, firstCol));
    return !!entry && entry.drawn === this.style && entry.ready;
  }

  hash(sheet: number, firstRow: number, firstCol: number) {
    return this.entries.get(keyOf(sheet, firstRow, firstCol))?.hash ?? null;
  }

  /** Says which tiles are wanted now, in order. Anything asked for earlier is forgotten. */
  want(tiles: TileAddress[]) {
    this.wanted = tiles;
    this.pump();
  }

  /** A sheet's tiles are out of date: it finished loading, say. Pictures stay until replaced. */
  invalidateSheet(sheet: number) {
    for (const entry of this.entries.values()) {
      if (entry.sheet === sheet) {
        entry.drawn = 0;
        entry.ready = false;
      }
    }
    this.pump();
  }

  /** Drops every picture, for when a different workbook opens. */
  clear() {
    for (const entry of this.entries.values()) entry.bitmap?.close();
    this.entries.clear();
    this.held = 0;
    this.wanted = [];
    this.style++;
  }

  dispose() {
    this.disposed = true;
    this.clear();
  }

  private pump() {
    if (!this.current) return;
    for (const { sheet, firstRow, firstCol } of this.wanted) {
      if (this.inFlight >= MAX_IN_FLIGHT) return;
      const key = keyOf(sheet, firstRow, firstCol);
      let entry = this.entries.get(key);
      if (!entry) {
        entry = {
          sheet,
          firstRow,
          firstCol,
          bitmap: null,
          hash: null,
          drawn: 0,
          pending: 0,
          ready: false,
          bytes: 0,
          used: ++this.clock,
        };
        this.entries.set(key, entry);
      }
      if (entry.drawn !== this.style && entry.pending !== this.style) this.request(entry);
    }
  }

  private request(entry: Entry) {
    const style = this.style;
    const current = this.current!;
    entry.pending = style;
    this.inFlight++;
    const { sheet, firstRow, firstCol } = entry;
    this.draw({
      sheet,
      firstRow,
      firstCol,
      scale: current.scale,
      palette: current.palette,
    }).then(
      ({ bitmap, hash, ready }) => {
        this.inFlight--;
        // The workbook may have changed, or a newer style may have arrived first.
        const live = this.entries.get(keyOf(sheet, firstRow, firstCol));
        if (this.disposed || live !== entry || style < entry.drawn) {
          bitmap.close();
          this.pump();
          return;
        }
        entry.bitmap?.close();
        this.held -= entry.bytes;
        entry.bitmap = bitmap;
        entry.hash = hash;
        entry.drawn = style;
        entry.pending = 0;
        entry.ready = ready;
        entry.bytes = bitmap.width * bitmap.height * 4;
        entry.used = ++this.clock;
        this.held += entry.bytes;
        this.evict();
        this.onChange();
        this.pump();
      },
      (error) => {
        this.inFlight--;
        if (!this.disposed) this.onError(error);
      },
    );
  }

  /** Frees the longest-unused pictures, never one that is wanted right now. */
  private evict() {
    if (this.held <= this.limitBytes) return;
    const keep = new Set(
      this.wanted.map((tile) => keyOf(tile.sheet, tile.firstRow, tile.firstCol)),
    );
    const oldest = [...this.entries.entries()]
      .filter(([key, entry]) => entry.bitmap && !keep.has(key))
      .sort((a, b) => a[1].used - b[1].used);
    for (const [key, entry] of oldest) {
      if (this.held <= this.limitBytes) return;
      entry.bitmap?.close();
      this.held -= entry.bytes;
      this.entries.delete(key);
    }
  }
}
