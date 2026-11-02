import { describe, expect, it } from 'vitest';
import {
  CELL_RTL,
  cellText,
  decodeViewBlock,
  EXTRA_MERGE,
  FONT_BOLD,
  hashBytes,
  KIND_TEXT,
} from './view-block';
import { encodeTile } from './test-tile';

describe('decodeViewBlock', () => {
  it('reads every section', () => {
    const block = decodeViewBlock(
      encodeTile({
        rowPx: [20, 0],
        colPx: [64],
        cells: [{ text: 'مرحبا', fill: 0x112233ff, flags: KIND_TEXT | CELL_RTL }, {}],
        styles: [{ font: 'Calibri', size: 14, fontFlags: FONT_BOLD, rightColor: 0xff0000ff }],
        extras: [{ kind: EXTRA_MERGE, x: -64, y: 0, width: 128, height: 40, text: 'across' }],
      }),
    );
    expect(block.rows).toBe(2);
    expect(block.cols).toBe(1);
    expect([...block.rowPx]).toEqual([20, 0]);
    expect([...block.colPx]).toEqual([64]);
    expect(cellText(block, 0, 0)).toBe('مرحبا');
    expect(cellText(block, 1, 0)).toBe('');
    expect(block.cells[2]).toBe(0x112233ff);
    expect(block.cells[4]! >>> 16).toBe(CELL_RTL);
    expect(block.styles[0]).toMatchObject({
      font: 'Calibri',
      size: 14,
      fontFlags: FONT_BOLD,
      rightColor: 0xff0000ff,
    });
    expect(block.extras[0]).toMatchObject({
      kind: EXTRA_MERGE,
      x: -64,
      width: 128,
      text: 'across',
    });
  });

  it('refuses what is not a tile, or is cut short', () => {
    expect(() => decodeViewBlock(new ArrayBuffer(8))).toThrow(/not a ViewBlock/);
    const whole = encodeTile({ rowPx: [20], colPx: [64] });
    const view = new Uint8Array(whole.slice(0));
    view[4] = 9;
    expect(() => decodeViewBlock(view.buffer)).toThrow(/Unknown ViewBlock version/);
    const longer = new Uint8Array(whole.byteLength + 1);
    longer.set(new Uint8Array(whole));
    expect(() => decodeViewBlock(longer.buffer)).toThrow(/cut short/);
  });
});

describe('hashBytes', () => {
  it('is FNV-1a', () => {
    expect(hashBytes(new Uint8Array())).toBe(0x811c9dc5);
    expect(hashBytes(new TextEncoder().encode('a'))).toBe(0xe40c292c);
  });
});
