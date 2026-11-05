// How tall a row or how wide a column must be for what it holds. It measures with the same fonts and
// the same room for text as drawing does, so that what fits here fits on the sheet.
import { singleLine, wrapLines } from './text-fit';
import { fontStack } from './tile-draw';
import type { ColFit, FitCell, RowFit, TilePalette } from './types';

type Measuring = Pick<OffscreenCanvasRenderingContext2D, 'font' | 'measureText'>;

const INSET = 2;
const INDENT = 9;

function fontCss(cell: FitCell, palette: TilePalette) {
  const px = (cell.size * 96) / 72;
  return (
    (cell.italic ? 'italic ' : '') +
    (cell.bold ? 'bold ' : '') +
    px.toFixed(2) +
    'px ' +
    fontStack(cell.font, palette)
  );
}

/** Excel's line height for a font of this size, which is what a row fits to per line. */
function lineHeight(cell: FitCell) {
  return Math.ceil(cell.size * 1.75);
}

function cellHeight(context: Measuring, cell: FitCell, palette: TilePalette) {
  context.font = fontCss(cell, palette);
  if (cell.rotation) {
    const line = singleLine(cell.text);
    if (cell.rotation === 255) return [...line].length * lineHeight(cell);
    const angle =
      (Math.min(cell.rotation, 180 - cell.rotation > 90 ? 90 : cell.rotation) * Math.PI) / 180;
    const degrees = cell.rotation <= 90 ? cell.rotation : cell.rotation - 90;
    const radians = (degrees * Math.PI) / 180 || angle;
    const length = context.measureText(line).width;
    return Math.ceil(
      Math.abs(Math.sin(radians)) * length + Math.abs(Math.cos(radians)) * lineHeight(cell),
    );
  }
  const room = Math.max(1, cell.width - INSET * 2 - cell.indent * INDENT - 1);
  const lines = wrapLines(cell.text, (candidate) => context.measureText(candidate).width <= room);
  return Math.max(1, lines.length) * lineHeight(cell);
}

/** Each row's height in pixels at 100%, as [sheet, row, pixels]. */
export function fitRows(
  context: Measuring,
  rows: RowFit[],
  palette: TilePalette,
): [number, number, number][] {
  return rows.map(({ sheet, row, base, cells }) => {
    let height = base;
    for (const cell of cells) height = Math.max(height, cellHeight(context, cell, palette));
    // Excel's limit for a row is 409.5 points.
    return [sheet, row, Math.min(height, 546)];
  });
}

/** Each column's width in pixels at 100%, as [column, pixels]. */
export function fitColumns(
  context: Measuring,
  cells: ColFit[],
  palette: TilePalette,
): [number, number][] {
  const widest = new Map<number, number>();
  for (const cell of cells) {
    context.font = fontCss(cell, palette);
    let width = 0;
    for (const line of cell.text.split(/\r?\n/)) {
      width = Math.max(width, context.measureText(line).width);
    }
    if (cell.rotation) {
      const degrees =
        cell.rotation === 255 ? 90 : cell.rotation <= 90 ? cell.rotation : cell.rotation - 90;
      const radians = (degrees * Math.PI) / 180;
      width =
        cell.rotation === 255
          ? (cell.size * 96) / 72
          : Math.abs(Math.cos(radians)) * width + Math.abs(Math.sin(radians)) * lineHeight(cell);
    }
    const needed = Math.ceil(width) + cell.indent * INDENT + COLUMN_PADDING;
    widest.set(cell.col, Math.max(widest.get(cell.col) ?? 0, needed));
  }
  return [...widest].map(([col, px]) => [col, Math.min(px, 1792)]);
}

/** What a column adds around its widest text: 14 for "x", 77 for ten digits, over COM. */
const COLUMN_PADDING = 7;
