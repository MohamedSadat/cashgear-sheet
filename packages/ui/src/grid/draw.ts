// What the grid draws on top of its tiles: headers, selection, panes, scrollbars and
// placeholders for drawings. Pure apart from the 2D context it is given.
import type { DrawingBox } from '@cgs/engine-client';

export interface GridColors {
  background: string;
  gridline: string;
  text: string;
  textMuted: string;
  headerBackground: string;
  headerSelected: string;
  headerSelectedText: string;
  accent: string;
  selectionFill: string;
  scrollTrack: string;
  scrollThumb: string;
  paneLine: string;
  placeholderFill: string;
  placeholderLine: string;
  fontFamily: string;
}

export type GridContext = Pick<
  CanvasRenderingContext2D,
  | 'beginPath'
  | 'clip'
  | 'fillRect'
  | 'fillStyle'
  | 'fill'
  | 'fillText'
  | 'font'
  | 'lineTo'
  | 'lineWidth'
  | 'moveTo'
  | 'rect'
  | 'restore'
  | 'save'
  | 'setLineDash'
  | 'stroke'
  | 'strokeRect'
  | 'strokeStyle'
  | 'textAlign'
  | 'textBaseline'
>;

export function drawHeaderCell(
  context: GridContext,
  colors: GridColors,
  label: string,
  x: number,
  y: number,
  width: number,
  height: number,
  selected: boolean,
  pixel: number,
) {
  context.fillStyle = selected ? colors.headerSelected : colors.headerBackground;
  context.fillRect(x, y, width, height);
  context.fillStyle = colors.gridline;
  context.fillRect(x + width - pixel, y, pixel, height);
  context.fillRect(x, y + height - pixel, width, pixel);
  context.fillStyle = selected ? colors.headerSelectedText : colors.text;
  context.fillText(label, x + width / 2, y + height / 2);
}

export function drawCorner(
  context: GridContext,
  colors: GridColors,
  width: number,
  height: number,
  pixel: number,
) {
  context.fillStyle = colors.headerBackground;
  context.fillRect(0, 0, width, height);
  context.fillStyle = colors.gridline;
  context.fillRect(width - pixel, 0, pixel, height);
  context.fillRect(0, height - pixel, width, pixel);
}

export interface ScrollbarShape {
  x: number;
  y: number;
  width: number;
  height: number;
  thumb: number;
  length: number;
  vertical: boolean;
}

export function drawScrollbar(context: GridContext, colors: GridColors, bar: ScrollbarShape) {
  context.fillStyle = colors.scrollTrack;
  context.fillRect(bar.x, bar.y, bar.width, bar.height);
  context.fillStyle = colors.scrollThumb;
  const inset = Math.max(1, Math.round((bar.vertical ? bar.width : bar.height) / 5));
  if (bar.vertical) {
    context.fillRect(bar.x + inset, bar.y + bar.thumb, bar.width - inset * 2, bar.length);
  } else {
    context.fillRect(bar.x + bar.thumb, bar.y + inset, bar.length, bar.height - inset * 2);
  }
}

/** A selection's rectangle in screen pixels, already clipped by the caller. */
export function drawSelection(
  context: GridContext,
  colors: GridColors,
  range: { x: number; y: number; width: number; height: number },
  active: { x: number; y: number; width: number; height: number },
  many: boolean,
  pixel: number,
) {
  if (many) {
    context.fillStyle = colors.selectionFill;
    context.fillRect(range.x, range.y, range.width, range.height);
    // The active cell stays clear, as in Excel.
    context.fillStyle = colors.background;
  }
  context.strokeStyle = colors.accent;
  context.setLineDash([]);
  context.lineWidth = pixel * 2;
  context.strokeRect(range.x - pixel / 2, range.y - pixel / 2, range.width, range.height);
  if (many) {
    context.lineWidth = pixel;
    context.strokeRect(active.x, active.y, active.width - pixel, active.height - pixel);
  }
}

/** Colours for the references in a formula being typed, one each, as in Excel's. */
export const REFERENCE_COLORS = [
  '#2f6fdb',
  '#d6373b',
  '#7a3bc4',
  '#18864b',
  '#c4478f',
  '#b4681a',
  '#17879a',
];

/** A reference in the formula being typed, marked on the sheet. */
export function drawReference(
  context: GridContext,
  color: string,
  box: { x: number; y: number; width: number; height: number },
  pixel: number,
) {
  context.strokeStyle = color;
  context.setLineDash([]);
  context.lineWidth = pixel * 2;
  context.strokeRect(box.x, box.y, box.width - pixel, box.height - pixel);
}

/** What was copied or cut, with a dashed edge. */
export function drawCopied(
  context: GridContext,
  colors: GridColors,
  box: { x: number; y: number; width: number; height: number },
  pixel: number,
) {
  context.strokeStyle = colors.accent;
  context.lineWidth = pixel * 2;
  context.setLineDash([pixel * 4, pixel * 3]);
  context.strokeRect(box.x, box.y, box.width - pixel, box.height - pixel);
  context.setLineDash([]);
}

/** A line showing where a resize would end, while it is dragged. */
export function drawResizeLine(
  context: GridContext,
  colors: GridColors,
  vertical: boolean,
  at: number,
  length: number,
  pixel: number,
) {
  context.fillStyle = colors.accent;
  if (vertical) context.fillRect(at, 0, pixel, length);
  else context.fillRect(0, at, length, pixel);
}

/** The square at a selection's corner that starts a fill. */
export function drawFillHandle(
  context: GridContext,
  colors: GridColors,
  x: number,
  y: number,
  size: number,
) {
  context.fillStyle = colors.background;
  context.fillRect(x - size / 2 - 1, y - size / 2 - 1, size + 2, size + 2);
  context.fillStyle = colors.accent;
  context.fillRect(x - size / 2, y - size / 2, size, size);
}

/** What a button in a cell shows: a filter's arrow, a filtered funnel, a sort, or a list's arrow. */
export type CellButtonLook = 'arrow' | 'filtered' | 'sortedUp' | 'sortedDown';

/** A filter's button, or a validation list's, as Excel 2016 draws them. */
export function drawCellButton(
  context: GridContext,
  colors: GridColors,
  box: { x: number; y: number; width: number; height: number },
  look: CellButtonLook,
  pixel: number,
) {
  const { x, y, width, height } = box;
  context.fillStyle = colors.headerBackground;
  context.fillRect(x, y, width, height);
  context.strokeStyle = colors.gridline;
  context.lineWidth = pixel;
  context.strokeRect(x + pixel / 2, y + pixel / 2, width - pixel, height - pixel);
  context.fillStyle = colors.text;
  const middle = x + width / 2;
  const size = Math.max(2, Math.round(width / 5));
  const arrow = (left: number, top: number) => {
    context.beginPath();
    context.moveTo(left - size, top);
    context.lineTo(left + size, top);
    context.lineTo(left, top + size);
    context.fill();
  };
  if (look === 'arrow') {
    arrow(middle, y + height / 2 - size / 2);
    return;
  }
  // A filtered column's funnel, or a sort's arrow, beside a smaller drop arrow.
  const left = x + width * 0.32;
  if (look === 'filtered') {
    context.beginPath();
    context.moveTo(left - size, y + height * 0.3);
    context.lineTo(left + size, y + height * 0.3);
    context.lineTo(left + pixel, y + height * 0.55);
    context.lineTo(left + pixel, y + height * 0.75);
    context.lineTo(left - pixel, y + height * 0.75);
    context.lineTo(left - pixel, y + height * 0.55);
    context.fill();
  } else {
    const top = y + height * 0.25;
    const bottom = y + height * 0.75;
    context.fillRect(left - pixel / 2, top, pixel, bottom - top);
    context.beginPath();
    const tip = look === 'sortedUp' ? top : bottom;
    const base = look === 'sortedUp' ? top + size : bottom - size;
    context.moveTo(left - size, base);
    context.lineTo(left + size, base);
    context.lineTo(left, tip);
    context.fill();
  }
  arrow(x + width * 0.72, y + height / 2 - size / 2);
}

/** Where a fill would reach, while it is being dragged. */
export function drawFillPreview(
  context: GridContext,
  colors: GridColors,
  box: { x: number; y: number; width: number; height: number },
  pixel: number,
) {
  context.strokeStyle = colors.textMuted;
  context.lineWidth = pixel;
  context.setLineDash([pixel * 3, pixel * 3]);
  context.strokeRect(box.x, box.y, box.width - pixel, box.height - pixel);
  context.setLineDash([]);
}

export const DRAWING_LABELS: Record<DrawingBox['kind'], string> = {
  chart: 'Chart',
  picture: 'Picture',
  shape: 'Shape',
  group: 'Group',
  other: 'Object',
};

/** A chart or image's place on the sheet, until Phase 7 draws the real thing. */
export function drawPlaceholder(
  context: GridContext,
  colors: GridColors,
  box: DrawingBox,
  x: number,
  y: number,
  width: number,
  height: number,
  pixel: number,
  fontPx: number,
) {
  context.save();
  context.beginPath();
  context.rect(x, y, width, height);
  context.clip();
  context.fillStyle = colors.placeholderFill;
  context.fillRect(x, y, width, height);
  context.strokeStyle = colors.placeholderLine;
  context.lineWidth = pixel;
  context.setLineDash([pixel * 4, pixel * 3]);
  context.strokeRect(x + pixel / 2, y + pixel / 2, width - pixel, height - pixel);
  context.setLineDash([]);
  context.beginPath();
  context.moveTo(x, y);
  context.lineTo(x + width, y + height);
  context.moveTo(x + width, y);
  context.lineTo(x, y + height);
  context.lineWidth = pixel;
  context.stroke();
  context.fillStyle = colors.text;
  context.font = fontPx + 'px ' + colors.fontFamily;
  context.textAlign = 'center';
  context.textBaseline = 'middle';
  const label = DRAWING_LABELS[box.kind];
  context.fillText(box.name ? label + ': ' + box.name : label, x + width / 2, y + height / 2);
  context.restore();
}
