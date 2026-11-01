import type { TileImage, TilePalette, TileRequest } from './types';

interface Entry {
  bitmap: ImageBitmap | null;
  hash: number | null;
  /** The style the bitmap was drawn with. */
  drawn: number;
  /** The style last asked for, so a tile is asked for once per style. */
  requested: number;
}

export interface TileStyle {
  scale: number;
  palette: TilePalette;
}

/**
 * The main thread's drawn tiles. When the style changes, the old bitmaps stay
 * visible until their replacements arrive. It holds pictures, never cell data.
 */
export class TileCache {
  private readonly entries = new Map<string, Entry>();
  private style = 0;
  private current: TileStyle | null = null;
  private disposed = false;

  constructor(
    private readonly draw: (request: TileRequest) => Promise<TileImage>,
    /** A tile arrived, so the view should composite again. */
    private readonly onChange: () => void,
    private readonly onError: (error: unknown) => void,
  ) {}

  /** Sets what tiles are drawn with. Tiles drawn with the old style become stale. */
  setStyle(style: TileStyle) {
    this.current = style;
    this.style++;
  }

  /** The tile's picture, stale or not, or null. Asks for a current one if it is missing. */
  get(firstRow: number, firstCol: number): ImageBitmap | null {
    const key = firstRow + ':' + firstCol;
    let entry = this.entries.get(key);
    if (!entry) {
      entry = { bitmap: null, hash: null, drawn: 0, requested: 0 };
      this.entries.set(key, entry);
    }
    if (this.current && entry.requested !== this.style) {
      entry.requested = this.style;
      this.request(entry, firstRow, firstCol, this.style, this.current);
    }
    return entry.bitmap;
  }

  /** True when the tile's picture was drawn with the current style. */
  isCurrent(firstRow: number, firstCol: number) {
    return this.entries.get(firstRow + ':' + firstCol)?.drawn === this.style;
  }

  hash(firstRow: number, firstCol: number) {
    return this.entries.get(firstRow + ':' + firstCol)?.hash ?? null;
  }

  dispose() {
    this.disposed = true;
    for (const entry of this.entries.values()) entry.bitmap?.close();
    this.entries.clear();
  }

  private request(
    entry: Entry,
    firstRow: number,
    firstCol: number,
    style: number,
    { scale, palette }: TileStyle,
  ) {
    this.draw({ firstRow, firstCol, scale, palette }).then(
      ({ bitmap, hash }) => {
        // A newer style may have arrived first, or the view may have gone away.
        if (this.disposed || style < entry.drawn) {
          bitmap.close();
          return;
        }
        entry.bitmap?.close();
        entry.bitmap = bitmap;
        entry.hash = hash;
        entry.drawn = style;
        this.onChange();
      },
      (error) => {
        if (!this.disposed) this.onError(error);
      },
    );
  }
}
