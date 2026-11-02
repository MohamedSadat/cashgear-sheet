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
