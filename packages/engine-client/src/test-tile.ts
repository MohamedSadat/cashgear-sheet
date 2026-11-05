// For tests.
/** Writes a tile the way crates/cgs-engine/src/view_block.rs does. */
export function encodeTile(options: {
  rowPx: number[];
  colPx: number[];
  cells?: { text?: string; fill?: number; color?: number; style?: number; flags?: number }[];
  styles?: {
    font?: string;
    size?: number;
    fontFlags?: number;
    underline?: number;
    horizontal?: number;
    vertical?: number;
    indent?: number;
    rotation?: number;
    layout?: number;
    rightLine?: number;
    bottomLine?: number;
    diagonalLine?: number;
    rightColor?: number;
    bottomColor?: number;
  }[];
  extras?: {
    kind: number;
    x: number;
    y: number;
    width: number;
    height: number;
    text: string;
    color?: number;
    style?: number;
    flags?: number;
  }[];
  flags?: number;
}): ArrayBuffer {
  const { rowPx, colPx, flags = 0 } = options;
  const count = rowPx.length * colPx.length;
  const heap: number[] = [];
  const encoder = new TextEncoder();
  const pushText = (text: string) => {
    const bytes = encoder.encode(text);
    const offset = heap.length;
    heap.push(...bytes);
    return [offset, bytes.length] as const;
  };
  const styles = options.styles ?? [{}];
  const extras = options.extras ?? [];
  const sizes = 36 + (rowPx.length + colPx.length) * 2;
  const cellsAt = sizes + ((4 - (sizes % 4)) % 4);
  const stylesAt = cellsAt + count * 24;
  const extrasAt = stylesAt + styles.length * 40;
  const fixed = extrasAt + extras.length * 40;
  const scratch = new DataView(new ArrayBuffer(fixed));
  scratch.setUint32(0, 0x42564743, true);
  scratch.setUint16(4, 3, true);
  scratch.setUint16(6, rowPx.length, true);
  scratch.setUint16(8, colPx.length, true);
  scratch.setUint16(10, flags, true);
  scratch.setUint32(20, styles.length, true);
  scratch.setUint32(24, extras.length, true);
  [...rowPx, ...colPx].forEach((px, at) => scratch.setUint16(36 + at * 2, px, true));
  styles.forEach((style, index) => {
    const at = stylesAt + index * 40;
    const [offset, length] = pushText(style.font ?? '');
    scratch.setUint32(at, offset, true);
    scratch.setUint32(at + 4, length, true);
    scratch.setUint16(at + 8, (style.size ?? 11) * 100, true);
    scratch.setUint8(at + 10, style.fontFlags ?? 0);
    scratch.setUint8(at + 11, style.underline ?? 0);
    scratch.setUint8(at + 12, style.horizontal ?? 0);
    scratch.setUint8(at + 13, style.vertical ?? 2);
    scratch.setUint8(at + 14, style.indent ?? 0);
    scratch.setUint8(at + 15, style.rotation ?? 0);
    scratch.setUint8(at + 16, style.layout ?? 0);
    scratch.setUint8(at + 17, style.rightLine ?? 0);
    scratch.setUint8(at + 18, style.bottomLine ?? 0);
    scratch.setUint8(at + 19, style.diagonalLine ?? 0);
    scratch.setUint32(at + 20, style.rightColor ?? 0, true);
    scratch.setUint32(at + 24, style.bottomColor ?? 0, true);
  });
  for (let index = 0; index < count; index++) {
    const cell = options.cells?.[index] ?? {};
    const at = cellsAt + index * 24;
    const [offset, length] = pushText(cell.text ?? '');
    scratch.setUint32(at, offset, true);
    scratch.setUint32(at + 4, length, true);
    scratch.setUint32(at + 8, cell.fill ?? 0, true);
    scratch.setUint32(at + 12, cell.color ?? 0, true);
    scratch.setUint16(at + 16, cell.style ?? 0, true);
    scratch.setUint16(at + 18, cell.flags ?? 0, true);
  }
  extras.forEach((extra, index) => {
    const at = extrasAt + index * 40;
    const [offset, length] = pushText(extra.text);
    scratch.setUint8(at, extra.kind);
    scratch.setInt32(at + 4, extra.x, true);
    scratch.setInt32(at + 8, extra.y, true);
    scratch.setInt32(at + 12, extra.width, true);
    scratch.setInt32(at + 16, extra.height, true);
    scratch.setUint32(at + 20, offset, true);
    scratch.setUint32(at + 24, length, true);
    scratch.setUint32(at + 28, extra.color ?? 0, true);
    scratch.setUint16(at + 32, extra.style ?? 0, true);
    scratch.setUint16(at + 34, extra.flags ?? 0, true);
  });
  scratch.setUint32(28, heap.length, true);
  const out = new Uint8Array(fixed + heap.length);
  out.set(new Uint8Array(scratch.buffer));
  out.set(heap, fixed);
  return out.buffer;
}
