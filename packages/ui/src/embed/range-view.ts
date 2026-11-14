// One range of a sheet, drawn in the embed (Track S3): the engine's tiles composited on a canvas
// the size of what shows, clipped to the range, at 100%, with no headings or selection. The page
// scrolls past the frame's size; the canvas stays put and draws what is in view. A right-to-left
// sheet is drawn through a mirrored context, as the editor draws it, so column A is on the right.
import {
  TILE_COLS,
  TILE_ROWS,
  TileCache,
  type CellArea,
  type EngineClient,
  type SheetLayout,
  type TileAddress,
  type TilePalette,
} from '@cgs/engine-client';
import { Axis, MAX_COLS, MAX_ROWS, tilesCovering } from '../grid/geometry';
import { mirroredContext } from '../grid/mirror';

/** A layout's runs of 1-based lines as the axis takes them: 0-based, cut to the sheet. */
export function zeroBased(runs: Uint32Array, count: number): Int32Array {
  const out = new Int32Array(runs.length);
  for (let at = 0; at < runs.length; at += 3) {
    out[at] = runs[at]! - 1;
    out[at + 1] = Math.min(runs[at + 1]!, count) - 1;
    out[at + 2] = runs[at + 2]!;
  }
  return out;
}

/** A sheet's two axes at a scale, and where a range sits along them. */
export class RangeGeometry {
  readonly rows: Axis;
  readonly cols: Axis;
  /** The range's top-left corner and size, in device pixels at this scale. */
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;

  constructor(
    layout: SheetLayout,
    readonly area: CellArea,
    readonly scale: number,
  ) {
    this.rows = new Axis(MAX_ROWS, layout.defaultRowPx, zeroBased(layout.rows, MAX_ROWS), scale);
    this.cols = new Axis(MAX_COLS, layout.defaultColPx, zeroBased(layout.cols, MAX_COLS), scale);
    this.x = this.cols.offset(area.left - 1);
    this.y = this.rows.offset(area.top - 1);
    this.width = this.cols.offset(area.right) - this.x;
    this.height = this.rows.offset(area.bottom) - this.y;
  }

  /** The tiles (0-based) covering a part of the range, `dx` and `dy` from its corner. */
  tilesIn(dx: number, dy: number, width: number, height: number) {
    const { area, rows, cols } = this;
    const clampRow = (row: number) => Math.min(Math.max(row, area.top - 1), area.bottom - 1);
    const clampCol = (col: number) => Math.min(Math.max(col, area.left - 1), area.right - 1);
    return tilesCovering({
      top: clampRow(rows.at(this.y + dy)),
      left: clampCol(cols.at(this.x + dx)),
      bottom: clampRow(rows.at(this.y + dy + Math.max(0, height - 1))),
      right: clampCol(cols.at(this.x + dx + Math.max(0, width - 1))),
    });
  }

  /** Where a tile's picture goes, from the range's corner, and its size. */
  tileBox(firstRow: number, firstCol: number) {
    const left = this.cols.offset(firstCol);
    const top = this.rows.offset(firstRow);
    return {
      x: left - this.x,
      y: top - this.y,
      width: this.cols.offset(firstCol + TILE_COLS) - left,
      height: this.rows.offset(firstRow + TILE_ROWS) - top,
    };
  }
}

/** The range's size in CSS pixels at 100%. */
export function cssSize(layout: SheetLayout, area: CellArea) {
  const at100 = new RangeGeometry(layout, area, 1);
  return { width: at100.width, height: at100.height };
}

export interface RangeShown {
  sheet: number;
  layout: SheetLayout;
  area: CellArea;
}

/** The live range, drawn into `host`, which scrolls. */
export class RangeView {
  private readonly scroller: HTMLDivElement;
  private readonly content: HTMLDivElement;
  private readonly canvas: HTMLCanvasElement;
  private readonly plain: CanvasRenderingContext2D;
  private readonly mirrored: CanvasRenderingContext2D;
  private readonly cache: TileCache;
  private shown: RangeShown | null = null;
  private geometry: RangeGeometry | null = null;
  private palette: TilePalette;
  private dpr = 1;
  private frame = 0;
  private timer = 0;
  private readonly observer: ResizeObserver;

  constructor(
    host: HTMLElement,
    engine: EngineClient,
    palette: TilePalette,
    onError: (error: unknown) => void,
    /** The frame's width or height changed, which can bring a scrollbar. */
    private readonly onViewport: () => void = () => undefined,
  ) {
    this.palette = palette;
    this.scroller = document.createElement('div');
    this.scroller.className = 'cgs-embed-scroll';
    this.content = document.createElement('div');
    this.content.className = 'cgs-embed-content';
    this.canvas = document.createElement('canvas');
    this.canvas.className = 'cgs-embed-canvas';
    this.canvas.setAttribute('aria-hidden', 'true');
    this.canvas.dataset.testid = 'embed-canvas';
    this.content.appendChild(this.canvas);
    this.scroller.appendChild(this.content);
    host.appendChild(this.scroller);
    const context = this.canvas.getContext('2d', { alpha: false });
    if (!context) throw new Error('This browser has no 2D canvas.');
    this.plain = context;
    this.mirrored = mirroredContext(context, () => this.canvas.width);
    this.cache = new TileCache((request) => engine.drawTile(request), this.schedule, onError);
    this.scroller.addEventListener('scroll', this.schedule, { passive: true });
    this.observer = new ResizeObserver(() => {
      this.fit();
      this.onViewport();
    });
    this.observer.observe(this.scroller);
  }

  /** Whether a horizontal scrollbar shows, and how tall it is in CSS pixels. */
  scrollbarHeight(): number {
    return this.scroller.offsetHeight - this.scroller.clientHeight;
  }

  /** Whether the range is wider than the frame. */
  overflowsWidth(): boolean {
    const size = this.shown ? cssSize(this.shown.layout, this.shown.area) : null;
    return !!size && size.width > this.scroller.clientWidth;
  }

  show(shown: RangeShown) {
    const sameSheet = this.shown?.sheet === shown.sheet;
    this.shown = shown;
    if (!sameSheet) this.cache.clear();
    this.scroller.dir = shown.layout.rightToLeft ? 'rtl' : 'ltr';
    this.scroller.scrollTop = 0;
    this.scroller.scrollLeft = 0;
    this.applyStyle();
    this.fit();
  }

  /** A new workbook: every picture drawn so far is of the old one. */
  reset() {
    this.cache.clear();
    this.shown = null;
    this.geometry = null;
    this.schedule();
  }

  /** The sheet's cells changed after it showed, as when calculation finishes. */
  invalidate(sheets: number[], tiles: TileAddress[]) {
    for (const sheet of sheets) this.cache.invalidateSheet(sheet);
    this.cache.invalidateTiles(tiles);
    this.schedule();
  }

  setPalette(palette: TilePalette) {
    this.palette = palette;
    this.canvas.dataset.ready = 'false';
    this.applyStyle();
    this.schedule();
  }

  setVisible(visible: boolean) {
    this.scroller.hidden = !visible;
    if (visible) this.fit();
  }

  dispose() {
    this.observer.disconnect();
    this.cache.dispose();
    cancelAnimationFrame(this.frame);
    clearTimeout(this.timer);
    this.scroller.remove();
  }

  private applyStyle() {
    this.cache.setStyle({
      scale: this.dpr,
      palette: {
        ...this.palette,
        erp: undefined,
        showGrid: this.shown?.layout.showGrid ?? true,
        rightToLeft: this.shown?.layout.rightToLeft ?? false,
      },
    });
  }

  /** Sizes the page to the range and the canvas to what of it shows. */
  private fit() {
    const dpr = globalThis.devicePixelRatio || 1;
    if (dpr !== this.dpr) {
      this.dpr = dpr;
      this.applyStyle();
    }
    if (!this.shown) {
      this.canvas.width = 0;
      this.canvas.height = 0;
      return;
    }
    const { layout, area } = this.shown;
    this.geometry = new RangeGeometry(layout, area, this.dpr);
    const size = cssSize(layout, area);
    this.content.style.width = size.width + 'px';
    this.content.style.height = size.height + 'px';
    const width = Math.min(size.width, this.scroller.clientWidth || size.width);
    const height = Math.min(size.height, this.scroller.clientHeight || size.height);
    this.canvas.style.width = width + 'px';
    this.canvas.style.height = height + 'px';
    this.canvas.width = Math.max(1, Math.min(this.geometry.width, Math.round(width * this.dpr)));
    this.canvas.height = Math.max(1, Math.min(this.geometry.height, Math.round(height * this.dpr)));
    this.schedule();
  }

  /** Draws on the next frame, or soon when frames don't come, as in a frame out of view. */
  private readonly schedule = () => {
    if (this.frame || this.timer) return;
    this.frame = requestAnimationFrame(this.draw);
    this.timer = window.setTimeout(this.draw, 100);
  };

  private readonly draw = () => {
    cancelAnimationFrame(this.frame);
    clearTimeout(this.timer);
    this.frame = 0;
    this.timer = 0;
    const geometry = this.geometry;
    const shown = this.shown;
    const { canvas } = this;
    const context = shown?.layout.rightToLeft ? this.mirrored : this.plain;
    context.fillStyle = this.palette.background;
    context.fillRect(0, 0, canvas.width, canvas.height);
    if (!geometry || !shown) {
      this.cache.want([]);
      return;
    }
    const dx = Math.round(Math.abs(this.scroller.scrollLeft) * this.dpr);
    const dy = Math.round(this.scroller.scrollTop * this.dpr);
    const tiles = geometry.tilesIn(dx, dy, canvas.width, canvas.height);
    const wanted = tiles.map((tile) => ({ sheet: shown.sheet, ...tile }));
    let ready = true;
    context.save();
    context.beginPath();
    context.rect(0, 0, Math.min(canvas.width, geometry.width - dx), canvas.height);
    context.clip();
    for (const { firstRow, firstCol } of tiles) {
      if (!this.cache.isCurrent(shown.sheet, firstRow, firstCol)) ready = false;
      const bitmap = this.cache.get(shown.sheet, firstRow, firstCol);
      if (!bitmap) continue;
      const box = geometry.tileBox(firstRow, firstCol);
      const x = box.x - dx;
      const y = box.y - dy;
      if (bitmap.width === box.width && bitmap.height === box.height) {
        context.drawImage(bitmap, x, y);
      } else {
        context.drawImage(bitmap, x, y, box.width, box.height);
      }
    }
    context.restore();
    this.cache.want(wanted);
    // Tiles that arrive schedule the next drawing themselves.
    canvas.dataset.ready = String(ready);
  };
}
