// Reads the binary tile the engine sends. crates/cgs-engine/src/view_block.rs writes
// it and describes the layout, so keep the two in step.

const MAGIC = 0x42564743; // "CGVB", little-endian
const VERSION = 1;
const HEADER_BYTES = 24;
const CELL_BYTES = 24;

export interface ViewBlock {
  rows: number;
  cols: number;
  firstRow: number;
  firstCol: number;
  view: DataView;
  heap: Uint8Array;
}

/** A cell's colours, `0xRRGGBBAA`. Alpha 0 means the theme supplies the colour. */
export interface CellColors {
  fill: number;
  text: number;
}

const decoder = new TextDecoder();

export function decodeViewBlock(buffer: ArrayBuffer): ViewBlock {
  const view = new DataView(buffer);
  if (buffer.byteLength < HEADER_BYTES || view.getUint32(0, true) !== MAGIC)
    throw new Error('This is not a ViewBlock tile.');
  const version = view.getUint16(4, true);
  if (version !== VERSION) throw new Error('Unknown ViewBlock version ' + version + '.');
  const rows = view.getUint16(6, true);
  const cols = view.getUint16(8, true);
  const heapBytes = view.getUint32(20, true);
  const heapStart = HEADER_BYTES + rows * cols * CELL_BYTES;
  if (heapStart + heapBytes !== buffer.byteLength)
    throw new Error('A ViewBlock tile is cut short.');
  return {
    rows,
    cols,
    firstRow: view.getUint32(12, true),
    firstCol: view.getUint32(16, true),
    view,
    heap: new Uint8Array(buffer, heapStart, heapBytes),
  };
}

function cellOffset(block: ViewBlock, row: number, col: number) {
  return HEADER_BYTES + (row * block.cols + col) * CELL_BYTES;
}

/** The colours of the cell at (`row`, `col`) within the tile. */
export function cellColors(block: ViewBlock, row: number, col: number): CellColors {
  const at = cellOffset(block, row, col);
  return { fill: block.view.getUint32(at + 8, true), text: block.view.getUint32(at + 12, true) };
}

/** The text of the cell at (`row`, `col`) within the tile. */
export function cellText(block: ViewBlock, row: number, col: number): string {
  const at = cellOffset(block, row, col);
  const length = block.view.getUint32(at + 4, true);
  if (!length) return '';
  const offset = block.view.getUint32(at, true);
  return decoder.decode(block.heap.subarray(offset, offset + length));
}

/** FNV-1a of a tile's bytes, the same as the Rust test's. */
export function hashBytes(bytes: Uint8Array): number {
  let hash = 0x811c9dc5;
  for (const byte of bytes) hash = Math.imul(hash ^ byte, 0x01000193);
  return hash >>> 0;
}
