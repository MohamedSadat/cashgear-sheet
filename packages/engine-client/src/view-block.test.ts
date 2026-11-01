import { describe, expect, it } from 'vitest';
import { cellColors, cellText, decodeViewBlock, hashBytes } from './view-block';

/** A one-row, two-column tile, laid out by hand as crates/cgs-engine writes it. */
function tile() {
  const text = new TextEncoder().encode('R1C1مرحبا');
  const buffer = new ArrayBuffer(24 + 2 * 24 + text.length);
  const view = new DataView(buffer);
  view.setUint32(0, 0x42564743, true);
  view.setUint16(4, 1, true);
  view.setUint16(6, 1, true);
  view.setUint16(8, 2, true);
  view.setUint32(12, 64, true);
  view.setUint32(16, 16, true);
  view.setUint32(20, text.length, true);
  view.setUint32(24 + 4, 4, true);
  view.setUint32(24 + 8, 0xff0000ff, true);
  view.setUint32(48, 4, true);
  view.setUint32(48 + 4, text.length - 4, true);
  view.setUint32(48 + 12, 0x112233ff, true);
  new Uint8Array(buffer, 72).set(text);
  return buffer;
}

describe('decodeViewBlock', () => {
  it('reads the header, colours and UTF-8 text', () => {
    const block = decodeViewBlock(tile());
    expect(block).toMatchObject({ rows: 1, cols: 2, firstRow: 64, firstCol: 16 });
    expect(cellText(block, 0, 0)).toBe('R1C1');
    expect(cellText(block, 0, 1)).toBe('مرحبا');
    expect(cellColors(block, 0, 0)).toEqual({ fill: 0xff0000ff, text: 0 });
    expect(cellColors(block, 0, 1)).toEqual({ fill: 0, text: 0x112233ff });
  });

  it('rejects anything else', () => {
    expect(() => decodeViewBlock(new ArrayBuffer(8))).toThrow('not a ViewBlock');
    const version = tile();
    new DataView(version).setUint16(4, 9, true);
    expect(() => decodeViewBlock(version)).toThrow('Unknown ViewBlock version 9');
    expect(() => decodeViewBlock(tile().slice(0, 60))).toThrow('cut short');
  });
});

describe('hashBytes', () => {
  it('matches FNV-1a', () => {
    expect(hashBytes(new TextEncoder().encode('a'))).toBe(0xe40c292c);
  });
});
