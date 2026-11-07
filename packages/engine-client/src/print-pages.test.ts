import { describe, expect, it } from 'vitest';
import { Axis, replaceErrors } from './print-pages';
import { cellText, KIND_OTHER, KIND_TEXT, type ViewBlock } from './view-block';

function block(cells: [string, number][]): ViewBlock {
  const encoder = new TextEncoder();
  const pieces = cells.map(([text]) => encoder.encode(text));
  const heap = new Uint8Array(pieces.reduce((n, p) => n + p.length, 0));
  const data = new Uint32Array(cells.length * 6);
  let at = 0;
  cells.forEach(([, kind], index) => {
    heap.set(pieces[index]!, at);
    data[index * 6] = at;
    data[index * 6 + 1] = pieces[index]!.length;
    data[index * 6 + 4] = kind << 16;
    at += pieces[index]!.length;
  });
  return {
    rows: 1,
    cols: cells.length,
    firstRow: 0,
    firstCol: 0,
    flags: 0,
    rowPx: new Uint16Array([20]),
    colPx: new Uint16Array(cells.length).fill(64),
    cells: data,
    styles: [],
    extras: [],
    runs: new Map(),
    marks: [],
    heap,
  };
}

describe('pages', () => {
  it('prints errors as Page Setup says, and leaves text that looks like one', () => {
    const source = block([
      ['#DIV/0!', KIND_OTHER],
      ['TRUE', KIND_OTHER],
      ['#N/A', KIND_TEXT],
      ['#REF!', KIND_OTHER],
    ]);
    expect(replaceErrors(source, 'displayed')).toBe(source);
    const dashed = replaceErrors(source, 'dash');
    expect([0, 1, 2, 3].map((col) => cellText(dashed, 0, col))).toEqual([
      '--',
      'TRUE',
      '#N/A',
      '--',
    ]);
    expect(cellText(replaceErrors(source, 'blank'), 0, 0)).toBe('');
    expect(cellText(replaceErrors(source, 'NA'), 0, 3)).toBe('#N/A');
    // The tile itself is left as it was.
    expect(cellText(source, 0, 0)).toBe('#DIV/0!');
  });

  it('sums sizes over stretches of runs', () => {
    // Rows 3 to 4 are 30 pixels and row 10 is hidden; the rest are 20.
    const axis = new Axis(new Uint32Array([3, 4, 30, 10, 10, 0]), 20);
    expect(axis.size(3)).toBe(30);
    expect(axis.size(10)).toBe(0);
    expect(axis.size(11)).toBe(20);
    expect(axis.sum(1, 10)).toBe(20 * 7 + 60);
    expect(axis.offset(5, 2)).toBe(-(20 + 60));
    expect(axis.offset(2, 5)).toBe(20 + 60);
  });
});
