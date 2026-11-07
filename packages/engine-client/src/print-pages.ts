// Draws a sheet's pages into a recording: the cells with the tile drawing code, in the file's own
// colours, with title rows and columns, headings, gridlines, drawings, and the header and footer.
// `crates/cgs-engine/src/print.rs` says where pages break; this only draws what each page shows.
import { drawTile, TILE_COLS, TILE_ROWS } from './tile-draw';
import { RecordingContext, type Matrix, type PdfOp, type Recording } from './recording';
import type { SheetLayout } from './layout';
import type { ViewBlock } from './view-block';
import { TILE_NOT_READY } from './view-block';
import type { DrawingBox, HfRun, HfSections, PageInfo, PagesInfo, TilePalette } from './types';

export interface RecordedPage {
  width: number;
  height: number;
  ops: PdfOp[];
}

/** What `&F`, `&Z`, `&D` and `&T` show. */
export interface PageFields {
  file: string;
  path: string;
  date: string;
  time: string;
}

export interface PrintSource {
  /** A tile of the sheet, in the file's own colours, by its first row and column from 0. */
  block(sheet: number, firstRow: number, firstCol: number): Promise<ViewBlock>;
}

export interface SheetPrint {
  info: PagesInfo;
  layout: SheetLayout;
  drawings: DrawingBox[];
}

export interface PrintOptions {
  recording: Recording;
  /** A real context, for measuring text. */
  measure: OffscreenCanvasRenderingContext2D;
  /** The screen's fallback fonts, so text fits as it does there. */
  fontFamily: string;
  fields: PageFields;
  /** The first page's number when the sheet doesn't set its own. */
  firstNumber: number;
  /** Pages in the whole print, for `&N`. */
  total: number;
  /** Draws a chart at the context's origin, `drawing.width` by `drawing.height` pixels. */
  drawChart?: (context: RecordingContext, drawing: DrawingBox) => Promise<void>;
  /** A picture's index in the recording, by its part. */
  picture?: (part: string) => Promise<number | null>;
  /** Changes a font for measuring, as when Arabic will be drawn in the bundled font. */
  measuring?: (css: string) => string;
}

/** Excel prints gridlines thin and dark grey. */
const PRINT_GRIDLINE = '#a0a0a0';
const POINT = 0.75;

/** Sizes along one axis, from the layout's runs, with sums over stretches. */
export class Axis {
  constructor(
    private readonly runs: Uint32Array,
    private readonly fallback: number,
  ) {}

  size(index: number): number {
    const runs = this.runs;
    let low = 0;
    let high = runs.length / 3 - 1;
    while (low <= high) {
      const mid = (low + high) >> 1;
      const first = runs[mid * 3]!;
      const last = runs[mid * 3 + 1]!;
      if (index < first) high = mid - 1;
      else if (index > last) low = mid + 1;
      else return runs[mid * 3 + 2]!;
    }
    return this.fallback;
  }

  /** The sum of `first` to `last`, inclusive. */
  sum(first: number, last: number): number {
    if (last < first) return 0;
    let total = (last - first + 1) * this.fallback;
    const runs = this.runs;
    for (let at = 0; at < runs.length; at += 3) {
      const a = Math.max(first, runs[at]!);
      const b = Math.min(last, runs[at + 1]!);
      if (a <= b) total += (b - a + 1) * (runs[at + 2]! - this.fallback);
      if (runs[at]! > last) break;
    }
    return total;
  }

  /** How far `to` starts from where `from` starts: negative before it. */
  offset(from: number, to: number): number {
    return to >= from ? this.sum(from, to - 1) : -this.sum(to, from - 1);
  }
}

function columnName(col: number): string {
  let name = '';
  let n = col;
  while (n > 0) {
    const rest = (n - 1) % 26;
    name = String.fromCharCode(65 + rest) + name;
    n = Math.floor((n - 1) / 26);
  }
  return name;
}

class Blocks {
  private readonly cache = new Map<string, Promise<ViewBlock>>();
  constructor(
    private readonly source: PrintSource,
    private readonly sheet: number,
  ) {}

  get(firstRow: number, firstCol: number): Promise<ViewBlock> {
    const key = firstRow + ':' + firstCol;
    let block = this.cache.get(key);
    if (!block) {
      block = this.read(firstRow, firstCol);
      this.cache.set(key, block);
    }
    return block;
  }

  /** Waits for a tile that is still calculating, as a printout must show the answers. */
  private async read(firstRow: number, firstCol: number): Promise<ViewBlock> {
    for (let attempt = 0; ; attempt++) {
      const block = await this.source.block(this.sheet, firstRow, firstCol);
      if (!(block.flags & TILE_NOT_READY) || attempt >= 200) return block;
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }
}

/** Draws cells `rows` by `cols` with their top left at `x`, `y` in the context's pixels. */
async function drawCells(
  context: RecordingContext,
  blocks: Blocks,
  rowAxis: Axis,
  colAxis: Axis,
  palette: TilePalette,
  rows: [number, number],
  cols: [number, number],
  x: number,
  y: number,
) {
  const width = colAxis.sum(cols[0], cols[1]);
  const height = rowAxis.sum(rows[0], rows[1]);
  if (!width || !height) return;
  context.save();
  context.beginPath();
  context.rect(x, y, width, height);
  context.clip();
  for (
    let tr = Math.floor((rows[0] - 1) / TILE_ROWS);
    tr <= Math.floor((rows[1] - 1) / TILE_ROWS);
    tr++
  ) {
    for (
      let tc = Math.floor((cols[0] - 1) / TILE_COLS);
      tc <= Math.floor((cols[1] - 1) / TILE_COLS);
      tc++
    ) {
      // Tiles are addressed from 0, and rows and columns here from 1.
      const firstRow = tr * TILE_ROWS + 1;
      const firstCol = tc * TILE_COLS + 1;
      const block = await blocks.get(firstRow - 1, firstCol - 1);
      context.save();
      context.translate(
        x + colAxis.offset(cols[0], firstCol),
        y + rowAxis.offset(rows[0], firstRow),
      );
      drawTile(context as never, block, palette, 1);
      context.restore();
    }
  }
  context.restore();
}

function drawHeadings(
  context: RecordingContext,
  rowAxis: Axis,
  colAxis: Axis,
  font: string,
  lines: { rows: [number, number][]; cols: [number, number][] },
  headW: number,
  headH: number,
) {
  context.save();
  context.strokeStyle = PRINT_GRIDLINE;
  context.fillStyle = '#000000';
  context.lineWidth = 1;
  context.font = font;
  context.textAlign = 'center';
  context.textBaseline = 'middle';
  context.direction = 'ltr';
  context.strokeRect(0.5, 0.5, headW - 1, headH - 1);
  let x = headW;
  for (const [first, last] of lines.cols) {
    for (let col = first; col <= last; col++) {
      const w = colAxis.size(col);
      if (!w) continue;
      context.strokeRect(x + 0.5, 0.5, w - 1, headH - 1);
      context.fillText(columnName(col), x + w / 2, headH / 2);
      x += w;
    }
  }
  let y = headH;
  for (const [first, last] of lines.rows) {
    for (let row = first; row <= last; row++) {
      const h = rowAxis.size(row);
      if (!h) continue;
      context.strokeRect(0.5, y + 0.5, headW - 1, h - 1);
      context.fillText(String(row), headW / 2, y + h / 2);
      y += h;
    }
  }
  context.restore();
}

/** `RRGGBB` as CSS, or black for a theme colour this can't read. */
function runColor(color: string | null): string {
  if (color && /^[0-9a-f]{6}$/i.test(color)) return '#' + color;
  return '#000000';
}

interface HfPiece {
  text: string;
  css: string;
  size: number;
  color: string;
  underline: number;
  strike: boolean;
  rise: number;
  width: number;
}

/** A section's runs as lines of measured pieces, fields filled in. */
function sectionLines(
  runs: HfRun[],
  fields: PageFields & { page: number; pages: number; sheet: string },
  baseFont: { name: string; size: number },
  fallback: string,
  scale: number,
  measure: OffscreenCanvasRenderingContext2D,
): HfPiece[][] {
  const lines: HfPiece[][] = [[]];
  for (const run of runs) {
    let text: string;
    switch (run.field) {
      case 'page':
        text = String(fields.page + Number(run.text ?? 0));
        break;
      case 'pages':
        text = String(fields.pages);
        break;
      case 'date':
        text = fields.date;
        break;
      case 'time':
        text = fields.time;
        break;
      case 'file':
        text = fields.file;
        break;
      case 'path':
        text = fields.path;
        break;
      case 'sheet':
        text = fields.sheet;
        break;
      case 'picture':
        text = '';
        break;
      default:
        text = String(run.text ?? '');
    }
    let size = (run.size ?? baseFont.size) * scale;
    let rise = 0;
    if (run.superscript || run.subscript) {
      rise = run.superscript ? -size * 0.33 : size * 0.15;
      size *= 0.65;
    }
    const css =
      (run.italic ? 'italic ' : '') +
      (run.bold ? 'bold ' : '') +
      size.toFixed(2) +
      'px "' +
      (run.font ?? baseFont.name) +
      '", ' +
      fallback;
    const parts = text.split(/\r?\n/);
    parts.forEach((part, index) => {
      if (index > 0) lines.push([]);
      if (!part) return;
      measure.font = css;
      lines[lines.length - 1]!.push({
        text: part,
        css,
        size,
        color: runColor(run.color),
        underline: run.underline,
        strike: run.strike,
        rise,
        width: measure.measureText(part).width,
      });
    });
  }
  return lines;
}

function drawHeaderFooter(
  context: RecordingContext,
  sections: HfSections | null,
  info: PagesInfo,
  options: PrintOptions,
  page: number,
  footer: boolean,
) {
  if (!sections) return;
  const [paperW, paperH] = info.paper;
  const hf = info.headerFooter;
  const scale = hf.scaleWithDoc ? info.zoom / 100 : 1;
  const left = hf.alignWithMargins ? info.margins.left : 0.7 * 72;
  const right = paperW - (hf.alignWithMargins ? info.margins.right : 0.7 * 72);
  const fields = { ...options.fields, page, pages: options.total, sheet: info.name };
  const measure = options.measure;
  for (const [key, runs] of [
    ['left', sections.left],
    ['center', sections.center],
    ['right', sections.right],
  ] as const) {
    if (!runs.length) continue;
    const lines = sectionLines(runs, fields, info.font, options.fontFamily, scale, measure);
    const heights = lines.map(
      (line) => Math.max(info.font.size * scale, ...line.map((p) => p.size)) * 1.2,
    );
    const total = heights.reduce((a, b) => a + b, 0);
    let y = footer ? paperH - info.margins.footer - total : info.margins.header;
    lines.forEach((line, index) => {
      const height = heights[index]!;
      const width = line.reduce((sum, piece) => sum + piece.width, 0);
      let x = key === 'left' ? left : key === 'right' ? right - width : (paperW - width) / 2;
      const baseline = y + height * 0.8;
      for (const piece of line) {
        context.font = piece.css;
        context.fillStyle = piece.color;
        context.textAlign = 'left';
        context.textBaseline = 'alphabetic';
        context.direction = 'ltr';
        context.fillText(piece.text, x, baseline + piece.rise);
        if (piece.underline || piece.strike) {
          context.fillStyle = piece.color;
          const thick = Math.max(0.5, piece.size / 18);
          if (piece.underline) {
            context.fillRect(x, baseline + piece.rise + thick * 2, piece.width, thick);
            if (piece.underline === 2) {
              context.fillRect(x, baseline + piece.rise + thick * 4, piece.width, thick);
            }
          }
          if (piece.strike) {
            context.fillRect(x, baseline + piece.rise - piece.size * 0.3, piece.width, thick);
          }
        }
        x += piece.width;
      }
      y += height;
    });
  }
}

/** The header and footer a page uses. */
function pageSections(info: PagesInfo, number: number, index: number) {
  const hf = info.headerFooter;
  if (hf.differentFirst && index === 0) return hf.first;
  if (hf.differentOddEven && number % 2 === 0) return hf.even;
  return hf.odd;
}

/** Records one page of a sheet. `index` counts from 0 within the sheet. */
async function recordPage(
  sheet: SheetPrint,
  page: PageInfo,
  index: number,
  blocks: Blocks,
  options: PrintOptions,
): Promise<RecordedPage> {
  const { info, layout } = sheet;
  const rowAxis = new Axis(layout.rows, layout.defaultRowPx);
  const colAxis = new Axis(layout.cols, layout.defaultColPx);
  const scale = (info.zoom / 100) * POINT;
  const content: Matrix = [scale, 0, 0, scale, page.x, page.y];
  const context = new RecordingContext(
    options.recording,
    options.measure,
    content,
    options.measuring,
  );
  const palette: TilePalette = {
    dark: false,
    background: '#ffffff',
    gridline: PRINT_GRIDLINE,
    text: '#000000',
    fontFamily: options.fontFamily,
    showGrid: info.gridlines && !info.draft,
  };
  const headW = info.headingSize ? info.headingSize[0] / POINT : 0;
  const headH = info.headingSize ? info.headingSize[1] / POINT : 0;
  const titleW = page.titleCols ? colAxis.sum(page.titleCols[0], page.titleCols[1]) : 0;
  const titleH = page.titleRows ? rowAxis.sum(page.titleRows[0], page.titleRows[1]) : 0;
  const cellsX = headW + titleW;
  const cellsY = headH + titleH;
  const rowLines: [number, number][] = [...(page.titleRows ? [page.titleRows] : []), page.rows];
  const colLines: [number, number][] = [...(page.titleCols ? [page.titleCols] : []), page.cols];
  // Titles first, then the page's own cells.
  if (page.titleRows && page.titleCols) {
    await drawCells(
      context,
      blocks,
      rowAxis,
      colAxis,
      palette,
      page.titleRows,
      page.titleCols,
      headW,
      headH,
    );
  }
  if (page.titleRows) {
    await drawCells(
      context,
      blocks,
      rowAxis,
      colAxis,
      palette,
      page.titleRows,
      page.cols,
      cellsX,
      headH,
    );
  }
  if (page.titleCols) {
    await drawCells(
      context,
      blocks,
      rowAxis,
      colAxis,
      palette,
      page.rows,
      page.titleCols,
      headW,
      cellsY,
    );
  }
  await drawCells(context, blocks, rowAxis, colAxis, palette, page.rows, page.cols, cellsX, cellsY);
  if (palette.showGrid) {
    // The tiles draw each cell's right and bottom lines; the printout's top and left close it.
    const w = titleW + colAxis.sum(page.cols[0], page.cols[1]);
    const h = titleH + rowAxis.sum(page.rows[0], page.rows[1]);
    context.fillStyle = PRINT_GRIDLINE;
    context.fillRect(headW, headH, w, 1);
    context.fillRect(headW, headH, 1, h);
  }
  if (info.headingSize) {
    const size = (info.font.size * 96) / 72;
    const font = size.toFixed(2) + 'px "' + info.font.name + '", ' + options.fontFamily;
    drawHeadings(context, rowAxis, colAxis, font, { rows: rowLines, cols: colLines }, headW, headH);
  }
  if (!info.draft) {
    const left = colAxis.sum(1, page.cols[0] - 1);
    const top = rowAxis.sum(1, page.rows[0] - 1);
    const width = colAxis.sum(page.cols[0], page.cols[1]);
    const height = rowAxis.sum(page.rows[0], page.rows[1]);
    for (const drawing of sheet.drawings) {
      const x = drawing.x - left;
      const y = drawing.y - top;
      if (x >= width || y >= height || x + drawing.width <= 0 || y + drawing.height <= 0) continue;
      context.save();
      context.beginPath();
      context.rect(cellsX, cellsY, width, height);
      context.clip();
      if (drawing.kind === 'picture' && drawing.image && options.picture) {
        const picture = await options.picture(drawing.image);
        if (picture !== null) {
          context.drawRecorded(picture, cellsX + x, cellsY + y, drawing.width, drawing.height);
        }
      } else if (drawing.kind === 'chart' && options.drawChart) {
        context.translate(cellsX + x, cellsY + y);
        await options.drawChart(context, drawing);
      }
      context.restore();
    }
  }
  const number = (info.firstPage ?? options.firstNumber) + index;
  const [header, footer] = pageSections(info, number, index);
  const paper = new RecordingContext(
    options.recording,
    options.measure,
    undefined,
    options.measuring,
  );
  drawHeaderFooter(paper, header, info, options, number, false);
  drawHeaderFooter(paper, footer, info, options, number, true);
  const ops = [...context.ops, ...paper.ops];
  return {
    width: info.paper[0],
    height: info.paper[1],
    ops: info.blackAndWhite ? ops.map(grey) : ops,
  };
}

function greyOf(color: string): string {
  const hex = color.replace('#', '');
  const r = parseInt(hex.slice(0, 2), 16);
  const g = parseInt(hex.slice(2, 4), 16);
  const b = parseInt(hex.slice(4, 6), 16);
  const level = Math.round(0.299 * r + 0.587 * g + 0.114 * b);
  const part = level.toString(16).padStart(2, '0');
  return '#' + part + part + part + (hex.slice(6, 8) || 'ff');
}

/** Black and white: every colour as its grey. */
function grey(op: PdfOp): PdfOp {
  switch (op.k) {
    case 'text':
      return { ...op, fill: greyOf(op.fill) };
    case 'path':
      return {
        ...op,
        fill:
          typeof op.fill === 'string'
            ? greyOf(op.fill)
            : op.fill && 'linear' in op.fill
              ? {
                  ...op.fill,
                  stops: op.fill.stops.map(([o, c]) => [o, greyOf(c)] as [number, string]),
                }
              : op.fill,
        stroke: op.stroke && greyOf(op.stroke),
      };
    default:
      return op;
  }
}

/** Records every page of a sheet, in order. */
export async function recordSheetPages(
  source: PrintSource,
  sheet: SheetPrint,
  options: PrintOptions,
): Promise<RecordedPage[]> {
  const blocks = new Blocks(source, sheet.info.sheet);
  const pages: RecordedPage[] = [];
  for (let index = 0; index < sheet.info.pages.length; index++) {
    pages.push(await recordPage(sheet, sheet.info.pages[index]!, index, blocks, options));
  }
  return pages;
}
