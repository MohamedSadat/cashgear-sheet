import {
  TileCache,
  TILE_COLS,
  TILE_ROWS,
  type DrawingBox,
  type EngineClient,
  type SheetLayout,
  type TileAddress,
  type TilePalette,
  type TileRequest,
} from '@cgs/engine-client';
import { actionFor, startsTyping, type GridAction } from '../shortcuts';
import {
  drawCopied,
  drawCorner,
  drawCellButton,
  drawFillHandle,
  type CellButtonLook,
  drawFillPreview,
  drawHeaderCell,
  drawPlaceholder,
  drawReference,
  drawResizeLine,
  drawScrollbar,
  drawSelection,
  REFERENCE_COLORS,
  type GridColors,
  type ScrollbarShape,
} from './draw';
import {
  Axis,
  cellName,
  columnName,
  MAX_COLS,
  MAX_ROWS,
  rangeName,
  scrollForThumb,
  thumbFor,
  tilesCovering,
  type CellRange,
} from './geometry';
import { mirroredContext } from './mirror';

const SCROLLBAR_PX = 14;
const HEADER_HEIGHT_PX = 20;
const MIN_THUMB_PX = 24;
const WHEEL_LINE_PX = 53;
export const MIN_ZOOM = 10;
export const MAX_ZOOM = 400;
const HEADER_FONT_PX = 11;

export interface GridSelection {
  /** 0-based. */
  row: number;
  col: number;
  range: CellRange;
  name: string;
}

export interface GridEvents {
  onSelection: (selection: GridSelection) => void;
  onZoom: (zoom: number) => void;
  onError: (error: unknown) => void;
  /** A key that starts typing went down. Whatever gets focus now receives that key. */
  onType?: () => void;
  /** A shortcut in the map. */
  onAction?: (action: GridAction) => void;
  /** Ctrl+arrow: where it lands is the engine's to say. */
  onJump?: (rows: number, cols: number, extend: boolean) => void;
  /** A cell was clicked while editing. Returns whether it went in the formula as a reference. */
  onEditClick?: (row: number, col: number) => boolean;
  /** The pointer dragged on from `onEditClick`. */
  onEditDrag?: (row: number, col: number) => void;
  /** The fill handle was dragged out, or double-clicked with `to` null. */
  onFill?: (from: CellRange, to: CellRange | null) => void;
  /** What is in view changed. 0-based. */
  onViewport?: (sheet: number, range: CellRange) => void;
  /** A drag or click that made a selection has finished. */
  onSelectEnd?: () => void;
  /** A line's edge was dragged, to this size in pixels at 100%. Lines are 1-based and inclusive. */
  onResize?: (axis: 'row' | 'col', spans: [number, number][], px: number) => void;
  /** A line's edge was double-clicked. */
  onAutoFit?: (axis: 'row' | 'col', spans: [number, number][]) => void;
  /** Right click or the menu key, in client coordinates. */
  onContextMenu?: (x: number, y: number, region: GridRegion) => void;
  /** A size being dragged, for showing what it comes to. Null when it ends. */
  onResizeHint?: (hint: ResizeHint | null) => void;
  /** A filter's or a list's button in a cell was clicked. 0-based, with its box in client pixels. */
  onCellButton?: (button: CellButton, box: ClientBox) => void;
  /** A cell was double-clicked. Returns whether that was dealt with, so editing doesn't start. */
  onDoubleCell?: (row: number, col: number) => boolean;
  /** A cell was clicked, without Shift or Ctrl. 0-based. For following a link. */
  onCellClick?: (row: number, col: number) => void;
  /** The pointer rests on another cell, or none. 0-based. For notes and links. */
  onHover?: (cell: { row: number; col: number } | null, clientX: number, clientY: number) => void;
}

/**
 * Where a chart or picture shows in one pane, in CSS pixels from the canvas's top left. The
 * drawing layer puts the real thing there, clipped to the pane.
 */
export interface DrawingFrame {
  /** The drawing's index on its sheet. */
  index: number;
  /** 0 is the scrolling pane; the frozen ones follow. */
  pane: number;
  clip: { x: number; y: number; width: number; height: number };
  x: number;
  y: number;
  width: number;
  height: number;
  /** The sheet's zoom as a factor. */
  zoom: number;
}

/** Drawings the layer above the grid shows, rather than the canvas. */
export function drawnByLayer(box: DrawingBox) {
  return (
    (box.kind === 'chart' && box.chart !== undefined) ||
    (box.kind === 'picture' && Boolean(box.image))
  );
}

/** A button the grid draws in a cell: a filter's in its heading, or a list's beside its cell. */
export interface CellButton {
  kind: 'filter' | 'list' | 'pivot';
  /** 0-based. */
  row: number;
  col: number;
  look: CellButtonLook;
}

export interface ClientBox {
  left: number;
  top: number;
  width: number;
  height: number;
}

export type GridRegion = 'cells' | 'rowHeader' | 'colHeader';

export interface ResizeHint {
  axis: 'row' | 'col';
  px: number;
  clientX: number;
  clientY: number;
}

export type { CellRange };

/** A reference in the formula being typed. */
export interface GridHighlight {
  sheet: number;
  range: CellRange;
  color: number;
}

/** What a sheet remembers while another one is showing. */
interface SheetView {
  zoom: number;
  scrollX: number;
  scrollY: number;
  row: number;
  col: number;
  range: CellRange;
  extra: CellRange[];
}

/** Where pages break, as Normal view shows it once page setup has been used. 1-based. */
export interface PageBreakLines {
  /** The last row and column of each page but the last. */
  rows: number[];
  cols: number[];
  /** The ones set by hand, drawn with longer dashes. */
  manualRows: number[];
  manualCols: number[];
  areas: { top: number; left: number; bottom: number; right: number }[];
}

interface Pane {
  x: number;
  y: number;
  width: number;
  height: number;
  /** The sheet position at the pane's top left. */
  sheetX: number;
  sheetY: number;
}

const defaultColWidthFallback = 64;
const defaultRowHeightFallback = 20;

/**
 * Composites tile pictures onto the visible canvas and handles input, outside React. The tile
 * worker draws them from the engine's data, so nothing here waits on the engine or sees a cell.
 */
export class GridController {
  private readonly plain: CanvasRenderingContext2D;
  private readonly mirrored: CanvasRenderingContext2D;
  /**
   * Sheet Right-to-Left. Everything here works in left-to-right canvas positions; drawing goes
   * through a mirrored context and pointer positions are mirrored on the way in, so column A
   * shows on the right and every pointer and keyboard path stays one path.
   */
  private rtl = false;
  private readonly cache: TileCache;
  private palette: TilePalette;
  private colors: GridColors;
  private dpr = 1;
  private width = 0;
  private height = 0;
  private frame = 0;

  private sheet = -1;
  private pageBreaks: PageBreakLines | null = null;
  private layout: SheetLayout | null = null;
  private drawings: DrawingBox[] = [];
  private drawingListener: ((sheet: number, frames: DrawingFrame[]) => void) | null = null;
  private frameListeners = new Set<() => void>();
  private frames: DrawingFrame[] = [];
  private merges: Uint32Array = new Uint32Array();
  private rows = new Axis(MAX_ROWS, defaultRowHeightFallback);
  private cols = new Axis(MAX_COLS, defaultColWidthFallback);
  private zoom = 100;
  private scrollX = 0;
  private scrollY = 0;
  private row = 0;
  private col = 0;
  private anchorRow = 0;
  private anchorCol = 0;
  private range: CellRange = { top: 0, left: 0, bottom: 0, right: 0 };
  /** Earlier areas of a selection built up with Ctrl+click. */
  private extra: CellRange[] = [];
  private readonly remembered = new Map<number, SheetView>();
  private highlights: GridHighlight[] = [];
  private copied: { sheet: number; range: CellRange } | null = null;
  /** The cell being edited, whose box the editor element follows. */
  private editing: { sheet: number; row: number; col: number } | null = null;
  private editorElement: HTMLElement | null = null;
  private fillTarget: CellRange | null = null;
  private lastViewport = '';

  private dragging:
    'cells' | 'rows' | 'cols' | 'point' | 'fill' | 'vertical' | 'horizontal' | 'resize' | null =
    null;
  private dragOffset = 0;
  /** A line's edge being dragged. Sizes are in canvas pixels. */
  private resizing: { axis: 'row' | 'col'; line: number; start: number; at: number } | null = null;
  private painter = false;
  private buttons: CellButton[] = [];
  private hovered = '';
  private lastPointer = { x: 0, y: 0 };
  private autoScroll = 0;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    client: EngineClient,
    palette: TilePalette,
    colors: GridColors,
    private readonly events: GridEvents,
  ) {
    const context = canvas.getContext('2d', { alpha: false });
    if (!context) throw new Error('This browser has no 2D canvas.');
    this.plain = context;
    this.mirrored = mirroredContext(context, () => this.width);
    this.palette = palette;
    this.colors = colors;
    canvas.dataset.background = palette.background;
    this.cache = new TileCache(
      (request) => client.drawTile(request),
      this.schedule,
      events.onError,
    );
    canvas.addEventListener('wheel', this.onWheel, { passive: false });
    canvas.addEventListener('pointerdown', this.onPointerDown);
    canvas.addEventListener('pointermove', this.onPointerMove);
    canvas.addEventListener('pointerup', this.onPointerUp);
    canvas.addEventListener('pointercancel', this.onPointerUp);
    canvas.addEventListener('keydown', this.onKeyDown);
    canvas.addEventListener('dblclick', this.onDoubleClick);
    canvas.addEventListener('mousedown', this.onMouseDown);
    canvas.addEventListener('contextmenu', this.onContextMenu);
    this.applyStyle();
  }

  private get scale() {
    return (this.zoom / 100) * this.dpr;
  }

  /** What the grid draws with: mirrored on a right-to-left sheet. */
  private get context() {
    return this.rtl ? this.mirrored : this.plain;
  }

  /** Whether the sheet showing is right to left. */
  get rightToLeft() {
    return this.rtl;
  }

  /** A canvas x as the grid's own left-to-right positions have it. Its own inverse. */
  private flipX(x: number) {
    return this.rtl ? this.width - x : x;
  }

  /** A box in left-to-right canvas positions, where it shows. */
  private shownBox<T extends { x: number; width: number }>(box: T): T {
    return this.rtl ? { ...box, x: this.width - box.x - box.width } : box;
  }

  private tileStyle() {
    return {
      scale: this.scale,
      palette: {
        ...this.palette,
        showGrid: this.layout?.showGrid ?? true,
        rightToLeft: this.rtl,
      },
    };
  }

  /** What drawing the tile at this cell (counted from 0) asks for, for the bench. */
  tileRequest(firstRow: number, firstCol: number): TileRequest {
    return { sheet: this.sheet, firstRow, firstCol, ...this.tileStyle() };
  }

  private applyStyle() {
    this.cache.setStyle(this.tileStyle());
  }

  /** Redraws tiles with the new theme's colours; the old ones stay until they arrive. */
  setTheme(palette: TilePalette, colors: GridColors) {
    this.palette = palette;
    this.colors = colors;
    this.canvas.dataset.background = palette.background;
    this.canvas.dataset.ready = 'false';
    this.applyStyle();
    this.schedule();
  }

  /** Takes the canvas's size in CSS pixels. */
  resize(width: number, height: number, dpr: number) {
    this.width = Math.round(width * dpr);
    this.height = Math.round(height * dpr);
    this.canvas.width = this.width;
    this.canvas.height = this.height;
    if (dpr !== this.dpr) {
      this.dpr = dpr;
      this.rows.setScale(this.scale);
      this.cols.setScale(this.scale);
      this.applyStyle();
    }
    this.schedule();
  }

  /** A different workbook is opening: everything drawn so far is gone. */
  reset() {
    this.cache.clear();
    this.remembered.clear();
    this.sheet = -1;
    this.layout = null;
    this.rtl = false;
    this.drawings = [];
    this.merges = new Uint32Array();
    this.schedule();
  }

  /** Shows a sheet. Comes back to where it was if it has been shown before. */
  showSheet(sheet: number, layout: SheetLayout, drawings: DrawingBox[]) {
    if (sheet !== this.sheet) this.pageBreaks = null;
    if (this.sheet >= 0) this.remember();
    const sameSheet = sheet === this.sheet;
    const kept = this.remembered.get(sheet);
    this.sheet = sheet;
    this.layout = layout;
    this.rtl = layout.rightToLeft;
    this.drawings = drawings;
    this.merges = layout.merges;
    this.zoom = sameSheet || kept ? (kept?.zoom ?? this.zoom) : layout.zoom;
    this.rows = new Axis(
      MAX_ROWS,
      layout.defaultRowPx,
      zeroBased(layout.rows, MAX_ROWS),
      this.scale,
    );
    this.cols = new Axis(
      MAX_COLS,
      layout.defaultColPx,
      zeroBased(layout.cols, MAX_COLS),
      this.scale,
    );
    this.applyStyle();
    if (kept) {
      this.scrollX = kept.scrollX;
      this.scrollY = kept.scrollY;
      this.row = kept.row;
      this.col = kept.col;
      this.anchorRow = kept.row;
      this.anchorCol = kept.col;
      this.range = kept.range;
      this.extra = kept.extra;
    } else if (!sameSheet) {
      this.scrollX = this.cols.offset(layout.leftCol - 1);
      this.scrollY = this.rows.offset(layout.topRow - 1);
      this.select(layout.activeRow - 1, layout.activeCol - 1, false, false);
    }
    this.clampScroll();
    this.events.onZoom(this.zoom);
    this.notifySelection();
    this.schedule();
  }

  /** The sheet's tiles were asked for before it was read. */
  sheetReady(sheet: number) {
    this.cache.invalidateSheet(sheet);
    this.schedule();
  }

  /** Cells changed. The pictures that show them stay until new ones arrive. */
  invalidate(sheets: number[], tiles: { sheet: number; firstRow: number; firstCol: number }[]) {
    for (const sheet of sheets) this.cache.invalidateSheet(sheet);
    if (tiles.length) this.cache.invalidateTiles(tiles);
    this.schedule();
  }

  /** The first row and column in view, 0-based, from which panes freeze. */
  visibleCorner() {
    const frozen = this.frozen();
    if (frozen.rows || frozen.cols) {
      return { row: frozen.firstRow, col: frozen.firstCol };
    }
    return { row: this.rows.at(this.scrollY), col: this.cols.at(this.scrollX) };
  }

  /** Shows that the format painter is on, by its cursor. */
  setPainter(on: boolean) {
    this.painter = on;
    this.canvas.style.cursor = on ? 'copy' : '';
  }

  /** Makes an area fit the view, as far as zoom can. */
  zoomToSelection() {
    const { range } = this;
    const width = (this.cols.offset(range.right + 1) - this.cols.offset(range.left)) / this.scale;
    const height = (this.rows.offset(range.bottom + 1) - this.rows.offset(range.top)) / this.scale;
    const view = this.view();
    if (!width || !height) return;
    const fit = Math.min(view.width / this.dpr / width, view.height / this.dpr / height) * 100;
    this.setZoom(Math.floor(fit));
    this.reveal(range.top, range.left);
    this.schedule();
  }

  /** The sheet showing now. */
  currentSheet() {
    return this.sheet;
  }

  /** The selection, 0-based: the active cell and every area. */
  selection() {
    return {
      sheet: this.sheet,
      row: this.row,
      col: this.col,
      range: this.range,
      areas: [...this.extra, this.range],
    };
  }

  /** Moves the selection, as a click there would. 0-based. */
  selectArea(range: CellRange, row = range.top, col = range.left) {
    this.extra = [];
    this.row = row;
    this.col = col;
    this.anchorRow = row;
    this.anchorCol = col;
    this.range = this.growToMerges(range);
    this.reveal(range.top, range.left);
    this.notifySelection();
    this.schedule();
  }

  /** The corner that Shift+arrow moves. */
  movingCorner() {
    return {
      row: this.anchorRow === this.range.top ? this.range.bottom : this.range.top,
      col: this.anchorCol === this.range.left ? this.range.right : this.range.left,
    };
  }

  /** Scrolls a cell into view. */
  revealCell(row: number, col: number) {
    this.reveal(row, col);
    this.schedule();
  }

  /** An area's address. */
  addressOf(range: CellRange) {
    return rangeName(range);
  }

  /** Goes one way from the active cell, as Enter or Tab does: around a selection of many. */
  moveBy(rows: number, cols: number) {
    const merge = this.mergeAt(this.row, this.col);
    const { range } = this;
    const many =
      !this.extra.length &&
      (range.top !== range.bottom || range.left !== range.right) &&
      !(
        merge &&
        merge.top === range.top &&
        merge.left === range.left &&
        merge.bottom === range.bottom &&
        merge.right === range.right
      );
    if (many) {
      let row = this.row + rows;
      let col = this.col + cols;
      if (row > range.bottom) {
        row = range.top;
        col = col + 1 > range.right ? range.left : col + 1;
      } else if (row < range.top) {
        row = range.bottom;
        col = col - 1 < range.left ? range.right : col - 1;
      } else if (col > range.right) {
        col = range.left;
        row = row + 1 > range.bottom ? range.top : row + 1;
      } else if (col < range.left) {
        col = range.right;
        row = row - 1 < range.top ? range.bottom : row - 1;
      }
      this.row = row;
      this.col = col;
      this.reveal(row, col);
      this.notifySelection();
      this.schedule();
      return;
    }
    const fromRow = merge ? (rows > 0 ? merge.bottom : merge.top) : this.row;
    const fromCol = merge ? (cols > 0 ? merge.right : merge.left) : this.col;
    const row = rows ? this.stepped(this.rows, fromRow, rows, MAX_ROWS) : this.row;
    const col = cols ? this.stepped(this.cols, fromCol, cols, MAX_COLS) : this.col;
    this.select(row, col, false);
  }

  /** Extends the selection to a cell, keeping where it started. */
  extendSelectionTo(row: number, col: number) {
    this.select(row, col, true);
  }

  /** The references in the formula being typed. */
  /** Shows where pages break on the current sheet, or nothing. */
  setPageBreaks(lines: PageBreakLines | null) {
    this.pageBreaks = lines;
    this.schedule();
  }

  /** Dashed lines between the cells where pages break, as Excel draws them. */
  private drawPageBreaks(pane: Pane) {
    const lines = this.pageBreaks;
    if (!lines) return;
    const { context, colors } = this;
    const pixel = Math.max(1, Math.round(this.dpr));
    const manualRows = new Set(lines.manualRows);
    const manualCols = new Set(lines.manualCols);
    context.save();
    context.strokeStyle = colors.textMuted;
    context.lineWidth = pixel;
    const top = pane.y;
    const bottom = pane.y + pane.height;
    const left = pane.x;
    const right = pane.x + pane.width;
    for (const col of lines.cols) {
      // After 1-based column `col`, which is the left edge of the 0-based column of that number.
      const x = this.cols.offset(col) - pane.sheetX + pane.x + 0.5 * pixel;
      if (x < left || x > right) continue;
      context.setLineDash(manualCols.has(col) ? [6 * pixel, 3 * pixel] : [2 * pixel, 2 * pixel]);
      context.beginPath();
      context.moveTo(x, top);
      context.lineTo(x, bottom);
      context.stroke();
    }
    for (const row of lines.rows) {
      const y = this.rows.offset(row) - pane.sheetY + pane.y + 0.5 * pixel;
      if (y < top || y > bottom) continue;
      context.setLineDash(manualRows.has(row) ? [6 * pixel, 3 * pixel] : [2 * pixel, 2 * pixel]);
      context.beginPath();
      context.moveTo(left, y);
      context.lineTo(right, y);
      context.stroke();
    }
    context.restore();
  }

  setHighlights(highlights: GridHighlight[]) {
    this.highlights = highlights;
    this.schedule();
  }

  /** What was copied or cut, which shows until it is pasted or let go. */
  setCopied(copied: { sheet: number; range: CellRange } | null) {
    this.copied = copied;
    this.schedule();
  }

  /** Tells the grid which cell is being edited, and which element follows it. */
  setEditing(
    editing: { sheet: number; row: number; col: number } | null,
    element: HTMLElement | null,
  ) {
    this.editing = editing;
    this.editorElement = element;
    this.placeEditor();
    this.schedule();
  }

  /** Puts the editor element over its cell, in CSS pixels. */
  private placeEditor() {
    const element = this.editorElement;
    const editing = this.editing;
    if (!element) return;
    if (!editing || editing.sheet !== this.sheet) {
      element.style.left = '-10000px';
      return;
    }
    const merge = this.mergeAt(editing.row, editing.col);
    const top = merge ? merge.top : editing.row;
    const left = merge ? merge.left : editing.col;
    const bottom = merge ? merge.bottom : editing.row;
    const right = merge ? merge.right : editing.col;
    const frozen = this.frozen();
    const view = this.view();
    const pane = this.panes()[0]!;
    const x =
      left < frozen.firstCol + frozen.cols
        ? view.x + this.cols.offset(left) - this.cols.offset(frozen.firstCol)
        : pane.x + this.cols.offset(left) - this.scrollX;
    const y =
      top < frozen.firstRow + frozen.rows
        ? view.y + this.rows.offset(top) - this.rows.offset(frozen.firstRow)
        : pane.y + this.rows.offset(top) - this.scrollY;
    const { dpr } = this;
    // On a right-to-left sheet the editor keeps to its cell's right edge and grows to the left.
    element.style.left = (this.rtl ? this.width - x : x) / dpr + 'px';
    element.style.transform = this.rtl ? 'translateX(-100%)' : '';
    element.style.top = y / dpr + 'px';
    element.style.minWidth = (this.cols.offset(right + 1) - this.cols.offset(left)) / dpr + 'px';
    element.style.minHeight = (this.rows.offset(bottom + 1) - this.rows.offset(top)) / dpr + 'px';
    element.style.maxWidth = Math.max(40, (view.x + view.width - x) / dpr) + 'px';
    element.style.setProperty('--cgs-cell-font', Math.round((this.zoom / 100) * 14.67) + 'px');
  }

  setZoom(zoom: number) {
    const next = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, Math.round(zoom)));
    if (next === this.zoom) return;
    // Keep the top-left cell where it is.
    const topRow = this.rows.at(this.scrollY);
    const leftCol = this.cols.at(this.scrollX);
    const rowInto = this.scrollY - this.rows.offset(topRow);
    const colInto = this.scrollX - this.cols.offset(leftCol);
    const rowShare = this.rows.size(topRow) ? rowInto / this.rows.size(topRow) : 0;
    const colShare = this.cols.size(leftCol) ? colInto / this.cols.size(leftCol) : 0;
    this.zoom = next;
    this.rows.setScale(this.scale);
    this.cols.setScale(this.scale);
    this.scrollY = this.rows.offset(topRow) + rowShare * this.rows.size(topRow);
    this.scrollX = this.cols.offset(leftCol) + colShare * this.cols.size(leftCol);
    this.applyStyle();
    this.clampScroll();
    this.events.onZoom(this.zoom);
    this.schedule();
  }

  getZoom() {
    return this.zoom;
  }

  focus() {
    this.canvas.focus({ preventScroll: true });
  }

  dispose() {
    cancelAnimationFrame(this.frame);
    cancelAnimationFrame(this.autoScroll);
    this.frame = 0;
    const { canvas } = this;
    canvas.removeEventListener('wheel', this.onWheel);
    canvas.removeEventListener('pointerdown', this.onPointerDown);
    canvas.removeEventListener('pointermove', this.onPointerMove);
    canvas.removeEventListener('pointerup', this.onPointerUp);
    canvas.removeEventListener('pointercancel', this.onPointerUp);
    canvas.removeEventListener('keydown', this.onKeyDown);
    canvas.removeEventListener('dblclick', this.onDoubleClick);
    canvas.removeEventListener('mousedown', this.onMouseDown);
    canvas.removeEventListener('contextmenu', this.onContextMenu);
    this.cache.dispose();
  }

  /** For tests: where things are. */
  state() {
    return {
      sheet: this.sheet,
      scrollX: this.scrollX,
      scrollY: this.scrollY,
      zoom: this.zoom,
      row: this.row,
      col: this.col,
      range: this.range,
      areas: [...this.extra, this.range],
      topRow: this.rows.at(this.scrollY),
      contentHeight: this.contentHeight(),
      drawings: this.drawings,
    };
  }

  /** For tests: where to click for a cell, in CSS pixels on the canvas. */
  pointForCell(row: number, col: number) {
    const pane = this.panes()[0]!;
    const frozen = this.frozen();
    const view = this.view();
    const inFrozenRows = row < frozen.firstRow + frozen.rows;
    const inFrozenCols = col < frozen.firstCol + frozen.cols;
    const x = inFrozenCols
      ? view.x + this.cols.offset(col) - this.cols.offset(frozen.firstCol)
      : pane.x + this.cols.offset(col) - this.scrollX;
    const y = inFrozenRows
      ? view.y + this.rows.offset(row) - this.rows.offset(frozen.firstRow)
      : pane.y + this.rows.offset(row) - this.scrollY;
    return {
      x: this.flipX(x + this.cols.size(col) / 2) / this.dpr,
      y: (y + this.rows.size(row) / 2) / this.dpr,
    };
  }

  /** For tests: a cell's box, in CSS pixels on the canvas. */
  boxForCell(row: number, col: number) {
    const middle = this.pointForCell(row, col);
    const width = this.cols.size(col) / this.dpr;
    const height = this.rows.size(row) / this.dpr;
    return { x: middle.x - width / 2, y: middle.y - height / 2, width, height };
  }

  /** For tests: moves the view as a scrollbar drag would. */
  scrollTo(x: number, y: number) {
    this.scrollX = x;
    this.scrollY = y;
    this.clampScroll();
    this.schedule();
  }

  private remember() {
    this.remembered.set(this.sheet, {
      zoom: this.zoom,
      scrollX: this.scrollX,
      scrollY: this.scrollY,
      row: this.row,
      col: this.col,
      range: this.range,
      extra: this.extra,
    });
  }

  private readonly schedule = () => {
    if (!this.frame) this.frame = requestAnimationFrame(this.composite);
  };

  private headerWidth() {
    if (this.layout && !this.layout.showHeaders) return 0;
    // Wide enough for the longest row number in view, with room around it.
    const last = Math.max(1000, this.rows.at(this.scrollY + this.height) + 1);
    const digits = String(last).length;
    return Math.round((digits * 7 + 12) * this.scale);
  }

  private headerHeight() {
    if (this.layout && !this.layout.showHeaders) return 0;
    return Math.round(HEADER_HEIGHT_PX * this.scale);
  }

  private frozen() {
    const layout = this.layout;
    const rows = layout?.frozenRows ?? 0;
    const cols = layout?.frozenCols ?? 0;
    const firstRow = (layout?.firstRow ?? 1) - 1;
    const firstCol = (layout?.firstCol ?? 1) - 1;
    return {
      rows,
      cols,
      firstRow,
      firstCol,
      height: rows ? this.rows.offset(firstRow + rows) - this.rows.offset(firstRow) : 0,
      width: cols ? this.cols.offset(firstCol + cols) - this.cols.offset(firstCol) : 0,
    };
  }

  private scrollbarSize() {
    return Math.round(SCROLLBAR_PX * this.dpr);
  }

  /** The area of the canvas that shows the scrolling pane and the frozen panes. */
  private view() {
    const header = { width: this.headerWidth(), height: this.headerHeight() };
    const bar = this.scrollbarSize();
    return {
      x: header.width,
      y: header.height,
      width: Math.max(0, this.width - header.width - bar),
      height: Math.max(0, this.height - header.height - bar),
    };
  }

  private panes(): Pane[] {
    const view = this.view();
    const frozen = this.frozen();
    const frozenX = this.cols.offset(frozen.firstCol);
    const frozenY = this.rows.offset(frozen.firstRow);
    const mainWidth = Math.max(0, view.width - frozen.width);
    const mainHeight = Math.max(0, view.height - frozen.height);
    const panes: Pane[] = [
      {
        x: view.x + frozen.width,
        y: view.y + frozen.height,
        width: mainWidth,
        height: mainHeight,
        sheetX: this.scrollX,
        sheetY: this.scrollY,
      },
    ];
    if (frozen.rows) {
      panes.push({
        x: view.x + frozen.width,
        y: view.y,
        width: mainWidth,
        height: Math.min(frozen.height, view.height),
        sheetX: this.scrollX,
        sheetY: frozenY,
      });
    }
    if (frozen.cols) {
      panes.push({
        x: view.x,
        y: view.y + frozen.height,
        width: Math.min(frozen.width, view.width),
        height: mainHeight,
        sheetX: frozenX,
        sheetY: this.scrollY,
      });
    }
    if (frozen.rows && frozen.cols) {
      panes.push({
        x: view.x,
        y: view.y,
        width: Math.min(frozen.width, view.width),
        height: Math.min(frozen.height, view.height),
        sheetX: frozenX,
        sheetY: frozenY,
      });
    }
    return panes;
  }

  private minScroll() {
    const frozen = this.frozen();
    return {
      x: this.cols.offset(frozen.firstCol + frozen.cols),
      y: this.rows.offset(frozen.firstRow + frozen.rows),
    };
  }

  private clampScroll() {
    const view = this.view();
    const frozen = this.frozen();
    const low = this.minScroll();
    const maxX = Math.max(low.x, this.cols.total - (view.width - frozen.width));
    const maxY = Math.max(low.y, this.rows.total - (view.height - frozen.height));
    this.scrollX = Math.max(low.x, Math.min(maxX, this.scrollX));
    this.scrollY = Math.max(low.y, Math.min(maxY, this.scrollY));
  }

  private contentHeight() {
    const used = this.layout?.usedRows ?? 0;
    const view = this.view();
    const low = this.minScroll().y;
    return Math.max(
      this.rows.offset(Math.min(MAX_ROWS, used + 16)) - low,
      this.scrollY - low + view.height,
      view.height,
    );
  }

  private contentWidth() {
    const used = this.layout?.usedCols ?? 0;
    const view = this.view();
    const low = this.minScroll().x;
    return Math.max(
      this.cols.offset(Math.min(MAX_COLS, used + 8)) - low,
      this.scrollX - low + view.width,
      view.width,
    );
  }

  private verticalBar(): ScrollbarShape {
    const bar = this.scrollbarSize();
    const view = this.view();
    const low = this.minScroll().y;
    const content = this.contentHeight();
    const track = view.height;
    const { position, length } = thumbFor(
      this.scrollY - low,
      view.height,
      content,
      track,
      MIN_THUMB_PX * this.dpr,
    );
    return {
      x: this.width - bar,
      y: view.y,
      width: bar,
      height: track,
      thumb: position,
      length,
      vertical: true,
    };
  }

  private horizontalBar(): ScrollbarShape {
    const bar = this.scrollbarSize();
    const view = this.view();
    const low = this.minScroll().x;
    const content = this.contentWidth();
    const track = view.width;
    const { position, length } = thumbFor(
      this.scrollX - low,
      view.width,
      content,
      track,
      MIN_THUMB_PX * this.dpr,
    );
    return {
      x: view.x,
      y: this.height - bar,
      width: track,
      height: bar,
      thumb: position,
      length,
      vertical: false,
    };
  }

  private readonly composite = () => {
    this.frame = 0;
    const { canvas, context, cache, colors } = this;
    context.fillStyle = colors.background;
    context.fillRect(0, 0, canvas.width, canvas.height);
    const panes = this.panes();
    const wanted: TileAddress[] = [];
    const visible: TileAddress[] = [];
    const sheet = Math.max(this.sheet, 0);
    this.frames = [];
    for (const pane of panes) {
      if (pane.width <= 0 || pane.height <= 0) continue;
      const range: CellRange = {
        top: this.rows.at(pane.sheetY),
        left: this.cols.at(pane.sheetX),
        bottom: this.rows.at(pane.sheetY + pane.height - 1),
        right: this.cols.at(pane.sheetX + pane.width - 1),
      };
      context.save();
      context.beginPath();
      context.rect(pane.x, pane.y, pane.width, pane.height);
      context.clip();
      for (const { firstRow, firstCol } of tilesCovering(range)) {
        visible.push({ sheet, firstRow, firstCol });
        const bitmap = cache.get(sheet, firstRow, firstCol);
        if (!bitmap) continue;
        const x = this.cols.offset(firstCol) - pane.sheetX + pane.x;
        const y = this.rows.offset(firstRow) - pane.sheetY + pane.y;
        const width = this.cols.offset(firstCol + TILE_COLS) - this.cols.offset(firstCol);
        const height = this.rows.offset(firstRow + TILE_ROWS) - this.rows.offset(firstRow);
        // A stale picture can be another size; it stands in, stretched, until its
        // replacement arrives.
        if (bitmap.width === width && bitmap.height === height) {
          context.drawImage(bitmap, x, y);
        } else {
          context.drawImage(bitmap, x, y, width, height);
        }
      }
      this.drawPageBreaks(pane);
      this.drawDrawings(pane, panes.indexOf(pane));
      this.drawSelectionIn(pane);
      this.drawButtons(pane);
      context.restore();
    }
    wanted.push(...visible);
    // One tile further each way, so that scrolling meets pictures that are ready.
    const main = panes[0]!;
    const below = this.rows.at(main.sheetY + main.height - 1);
    const above = this.rows.at(main.sheetY);
    const rightmost = this.cols.at(main.sheetX + main.width - 1);
    const leftmost = this.cols.at(main.sheetX);
    const around: CellRange = {
      top: Math.max(0, above - TILE_ROWS),
      left: Math.max(0, leftmost - TILE_COLS),
      bottom: Math.min(MAX_ROWS - 1, below + TILE_ROWS),
      right: Math.min(MAX_COLS - 1, rightmost + TILE_COLS),
    };
    for (const { firstRow, firstCol } of tilesCovering(around)) {
      if (!visible.some((tile) => tile.firstRow === firstRow && tile.firstCol === firstCol)) {
        wanted.push({ sheet, firstRow, firstCol });
      }
    }
    cache.want(wanted);

    this.drawingListener?.(this.sheet, this.frames);
    for (const listener of this.frameListeners) listener();
    this.drawHeaders();
    this.drawPaneLines();
    if (this.resizing) {
      const pixel = Math.max(1, Math.round(this.dpr));
      const vertical = this.resizing.axis === 'col';
      drawResizeLine(
        context,
        colors,
        vertical,
        this.resizing.at,
        vertical ? this.height : this.width,
        pixel,
      );
    }
    this.placeEditor();
    this.reportViewport(panes[0]!);
    drawScrollbar(context, colors, this.verticalBar());
    drawScrollbar(context, colors, this.horizontalBar());
    context.fillStyle = colors.scrollTrack;
    const bar = this.scrollbarSize();
    context.fillRect(this.width - bar, this.height - bar, bar, bar);

    // For tests: the picture is complete once every tile is drawn in the current style.
    const ready =
      this.sheet >= 0 &&
      visible.length > 0 &&
      visible.every((tile) => cache.isCurrent(tile.sheet, tile.firstRow, tile.firstCol));
    canvas.dataset.ready = String(ready);
    canvas.dataset.tiles = String(visible.length);
    canvas.dataset.firstTileHash = String(cache.hash(sheet, 0, 0) ?? '');
  };

  /** Says what is in view, which calculation gets to first. */
  private reportViewport(pane: Pane) {
    if (this.sheet < 0 || pane.width <= 0 || pane.height <= 0) return;
    const range: CellRange = {
      top: this.rows.at(pane.sheetY),
      left: this.cols.at(pane.sheetX),
      bottom: this.rows.at(pane.sheetY + pane.height - 1),
      right: this.cols.at(pane.sheetX + pane.width - 1),
    };
    const key = [this.sheet, range.top, range.left, range.bottom, range.right].join(':');
    if (key !== this.lastViewport) {
      this.lastViewport = key;
      this.events.onViewport?.(this.sheet, range);
    }
  }

  private drawDrawings(pane: Pane, paneIndex: number) {
    if (!this.drawings.length) return;
    const pixel = Math.max(1, Math.round(this.dpr));
    const fontPx = Math.round(HEADER_FONT_PX * this.scale);
    const { dpr } = this;
    for (const box of this.drawings) {
      const x = box.x * this.scale - pane.sheetX + pane.x;
      const y = box.y * this.scale - pane.sheetY + pane.y;
      const width = box.width * this.scale;
      const height = box.height * this.scale;
      if (x > pane.x + pane.width || y > pane.y + pane.height) continue;
      if (x + width < pane.x || y + height < pane.y) continue;
      if (this.drawingListener && drawnByLayer(box)) {
        // Placed from the right on a right-to-left sheet, as Excel places them.
        const clip = this.shownBox(pane);
        const shown = this.shownBox({ x, width });
        this.frames.push({
          index: box.index,
          pane: paneIndex,
          clip: {
            x: clip.x / dpr,
            y: pane.y / dpr,
            width: pane.width / dpr,
            height: pane.height / dpr,
          },
          x: shown.x / dpr,
          y: y / dpr,
          width: width / dpr,
          height: height / dpr,
          zoom: this.zoom / 100,
        });
        continue;
      }
      drawPlaceholder(this.context, this.colors, box, x, y, width, height, pixel, fontPx);
    }
  }

  /**
   * Hands each frame's drawing places to the layer above the grid, which then shows charts and
   * pictures itself. `null` gives them back to the canvas's placeholders.
   */
  setDrawingListener(listener: ((sheet: number, frames: DrawingFrame[]) => void) | null) {
    this.drawingListener = listener;
    this.schedule();
  }

  /**
   * Calls back after each frame, for what is placed over cells, such as notes' boxes. Gives what
   * stops it.
   */
  onFrame(listener: () => void): () => void {
    this.frameListeners.add(listener);
    this.schedule();
    return () => {
      this.frameListeners.delete(listener);
    };
  }

  /** Scrolls as a wheel over the canvas would, for a wheel over a chart or picture. */
  wheel(event: WheelEvent) {
    this.onWheel(event);
  }

  /** The middle of the scrolling pane, in sheet pixels at 100%, where Excel puts a new chart. */
  viewCenter(): { x: number; y: number } {
    const pane = this.panes()[0]!;
    return {
      x: (pane.sheetX + pane.width / 2) / this.scale,
      y: (pane.sheetY + pane.height / 2) / this.scale,
    };
  }

  /** A cell's top-left corner in sheet pixels at 100%. 0-based. */
  cellOrigin(row: number, col: number): { x: number; y: number } {
    return { x: this.cols.offset(col) / this.scale, y: this.rows.offset(row) / this.scale };
  }

  /** The canvas, for placing things over it. */
  get element() {
    return this.canvas;
  }

  /** Where a cell is on the page, in client pixels, if it shows in the scrolling pane. 0-based. */
  cellBox(row: number, col: number): ClientBox | null {
    const pane = this.panes().find(
      (candidate) =>
        candidate.width > 0 &&
        this.cols.offset(col) >= candidate.sheetX &&
        this.cols.offset(col) < candidate.sheetX + candidate.width &&
        this.rows.offset(row) >= candidate.sheetY &&
        this.rows.offset(row) < candidate.sheetY + candidate.height,
    );
    if (!pane) return null;
    const box = this.shownBox(this.cellRect(row, col, pane));
    const bounds = this.canvas.getBoundingClientRect();
    return {
      left: bounds.left + box.x / this.dpr,
      top: bounds.top + box.y / this.dpr,
      width: box.width / this.dpr,
      height: box.height / this.dpr,
    };
  }

  /** Sets the filter buttons and the list button to draw. */
  setButtons(buttons: CellButton[]) {
    this.buttons = buttons;
    this.schedule();
  }

  /** A button's box in a pane: inside its cell's right edge, or just past it for a list. */
  private buttonBox(button: CellButton, pane: Pane) {
    const cell = this.cellRect(button.row, button.col, pane);
    const size = Math.min(cell.height, Math.round(17 * this.scale * this.dpr));
    const x = button.kind === 'list' ? cell.x + cell.width + 1 : cell.x + cell.width - size;
    return { x, y: cell.y + cell.height - size, width: size, height: size };
  }

  private drawButtons(pane: Pane) {
    if (!this.buttons.length) return;
    const pixel = Math.max(1, Math.round(this.dpr));
    for (const button of this.buttons) {
      if (this.rows.size(button.row) === 0 || this.cols.size(button.col) === 0) continue;
      const box = this.buttonBox(button, pane);
      if (box.x > pane.x + pane.width || box.y > pane.y + pane.height) continue;
      if (box.x + box.width < pane.x || box.y + box.height < pane.y) continue;
      drawCellButton(this.context, this.colors, box, button.look, pixel);
    }
  }

  /** The button under a canvas position, with its box in client pixels. */
  private buttonAt(x: number, y: number): { button: CellButton; box: ClientBox } | null {
    if (!this.buttons.length) return null;
    const bounds = this.canvas.getBoundingClientRect();
    for (const pane of this.panes()) {
      if (x < pane.x || y < pane.y || x >= pane.x + pane.width || y >= pane.y + pane.height) {
        continue;
      }
      for (const button of this.buttons) {
        const box = this.buttonBox(button, pane);
        if (x >= box.x && x < box.x + box.width && y >= box.y && y < box.y + box.height) {
          const shown = this.shownBox(box);
          return {
            button,
            box: {
              left: bounds.left + shown.x / this.dpr,
              top: bounds.top + box.y / this.dpr,
              width: box.width / this.dpr,
              height: box.height / this.dpr,
            },
          };
        }
      }
    }
    return null;
  }

  private cellRect(row: number, col: number, pane: Pane) {
    return {
      x: this.cols.offset(col) - pane.sheetX + pane.x,
      y: this.rows.offset(row) - pane.sheetY + pane.y,
      width: this.cols.size(col),
      height: this.rows.size(row),
    };
  }

  private drawSelectionIn(pane: Pane) {
    const { range } = this;
    const x = this.cols.offset(range.left) - pane.sheetX + pane.x;
    const y = this.rows.offset(range.top) - pane.sheetY + pane.y;
    const width = this.cols.offset(range.right + 1) - this.cols.offset(range.left);
    const height = this.rows.offset(range.bottom + 1) - this.rows.offset(range.top);
    const merged = this.mergeAt(this.row, this.col);
    const active = merged
      ? {
          x: this.cols.offset(merged.left) - pane.sheetX + pane.x,
          y: this.rows.offset(merged.top) - pane.sheetY + pane.y,
          width: this.cols.offset(merged.right + 1) - this.cols.offset(merged.left),
          height: this.rows.offset(merged.bottom + 1) - this.rows.offset(merged.top),
        }
      : this.cellRect(this.row, this.col, pane);
    const many =
      range.top !== range.bottom || range.left !== range.right
        ? !merged ||
          merged.top !== range.top ||
          merged.left !== range.left ||
          merged.bottom !== range.bottom ||
          merged.right !== range.right
        : false;
    const pixel = Math.max(1, Math.round(this.dpr));
    const boxOf = (area: CellRange) => ({
      x: this.cols.offset(area.left) - pane.sheetX + pane.x,
      y: this.rows.offset(area.top) - pane.sheetY + pane.y,
      width: this.cols.offset(area.right + 1) - this.cols.offset(area.left),
      height: this.rows.offset(area.bottom + 1) - this.rows.offset(area.top),
    });
    if (!this.editing) {
      for (const area of this.extra) {
        const box = boxOf(area);
        this.context.fillStyle = this.colors.selectionFill;
        this.context.fillRect(box.x, box.y, box.width, box.height);
      }
    }
    drawSelection(this.context, this.colors, { x, y, width, height }, active, many, pixel);
    if (!this.editing && !this.extra.length) {
      drawFillHandle(
        this.context,
        this.colors,
        x + width,
        y + height,
        Math.max(5, Math.round(6 * this.dpr)),
      );
    }
    if (this.copied?.sheet === this.sheet) {
      drawCopied(this.context, this.colors, boxOf(this.copied.range), pixel);
    }
    if (this.fillTarget) {
      drawFillPreview(this.context, this.colors, boxOf(this.fillTarget), pixel);
    }
    for (const highlight of this.highlights) {
      if (highlight.sheet !== this.sheet) continue;
      const color = REFERENCE_COLORS[highlight.color % REFERENCE_COLORS.length]!;
      drawReference(this.context, color, boxOf(highlight.range), pixel);
    }
  }

  private drawHeaders() {
    const { context, colors, layout } = this;
    if (layout && !layout.showHeaders) return;
    const headerWidth = this.headerWidth();
    const headerHeight = this.headerHeight();
    const pixel = Math.max(1, Math.round(this.dpr));
    context.font = Math.round(HEADER_FONT_PX * this.scale) + 'px ' + colors.fontFamily;
    context.textAlign = 'center';
    context.textBaseline = 'middle';
    const { range } = this;
    for (const pane of this.panes()) {
      if (pane.y === this.view().y && pane.width > 0) {
        // Column headers, once per horizontal stretch.
        const first = this.cols.at(pane.sheetX);
        const end = pane.sheetX + pane.width;
        context.save();
        context.beginPath();
        context.rect(pane.x, 0, pane.width, headerHeight);
        context.clip();
        for (let col = first; col < MAX_COLS; col++) {
          const start = this.cols.offset(col);
          if (start >= end) break;
          const size = this.cols.size(col);
          if (!size) continue;
          drawHeaderCell(
            context,
            colors,
            columnName(col),
            start - pane.sheetX + pane.x,
            0,
            size,
            headerHeight,
            col >= range.left && col <= range.right,
            pixel,
          );
        }
        context.restore();
      }
      if (pane.x === this.view().x && pane.height > 0) {
        const first = this.rows.at(pane.sheetY);
        const end = pane.sheetY + pane.height;
        context.save();
        context.beginPath();
        context.rect(0, pane.y, headerWidth, pane.height);
        context.clip();
        for (let row = first; row < MAX_ROWS; row++) {
          const start = this.rows.offset(row);
          if (start >= end) break;
          const size = this.rows.size(row);
          if (!size) continue;
          drawHeaderCell(
            context,
            colors,
            String(row + 1),
            0,
            start - pane.sheetY + pane.y,
            headerWidth,
            size,
            row >= range.top && row <= range.bottom,
            pixel,
          );
        }
        context.restore();
      }
    }
    drawCorner(context, colors, headerWidth, headerHeight, pixel);
  }

  private drawPaneLines() {
    const frozen = this.frozen();
    if (!frozen.rows && !frozen.cols) return;
    const { context, colors } = this;
    const view = this.view();
    const pixel = Math.max(1, Math.round(this.dpr));
    context.fillStyle = colors.paneLine;
    if (frozen.rows) {
      context.fillRect(view.x, view.y + frozen.height - pixel, view.width, pixel);
    }
    if (frozen.cols) {
      context.fillRect(view.x + frozen.width - pixel, view.y, pixel, view.height);
    }
  }

  private mergeAt(row: number, col: number): CellRange | null {
    const { merges } = this;
    for (let at = 0; at < merges.length; at += 4) {
      const top = merges[at]! - 1;
      const left = merges[at + 1]! - 1;
      const bottom = merges[at + 2]! - 1;
      const right = merges[at + 3]! - 1;
      if (row >= top && row <= bottom && col >= left && col <= right) {
        return { top, left, bottom, right };
      }
    }
    return null;
  }

  /** Grows a range until no merged area hangs over its edge. */
  private growToMerges(range: CellRange): CellRange {
    const { merges } = this;
    if (!merges.length) return range;
    const out = { ...range };
    for (let changed = true; changed;) {
      changed = false;
      for (let at = 0; at < merges.length; at += 4) {
        const top = merges[at]! - 1;
        const left = merges[at + 1]! - 1;
        const bottom = merges[at + 2]! - 1;
        const right = merges[at + 3]! - 1;
        const touches =
          top <= out.bottom && bottom >= out.top && left <= out.right && right >= out.left;
        if (
          touches &&
          (top < out.top || left < out.left || bottom > out.bottom || right > out.right)
        ) {
          out.top = Math.min(out.top, top);
          out.left = Math.min(out.left, left);
          out.bottom = Math.max(out.bottom, bottom);
          out.right = Math.max(out.right, right);
          changed = true;
        }
      }
    }
    return out;
  }

  private select(row: number, col: number, extend: boolean, reveal = true, adding = false) {
    const safeRow = Math.max(0, Math.min(MAX_ROWS - 1, row));
    const safeCol = Math.max(0, Math.min(MAX_COLS - 1, col));
    if (adding) this.extra = [...this.extra, this.range];
    else if (!extend) this.extra = [];
    if (extend) {
      this.range = this.growToMerges({
        top: Math.min(this.anchorRow, safeRow),
        left: Math.min(this.anchorCol, safeCol),
        bottom: Math.max(this.anchorRow, safeRow),
        right: Math.max(this.anchorCol, safeCol),
      });
    } else {
      const merge = this.mergeAt(safeRow, safeCol);
      this.row = merge ? merge.top : safeRow;
      this.col = merge ? merge.left : safeCol;
      this.anchorRow = this.row;
      this.anchorCol = this.col;
      this.range = merge ?? { top: this.row, left: this.col, bottom: this.row, right: this.col };
    }
    if (reveal) this.reveal(safeRow, safeCol);
    this.notifySelection();
    this.schedule();
  }

  private notifySelection() {
    const merge = this.mergeAt(this.row, this.col);
    const same =
      merge &&
      merge.top === this.range.top &&
      merge.left === this.range.left &&
      merge.bottom === this.range.bottom &&
      merge.right === this.range.right;
    this.events.onSelection({
      row: this.row,
      col: this.col,
      range: this.range,
      // A merged area goes by its first cell, as in Excel's name box.
      name: same ? cellName(this.row, this.col) : rangeName(this.range),
    });
  }

  /** Scrolls so that a cell in the scrolling area is in view. */
  private reveal(row: number, col: number) {
    const frozen = this.frozen();
    const view = this.view();
    const mainWidth = view.width - frozen.width;
    const mainHeight = view.height - frozen.height;
    if (row >= frozen.firstRow + frozen.rows) {
      const top = this.rows.offset(row);
      const bottom = top + this.rows.size(row);
      if (top < this.scrollY) this.scrollY = top;
      else if (bottom > this.scrollY + mainHeight) this.scrollY = bottom - mainHeight;
    }
    if (col >= frozen.firstCol + frozen.cols) {
      const left = this.cols.offset(col);
      const right = left + this.cols.size(col);
      if (left < this.scrollX) this.scrollX = left;
      else if (right > this.scrollX + mainWidth) this.scrollX = right - mainWidth;
    }
    this.clampScroll();
  }

  private readonly onWheel = (event: WheelEvent) => {
    event.preventDefault();
    if (event.ctrlKey || event.metaKey) {
      const step = event.deltaY < 0 ? 10 : -10;
      this.setZoom(this.zoom + step);
      return;
    }
    const unit = event.deltaMode === 1 ? WHEEL_LINE_PX : event.deltaMode === 2 ? this.height : 1;
    // A sideways swipe moves the sheet the way it shows.
    let dx = event.deltaX * unit * this.dpr * (this.rtl ? -1 : 1);
    let dy = event.deltaY * unit * this.dpr;
    if (event.shiftKey && !dx) {
      dx = dy;
      dy = 0;
    }
    this.scrollX += dx;
    this.scrollY += dy;
    this.clampScroll();
    this.schedule();
  };

  /** Tells which cell the pointer rests on, when that changes. */
  private hover(x: number, y: number, event: PointerEvent) {
    if (!this.events.onHover || this.sheet < 0) return;
    const view = this.view();
    const inside = x >= view.x && y >= view.y && x < this.width && y < this.height;
    const cell = inside ? this.cellAt(x, y) : null;
    const key = cell ? cell.row + ':' + cell.col : '';
    if (key === this.hovered) return;
    this.hovered = key;
    this.events.onHover(cell, event.clientX, event.clientY);
  }

  private pointerPosition(event: PointerEvent) {
    const bounds = this.canvas.getBoundingClientRect();
    return {
      x: this.flipX((event.clientX - bounds.left) * this.dpr),
      y: (event.clientY - bounds.top) * this.dpr,
    };
  }

  /** The cell under a canvas position, in whichever pane it falls. */
  private cellAt(x: number, y: number) {
    const frozen = this.frozen();
    const view = this.view();
    const inFrozenCols = x < view.x + frozen.width;
    const inFrozenRows = y < view.y + frozen.height;
    const sheetX = inFrozenCols
      ? x - view.x + this.cols.offset(frozen.firstCol)
      : x - (view.x + frozen.width) + this.scrollX;
    const sheetY = inFrozenRows
      ? y - view.y + this.rows.offset(frozen.firstRow)
      : y - (view.y + frozen.height) + this.scrollY;
    return { row: this.rows.at(sheetY), col: this.cols.at(sheetX) };
  }

  /** The line whose far edge is at a header position, if the pointer is close enough to it. */
  private edgeAt(x: number, y: number): { axis: 'row' | 'col'; line: number } | null {
    const view = this.view();
    const reach = Math.max(3, Math.round(4 * this.dpr));
    const frozen = this.frozen();
    if (y < view.y && x >= view.x) {
      const inFrozen = x < view.x + frozen.width;
      const sheetX = inFrozen
        ? x - view.x + this.cols.offset(frozen.firstCol)
        : x - (view.x + frozen.width) + this.scrollX;
      const col = this.cols.at(sheetX);
      if (this.cols.offset(col + 1) - sheetX <= reach) return { axis: 'col', line: col };
      if (sheetX - this.cols.offset(col) <= reach && col > 0) {
        return { axis: 'col', line: this.cols.previousVisible(col - 1) };
      }
    }
    if (x < view.x && y >= view.y) {
      const inFrozen = y < view.y + frozen.height;
      const sheetY = inFrozen
        ? y - view.y + this.rows.offset(frozen.firstRow)
        : y - (view.y + frozen.height) + this.scrollY;
      const row = this.rows.at(sheetY);
      if (this.rows.offset(row + 1) - sheetY <= reach) return { axis: 'row', line: row };
      if (sheetY - this.rows.offset(row) <= reach && row > 0) {
        return { axis: 'row', line: this.rows.previousVisible(row - 1) };
      }
    }
    return null;
  }

  /** The lines a resize of this one applies to: all that are selected whole, if it's one of them. */
  private linesFor(axis: 'row' | 'col', line: number): [number, number][] {
    const areas = [...this.extra, this.range];
    const whole = (area: CellRange) =>
      axis === 'row'
        ? area.left === 0 && area.right === MAX_COLS - 1
        : area.top === 0 && area.bottom === MAX_ROWS - 1;
    const first = (area: CellRange) => (axis === 'row' ? area.top : area.left);
    const last = (area: CellRange) => (axis === 'row' ? area.bottom : area.right);
    const holds = areas.some((area) => whole(area) && line >= first(area) && line <= last(area));
    if (!holds) return [[line + 1, line + 1]];
    return areas.filter(whole).map((area) => [first(area) + 1, last(area) + 1]);
  }

  /** Whether a position is on the fill handle. */
  private onFillHandle(x: number, y: number) {
    if (this.editing || this.extra.length) return false;
    const panes = this.panes();
    const size = Math.max(6, Math.round(8 * this.dpr));
    return panes.some((pane) => {
      const cornerX = this.cols.offset(this.range.right + 1) - pane.sheetX + pane.x;
      const cornerY = this.rows.offset(this.range.bottom + 1) - pane.sheetY + pane.y;
      const inside =
        cornerX >= pane.x &&
        cornerX <= pane.x + pane.width + size &&
        cornerY >= pane.y &&
        cornerY <= pane.y + pane.height + size;
      return inside && Math.abs(x - cornerX) <= size && Math.abs(y - cornerY) <= size;
    });
  }

  /** While editing, a click on the sheet must leave the keyboard with the editor. */
  private readonly onMouseDown = (event: MouseEvent) => {
    if (this.editing) event.preventDefault();
  };

  private readonly onDoubleClick = (event: MouseEvent) => {
    const bounds = this.canvas.getBoundingClientRect();
    const x = this.flipX((event.clientX - bounds.left) * this.dpr);
    const y = (event.clientY - bounds.top) * this.dpr;
    if (this.onFillHandle(x, y)) {
      this.events.onFill?.(this.range, null);
      return;
    }
    const edge = this.edgeAt(x, y);
    if (edge) {
      this.events.onAutoFit?.(edge.axis, this.linesFor(edge.axis, edge.line));
      return;
    }
    const view = this.view();
    if (x >= view.x && y >= view.y && !this.editing) {
      const cell = this.cellAt(x, y);
      if (cell && this.events.onDoubleCell?.(cell.row, cell.col)) return;
      this.events.onAction?.('edit');
    }
  };

  private readonly onPointerDown = (event: PointerEvent) => {
    if (event.button !== 0) return;
    const { x, y } = this.pointerPosition(event);
    if (!this.editing) this.focus();
    const vertical = this.verticalBar();
    const horizontal = this.horizontalBar();
    this.canvas.setPointerCapture(event.pointerId);
    if (x >= vertical.x && y >= vertical.y && y < vertical.y + vertical.height) {
      const along = y - vertical.y;
      if (along < vertical.thumb || along > vertical.thumb + vertical.length) {
        const page = this.view().height;
        this.scrollY += along < vertical.thumb ? -page : page;
        this.clampScroll();
        this.schedule();
        return;
      }
      this.dragging = 'vertical';
      this.dragOffset = along - vertical.thumb;
      return;
    }
    if (y >= horizontal.y && x >= horizontal.x && x < horizontal.x + horizontal.width) {
      const along = x - horizontal.x;
      if (along < horizontal.thumb || along > horizontal.thumb + horizontal.length) {
        const page = this.view().width;
        this.scrollX += along < horizontal.thumb ? -page : page;
        this.clampScroll();
        this.schedule();
        return;
      }
      this.dragging = 'horizontal';
      this.dragOffset = along - horizontal.thumb;
      return;
    }
    const view = this.view();
    const hit = this.editing ? null : this.buttonAt(x, y);
    if (hit) {
      this.events.onCellButton?.(hit.button, hit.box);
      return;
    }
    const edge = this.editing ? null : this.edgeAt(x, y);
    if (edge) {
      const axis = edge.axis === 'col' ? this.cols : this.rows;
      const size = axis.size(edge.line);
      this.dragging = 'resize';
      this.resizing = {
        axis: edge.axis,
        line: edge.line,
        start: edge.axis === 'col' ? x - size : y - size,
        at: edge.axis === 'col' ? x : y,
      };
      this.schedule();
      return;
    }
    if (this.onFillHandle(x, y)) {
      this.dragging = 'fill';
      this.fillTarget = this.range;
      this.lastPointer = { x, y };
      return;
    }
    if (x < view.x && y < view.y) {
      // The corner selects the whole sheet.
      this.extra = [];
      this.row = 0;
      this.col = 0;
      this.anchorRow = 0;
      this.anchorCol = 0;
      this.range = { top: 0, left: 0, bottom: MAX_ROWS - 1, right: MAX_COLS - 1 };
      this.notifySelection();
      this.schedule();
      return;
    }
    const { row, col } = this.cellAt(Math.max(x, view.x), Math.max(y, view.y));
    // While a formula is being typed, a click on the sheet is a reference.
    if (this.editing && this.events.onEditClick?.(row, col)) {
      this.dragging = 'point';
      this.lastPointer = { x, y };
      return;
    }
    if (x < view.x) {
      if (!event.ctrlKey && !event.metaKey) this.extra = [];
      else this.extra = [...this.extra, this.range];
      this.row = row;
      this.col = this.cols.nextVisible(0);
      this.anchorRow = row;
      this.anchorCol = this.col;
      this.range = { top: row, left: 0, bottom: row, right: MAX_COLS - 1 };
      this.dragging = 'rows';
      this.notifySelection();
      this.schedule();
      return;
    }
    if (y < view.y) {
      if (!event.ctrlKey && !event.metaKey) this.extra = [];
      else this.extra = [...this.extra, this.range];
      this.col = col;
      this.row = this.rows.nextVisible(0);
      this.anchorRow = this.row;
      this.anchorCol = col;
      this.range = { top: 0, left: col, bottom: MAX_ROWS - 1, right: col };
      this.dragging = 'cols';
      this.notifySelection();
      this.schedule();
      return;
    }
    this.dragging = 'cells';
    this.lastPointer = { x, y };
    this.select(row, col, event.shiftKey, true, event.ctrlKey || event.metaKey);
    if (!event.shiftKey && !event.ctrlKey && !event.metaKey) this.events.onCellClick?.(row, col);
  };

  private readonly onPointerMove = (event: PointerEvent) => {
    const { x, y } = this.pointerPosition(event);
    if (!this.dragging) {
      if (event.buttons === 0) {
        this.setCursor(x, y);
        this.hover(x, y, event);
      }
      return;
    }
    this.lastPointer = { x, y };
    if (this.dragging === 'resize' && this.resizing) {
      const { axis, start } = this.resizing;
      const at = Math.max(start, axis === 'col' ? x : y);
      this.resizing.at = at;
      this.events.onResizeHint?.({
        axis,
        px: Math.round((at - start) / this.scale),
        clientX: event.clientX,
        clientY: event.clientY,
      });
      this.schedule();
      return;
    }
    if (this.dragging === 'vertical') {
      const bar = this.verticalBar();
      const low = this.minScroll().y;
      this.scrollY =
        low +
        scrollForThumb(
          y - bar.y - this.dragOffset,
          bar.length,
          bar.height,
          this.contentHeight(),
          this.view().height,
        );
      this.clampScroll();
      this.schedule();
      return;
    }
    if (this.dragging === 'horizontal') {
      const bar = this.horizontalBar();
      const low = this.minScroll().x;
      this.scrollX =
        low +
        scrollForThumb(
          x - bar.x - this.dragOffset,
          bar.length,
          bar.width,
          this.contentWidth(),
          this.view().width,
        );
      this.clampScroll();
      this.schedule();
      return;
    }
    if (this.dragging === 'fill') {
      this.fillTarget = this.fillTargetFor(x, y);
      this.startAutoScroll();
      this.schedule();
      return;
    }
    if (this.dragging === 'point') {
      const view = this.view();
      const { row, col } = this.cellAt(
        Math.max(view.x, Math.min(view.x + view.width - 1, x)),
        Math.max(view.y, Math.min(view.y + view.height - 1, y)),
      );
      this.events.onEditDrag?.(row, col);
      return;
    }
    if (this.dragging === 'rows' || this.dragging === 'cols') {
      const view = this.view();
      const { row, col } = this.cellAt(
        Math.max(view.x, Math.min(view.x + view.width - 1, x)),
        Math.max(view.y, Math.min(view.y + view.height - 1, y)),
      );
      if (this.dragging === 'rows') {
        this.range = {
          top: Math.min(this.anchorRow, row),
          left: 0,
          bottom: Math.max(this.anchorRow, row),
          right: MAX_COLS - 1,
        };
      } else {
        this.range = {
          top: 0,
          left: Math.min(this.anchorCol, col),
          bottom: MAX_ROWS - 1,
          right: Math.max(this.anchorCol, col),
        };
      }
      this.notifySelection();
      this.schedule();
      return;
    }
    this.extendTo(x, y);
    this.startAutoScroll();
  };

  /** Where a fill reaches for this position: along whichever way the pointer has gone further. */
  private fillTargetFor(x: number, y: number): CellRange {
    const view = this.view();
    const { row, col } = this.cellAt(
      Math.max(view.x, Math.min(view.x + view.width - 1, x)),
      Math.max(view.y, Math.min(view.y + view.height - 1, y)),
    );
    const { range } = this;
    const below = row > range.bottom ? row - range.bottom : row < range.top ? range.top - row : 0;
    const beside = col > range.right ? col - range.right : col < range.left ? range.left - col : 0;
    if (!below && !beside) {
      // Dragged back inside: what it leaves behind goes.
      return { ...range, bottom: Math.max(range.top, row), right: range.right };
    }
    if (below >= beside) {
      return {
        ...range,
        top: Math.min(range.top, row),
        bottom: Math.max(range.bottom, row),
      };
    }
    return {
      ...range,
      left: Math.min(range.left, col),
      right: Math.max(range.right, col),
    };
  }

  private extendTo(x: number, y: number) {
    const view = this.view();
    const { row, col } = this.cellAt(
      Math.max(view.x, Math.min(view.x + view.width - 1, x)),
      Math.max(view.y, Math.min(view.y + view.height - 1, y)),
    );
    this.select(row, col, true);
  }

  /** Keeps scrolling while a drag holds the pointer past the sheet's edge. */
  private startAutoScroll() {
    if (this.autoScroll) return;
    const tick = () => {
      this.autoScroll = 0;
      if (this.dragging !== 'cells' && this.dragging !== 'fill') return;
      const view = this.view();
      const { x, y } = this.lastPointer;
      const dy = y < view.y ? y - view.y : y > view.y + view.height ? y - view.y - view.height : 0;
      const dx = x < view.x ? x - view.x : x > view.x + view.width ? x - view.x - view.width : 0;
      if (!dx && !dy) return;
      this.scrollX += dx / 4;
      this.scrollY += dy / 4;
      this.clampScroll();
      if (this.dragging === 'fill') {
        this.fillTarget = this.fillTargetFor(x, y);
        this.schedule();
      } else {
        this.extendTo(x, y);
      }
      this.autoScroll = requestAnimationFrame(tick);
    };
    this.autoScroll = requestAnimationFrame(tick);
  }

  /** Says what a drag from here would do, by the cursor. */
  private setCursor(x: number, y: number) {
    const edge = this.editing ? null : this.edgeAt(x, y);
    const view = this.view();
    let cursor = '';
    if (edge) cursor = edge.axis === 'col' ? 'col-resize' : 'row-resize';
    else if (this.painter) cursor = 'copy';
    else if (x >= view.x && y >= view.y && this.onFillHandle(x, y)) cursor = 'crosshair';
    if (this.canvas.style.cursor !== cursor) this.canvas.style.cursor = cursor;
  }

  private readonly onContextMenu = (event: MouseEvent) => {
    const bounds = this.canvas.getBoundingClientRect();
    const x = this.flipX((event.clientX - bounds.left) * this.dpr);
    const y = (event.clientY - bounds.top) * this.dpr;
    const view = this.view();
    const bar = this.scrollbarSize();
    if (x >= this.width - bar || y >= this.height - bar) return;
    event.preventDefault();
    if (this.editing || (x < view.x && y < view.y)) return;
    const { row, col } = this.cellAt(Math.max(x, view.x), Math.max(y, view.y));
    const inAny = (test: (area: CellRange) => boolean) => [...this.extra, this.range].some(test);
    let region: GridRegion = 'cells';
    if (x < view.x) {
      region = 'rowHeader';
      const held = inAny(
        (area) =>
          area.left === 0 && area.right === MAX_COLS - 1 && row >= area.top && row <= area.bottom,
      );
      if (!held) this.selectArea({ top: row, left: 0, bottom: row, right: MAX_COLS - 1 }, row, 0);
    } else if (y < view.y) {
      region = 'colHeader';
      const held = inAny(
        (area) =>
          area.top === 0 && area.bottom === MAX_ROWS - 1 && col >= area.left && col <= area.right,
      );
      if (!held) this.selectArea({ top: 0, left: col, bottom: MAX_ROWS - 1, right: col }, 0, col);
    } else {
      const held = inAny(
        (area) => row >= area.top && row <= area.bottom && col >= area.left && col <= area.right,
      );
      if (!held) this.select(row, col, false, false);
    }
    this.focus();
    this.events.onContextMenu?.(event.clientX, event.clientY, region);
  };

  /** Opens the context menu from the keyboard, at the active cell. */
  contextMenuAtActive() {
    const point = this.pointForCell(this.row, this.col);
    const bounds = this.canvas.getBoundingClientRect();
    this.events.onContextMenu?.(bounds.left + point.x, bounds.top + point.y, 'cells');
  }

  private readonly onPointerUp = () => {
    if (this.dragging === 'resize' && this.resizing) {
      const { axis, line, start, at } = this.resizing;
      this.resizing = null;
      this.dragging = null;
      this.events.onResizeHint?.(null);
      const size = (axis === 'col' ? this.cols : this.rows).size(line);
      if (at - start !== size) {
        this.events.onResize?.(
          axis,
          this.linesFor(axis, line),
          Math.round((at - start) / this.scale),
        );
      }
      this.schedule();
      return;
    }
    const selecting =
      this.dragging === 'cells' || this.dragging === 'rows' || this.dragging === 'cols';
    if (selecting) this.events.onSelectEnd?.();
    if (this.dragging === 'fill' && this.fillTarget) {
      const from = this.range;
      const to = this.fillTarget;
      this.fillTarget = null;
      const same =
        to.top === from.top &&
        to.left === from.left &&
        to.bottom === from.bottom &&
        to.right === from.right;
      if (!same) {
        this.events.onFill?.(from, to);
        this.range = to;
        this.notifySelection();
      }
      this.schedule();
    }
    this.dragging = null;
  };

  /** Columns an arrow key moves by, as it points on screen. */
  columnsFor(key: string) {
    const cols = key === 'ArrowRight' ? 1 : key === 'ArrowLeft' ? -1 : 0;
    return this.rtl ? -cols : cols;
  }

  /** One line from this one, skipping hidden lines, or leaving a merged area. */
  private stepped(axis: Axis, line: number, direction: number, maximum: number) {
    const next = line + direction;
    if (next < 0 || next >= maximum) return line;
    return direction > 0 ? axis.nextVisible(next) : axis.previousVisible(next);
  }

  private readonly onKeyDown = (event: KeyboardEvent) => {
    const action = actionFor(event);
    if (action) {
      // The browser's paste event is what brings the clipboard, without asking for permission.
      if (action !== 'paste' && action !== 'pasteValues') event.preventDefault();
      this.events.onAction?.(action);
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key.startsWith('Arrow')) {
      event.preventDefault();
      const rows = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
      const cols = this.columnsFor(event.key);
      this.events.onJump?.(rows, cols, event.shiftKey);
      return;
    }
    if (startsTyping(event)) {
      // Not prevented: the key's default goes to the editor, which takes focus here.
      this.events.onType?.();
      return;
    }
    if (event.key === 'Backspace') {
      this.events.onType?.();
      event.preventDefault();
      return;
    }
    const extend = event.shiftKey;
    const focusRow = extend
      ? this.anchorRow === this.range.top
        ? this.range.bottom
        : this.range.top
      : this.row;
    const focusCol = extend
      ? this.anchorCol === this.range.left
        ? this.range.right
        : this.range.left
      : this.col;
    const view = this.view();
    let row = focusRow;
    let col = focusCol;
    const merge = extend ? null : this.mergeAt(this.row, this.col);
    switch (event.key) {
      case 'ArrowDown':
        row = this.stepped(this.rows, merge ? merge.bottom : focusRow, 1, MAX_ROWS);
        break;
      case 'ArrowUp':
        row = this.stepped(this.rows, merge ? merge.top : focusRow, -1, MAX_ROWS);
        break;
      case 'ArrowRight':
      case 'ArrowLeft': {
        // An arrow goes the way it points, which is towards column A on a right-to-left sheet.
        const step = this.columnsFor(event.key);
        const from = merge ? (step > 0 ? merge.right : merge.left) : focusCol;
        col = this.stepped(this.cols, from, step, MAX_COLS);
        break;
      }
      case 'Tab':
        event.preventDefault();
        this.moveBy(0, event.shiftKey ? -1 : 1);
        return;
      case 'Enter':
        event.preventDefault();
        this.moveBy(event.shiftKey ? -1 : 1, 0);
        return;
      case 'PageDown': {
        const page = view.height;
        const target = this.rows.at(this.rows.offset(focusRow) + page);
        this.scrollY += page;
        this.clampScroll();
        row = target;
        break;
      }
      case 'PageUp': {
        const page = view.height;
        const target = this.rows.at(Math.max(0, this.rows.offset(focusRow) - page));
        this.scrollY -= page;
        this.clampScroll();
        row = target;
        break;
      }
      case 'Home':
        col = 0;
        if (event.ctrlKey || event.metaKey) row = 0;
        break;
      case 'End':
        if (event.ctrlKey || event.metaKey) {
          row = Math.max(0, (this.layout?.usedRows ?? 1) - 1);
          col = Math.max(0, (this.layout?.usedCols ?? 1) - 1);
        } else {
          return;
        }
        break;
      default:
        return;
    }
    event.preventDefault();
    this.select(row, col, extend);
  };
}

/** A layout's 1-based runs of (first, last, pixels) as the axis takes them, from 0. */
function zeroBased(runs: Uint32Array, count: number) {
  const out = new Int32Array(runs.length);
  for (let at = 0; at < runs.length; at += 3) {
    out[at] = runs[at]! - 1;
    out[at + 1] = Math.min(runs[at + 1]!, count) - 1;
    out[at + 2] = runs[at + 2]!;
  }
  return out;
}
