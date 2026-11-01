// Draws a tile on any 2D context, so drawing can move threads if it needs to. It
// runs in the tile worker today. Pure apart from the context: no DOM, no engine.
import type { TilePalette } from './types';
import { cellColors, cellText, type ViewBlock } from './view-block';

export const CELL_WIDTH = 64;
export const ROW_HEIGHT = 20;
export const TILE_ROWS = 64;
export const TILE_COLS = 16;
const FONT_SIZE = 12;
const TEXT_INSET = 4;

export type TileContext = Pick<
  OffscreenCanvasRenderingContext2D,
  | 'beginPath'
  | 'clip'
  | 'fillRect'
  | 'fillStyle'
  | 'fillText'
  | 'font'
  | 'rect'
  | 'restore'
  | 'save'
  | 'textBaseline'
>;

/** A cell's size in whole device pixels. */
export function cellPixels(scale: number) {
  return { width: Math.round(CELL_WIDTH * scale), height: Math.round(ROW_HEIGHT * scale) };
}

/** A tile's size in whole device pixels. */
export function tilePixels(scale: number) {
  const cell = cellPixels(scale);
  return { width: cell.width * TILE_COLS, height: cell.height * TILE_ROWS };
}

/** `0xRRGGBBAA` as a CSS colour. */
export function cssColor(rgba: number): string {
  return '#' + (rgba >>> 0).toString(16).padStart(8, '0');
}

const hasAlpha = (rgba: number) => (rgba & 0xff) !== 0;

export function drawTile(
  context: TileContext,
  block: ViewBlock,
  palette: TilePalette,
  scale: number,
) {
  const cell = cellPixels(scale);
  context.fillStyle = palette.background;
  context.fillRect(0, 0, cell.width * block.cols, cell.height * block.rows);

  // Fills hide gridlines, as in Excel.
  for (let row = 0; row < block.rows; row++) {
    for (let col = 0; col < block.cols; col++) {
      const x = col * cell.width;
      const y = row * cell.height;
      const { fill } = cellColors(block, row, col);
      if (hasAlpha(fill)) {
        context.fillStyle = cssColor(fill);
        context.fillRect(x, y, cell.width, cell.height);
      } else {
        context.fillStyle = palette.gridline;
        context.fillRect(x + cell.width - 1, y, 1, cell.height);
        context.fillRect(x, y + cell.height - 1, cell.width, 1);
      }
    }
  }

  context.font = FONT_SIZE * scale + 'px ' + palette.fontFamily;
  context.textBaseline = 'middle';
  for (let row = 0; row < block.rows; row++) {
    for (let col = 0; col < block.cols; col++) {
      const text = cellText(block, row, col);
      if (!text) continue;
      const x = col * cell.width;
      const y = row * cell.height;
      const color = cellColors(block, row, col).text;
      context.fillStyle = hasAlpha(color) ? cssColor(color) : palette.text;
      context.save();
      context.beginPath();
      context.rect(x, y, cell.width - 1, cell.height - 1);
      context.clip();
      context.fillText(text, x + TEXT_INSET * scale, y + cell.height / 2);
      context.restore();
    }
  }
}
