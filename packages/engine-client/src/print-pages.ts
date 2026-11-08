// Draws a sheet's pages into a recording: the cells with the tile drawing code, in the file's own
// colours, with title rows and columns, headings, gridlines, drawings, and the header and footer.
// `crates/cgs-engine/src/print.rs` says where pages break; this only draws what each page shows.
import { drawTile, TILE_COLS, TILE_ROWS, visualColPx } from './tile-draw';
import { RecordingContext, type Matrix, type PdfOp, type Recording } from './recording';
import type { SheetLayout } from './layout';
import type { ViewBlock } from './view-block';
import { cellText, KIND_MASK, KIND_OTHER, TILE_NOT_READY } from './view-block';
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

/** Excel prints gridlines and headings' lines black, whatever their colour on screen. */
const PRINT_GRIDLINE = '#000000';

/**
 * Sizes along one axis, from the layout's runs, with sums over stretches. `unit` turns a line's
 * pixels into the units pages are drawn in, such as Excel's printer dots.
 */
export class Axis {
  constructor(
    private readonly runs: Uint32Array,
    private readonly fallbackPx: number,
    readonly unit: (px: number) => number = (px) => px,
  ) {}

  private get fallback() {
    return this.unit(this.fallbackPx);
  }

  size(index: number): number {
    return this.unit(this.px(index));
  }

  /** A line's pixels at 100%. */
  px(index: number): number {
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
    return this.fallbackPx;
  }

  /** The sum of `first` to `last`, inclusive. */
  sum(first: number, last: number): number {
    if (last < first) return 0;
    let total = (last - first + 1) * this.fallback;
    const runs = this.runs;
    for (let at = 0; at < runs.length; at += 3) {
      const a = Math.max(first, runs[at]!);
      const b = Math.min(last, runs[at + 1]!);
      if (a <= b) total += (b - a + 1) * (this.unit(runs[at + 2]!) - this.fallback);
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

const ERRORS =
  /^#(N\/A|DIV\/0!|VALUE!|REF!|NAME\?|NUM!|NULL!|SPILL!|CALC!|FIELD!|BLOCKED!|CONNECT!|BUSY!|UNKNOWN!|GETTING_DATA)$/;

/** Page Setup's "Cell errors as": error values shown blank, as `--` or as `#N/A`. */
export function replaceErrors(block: ViewBlock, mode: PagesInfo['errors']): ViewBlock {
  if (mode === 'displayed') return block;
  const shown = mode === 'blank' ? '' : mode === 'dash' ? '--' : '#N/A';
  const extra = new TextEncoder().encode(shown);
  const heap = new Uint8Array(block.heap.length + extra.length);
  heap.set(block.heap);
  heap.set(extra, block.heap.length);
  const cells = block.cells.slice();
  for (let row = 0; row < block.rows; row++) {
    for (let col = 0; col < block.cols; col++) {
      const at = (row * block.cols + col) * 6;
      const flags = cells[at + 4]! >>> 16;
      if ((flags & KIND_MASK) !== KIND_OTHER || !ERRORS.test(cellText(block, row, col))) continue;
      cells[at] = block.heap.length;
      cells[at + 1] = extra.length;
    }
  }
  return { ...block, cells, heap };
}

class Blocks {
  private readonly cache = new Map<string, Promise<ViewBlock>>();
  constructor(
    private readonly source: PrintSource,
    private readonly sheet: number,
    private readonly errors: PagesInfo['errors'] = 'displayed',
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
      if (!(block.flags & TILE_NOT_READY) || attempt >= 200) {
        return replaceErrors(block, this.errors);
      }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }
}

/** Draws cells `rows` by `cols` with their top left at `x`, `y` in the context's pixels. */
/** Excel's printer units for a page: dots per inch, and dots per pixel at the page's scale. */
interface PageUnits {
  scale: number;
  perPixelX: number;
  perPixelY: number;
}

/** A tile's cell edges in page units. */
function edgesOf(sizes: ArrayLike<number>, axis: Axis): Float64Array {
  const edges = new Float64Array(sizes.length + 1);
  for (let at = 0; at < sizes.length; at++) edges[at + 1] = edges[at]! + axis.unit(sizes[at]!);
  return edges;
}

/**
 * Draws cells with their top left at `x`, `y`. On a right-to-left sheet `x` is still the area's
 * left, and its first column is on the right.
 */
async function drawCells(
  context: RecordingContext,
  blocks: Blocks,
  rowAxis: Axis,
  colAxis: Axis,
  palette: TilePalette,
  units: PageUnits,
  rows: [number, number],
  cols: [number, number],
  x: number,
  y: number,
) {
  const rtl = palette.rightToLeft === true;
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
      const xs = edgesOf(visualColPx(block, rtl), colAxis);
      const along = colAxis.offset(cols[0], firstCol);
      context.save();
      context.translate(
        rtl ? x + width - along - xs[xs.length - 1]! : x + along,
        y + rowAxis.offset(rows[0], firstRow),
      );
      drawTile(context as never, block, palette, units.scale, {
        xs,
        ys: edgesOf(block.rowPx, rowAxis),
        perPixelX: units.perPixelX,
        perPixelY: units.perPixelY,
      });
      context.restore();
    }
  }
  context.restore();
}

/**
 * The row and column headings. On a right-to-left sheet the row numbers are on the right of a
 * block `blockW` wide, and the column letters run from there to the left.
 */
function drawHeadings(
  context: RecordingContext,
  rowAxis: Axis,
  colAxis: Axis,
  font: string,
  lines: { rows: [number, number][]; cols: [number, number][] },
  headW: number,
  headH: number,
  line: number,
  blockW: number | null,
) {
  const place = (x: number, w: number) => (blockW === null ? x : blockW - x - w);
  context.save();
  context.strokeStyle = PRINT_GRIDLINE;
  context.fillStyle = '#000000';
  context.lineWidth = line;
  context.font = font;
  context.textAlign = 'center';
  context.textBaseline = 'middle';
  context.direction = 'ltr';
  context.strokeRect(place(0, headW) + line / 2, line / 2, headW - line, headH - line);
  let x = headW;
  for (const [first, last] of lines.cols) {
    for (let col = first; col <= last; col++) {
      const w = colAxis.size(col);
      if (!w) continue;
      const left = place(x, w);
      context.strokeRect(left + line / 2, line / 2, w - line, headH - line);
      context.fillText(columnName(col), left + w / 2, headH / 2);
      x += w;
    }
  }
  let y = headH;
  const rowLeft = place(0, headW);
  for (const [first, last] of lines.rows) {
    for (let row = first; row <= last; row++) {
      const h = rowAxis.size(row);
      if (!h) continue;
      context.strokeRect(rowLeft + line / 2, y + line / 2, headW - line, h - line);
      context.fillText(String(row), rowLeft + headW / 2, y + h / 2);
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
  /** A picture's index in the recording, drawn instead of text. */
  image?: number;
  /** A picture's height. */
  height?: number;
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
  picture: { image: number; width: number; height: number } | null,
): HfPiece[][] {
  const lines: HfPiece[][] = [[]];
  for (const run of runs) {
    if (run.field === 'picture') {
      if (picture) {
        lines[lines.length - 1]!.push({
          image: picture.image,
          height: picture.height * scale,
          text: '',
          css: '',
          size: 0,
          color: '#000000',
          underline: 0,
          strike: false,
          rise: 0,
          width: picture.width * scale,
        });
      }
      continue;
    }
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
  /** The pictures this page's header or footer shows, by section: `L`, `C` or `R`. */
  pictures: Record<string, { image: number; width: number; height: number } | null>,
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
    const picture = pictures[key[0]!.toUpperCase()] ?? null;
    const lines = sectionLines(
      runs,
      fields,
      info.font,
      options.fontFamily,
      scale,
      measure,
      picture,
    );
    const heights = lines.map((line) =>
      Math.max(
        info.font.size * scale * 1.2,
        ...line.map((p) => (p.image !== undefined ? (p.height ?? 0) : p.size * 1.2)),
      ),
    );
    const total = heights.reduce((a, b) => a + b, 0);
    let y = footer ? paperH - info.margins.footer - total : info.margins.header;
    lines.forEach((line, index) => {
      const height = heights[index]!;
      const width = line.reduce((sum, piece) => sum + piece.width, 0);
      let x = key === 'left' ? left : key === 'right' ? right - width : (paperW - width) / 2;
      const baseline = y + height * 0.8;
      for (const piece of line) {
        if (piece.image !== undefined) {
          // A picture sits on the line's bottom, as Excel lines it up.
          const h = piece.height ?? 0;
          context.drawRecorded(piece.image, x, y + height - h, piece.width, h);
          x += piece.width;
          continue;
        }
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
  // Excel's printer dots, as `crates/cgs-engine/src/print.rs` breaks pages in them: rows a
  // little shorter and columns a little wider than on screen, while text keeps its size.
  const zoom = info.zoom / 100;
  const dots = info.units ?? { dpi: 600, row: 6.0567, col: 6.75 };
  const rowAxis = new Axis(layout.rows, layout.defaultRowPx, (px) =>
    Math.round(Math.floor(px * dots.row) * zoom),
  );
  const colAxis = new Axis(
    layout.cols,
    layout.defaultColPx,
    (px) => Math.round(px * dots.col) * zoom,
  );
  const rowPx = new Axis(layout.rows, layout.defaultRowPx);
  const colPx = new Axis(layout.cols, layout.defaultColPx);
  const units: PageUnits = {
    scale: (dots.dpi / 96) * zoom,
    perPixelX: dots.col * zoom,
    perPixelY: dots.row * zoom,
  };
  const toPoints = 72 / dots.dpi;
  const content: Matrix = [toPoints, 0, 0, toPoints, page.x, page.y];
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
    rightToLeft: layout.rightToLeft,
  };
  const rtl = layout.rightToLeft;
  const headW = info.headingSize ? colAxis.unit(info.headingSize[0]) : 0;
  const headH = info.headingSize ? rowAxis.unit(info.headingSize[1]) : 0;
  const titleW = page.titleCols ? colAxis.sum(page.titleCols[0], page.titleCols[1]) : 0;
  const titleH = page.titleRows ? rowAxis.sum(page.titleRows[0], page.titleRows[1]) : 0;
  const pageW = colAxis.sum(page.cols[0], page.cols[1]);
  // A right-to-left sheet prints mirrored, as Excel prints it: headings on the right, then the
  // title columns, then the page's own columns running to the left.
  const blockW = headW + titleW + pageW;
  const place = (x: number, w: number) => (rtl ? blockW - x - w : x);
  const titleX = place(headW, titleW);
  const cellsX = place(headW + titleW, pageW);
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
      units,
      page.titleRows,
      page.titleCols,
      titleX,
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
      units,
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
      units,
      page.rows,
      page.titleCols,
      titleX,
      cellsY,
    );
  }
  await drawCells(
    context,
    blocks,
    rowAxis,
    colAxis,
    palette,
    units,
    page.rows,
    page.cols,
    cellsX,
    cellsY,
  );
  if (palette.showGrid) {
    // The tiles draw each cell's line after it and its bottom line; the printout's top and its
    // side before the first column close it, which is its right on a right-to-left sheet.
    const w = titleW + pageW;
    const h = titleH + rowAxis.sum(page.rows[0], page.rows[1]);
    const left = place(headW, w);
    context.fillStyle = PRINT_GRIDLINE;
    context.fillRect(left, headH, w, units.scale);
    context.fillRect(rtl ? left + w - units.scale : left, headH, units.scale, h);
  }
  if (info.headingSize) {
    const size = ((info.font.size * 96) / 72) * units.scale;
    const font = size.toFixed(2) + 'px "' + info.font.name + '", ' + options.fontFamily;
    drawHeadings(
      context,
      rowAxis,
      colAxis,
      font,
      { rows: rowLines, cols: colLines },
      headW,
      headH,
      units.scale,
      rtl ? blockW : null,
    );
  }
  if (!info.draft) {
    // Drawings stay where their cells are and keep their own size, as Excel prints them.
    const left = colPx.sum(1, page.cols[0] - 1);
    const top = rowPx.sum(1, page.rows[0] - 1);
    const width = pageW;
    const height = rowAxis.sum(page.rows[0], page.rows[1]);
    for (const drawing of sheet.drawings) {
      if (drawing.prints === false) continue;
      const w = drawing.width * units.scale;
      const h = drawing.height * units.scale;
      // From the right on a right-to-left sheet, as Excel places them; never turned over.
      const along = (drawing.x - left) * units.perPixelX;
      const x = rtl ? width - along - w : along;
      const y = (drawing.y - top) * units.perPixelY;
      if (x >= width || y >= height || x + w <= 0 || y + h <= 0) continue;
      context.save();
      context.beginPath();
      context.rect(cellsX, cellsY, width, height);
      context.clip();
      if (drawing.kind === 'picture' && drawing.image && options.picture) {
        const picture = await options.picture(drawing.image);
        if (picture !== null) {
          context.drawRecorded(picture, cellsX + x, cellsY + y, w, h);
        }
      } else if (drawing.kind === 'chart' && options.drawChart) {
        context.translate(cellsX + x, cellsY + y);
        context.scale(units.scale, units.scale);
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
  const hf = info.headerFooter;
  const suffix =
    hf.differentFirst && index === 0
      ? 'FIRST'
      : hf.differentOddEven && number % 2 === 0
        ? 'EVEN'
        : '';
  const pictures = async (part: 'H' | 'F') => {
    const out: Record<string, { image: number; width: number; height: number } | null> = {};
    for (const section of ['L', 'C', 'R']) {
      const found = info.pictures?.find((p) => p.place.toUpperCase() === section + part + suffix);
      const image = found && options.picture ? await options.picture(found.part) : null;
      out[section] =
        found && image !== null && image !== undefined
          ? { image, width: found.width, height: found.height }
          : null;
    }
    return out;
  };
  drawHeaderFooter(paper, header, info, options, number, false, await pictures('H'));
  drawHeaderFooter(paper, footer, info, options, number, true, await pictures('F'));
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
  const blocks = new Blocks(source, sheet.info.sheet, sheet.info.errors);
  const pages: RecordedPage[] = [];
  for (let index = 0; index < sheet.info.pages.length; index++) {
    pages.push(await recordPage(sheet, sheet.info.pages[index]!, index, blocks, options));
  }
  return pages;
}
