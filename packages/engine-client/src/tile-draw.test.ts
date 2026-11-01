import { describe, expect, it } from 'vitest';
import { cellPixels, cssColor, drawTile, tilePixels, type TileContext } from './tile-draw';
import type { TilePalette } from './types';
import { decodeViewBlock } from './view-block';

const palette: TilePalette = {
  background: '#ffffff',
  gridline: '#dddddd',
  text: '#111111',
  fontFamily: 'sans-serif',
};

/** A context that records what is drawn, with the fill style in force. */
function recorder() {
  const rects: [string, number, number, number, number][] = [];
  const texts: [string, string, number, number][] = [];
  const context = {
    fillStyle: '',
    font: '',
    textBaseline: 'alphabetic',
    save() {},
    restore() {},
    beginPath() {},
    rect() {},
    clip() {},
    fillRect(x: number, y: number, width: number, height: number) {
      rects.push([String(this.fillStyle), x, y, width, height]);
    },
    fillText(text: string, x: number, y: number) {
      texts.push([text, String(this.fillStyle), x, y]);
    },
  };
  return { context: context as unknown as TileContext, rects, texts };
}

/** A tile of one row: a red cell with no text, then a cell with automatic colours and text. */
function block() {
  const text = new TextEncoder().encode('Hi');
  const buffer = new ArrayBuffer(24 + 2 * 24 + text.length);
  const view = new DataView(buffer);
  view.setUint32(0, 0x42564743, true);
  view.setUint16(4, 1, true);
  view.setUint16(6, 1, true);
  view.setUint16(8, 2, true);
  view.setUint32(20, text.length, true);
  view.setUint32(24 + 8, 0xff0000ff, true);
  view.setUint32(48 + 4, text.length, true);
  new Uint8Array(buffer, 72).set(text);
  return decodeViewBlock(buffer);
}

describe('tile sizes', () => {
  it('keeps cells in whole device pixels', () => {
    expect(cellPixels(1)).toEqual({ width: 64, height: 20 });
    expect(cellPixels(1.5)).toEqual({ width: 96, height: 30 });
    expect(cellPixels(1.1)).toEqual({ width: 70, height: 22 });
    expect(tilePixels(1)).toEqual({ width: 1024, height: 1280 });
    expect(tilePixels(1.1)).toEqual({ width: 1120, height: 1408 });
  });
});

describe('cssColor', () => {
  it('keeps leading zeros and high bits', () => {
    expect(cssColor(0x0000ffff)).toBe('#0000ffff');
    expect(cssColor(0xff0000ff)).toBe('#ff0000ff');
  });
});

describe('drawTile', () => {
  it('draws explicit fills as stored, and takes the rest from the palette', () => {
    const { context, rects, texts } = recorder();
    drawTile(context, block(), palette, 1);
    expect(rects[0]).toEqual(['#ffffff', 0, 0, 128, 20]);
    expect(rects[1]).toEqual(['#ff0000ff', 0, 0, 64, 20]);
    // Only the cell with no fill has gridlines.
    expect(rects.slice(2)).toEqual([
      ['#dddddd', 127, 0, 1, 20],
      ['#dddddd', 64, 19, 64, 1],
    ]);
    expect(texts).toEqual([['Hi', '#111111', 68, 10]]);
  });

  it('draws a different palette from the same data', () => {
    const { context, rects, texts } = recorder();
    drawTile(context, block(), { ...palette, background: '#000000', text: '#eeeeee' }, 1);
    expect(rects[0][0]).toBe('#000000');
    expect(rects[1][0]).toBe('#ff0000ff');
    expect(texts[0][1]).toBe('#eeeeee');
  });

  it('scales to device pixels', () => {
    const { context, rects, texts } = recorder();
    drawTile(context, block(), palette, 2);
    expect(rects[0]).toEqual(['#ffffff', 0, 0, 256, 40]);
    expect(texts).toEqual([['Hi', '#111111', 136, 20]]);
    expect(context.font).toBe('24px sans-serif');
  });
});
