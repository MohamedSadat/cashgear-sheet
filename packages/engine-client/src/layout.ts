// Reads `Engine::layout_numbers` in crates/cgs-engine/src/lib.rs, so keep the two in step.

export interface SheetLayout {
  defaultRowPx: number;
  defaultColPx: number;
  usedRows: number;
  usedCols: number;
  zoom: number;
  showGrid: boolean;
  showHeaders: boolean;
  frozenRows: number;
  frozenCols: number;
  /** 1-based: the first row and column of the frozen area. */
  firstRow: number;
  firstCol: number;
  /** 1-based: the first visible cell of the scrolling area. */
  topRow: number;
  leftCol: number;
  activeRow: number;
  activeCol: number;
  /** Pairs of 1-based row and pixels, sorted. */
  rows: Uint32Array;
  /** Triples of 1-based first column, last column and pixels, sorted. */
  cols: Uint32Array;
  /** Fours of 1-based top, left, bottom and right. */
  merges: Uint32Array;
}

const HEADER = 18;

export function decodeLayout(numbers: Uint32Array): SheetLayout {
  if (numbers.length < HEADER) throw new Error('This is not a sheet layout.');
  const [
    defaultRowPx,
    defaultColPx,
    usedRows,
    usedCols,
    zoom,
    showGrid,
    showHeaders,
    frozenRows,
    frozenCols,
    firstRow,
    firstCol,
    topRow,
    leftCol,
    activeRow,
    activeCol,
    rowCount,
    colCount,
    mergeCount,
  ] = numbers as unknown as number[] & { length: 18 };
  const rowsEnd = HEADER + rowCount! * 2;
  const colsEnd = rowsEnd + colCount! * 3;
  return {
    defaultRowPx: defaultRowPx!,
    defaultColPx: defaultColPx!,
    usedRows: usedRows!,
    usedCols: usedCols!,
    zoom: zoom!,
    showGrid: showGrid === 1,
    showHeaders: showHeaders === 1,
    frozenRows: frozenRows!,
    frozenCols: frozenCols!,
    firstRow: firstRow!,
    firstCol: firstCol!,
    topRow: topRow!,
    leftCol: leftCol!,
    activeRow: activeRow!,
    activeCol: activeCol!,
    rows: numbers.subarray(HEADER, rowsEnd),
    cols: numbers.subarray(rowsEnd, colsEnd),
    merges: numbers.subarray(colsEnd, colsEnd + mergeCount! * 4),
  };
}
