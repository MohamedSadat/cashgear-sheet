// Ranges as the embed takes and answers them: A1 notation without a sheet or `$`, 1-based areas,
// and the used range cut to the cell limit (decisions, 2026-11-14, Track S3 to S5).
import type { CellArea } from '@cgs/engine-client';
import { MAX_CELLS, parseCell } from './contract';
import { columnName } from '../grid/geometry';

/** A range's area, top-left first whichever way its corners came. Null when it isn't one. */
export function parseRange(text: string): CellArea | null {
  const [first, second = first] = text.split(':');
  const a = first ? parseCell(first) : null;
  const b = second ? parseCell(second) : null;
  if (!a || !b) return null;
  return {
    top: Math.min(a.row, b.row),
    left: Math.min(a.col, b.col),
    bottom: Math.max(a.row, b.row),
    right: Math.max(a.col, b.col),
  };
}

/** `A1`, or `A1:D20`. */
export function rangeText(area: CellArea): string {
  const first = columnName(area.left - 1) + area.top;
  if (area.top === area.bottom && area.left === area.right) return first;
  return first + ':' + columnName(area.right - 1) + area.bottom;
}

export const cellCount = (area: CellArea) =>
  (area.bottom - area.top + 1) * (area.right - area.left + 1);

/** Columns a cut used range keeps at least, when it has more. */
const KEPT_COLUMNS = 50;

/**
 * The used range cut to `limit` cells from its top-left corner: every column when there are at
 * most 50, else `max(50, floor(limit / rows))` of them, then the rows that fit.
 */
export function capArea(area: CellArea, limit = MAX_CELLS): { area: CellArea; capped: boolean } {
  if (cellCount(area) <= limit) return { area, capped: false };
  const rows = area.bottom - area.top + 1;
  const cols = area.right - area.left + 1;
  const keptCols =
    cols <= KEPT_COLUMNS
      ? cols
      : Math.min(cols, limit, Math.max(KEPT_COLUMNS, Math.floor(limit / rows)));
  const keptRows = Math.min(rows, Math.max(1, Math.floor(limit / keptCols)));
  return {
    area: {
      top: area.top,
      left: area.left,
      bottom: area.top + keptRows - 1,
      right: area.left + keptCols - 1,
    },
    capped: true,
  };
}
