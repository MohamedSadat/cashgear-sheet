// Reads the binary tile the engine sends. crates/cgs-engine/src/view_block.rs writes
// it and describes the layout, so keep the two in step.

const MAGIC = 0x42564743; // "CGVB", little-endian
const VERSION = 2;
const HEADER_BYTES = 32;
const CELL_BYTES = 24;
const STYLE_BYTES = 32;
const EXTRA_BYTES = 40;

export const TILE_NOT_READY = 1;

export const KIND_TEXT = 0;
export const KIND_NUMBER = 1;
export const KIND_GENERAL = 2;
export const KIND_OTHER = 3;
export const KIND_MASK = 3;
export const CELL_RTL = 1 << 2;
export const CELL_MERGED = 1 << 3;
export const CELL_NO_RIGHT_GRID = 1 << 4;
export const CELL_NO_BOTTOM_GRID = 1 << 5;
export const CELL_HASHES = 1 << 6;
export const CELL_OCCUPIED = 1 << 7;

export const FONT_BOLD = 1;
export const FONT_ITALIC = 2;
export const FONT_STRIKE = 4;

export const LAYOUT_WRAP = 1;
export const LAYOUT_SHRINK = 2;
export const LAYOUT_DIAGONAL_UP = 4;
export const LAYOUT_DIAGONAL_DOWN = 8;

export const EXTRA_OVERFLOW = 0;
export const EXTRA_MERGE = 1;

export const ALIGN_LEFT = 0;
export const ALIGN_CENTER = 1;
export const ALIGN_RIGHT = 2;
export const ALIGN_FILL = 3;
export const ALIGN_JUSTIFY = 4;
export const ALIGN_CENTER_CONTINUOUS = 5;
export const ALIGN_DISTRIBUTED = 6;

export const VALIGN_TOP = 0;
export const VALIGN_CENTER = 1;
export const VALIGN_BOTTOM = 2;

/** `LineStyle` in crates/cgs-model/src/style.rs. */
export const LINE_NONE = 0;
export const LINE_HAIR = 1;
export const LINE_DOTTED = 2;
export const LINE_DASHED = 3;
export const LINE_DASH_DOT = 4;
export const LINE_DASH_DOT_DOT = 5;
export const LINE_THIN = 6;
export const LINE_MEDIUM = 7;
export const LINE_MEDIUM_DASHED = 8;
export const LINE_MEDIUM_DASH_DOT = 9;
export const LINE_MEDIUM_DASH_DOT_DOT = 10;
export const LINE_SLANT_DASH_DOT = 11;
export const LINE_THICK = 12;
export const LINE_DOUBLE = 13;

export interface TileStyle {
  font: string;
  /** In points. */
  size: number;
  fontFlags: number;
  underline: number;
  horizontal: number;
  vertical: number;
  indent: number;
  rotation: number;
  layout: number;
  rightLine: number;
  bottomLine: number;
  diagonalLine: number;
  rightColor: number;
  bottomColor: number;
  diagonalColor: number;
}

export interface TileExtra {
  kind: number;
  /** The source cell or merged area in CSS pixels at 100%, relative to the tile's corner. */
  x: number;
  y: number;
  width: number;
  height: number;
  text: string;
  color: number;
  style: number;
  flags: number;
  fillAt: number;
}

export interface ViewBlock {
  rows: number;
  cols: number;
  firstRow: number;
  firstCol: number;
  flags: number;
  rowPx: Uint16Array;
  colPx: Uint16Array;
  /** Six numbers a cell: text offset, text length, fill, colour, style and flags, fill position. */
  cells: Uint32Array;
  styles: TileStyle[];
  extras: TileExtra[];
  heap: Uint8Array;
}

const decoder = new TextDecoder();

function readText(heap: Uint8Array, offset: number, length: number) {
  return length ? decoder.decode(heap.subarray(offset, offset + length)) : '';
}

export function decodeViewBlock(buffer: ArrayBuffer): ViewBlock {
  const view = new DataView(buffer);
  if (buffer.byteLength < HEADER_BYTES || view.getUint32(0, true) !== MAGIC)
    throw new Error('This is not a ViewBlock tile.');
  const version = view.getUint16(4, true);
  if (version !== VERSION) throw new Error('Unknown ViewBlock version ' + version + '.');
  const rows = view.getUint16(6, true);
  const cols = view.getUint16(8, true);
  const styleCount = view.getUint32(20, true);
  const extraCount = view.getUint32(24, true);
  const heapBytes = view.getUint32(28, true);
  const rowPx = new Uint16Array(buffer, HEADER_BYTES, rows);
  const colPx = new Uint16Array(buffer, HEADER_BYTES + rows * 2, cols);
  let at = HEADER_BYTES + (rows + cols) * 2;
  at += (4 - (at % 4)) % 4;
  const cells = new Uint32Array(buffer, at, (rows * cols * CELL_BYTES) / 4);
  const stylesAt = at + rows * cols * CELL_BYTES;
  const extrasAt = stylesAt + styleCount * STYLE_BYTES;
  const heapAt = extrasAt + extraCount * EXTRA_BYTES;
  if (heapAt + heapBytes !== buffer.byteLength) throw new Error('A ViewBlock tile is cut short.');
  const heap = new Uint8Array(buffer, heapAt, heapBytes);

  const styles: TileStyle[] = [];
  for (let index = 0; index < styleCount; index++) {
    const start = stylesAt + index * STYLE_BYTES;
    styles.push({
      font: readText(heap, view.getUint32(start, true), view.getUint32(start + 4, true)),
      size: view.getUint16(start + 8, true) / 100,
      fontFlags: view.getUint8(start + 10),
      underline: view.getUint8(start + 11),
      horizontal: view.getUint8(start + 12),
      vertical: view.getUint8(start + 13),
      indent: view.getUint8(start + 14),
      rotation: view.getUint8(start + 15),
      layout: view.getUint8(start + 16),
      rightLine: view.getUint8(start + 17),
      bottomLine: view.getUint8(start + 18),
      diagonalLine: view.getUint8(start + 19),
      rightColor: view.getUint32(start + 20, true),
      bottomColor: view.getUint32(start + 24, true),
      diagonalColor: view.getUint32(start + 28, true),
    });
  }
  const extras: TileExtra[] = [];
  for (let index = 0; index < extraCount; index++) {
    const start = extrasAt + index * EXTRA_BYTES;
    extras.push({
      kind: view.getUint8(start),
      x: view.getInt32(start + 4, true),
      y: view.getInt32(start + 8, true),
      width: view.getInt32(start + 12, true),
      height: view.getInt32(start + 16, true),
      text: readText(heap, view.getUint32(start + 20, true), view.getUint32(start + 24, true)),
      color: view.getUint32(start + 28, true),
      style: view.getUint16(start + 32, true),
      flags: view.getUint16(start + 34, true),
      fillAt: view.getUint32(start + 36, true),
    });
  }
  return {
    rows,
    cols,
    firstRow: view.getUint32(12, true),
    firstCol: view.getUint32(16, true),
    flags: view.getUint16(10, true),
    rowPx,
    colPx,
    cells,
    styles,
    extras,
    heap,
  };
}

export function cellText(block: ViewBlock, row: number, col: number): string {
  const at = (row * block.cols + col) * 6;
  return readText(block.heap, block.cells[at]!, block.cells[at + 1]!);
}

/** FNV-1a of a tile's bytes, so tests can compare what web and desktop drew from. */
export function hashBytes(bytes: Uint8Array): number {
  let hash = 0x811c9dc5;
  for (const byte of bytes) hash = Math.imul(hash ^ byte, 0x01000193);
  return hash >>> 0;
}
