// Pure grid geometry, in whole device pixels. No DOM, no engine.
import { devicePx, TILE_COLS, TILE_ROWS } from '@cgs/engine-client';

export const MAX_ROWS = 1_048_576;
export const MAX_COLS = 16_384;

/**
 * Sizes and offsets along one direction. Most lines have the default size, so only runs of
 * the others are stored, with running totals: a sheet of a million rows costs nothing until
 * its rows differ, and a filter that hides most of them is a few runs. Hidden lines are size 0.
 * Each line rounds alone at any scale, the way a tile's cells do, so a tile's picture always fits
 * exactly where the axis puts it.
 */
export class Axis {
  private readonly firsts: Int32Array;
  private readonly lasts: Int32Array;
  private readonly px: Int32Array;
  private readonly totals: Float64Array;
  private scale = 1;
  private defaultSize = 0;

  /** `runs` are (0-based first line, last line, CSS pixels at 100%), sorted and apart. */
  constructor(
    readonly count: number,
    private readonly defaultPx: number,
    runs: ArrayLike<number> = [],
    scale = 1,
  ) {
    const size = runs.length / 3;
    this.firsts = new Int32Array(size);
    this.lasts = new Int32Array(size);
    this.px = new Int32Array(size);
    for (let at = 0; at < size; at++) {
      this.firsts[at] = runs[at * 3]!;
      this.lasts[at] = runs[at * 3 + 1]!;
      this.px[at] = runs[at * 3 + 2]!;
    }
    this.totals = new Float64Array(size + 1);
    this.setScale(scale);
  }

  setScale(scale: number) {
    this.scale = scale;
    this.defaultSize = devicePx(this.defaultPx, scale);
    for (let at = 0; at < this.px.length; at++) {
      const lines = this.lasts[at]! - this.firsts[at]! + 1;
      this.totals[at + 1] =
        this.totals[at]! + lines * (devicePx(this.px[at]!, scale) - this.defaultSize);
    }
  }

  /** The last run starting before this line, or -1. */
  private runBefore(line: number) {
    let low = 0;
    let high = this.firsts.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (this.firsts[middle]! < line) low = middle + 1;
      else high = middle;
    }
    return low - 1;
  }

  /** The run holding this line, or -1. */
  private runAt(line: number) {
    const at = this.runBefore(line + 1);
    return at >= 0 && this.lasts[at]! >= line ? at : -1;
  }

  size(line: number) {
    const at = this.runAt(line);
    return at >= 0 ? devicePx(this.px[at]!, this.scale) : this.defaultSize;
  }

  /** Where this line starts. */
  offset(line: number) {
    const at = this.runBefore(line);
    if (at < 0) return line * this.defaultSize;
    if (this.lasts[at]! < line) return line * this.defaultSize + this.totals[at + 1]!;
    const inside = line - this.firsts[at]!;
    return (
      line * this.defaultSize +
      this.totals[at]! +
      inside * (devicePx(this.px[at]!, this.scale) - this.defaultSize)
    );
  }

  /** The visible line at this position, or the last one past the end. */
  at(position: number) {
    if (position <= 0) return this.nextVisible(0);
    let low = 0;
    let high = this.count;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (this.offset(middle + 1) <= position) low = middle + 1;
      else high = middle;
    }
    return Math.min(low, this.count - 1);
  }

  get total() {
    return this.offset(this.count);
  }

  /** The nearest line at or after this one with any size, or this one if there is none. */
  nextVisible(line: number) {
    for (let at = line; at < this.count;) {
      if (this.size(at) > 0) return at;
      // A hidden run is passed in one step.
      const run = this.runAt(at);
      at = run >= 0 ? this.lasts[run]! + 1 : at + 1;
    }
    return Math.min(line, this.count - 1);
  }

  /** The nearest line at or before this one with any size, or this one if there is none. */
  previousVisible(line: number) {
    for (let at = line; at >= 0;) {
      if (this.size(at) > 0) return at;
      const run = this.runAt(at);
      at = run >= 0 ? this.firsts[run]! - 1 : at - 1;
    }
    return Math.max(line, 0);
  }
}

/** Column letters from a 0-based index. */
export function columnName(col: number) {
  let text = '';
  for (let rest = col + 1; rest > 0; rest = Math.floor((rest - 1) / 26)) {
    text = String.fromCharCode(65 + ((rest - 1) % 26)) + text;
  }
  return text;
}

/** A cell's address from 0-based indices. */
export function cellName(row: number, col: number) {
  return columnName(col) + (row + 1);
}

export interface CellRange {
  top: number;
  left: number;
  bottom: number;
  right: number;
}

export function rangeName(range: CellRange) {
  const wholeRows = range.left === 0 && range.right === MAX_COLS - 1;
  const wholeCols = range.top === 0 && range.bottom === MAX_ROWS - 1;
  if (wholeRows && !wholeCols) return `${range.top + 1}:${range.bottom + 1}`;
  if (wholeCols && !wholeRows) {
    const letters = (col: number) => cellName(0, col).replace(/\d+$/, '');
    return `${letters(range.left)}:${letters(range.right)}`;
  }
  const first = cellName(range.top, range.left);
  if (range.top === range.bottom && range.left === range.right) return first;
  return first + ':' + cellName(range.bottom, range.right);
}

/** The first tile line at or before this line. */
export const tileRow = (row: number) => Math.floor(row / TILE_ROWS) * TILE_ROWS;
export const tileCol = (col: number) => Math.floor(col / TILE_COLS) * TILE_COLS;

/** Tiles covering these lines, nearest the view's centre first. */
export function tilesCovering(range: CellRange): { firstRow: number; firstCol: number }[] {
  const out: { firstRow: number; firstCol: number; distance: number }[] = [];
  const middleRow = (range.top + range.bottom) / 2;
  const middleCol = (range.left + range.right) / 2;
  for (let row = tileRow(range.top); row <= range.bottom; row += TILE_ROWS) {
    for (let col = tileCol(range.left); col <= range.right; col += TILE_COLS) {
      const distance =
        Math.abs(row + TILE_ROWS / 2 - middleRow) / TILE_ROWS +
        Math.abs(col + TILE_COLS / 2 - middleCol) / TILE_COLS;
      out.push({ firstRow: row, firstCol: col, distance });
    }
  }
  out.sort((a, b) => a.distance - b.distance);
  return out.map(({ firstRow, firstCol }) => ({ firstRow, firstCol }));
}

/** A scrollbar's thumb: where it sits and how long it is, along a track of this length. */
export function thumbFor(
  scroll: number,
  view: number,
  content: number,
  track: number,
  minimum: number,
) {
  const length = Math.max(minimum, Math.min(track, Math.round((view / content) * track)));
  const room = Math.max(0, content - view);
  const position = room > 0 ? Math.round((scroll / room) * (track - length)) : 0;
  return { position, length };
}

/** The scroll position a dragged thumb stands for. */
export function scrollForThumb(
  position: number,
  length: number,
  track: number,
  content: number,
  view: number,
) {
  const room = Math.max(1, track - length);
  return (Math.max(0, Math.min(room, position)) / room) * Math.max(0, content - view);
}

/** The zoom slider's position for a zoom, with 100% at the middle as in Excel. */
export function zoomToSlider(zoom: number) {
  return zoom <= 100 ? ((zoom - 10) / 90) * 50 : 50 + ((zoom - 100) / 300) * 50;
}

export function sliderToZoom(position: number) {
  const zoom = position <= 50 ? 10 + (position / 50) * 90 : 100 + ((position - 50) / 50) * 300;
  return Math.round(zoom);
}
