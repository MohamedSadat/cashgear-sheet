// Draws a tile on any 2D context, so drawing can move threads if it needs to. It runs in
// the tile worker today. Pure apart from the context: no DOM, no engine.
import { drawIcon } from './icons';
import { shortenGeneral, singleLine, wrapLines } from './text-fit';
import type { ErpStateColors, TilePalette } from './types';
import {
  BAR_AXIS,
  BAR_BORDER,
  BAR_GRADIENT,
  MARK_BAR,
  MARK_ICON,
  MARK_INVALID,
  MARK_NOTE,
  MARK_THREAD,
  MARK_ERROR,
  ALIGN_CENTER,
  ALIGN_CENTER_CONTINUOUS,
  ALIGN_DISTRIBUTED,
  ALIGN_FILL,
  ALIGN_JUSTIFY,
  ALIGN_LEFT,
  ALIGN_RIGHT,
  CELL_ERP_FORMULA,
  CELL_ERP_INVALID,
  CELL_ERP_LOCK,
  CELL_ERP_MODIFIED,
  CELL_ERP_READ_ONLY,
  CELL_HASHES,
  CELL_MERGED,
  CELL_NO_BOTTOM_GRID,
  CELL_NO_RIGHT_GRID,
  CELL_OCCUPIED,
  CELL_RTL,
  cellText,
  FONT_BOLD,
  FONT_ITALIC,
  FONT_STRIKE,
  FONT_SUBSCRIPT,
  FONT_SUPERSCRIPT,
  KIND_GENERAL,
  KIND_MASK,
  KIND_NUMBER,
  KIND_TEXT,
  LAYOUT_DIAGONAL_DOWN,
  LAYOUT_DIAGONAL_UP,
  LAYOUT_SHRINK,
  LAYOUT_WRAP,
  LINE_DASHED,
  LINE_DASH_DOT,
  LINE_DASH_DOT_DOT,
  LINE_DOTTED,
  LINE_DOUBLE,
  LINE_HAIR,
  LINE_MEDIUM,
  LINE_MEDIUM_DASHED,
  LINE_MEDIUM_DASH_DOT,
  LINE_MEDIUM_DASH_DOT_DOT,
  LINE_NONE,
  LINE_SLANT_DASH_DOT,
  LINE_THICK,
  VALIGN_BOTTOM,
  VALIGN_CENTER,
  VALIGN_TOP,
  type TileRun,
  type TileStyle,
  type ViewBlock,
} from './view-block';
import { patternImage } from './patterns';

export const TILE_ROWS = 64;
export const TILE_COLS = 16;
const CELL_INSET = 2;
const INDENT_PX = 9;
/** Room a read-only ERP column's heading leaves before its text for the lock, at 100%. */
export const LOCK_PX = 11;
const ERP_STATES = CELL_ERP_MODIFIED | CELL_ERP_INVALID | CELL_ERP_FORMULA | CELL_ERP_LOCK;
const EMPTY_STYLE: TileStyle = {
  font: '',
  size: 11,
  fontFlags: 0,
  underline: 0,
  horizontal: ALIGN_LEFT,
  vertical: VALIGN_BOTTOM,
  indent: 0,
  rotation: 0,
  layout: 0,
  rightLine: LINE_NONE,
  bottomLine: LINE_NONE,
  diagonalLine: LINE_NONE,
  rightColor: 0,
  bottomColor: 0,
  diagonalColor: 0,
  pattern: 0,
  patternColor: 0,
};

export type TileContext = Pick<
  OffscreenCanvasRenderingContext2D,
  | 'arc'
  | 'beginPath'
  | 'clip'
  | 'closePath'
  | 'createLinearGradient'
  | 'createPattern'
  | 'ellipse'
  | 'fill'
  | 'globalAlpha'
  | 'strokeRect'
  | 'direction'
  | 'fillRect'
  | 'fillStyle'
  | 'fillText'
  | 'font'
  | 'lineTo'
  | 'lineWidth'
  | 'measureText'
  | 'moveTo'
  | 'rect'
  | 'restore'
  | 'rotate'
  | 'save'
  | 'setLineDash'
  | 'stroke'
  | 'strokeStyle'
  | 'textAlign'
  | 'textBaseline'
  | 'translate'
>;

/** A size in whole device pixels. Every cell rounds alone, so sizes add up exactly. */
export function devicePx(px: number, scale: number) {
  return Math.round(px * scale);
}

/** Device positions of a tile's lines: `xs[i]` is column i's left edge, `xs[cols]` its end. */
export function tileEdges(sizes: ArrayLike<number>, scale: number): Int32Array {
  const edges = new Int32Array(sizes.length + 1);
  for (let index = 0; index < sizes.length; index++) {
    edges[index + 1] = edges[index]! + devicePx(sizes[index]!, scale);
  }
  return edges;
}

/** `0xRRGGBBAA` as a CSS colour. */
export function cssColor(rgba: number): string {
  return '#' + (rgba >>> 0).toString(16).padStart(8, '0');
}

const hasAlpha = (rgba: number) => (rgba & 0xff) !== 0;

/** Whether an `0xRRGGBBAA` fill is pale, by its luma. */
const isLight = (rgba: number) =>
  (((rgba >>> 24) & 0xff) * 299 + ((rgba >>> 16) & 0xff) * 587 + ((rgba >>> 8) & 0xff) * 114) /
    1000 >
  140;

/** What a font falls back on. Calibri has Carlito, whose widths match. */
export function fontStack(name: string, palette: TilePalette): string {
  if (palette.fontOverride) return '"' + palette.fontOverride + '", ' + palette.fontFamily;
  const known = '"' + name.replace(/"/g, '') + '"';
  const lower = name.toLowerCase();
  if (!name) return 'Carlito, ' + palette.fontFamily;
  if (lower.startsWith('calibri') || lower.startsWith('aptos'))
    return known + ', Carlito, ' + palette.fontFamily;
  if (lower === 'cambria' || lower.includes('times') || lower === 'georgia')
    return known + ', Georgia, serif';
  if (lower.includes('courier') || lower === 'consolas') return known + ', monospace';
  return known + ', ' + palette.fontFamily;
}

interface FontMetrics {
  css: string;
  ascent: number;
  descent: number;
  px: number;
  /** How far superscript text rises, or subscript text falls, from the line's baseline. */
  shift: number;
}

interface RichPiece {
  run: TileRun;
  style: TileStyle;
  metrics: FontMetrics;
  text: string;
  width: number;
}

interface RichLine {
  pieces: RichPiece[];
  width: number;
  ascent: number;
  descent: number;
}

function lineOf(pieces: RichPiece[]): RichLine {
  let width = 0;
  let ascent = 0;
  let descent = 0;
  for (const piece of pieces) {
    width += piece.width;
    ascent = Math.max(ascent, piece.metrics.ascent);
    descent = Math.max(descent, piece.metrics.descent);
  }
  return { pieces, width, ascent, descent };
}

/**
 * Cell edges given instead of worked out from the tile's sizes, as pages draw them: each axis in
 * device units, with how many of those a pixel past the tile's edge takes. On a right-to-left
 * sheet `xs` runs from the tile's last column on the left to its first on the right.
 */
export interface TileEdges {
  xs: ArrayLike<number>;
  ys: ArrayLike<number>;
  perPixelX: number;
  perPixelY: number;
}

/** A tile's column sizes as they show from the left: reversed on a right-to-left sheet. */
export function visualColPx(block: ViewBlock, rightToLeft: boolean): ArrayLike<number> {
  return rightToLeft ? Array.from(block.colPx).reverse() : block.colPx;
}

/**
 * Everything drawing one tile needs. It lives as long as the drawing does.
 *
 * Columns here are counted as they show, from the left. On a right-to-left sheet that is from the
 * tile's last column, so text lays out and overflows by where cells are on screen, as Excel's
 * does: alignment isn't mirrored, but cells, borders, gridlines, data bars and notes' corners are.
 * Each cell draws its own right line, the line after it, at its right on screen in a left-to-right
 * sheet and at its left in a right-to-left one, so no line is needed from another tile.
 */
class Painter {
  private fonts = new Map<number, FontMetrics>();
  private currentFont = '';
  readonly xs: ArrayLike<number>;
  readonly ys: ArrayLike<number>;
  private readonly rtl: boolean;
  private readonly colPx: ArrayLike<number>;
  /** Device pixels a cell gives its gridline on its left and on its right. */
  private readonly gridLeft: number;
  private readonly gridRight: number;

  constructor(
    private readonly context: TileContext,
    private readonly block: ViewBlock,
    private readonly palette: TilePalette,
    private readonly scale: number,
    private readonly edges?: TileEdges,
  ) {
    this.rtl = palette.rightToLeft === true;
    this.colPx = visualColPx(block, this.rtl);
    this.xs = edges?.xs ?? tileEdges(this.colPx, scale);
    this.ys = edges?.ys ?? tileEdges(block.rowPx, scale);
    this.gridLeft = this.rtl ? 1 : 0;
    this.gridRight = this.rtl ? 0 : 1;
    context.textBaseline = 'alphabetic';
    context.direction = 'ltr';
  }

  /** The block's own column for a column on screen. */
  private logical(col: number) {
    return this.rtl ? this.block.cols - 1 - col : col;
  }

  private cell(row: number, col: number) {
    const at = (row * this.block.cols + this.logical(col)) * 6;
    const { cells } = this.block;
    const packed = cells[at + 4]!;
    return {
      textOffset: cells[at]!,
      textLength: cells[at + 1]!,
      fill: cells[at + 2]!,
      color: cells[at + 3]!,
      style: packed & 0xffff,
      flags: packed >>> 16,
      fillAt: cells[at + 5]!,
    };
  }

  private styleOf(index: number) {
    return this.block.styles[index] ?? EMPTY_STYLE;
  }

  private fontFor(index: number, style: TileStyle, shrink = 1): FontMetrics {
    const key = index * 1024 + Math.round(shrink * 1000);
    let metrics = this.fonts.get(key);
    if (!metrics) {
      const full = ((style.size * 96) / 72) * this.scale * shrink;
      // Superscript and subscript in two thirds of the size, raised a third or lowered a
      // seventh of it, as Excel draws them.
      const raised = style.fontFlags & FONT_SUPERSCRIPT;
      const lowered = style.fontFlags & FONT_SUBSCRIPT;
      const px = raised || lowered ? full * 0.66 : full;
      const shift = raised ? -full * 0.33 : lowered ? full * 0.14 : 0;
      const css =
        (style.fontFlags & FONT_ITALIC ? 'italic ' : '') +
        (style.fontFlags & FONT_BOLD ? 'bold ' : '') +
        px.toFixed(2) +
        'px ' +
        fontStack(style.font, this.palette);
      this.context.font = css;
      this.currentFont = css;
      const measured = this.context.measureText('Mg');
      metrics = {
        css,
        px,
        ascent: (measured.fontBoundingBoxAscent ?? px * 0.8) - Math.min(0, shift),
        descent: (measured.fontBoundingBoxDescent ?? px * 0.2) + Math.max(0, shift),
        shift,
      };
      this.fonts.set(key, metrics);
    }
    if (this.currentFont !== metrics.css) {
      this.context.font = metrics.css;
      this.currentFont = metrics.css;
    }
    return metrics;
  }

  private width(text: string) {
    return this.context.measureText(text).width;
  }

  drawBackground() {
    const { context, block, palette, xs, ys } = this;
    const pixel = Math.max(1, Math.round(this.scale));
    context.fillStyle = palette.background;
    context.fillRect(0, 0, xs[block.cols]!, ys[block.rows]!);
    // Fills hide gridlines, as in Excel.
    for (let row = 0; row < block.rows; row++) {
      const top = ys[row]!;
      const height = ys[row + 1]! - top;
      if (!height) continue;
      for (let col = 0; col < block.cols; col++) {
        const left = xs[col]!;
        const width = xs[col + 1]! - left;
        if (!width) continue;
        const { fill, flags, style } = this.cell(row, col);
        const pattern = this.styleOf(style).pattern;
        if (pattern) {
          context.fillStyle = hasAlpha(fill) ? cssColor(fill) : palette.background;
          context.fillRect(left, top, width, height);
          this.fillPattern(pattern, this.styleOf(style).patternColor, left, top, width, height);
          this.tint(flags, fill, left, top, width, height);
        } else if (hasAlpha(fill)) {
          context.fillStyle = cssColor(fill);
          context.fillRect(left, top, width, height);
          this.tint(flags, fill, left, top, width, height);
        } else if (palette.showGrid !== false) {
          this.tint(flags, fill, left, top, width, height);
          context.fillStyle = palette.gridline;
          if (!(flags & CELL_NO_RIGHT_GRID))
            context.fillRect(this.rtl ? left : left + width - pixel, top, pixel, height);
          if (!(flags & CELL_NO_BOTTOM_GRID))
            context.fillRect(left, top + height - pixel, width, pixel);
        } else {
          this.tint(flags, fill, left, top, width, height);
        }
      }
    }
  }

  /**
   * The colours of a cell's ERP states: the light ones on a pale fill whatever the theme, so they
   * show on a table's light bands in dark mode. None on pages, which never show states.
   */
  private erpColors(fill: number): ErpStateColors | null {
    const erp = this.palette.erp;
    if (!erp || this.edges) return null;
    if (hasAlpha(fill)) return isLight(fill) ? erp.light : erp.dark;
    return this.palette.dark ? erp.dark : erp.light;
  }

  /** A modified value's tint, or a read-only one's wash, under its text. */
  private tint(flags: number, fill: number, left: number, top: number, w: number, h: number) {
    if (!(flags & (CELL_ERP_MODIFIED | CELL_ERP_READ_ONLY))) return;
    const colors = this.erpColors(fill);
    if (!colors) return;
    this.context.fillStyle = flags & CELL_ERP_MODIFIED ? colors.modifiedTint : colors.readOnly;
    this.context.fillRect(left, top, w, h);
  }

  /** Room before a heading's text for a read-only column's lock. */
  private lockRoom(flags: number) {
    return flags & CELL_ERP_LOCK && this.palette.erp && !this.edges
      ? devicePx(LOCK_PX, this.scale)
      : 0;
  }

  /**
   * ERP states over the text: Modified's bar and a formula's dotted one on the cell's leading
   * edge, which is its right on a right-to-left sheet; Invalid's border; a read-only column's
   * lock in its heading. Never on pages.
   */
  drawStates() {
    const { context, block, xs, ys, scale } = this;
    if (!this.palette.erp || this.edges) return;
    // Most tiles hold no ERP data: a look at the packed flags, with nothing made per cell.
    let any = false;
    for (let at = 4; at < block.cells.length; at += 6) {
      if ((block.cells[at]! >>> 16) & ERP_STATES) {
        any = true;
        break;
      }
    }
    if (!any) return;
    const pixel = Math.max(1, Math.round(scale));
    const bar = Math.max(2, Math.round(3 * scale));
    for (let row = 0; row < block.rows; row++) {
      const top = ys[row]!;
      const height = ys[row + 1]! - top;
      if (!height) continue;
      for (let col = 0; col < block.cols; col++) {
        const { flags, fill, color } = this.cell(row, col);
        if (!(flags & ERP_STATES)) continue;
        const left = xs[col]!;
        const width = xs[col + 1]! - left;
        const colors = this.erpColors(fill);
        if (!width || !colors) continue;
        const lead = this.rtl ? left + width - bar : left;
        if (flags & CELL_ERP_FORMULA) {
          context.fillStyle = colors.formula;
          for (let y = top; y < top + height - pixel; y += 2 * bar)
            context.fillRect(lead, y, bar, Math.min(bar, top + height - pixel - y));
        } else if (flags & CELL_ERP_MODIFIED) {
          context.fillStyle = colors.modified;
          context.fillRect(lead, top, bar, height - pixel);
        }
        if (flags & CELL_ERP_INVALID) {
          const line = 2 * pixel;
          context.strokeStyle = colors.invalid;
          context.lineWidth = line;
          context.strokeRect(
            left + line / 2,
            top + line / 2,
            width - line - pixel,
            height - line - pixel,
          );
        }
        if (flags & CELL_ERP_LOCK) {
          this.drawLock(
            this.rtl ? left + width - devicePx(LOCK_PX, scale) : left,
            top,
            height,
            hasAlpha(color) ? cssColor(color) : this.palette.text,
          );
        }
      }
    }
  }

  /** A small padlock in the room a heading leaves before its text. */
  private drawLock(left: number, top: number, height: number, color: string) {
    const { context, scale } = this;
    const room = devicePx(LOCK_PX, scale);
    const body = Math.max(5, Math.round(7 * scale));
    const bodyHeight = Math.max(4, Math.round(5 * scale));
    const x = left + Math.round((room - body) / 2) + Math.round(scale);
    const y = top + Math.round((height - bodyHeight) / 2) + Math.round(2 * scale);
    const stroke = Math.max(1, Math.round(1.5 * scale));
    context.fillStyle = color;
    context.fillRect(x, y, body, bodyHeight);
    context.strokeStyle = color;
    context.lineWidth = stroke;
    context.beginPath();
    context.arc(x + body / 2, y, body / 2 - stroke / 2, Math.PI, 0);
    context.stroke();
  }

  /** Data bars and icons under the text, and notes' corners and invalid cells' rings over it. */
  drawMarks(over: boolean) {
    const { context, block, xs, ys } = this;
    const pixel = Math.max(1, Math.round(this.scale));
    for (const mark of block.marks) {
      const row = Math.floor(mark.cell / block.cols);
      const col = this.logical(mark.cell % block.cols);
      const left = xs[col]!;
      const top = ys[row]!;
      const width = xs[col + 1]! - left;
      const height = ys[row + 1]! - top;
      if (!width || !height) continue;
      if (!over && mark.kind === MARK_BAR) {
        const inset = 2 * pixel;
        const room = width - 2 * inset;
        // A right-to-left sheet's bars grow from the right, as Excel draws them.
        const edge = (share: number) =>
          this.rtl
            ? left + width - inset - Math.round(share * room)
            : left + inset + Math.round(share * room);
        const from = this.rtl ? edge(mark.end) : edge(mark.start);
        const to = this.rtl ? edge(mark.start) : edge(mark.end);
        const barTop = top + inset;
        const barHeight = Math.max(1, height - 2 * inset - pixel);
        const color = cssColor(mark.color);
        if (mark.flags & BAR_GRADIENT && to > from) {
          const gradient = this.rtl
            ? context.createLinearGradient(to, 0, from, 0)
            : context.createLinearGradient(from, 0, to, 0);
          gradient.addColorStop(0, color);
          gradient.addColorStop(1, cssColor((mark.color & 0xffffff00) | 0x18));
          context.fillStyle = gradient;
        } else {
          context.fillStyle = color;
        }
        context.fillRect(from, barTop, Math.max(0, to - from), barHeight);
        if (mark.flags & BAR_BORDER && to > from) {
          context.strokeStyle = cssColor(mark.second || mark.color);
          context.lineWidth = pixel;
          context.strokeRect(
            from + pixel / 2,
            barTop + pixel / 2,
            to - from - pixel,
            barHeight - pixel,
          );
        }
        if (mark.flags & BAR_AXIS) {
          const axis = this.rtl ? edge(mark.start) - pixel : edge(mark.start);
          context.fillStyle = cssColor(mark.second || 0x000000ff);
          for (let y = top; y < top + height; y += 3 * pixel)
            context.fillRect(axis, y, pixel, pixel);
        }
      } else if (!over && mark.kind === MARK_ICON) {
        const size = Math.min(height - 2 * pixel, Math.round(16 * this.scale));
        drawIcon(context, mark.icon, left + 2 * pixel, top + (height - size) / 2, size);
      } else if (over && (mark.kind === MARK_NOTE || mark.kind === MARK_THREAD) && !this.edges) {
        // Notes' corners show on screen only: Excel never prints them.
        const size = 5 * pixel;
        context.fillStyle = mark.kind === MARK_NOTE ? '#d00000' : '#7b45b8';
        // In the cell's top corner after its text: its left on a right-to-left sheet.
        const corner = this.rtl ? left : left + width;
        const inward = this.rtl ? size : -size;
        context.beginPath();
        context.moveTo(corner + inward, top);
        context.lineTo(corner, top);
        context.lineTo(corner, top + size);
        context.closePath();
        context.fill();
      } else if (over && mark.kind === MARK_ERROR && !this.edges) {
        // Excel's green triangle, which never prints either.
        const size = 5 * pixel;
        context.fillStyle = '#008a00';
        const corner = this.rtl ? left + width : left;
        const inward = this.rtl ? -size : size;
        context.beginPath();
        context.moveTo(corner + inward, top);
        context.lineTo(corner, top);
        context.lineTo(corner, top + size);
        context.closePath();
        context.fill();
      } else if (over && mark.kind === MARK_INVALID) {
        context.strokeStyle = '#e00000';
        context.lineWidth = 1.5 * pixel;
        context.beginPath();
        context.ellipse(
          left + width / 2,
          top + height / 2,
          width / 2,
          height / 2 + pixel,
          0,
          0,
          Math.PI * 2,
        );
        context.stroke();
      }
    }
  }

  /** One of Excel's patterns, lined up with the tile so that neighbours join. */
  private fillPattern(
    pattern: number,
    color: number,
    left: number,
    top: number,
    width: number,
    height: number,
  ) {
    const { context, scale } = this;
    const image = patternImage(pattern, cssColor(color), Math.max(1, Math.round(scale)));
    const fill = image && context.createPattern?.(image, 'repeat');
    if (!fill) return;
    context.fillStyle = fill;
    context.fillRect(left, top, width, height);
  }

  /** Is this cell free for text to overflow across? */
  private free(row: number, col: number) {
    const { textLength, flags } = this.cell(row, col);
    return !textLength && !(flags & (CELL_MERGED | CELL_OCCUPIED | CELL_HASHES));
  }

  /** Clears the gridlines text runs across, between these columns of a row. */
  private clearGrid(row: number, fromCol: number, toCol: number) {
    const { context, palette, xs, ys } = this;
    if (palette.showGrid === false) return;
    const pixel = Math.max(1, Math.round(this.scale));
    const top = ys[row]!;
    const height = ys[row + 1]! - top - pixel;
    if (height <= 0) return;
    for (let col = fromCol; col < toCol; col++) {
      // The line between two cells is drawn inside the one it belongs to.
      const { fill } = this.cell(row, this.rtl ? col + 1 : col);
      context.fillStyle = hasAlpha(fill) ? cssColor(fill) : palette.background;
      context.fillRect(this.rtl ? xs[col + 1]! : xs[col + 1]! - pixel, top, pixel, height);
    }
  }

  drawCellsText() {
    const { block, xs, ys } = this;
    for (let row = 0; row < block.rows; row++) {
      if (ys[row + 1] === ys[row]) continue;
      for (let col = 0; col < block.cols; col++) {
        if (xs[col + 1] === xs[col]) continue;
        const { textOffset, textLength, flags, style, color, fillAt } = this.cell(row, col);
        if (!textLength && !(flags & CELL_HASHES)) continue;
        this.drawText(
          cellText(block, row, this.logical(col)),
          { row, col, flags, style, color, fillAt, runs: block.runs.get(textOffset) },
          xs[col]!,
          ys[row]!,
          xs[col + 1]! - xs[col]!,
          ys[row + 1]! - ys[row]!,
        );
      }
    }
  }

  /** A CSS position, as device pixels from the tile's corner. Lines inside the tile are exact. */
  /**
   * How tall a cell's text may draw. On paper, text may hang a quarter of the row below its cell,
   * as the font a page draws in can set marks lower than the one Excel measured with: Noto's dots
   * under a final yeh fall below a 15-point row.
   */
  private hang(top: number, bottom: number) {
    return this.edges ? (bottom - top) * 1.25 : bottom - top;
  }

  private mapped(css: number, sizes: ArrayLike<number>, edges: ArrayLike<number>) {
    const perPixel =
      edges === this.xs
        ? this.edges?.perPixelX
        : edges === this.ys
          ? this.edges?.perPixelY
          : undefined;
    const beyond = (px: number) =>
      perPixel === undefined ? devicePx(px, this.scale) : px * perPixel;
    if (css <= 0) return beyond(css);
    let total = 0;
    for (let index = 0; index < sizes.length; index++) {
      if (total === css) return edges[index]!;
      total += sizes[index]!;
    }
    return edges[sizes.length]! + beyond(css - total);
  }

  drawExtras() {
    const { block, xs, ys, context } = this;
    for (const extra of block.extras) {
      // The extra's place from the tile's left on screen.
      const x = this.rtl ? sum(block.colPx) - extra.x - extra.width : extra.x;
      const left = this.mapped(x, this.colPx, xs);
      const top = this.mapped(extra.y, block.rowPx, ys);
      const right = this.mapped(x + extra.width, this.colPx, xs);
      const bottom = this.mapped(extra.y + extra.height, block.rowPx, ys);
      if (extra.kind === 1) {
        this.drawText(
          extra.text,
          {
            row: -1,
            col: -1,
            flags: extra.flags,
            style: extra.style,
            color: extra.color,
            fillAt: extra.fillAt,
            runs: block.runs.get(extra.textOffset),
          },
          left,
          top,
          right - left,
          bottom - top,
        );
        continue;
      }
      // Overflow from outside the tile: how far does it reach in?
      const rowIndex = this.rowAt(extra.y);
      if (rowIndex < 0) continue;
      const style = this.styleOf(extra.style);
      const metrics = this.fontFor(extra.style, style);
      const line = singleLine(extra.text);
      const runs = block.runs.get(extra.textOffset);
      const rich = runs ? this.richLine(runs) : undefined;
      if (!rich) this.fontFor(extra.style, style);
      const width = rich ? rich.width : this.width(line);
      const inset = devicePx(CELL_INSET, this.scale);
      const fromLeft = x < 0;
      const start = this.textStart(style, left, right, width, inset);
      const end = start + width;
      if (fromLeft ? end <= 0 : start >= xs[block.cols]!) continue;
      // Stop at the first occupied cell the text would meet.
      let clipLeft = 0;
      let clipRight = xs[block.cols]!;
      if (fromLeft) {
        let col = 0;
        while (col < block.cols && this.free(rowIndex, col) && xs[col]! < end) col++;
        clipRight = xs[col]!;
        this.clearGrid(rowIndex, 0, Math.max(0, col - 1));
        if (col > 0 && xs[col - 1]! < end && end > xs[col]!) {
          this.clearGrid(rowIndex, col - 1, col - 1);
        }
      } else {
        let col = block.cols - 1;
        while (col >= 0 && this.free(rowIndex, col) && xs[col + 1]! > start) col--;
        clipLeft = xs[col + 1]! + (col >= 0 ? this.gridLeft : 0);
        this.clearGrid(rowIndex, col + 1, block.cols - 1);
      }
      if (clipRight <= clipLeft) continue;
      context.save();
      context.beginPath();
      context.rect(clipLeft, top, clipRight - clipLeft, this.hang(top, bottom));
      context.clip();
      if (rich) this.paintRichLine(rich, style, extra.flags, start, top, bottom);
      else
        this.paintLine(line, style, metrics, extra.color, extra.flags, start, width, top, bottom);
      context.restore();
    }
  }

  /** Each run of a line, measured. A line is as tall as its tallest font. */
  private richPieces(runs: TileRun[]) {
    return runs.map((run) => {
      const style = this.styleOf(run.style);
      const metrics = this.fontFor(run.style, style);
      return { run, style, metrics, text: run.text, width: 0 };
    });
  }

  private richLine(runs: TileRun[]): RichLine {
    const pieces: RichPiece[] = [];
    for (const piece of this.richPieces(runs)) {
      const text = singleLine(piece.text);
      this.fontFor(piece.run.style, piece.style);
      pieces.push({ ...piece, text, width: this.width(text) });
    }
    return lineOf(pieces);
  }

  /** Runs broken into words, then into lines that fit. */
  private richLines(runs: TileRun[], room: number): RichLine[] {
    const lines: RichPiece[][] = [[]];
    let used = 0;
    for (const piece of this.richPieces(runs)) {
      this.fontFor(piece.run.style, piece.style);
      for (const word of piece.text.split(/(\r?\n|\s+)/)) {
        if (!word) continue;
        if (word === '\n' || word === '\r\n') {
          lines.push([]);
          used = 0;
          continue;
        }
        const width = this.width(word);
        const line = lines[lines.length - 1]!;
        if (used + width > room && line.length) {
          // Spaces at a break go nowhere.
          if (!word.trim()) continue;
          lines.push([]);
          used = 0;
        }
        lines[lines.length - 1]!.push({ ...piece, text: word, width });
        used += width;
      }
    }
    return lines.map(lineOf);
  }

  /** Reading order runs from the right in right-to-left text. */
  private paintRichLine(
    line: RichLine,
    style: TileStyle,
    flags: number,
    start: number,
    top: number,
    bottom: number,
    y?: number,
  ) {
    const baseline = y ?? this.baseline(style, line, top, bottom);
    const pieces = flags & CELL_RTL ? [...line.pieces].reverse() : line.pieces;
    let x = start;
    for (const piece of pieces) {
      const metrics = this.fontFor(piece.run.style, piece.style);
      this.paintLine(
        piece.text,
        piece.style,
        metrics,
        piece.run.color,
        flags,
        x,
        piece.width,
        top,
        bottom,
        baseline,
      );
      x += piece.width;
    }
  }

  private drawRichWrapped(
    runs: TileRun[],
    style: TileStyle,
    flags: number,
    left: number,
    top: number,
    right: number,
    bottom: number,
    room: number,
    inset: number,
  ) {
    const { context } = this;
    const lines = this.richLines(runs, room);
    const total = lines.reduce((sum, line) => sum + line.ascent + line.descent, 0);
    let y: number;
    if (style.vertical === VALIGN_CENTER) y = (top + bottom - total) / 2;
    else if (style.vertical === VALIGN_BOTTOM)
      y = bottom - total - Math.max(1, Math.round(this.scale));
    else y = top;
    context.save();
    context.beginPath();
    context.rect(left + this.gridLeft, top, right - left - 1, this.hang(top, bottom));
    context.clip();
    for (const line of lines) {
      const start = this.textStart(style, left, right, line.width, inset);
      this.paintRichLine(line, style, flags, start, top, bottom, y + line.ascent);
      y += line.ascent + line.descent;
    }
    context.restore();
  }

  private rowAt(css: number) {
    let total = 0;
    const { rowPx } = this.block;
    for (let row = 0; row < rowPx.length; row++) {
      if (total === css) return row;
      total += rowPx[row]!;
    }
    return -1;
  }

  /** Where a line of this width starts in a cell, by its alignment. */
  private textStart(
    style: TileStyle,
    left: number,
    right: number,
    width: number,
    inset: number,
    flags = 0,
  ) {
    const indent = devicePx(style.indent * INDENT_PX, this.scale) + this.lockRoom(flags);
    switch (style.horizontal) {
      case ALIGN_RIGHT:
        return right - inset - indent - width - this.gridRight;
      case ALIGN_CENTER:
      case ALIGN_CENTER_CONTINUOUS:
      case ALIGN_DISTRIBUTED:
        return (left + right - width) / 2;
      default:
        return left + inset + indent + this.gridLeft;
    }
  }

  private baseline(
    style: TileStyle,
    metrics: { ascent: number; descent: number },
    top: number,
    bottom: number,
  ) {
    const pixel = Math.max(1, Math.round(this.scale));
    switch (style.vertical) {
      case VALIGN_TOP:
        return top + metrics.ascent;
      case VALIGN_CENTER:
        return (top + bottom - metrics.ascent - metrics.descent) / 2 + metrics.ascent;
      case VALIGN_BOTTOM:
      default:
        return bottom - metrics.descent - pixel;
    }
  }

  private paintLine(
    line: string,
    style: TileStyle,
    metrics: FontMetrics,
    color: number,
    flags: number,
    start: number,
    width: number,
    top: number,
    bottom: number,
    y?: number,
  ) {
    const { context, palette, scale } = this;
    const baseline = (y ?? this.baseline(style, metrics, top, bottom)) + metrics.shift;
    context.fillStyle = hasAlpha(color) ? cssColor(color) : palette.text;
    // Set every time: `restore` puts back whatever direction its `save` saw.
    context.direction = flags & CELL_RTL ? 'rtl' : 'ltr';
    context.textAlign = 'left';
    context.fillText(line, start, baseline);
    if (style.underline) {
      const weight = Math.max(1, Math.round(metrics.px / 14));
      const under = baseline + Math.max(1, Math.round(1.5 * scale));
      context.fillRect(start, under, width, weight);
      if (style.underline === 2 || style.underline === 4) {
        context.fillRect(start, under + weight + Math.max(1, Math.round(scale)), width, weight);
      }
    }
    if (style.fontFlags & FONT_STRIKE) {
      const weight = Math.max(1, Math.round(metrics.px / 14));
      context.fillRect(start, baseline - metrics.px * 0.3, width, weight);
    }
  }

  private drawText(
    raw: string,
    cell: {
      row: number;
      col: number;
      flags: number;
      style: number;
      color: number;
      fillAt: number;
      runs?: TileRun[] | undefined;
    },
    left: number,
    top: number,
    width: number,
    height: number,
  ) {
    const { context, block, xs, scale } = this;
    const style = this.styleOf(cell.style);
    let right = left + width;
    const bottom = top + height;
    // Centred across a selection: over the empty cells after it that say the same, which are
    // to its left on a right-to-left sheet.
    if (style.horizontal === ALIGN_CENTER_CONTINUOUS && cell.row >= 0) {
      const step = this.rtl ? -1 : 1;
      let last = cell.col;
      while (
        last + step >= 0 &&
        last + step < block.cols &&
        this.free(cell.row, last + step) &&
        this.styleOf(this.cell(cell.row, last + step).style).horizontal === ALIGN_CENTER_CONTINUOUS
      ) {
        last += step;
      }
      if (this.rtl) left = xs[last]!;
      else right = xs[last + 1]!;
    }
    const inset = devicePx(CELL_INSET, scale);
    const indent = devicePx(style.indent * INDENT_PX, scale) + this.lockRoom(cell.flags);
    const room = Math.max(1, right - left - inset * 2 - indent - 1);
    const kind = cell.flags & KIND_MASK;
    const wrap =
      (style.layout & LAYOUT_WRAP) !== 0 ||
      style.horizontal === ALIGN_JUSTIFY ||
      style.horizontal === ALIGN_DISTRIBUTED;
    let metrics = this.fontFor(cell.style, style);

    if (cell.flags & CELL_HASHES) {
      this.paintHashes(style, metrics, cell, left, right, top, bottom, room);
      return;
    }

    if (style.rotation) {
      this.drawRotated(raw, style, metrics, cell, left, top, width, height);
      return;
    }

    if (wrap && kind === KIND_TEXT) {
      if (cell.runs) {
        const { flags } = cell;
        this.drawRichWrapped(cell.runs, style, flags, left, top, right, bottom, room, inset);
        return;
      }
      this.drawWrapped(raw, style, metrics, cell, left, top, right, bottom, room, inset);
      return;
    }

    // Rich text on one line. It can't shrink or repeat, which only its plain text does.
    const rich =
      cell.runs && kind === KIND_TEXT && !(style.layout & LAYOUT_SHRINK) && !cell.fillAt
        ? this.richLine(cell.runs)
        : undefined;

    let line = singleLine(raw);
    let shrink = 1;
    if (style.layout & LAYOUT_SHRINK) {
      const full = this.width(line);
      if (full > room) {
        shrink = Math.max(0.1, room / full);
        metrics = this.fontFor(cell.style, style, shrink);
      }
    }
    if (cell.fillAt) {
      line = this.expandFill(line, cell.fillAt - 1, room);
    } else if (style.horizontal === ALIGN_FILL && line) {
      const one = this.width(line);
      if (one > 0 && one < room) line = line.repeat(Math.floor(room / one));
    }

    // Worth measuring only when the text could be too wide.
    const textWidth =
      line.length * metrics.px * 1.1 <= room ||
      (style.horizontal === ALIGN_LEFT && kind === KIND_TEXT && !style.underline)
        ? null
        : this.width(line);
    const isNumber = kind === KIND_NUMBER || kind === KIND_GENERAL;
    if (isNumber && textWidth !== null && textWidth > room) {
      if (kind === KIND_GENERAL) {
        const short = shortenGeneral(line, (candidate) => this.width(candidate) <= room);
        if (short === null) {
          this.paintHashes(style, metrics, cell, left, right, top, bottom, room);
          return;
        }
        line = short;
      } else {
        this.paintHashes(style, metrics, cell, left, right, top, bottom, room);
        return;
      }
    }

    // Measuring rich text left another font current.
    if (!rich) metrics = this.fontFor(cell.style, style, shrink);
    const measured = rich ? rich.width : this.width(line);
    const start = this.textStart(style, left, right, measured, inset, cell.flags);
    let clipLeft = left + this.gridLeft;
    let clipRight = right - this.gridRight;
    const overflowEnd = left + width;
    const canOverflow =
      kind === KIND_TEXT && cell.row >= 0 && !cell.fillAt && style.horizontal !== ALIGN_FILL;
    if (canOverflow && (start < left || start + measured > overflowEnd)) {
      const end = start + measured;
      let last = cell.col;
      while (last + 1 < block.cols && xs[last + 1]! < end && this.free(cell.row, last + 1)) {
        last++;
      }
      let first = cell.col;
      while (first > 0 && xs[first]! > start && this.free(cell.row, first - 1)) first--;
      clipLeft = first === 0 ? xs[0]! : xs[first]! + this.gridLeft;
      // Text reaching the tile's edge carries on in the next tile.
      clipRight = last === block.cols - 1 ? xs[block.cols]! : xs[last + 1]! - this.gridRight;
      if (first < last) this.clearGrid(cell.row, first, last);
    }
    context.save();
    context.beginPath();
    context.rect(clipLeft, top, clipRight - clipLeft, this.hang(top, top + height));
    context.clip();
    if (rich) this.paintRichLine(rich, style, cell.flags, start, top, bottom);
    else this.paintLine(line, style, metrics, cell.color, cell.flags, start, measured, top, bottom);
    context.restore();
  }

  private paintHashes(
    style: TileStyle,
    metrics: FontMetrics,
    cell: { flags: number; color: number },
    left: number,
    right: number,
    top: number,
    bottom: number,
    room: number,
  ) {
    const { context } = this;
    const one = this.width('#');
    const hashes = '#'.repeat(Math.max(1, Math.floor(room / one)));
    const width = this.width(hashes);
    context.save();
    context.beginPath();
    context.rect(left, top, right - left, bottom - top);
    context.clip();
    const plain = { ...style, underline: 0, fontFlags: style.fontFlags & ~FONT_STRIKE };
    this.paintLine(
      hashes,
      plain,
      metrics,
      cell.color,
      cell.flags & ~CELL_RTL,
      (left + right - width) / 2,
      width,
      top,
      bottom,
    );
    context.restore();
  }

  /** Repeats the `*x` character at this byte position until the line fills the room. */
  private expandFill(line: string, byteAt: number, room: number) {
    const bytes = new TextEncoder().encode(line);
    const decoder = new TextDecoder();
    const before = decoder.decode(bytes.subarray(0, byteAt));
    const rest = decoder.decode(bytes.subarray(byteAt));
    const fill = [...rest][0] ?? ' ';
    const after = rest.slice(fill.length);
    const one = this.width(fill);
    const spare = room - this.width(before + after);
    const count = one > 0 ? Math.max(0, Math.floor(spare / one)) : 0;
    return before + fill.repeat(count) + after;
  }

  private drawWrapped(
    raw: string,
    style: TileStyle,
    metrics: FontMetrics,
    cell: { flags: number; color: number },
    left: number,
    top: number,
    right: number,
    bottom: number,
    room: number,
    inset: number,
  ) {
    const { context } = this;
    const lines = wrapLines(raw, (candidate) => this.width(candidate) <= room);
    const lineHeight = metrics.ascent + metrics.descent;
    const blockHeight = lineHeight * lines.length;
    let y: number;
    switch (style.vertical) {
      case VALIGN_TOP:
        y = top + metrics.ascent;
        break;
      case VALIGN_CENTER:
        y = (top + bottom - blockHeight) / 2 + metrics.ascent;
        break;
      case VALIGN_BOTTOM:
        y = bottom - blockHeight + metrics.ascent - Math.max(1, Math.round(this.scale));
        break;
      default:
        y = top + metrics.ascent;
    }
    context.save();
    context.beginPath();
    context.rect(left + this.gridLeft, top, right - left - 1, this.hang(top, bottom));
    context.clip();
    for (const line of lines) {
      const width = this.width(line);
      const start = this.textStart(style, left, right, width, inset);
      this.paintLine(line, style, metrics, cell.color, cell.flags, start, width, top, bottom, y);
      y += lineHeight;
    }
    context.restore();
  }

  private drawRotated(
    raw: string,
    style: TileStyle,
    metrics: FontMetrics,
    cell: { flags: number; color: number },
    left: number,
    top: number,
    width: number,
    height: number,
  ) {
    const { context, palette, scale } = this;
    const line = singleLine(raw);
    const lineHeight = metrics.ascent + metrics.descent;
    const inset = devicePx(CELL_INSET, scale);
    /** The middle of a box this size, placed in the cell by the style's alignment. */
    const middle = (boxWidth: number, boxHeight: number) => {
      const x =
        style.horizontal === ALIGN_LEFT
          ? left + inset + boxWidth / 2
          : style.horizontal === ALIGN_RIGHT
            ? left + width - inset - boxWidth / 2
            : left + width / 2;
      const y =
        style.vertical === VALIGN_TOP
          ? top + boxHeight / 2
          : style.vertical === VALIGN_CENTER
            ? top + height / 2
            : top + height - boxHeight / 2 - Math.max(1, Math.round(scale));
      return { x, y };
    };
    context.save();
    context.beginPath();
    context.rect(left + this.gridLeft, top, width - 1, this.hang(top, top + height));
    context.clip();
    context.fillStyle = hasAlpha(cell.color) ? cssColor(cell.color) : palette.text;
    context.direction = cell.flags & CELL_RTL ? 'rtl' : 'ltr';
    context.textAlign = 'center';
    if (style.rotation === 255) {
      // Stacked: one character under another.
      const letters = [...line];
      const { x, y } = middle(metrics.px, lineHeight * letters.length);
      let baseline = y - (lineHeight * letters.length) / 2 + metrics.ascent;
      for (const ch of letters) {
        context.fillText(ch, x, baseline);
        baseline += lineHeight;
      }
    } else {
      // A right-to-left sheet turns text the other way, as Excel does.
      const turned = style.rotation <= 90 ? style.rotation : 90 - style.rotation;
      const angle = this.rtl ? -turned : turned;
      const radians = (angle * Math.PI) / 180;
      const length = this.width(line);
      const { x, y } = middle(
        Math.abs(Math.cos(radians)) * length + Math.abs(Math.sin(radians)) * lineHeight,
        Math.abs(Math.sin(radians)) * length + Math.abs(Math.cos(radians)) * lineHeight,
      );
      context.translate(x, y);
      context.rotate(-radians);
      context.fillText(line, 0, (metrics.ascent - metrics.descent) / 2);
    }
    context.textAlign = 'left';
    context.restore();
  }

  drawBorders() {
    const { block, xs, ys } = this;
    for (let row = 0; row < block.rows; row++) {
      const top = ys[row]!;
      const bottom = ys[row + 1]!;
      if (top === bottom) continue;
      for (let col = 0; col < block.cols; col++) {
        const left = xs[col]!;
        const right = xs[col + 1]!;
        if (left === right) continue;
        const { style: index } = this.cell(row, col);
        const style = this.styleOf(index);
        if (style.rightLine) {
          // The line after the cell, which is on its left on a right-to-left sheet.
          const x = this.rtl ? left : right;
          this.drawLine(style.rightLine, style.rightColor, x, top, x, bottom, true);
        }
        if (style.bottomLine) {
          this.drawLine(style.bottomLine, style.bottomColor, left, bottom, right, bottom, false);
        }
        if (style.diagonalLine) this.drawDiagonal(style, left, top, right, bottom);
      }
    }
  }

  /**
   * A line along a cell's right or bottom edge, kept inside the cell so tiles can't cut it. On a
   * right-to-left sheet a vertical line is on the cell's left edge, inside it to its right.
   */
  private drawLine(
    kind: number,
    color: number,
    x1: number,
    y1: number,
    x2: number,
    y2: number,
    vertical: boolean,
  ) {
    const { context, palette, scale } = this;
    const pixel = Math.max(1, Math.round(scale));
    let weight = pixel;
    if (kind === LINE_MEDIUM || kind === LINE_MEDIUM_DASHED) weight = pixel * 2;
    if (kind === LINE_MEDIUM_DASH_DOT || kind === LINE_MEDIUM_DASH_DOT_DOT) weight = pixel * 2;
    if (kind === LINE_SLANT_DASH_DOT) weight = pixel * 2;
    if (kind === LINE_THICK) weight = pixel * 3;
    const css = hasAlpha(color) ? cssColor(color) : palette.text;
    // Where a vertical line of this weight starts, inside its cell.
    const inside = (size: number) => (this.rtl ? x1 : x1 - size);
    if (kind === LINE_DOUBLE) {
      context.fillStyle = css;
      if (vertical) {
        context.fillRect(inside(pixel), y1, pixel, y2 - y1);
        context.fillRect(this.rtl ? x1 + pixel * 2 : x1 - pixel * 3, y1, pixel, y2 - y1);
      } else {
        context.fillRect(x1, y1 - pixel, x2 - x1, pixel);
        context.fillRect(x1, y1 - pixel * 3, x2 - x1, pixel);
      }
      return;
    }
    const dash = lineDash(kind, pixel);
    if (!dash) {
      context.fillStyle = css;
      if (vertical) context.fillRect(inside(weight), y1, weight, y2 - y1);
      else context.fillRect(x1, y1 - weight, x2 - x1, weight);
      return;
    }
    context.save();
    context.strokeStyle = css;
    context.lineWidth = weight;
    context.setLineDash(dash);
    context.beginPath();
    const offset = weight / 2;
    if (vertical) {
      const x = this.rtl ? x1 + offset : x1 - offset;
      context.moveTo(x, y1);
      context.lineTo(x, y2);
    } else {
      context.moveTo(x1, y1 - offset);
      context.lineTo(x2, y2 - offset);
    }
    context.stroke();
    context.restore();
  }

  private drawDiagonal(style: TileStyle, left: number, top: number, right: number, bottom: number) {
    const { context, palette, scale } = this;
    const pixel = Math.max(1, Math.round(scale));
    context.save();
    context.strokeStyle = hasAlpha(style.diagonalColor)
      ? cssColor(style.diagonalColor)
      : palette.text;
    context.lineWidth = style.diagonalLine === LINE_THICK ? pixel * 3 : pixel;
    context.setLineDash(lineDash(style.diagonalLine, pixel) ?? []);
    context.beginPath();
    if (style.layout & LAYOUT_DIAGONAL_DOWN) {
      context.moveTo(left, top);
      context.lineTo(right, bottom);
    }
    if (style.layout & LAYOUT_DIAGONAL_UP) {
      context.moveTo(left, bottom);
      context.lineTo(right, top);
    }
    context.stroke();
    context.restore();
  }
}

function sum(values: ArrayLike<number>) {
  let total = 0;
  for (let index = 0; index < values.length; index++) total += values[index]!;
  return total;
}

function lineDash(kind: number, pixel: number): number[] | null {
  switch (kind) {
    case LINE_HAIR:
    case LINE_DOTTED:
      return [pixel, pixel];
    case LINE_DASHED:
    case LINE_MEDIUM_DASHED:
      return [pixel * 4, pixel * 2];
    case LINE_DASH_DOT:
    case LINE_MEDIUM_DASH_DOT:
    case LINE_SLANT_DASH_DOT:
      return [pixel * 4, pixel * 2, pixel, pixel * 2];
    case LINE_DASH_DOT_DOT:
    case LINE_MEDIUM_DASH_DOT_DOT:
      return [pixel * 4, pixel * 2, pixel, pixel * 2, pixel, pixel * 2];
    default:
      return null;
  }
}

export function drawTile(
  context: TileContext,
  block: ViewBlock,
  palette: TilePalette,
  scale: number,
  edges?: TileEdges,
) {
  const painter = new Painter(context, block, palette, scale, edges);
  painter.drawBackground();
  painter.drawMarks(false);
  painter.drawCellsText();
  painter.drawExtras();
  painter.drawBorders();
  painter.drawMarks(true);
  painter.drawStates();
}

export { ALIGN_CENTER };
