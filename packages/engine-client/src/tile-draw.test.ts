import { describe, expect, it } from 'vitest';
import { cssColor, drawTile, fontStack, tileEdges, type TileContext } from './tile-draw';
import type { TilePalette } from './types';
import {
  CELL_ERP_COMMITTED,
  CELL_ERP_CONFLICT,
  CELL_ERP_FORMULA,
  CELL_ERP_INVALID,
  CELL_ERP_LOCK,
  CELL_ERP_MODIFIED,
  CELL_ERP_READ_ONLY,
  CELL_HASHES,
  CELL_NO_RIGHT_GRID,
  CELL_OCCUPIED,
  CELL_RTL,
  decodeViewBlock,
  EXTRA_MERGE,
  EXTRA_OVERFLOW,
  KIND_GENERAL,
  KIND_NUMBER,
  KIND_TEXT,
  LAYOUT_WRAP,
  LINE_MEDIUM,
  LINE_THIN,
} from './view-block';
import { encodeTile } from './test-tile';

const palette: TilePalette = {
  dark: false,
  background: '#ffffff',
  gridline: '#dddddd',
  text: '#111111',
  fontFamily: 'sans-serif',
};

/** A context that records what is drawn. Each character is 6 px wide. */
function recorder() {
  const rects: [string, number, number, number, number][] = [];
  const texts: { text: string; color: string; x: number; y: number; direction: string }[] = [];
  const clips: number[][] = [];
  const context = {
    fillStyle: '',
    font: '',
    strokeStyle: '',
    lineWidth: 1,
    textBaseline: 'alphabetic',
    textAlign: 'left',
    direction: 'ltr',
    save() {},
    restore() {},
    beginPath() {},
    moveTo() {},
    lineTo() {},
    stroke() {},
    setLineDash() {},
    rotate() {},
    translate() {},
    rect(x: number, y: number, width: number, height: number) {
      clips.push([x, y, width, height]);
    },
    clip() {},
    fillRect(x: number, y: number, width: number, height: number) {
      rects.push([String(this.fillStyle), x, y, width, height]);
    },
    strokeRect(x: number, y: number, width: number, height: number) {
      rects.push(['stroke ' + String(this.strokeStyle), x, y, width, height]);
    },
    arc() {},
    fillText(text: string, x: number, y: number) {
      texts.push({ text, color: String(this.fillStyle), x, y, direction: this.direction });
    },
    measureText(text: string) {
      return { width: [...text].length * 6, fontBoundingBoxAscent: 10, fontBoundingBoxDescent: 3 };
    },
  };
  return { context: context as unknown as TileContext, rects, texts, clips };
}

function draw(tile: Parameters<typeof encodeTile>[0], scale = 1) {
  const recording = recorder();
  drawTile(recording.context, decodeViewBlock(encodeTile(tile)), palette, scale);
  return recording;
}

describe('sizes', () => {
  it('rounds each line alone and adds them up', () => {
    expect([...tileEdges([64, 0, 21], 1.5)]).toEqual([0, 96, 96, 128]);
  });
});

describe('colours', () => {
  it('writes #rrggbbaa', () => {
    expect(cssColor(0x11223344)).toBe('#11223344');
    expect(cssColor(0xff0000ff)).toBe('#ff0000ff');
  });
});

describe('fonts', () => {
  it('falls back on Carlito for Calibri and Aptos only', () => {
    expect(fontStack('Calibri', palette)).toBe('"Calibri", Carlito, sans-serif');
    expect(fontStack('Aptos Narrow', palette)).toBe('"Aptos Narrow", Carlito, sans-serif');
    expect(fontStack('Arial', palette)).toBe('"Arial", sans-serif');
    expect(fontStack('Calibri', { ...palette, fontOverride: 'Carlito' })).toBe(
      '"Carlito", sans-serif',
    );
  });
});

describe('drawTile', () => {
  it('fills the background and draws gridlines, which fills hide', () => {
    const { rects } = draw({
      rowPx: [20],
      colPx: [64, 64],
      cells: [{ fill: 0xff0000ff }, {}],
    });
    expect(rects[0]).toEqual(['#ffffff', 0, 0, 128, 20]);
    expect(rects).toContainEqual(['#ff0000ff', 0, 0, 64, 20]);
    expect(rects).toContainEqual(['#dddddd', 127, 0, 1, 20]);
    expect(rects).toContainEqual(['#dddddd', 64, 19, 64, 1]);
    expect(rects).not.toContainEqual(['#dddddd', 63, 0, 1, 20]);
  });

  it('fills neighbouring cells of one fill as one rectangle, and runs bottom gridlines', () => {
    const { rects } = draw({
      rowPx: [20, 20],
      colPx: [64, 64, 64, 64],
      cells: [
        { fill: 0xd9e1f2ff },
        { fill: 0xd9e1f2ff },
        { fill: 0xd9e1f2ff },
        { fill: 0xff0000ff },
        {},
        {},
        {},
        {},
      ],
    });
    expect(rects.filter(([color]) => color === '#d9e1f2ff')).toEqual([
      ['#d9e1f2ff', 0, 0, 192, 20],
    ]);
    expect(rects).toContainEqual(['#ff0000ff', 192, 0, 64, 20]);
    // The second row's cells keep their right gridlines; their bottom ones are one line.
    const grid = rects.filter(([color, , y]) => color === '#dddddd' && y >= 20);
    expect(grid).toEqual([
      ['#dddddd', 63, 20, 1, 20],
      ['#dddddd', 127, 20, 1, 20],
      ['#dddddd', 191, 20, 1, 20],
      ['#dddddd', 255, 20, 1, 20],
      ['#dddddd', 0, 39, 256, 1],
    ]);
  });

  it('leaves out gridlines a sheet turned off, and those inside a merged area', () => {
    const recording = recorder();
    drawTile(
      recording.context,
      decodeViewBlock(encodeTile({ rowPx: [20], colPx: [64] })),
      { ...palette, showGrid: false },
      1,
    );
    expect(recording.rects.filter(([color]) => color === '#dddddd')).toEqual([]);
    const { rects } = draw({
      rowPx: [20],
      colPx: [64],
      cells: [{ flags: CELL_NO_RIGHT_GRID }],
    });
    expect(rects).not.toContainEqual(['#dddddd', 63, 0, 1, 20]);
    expect(rects).toContainEqual(['#dddddd', 0, 19, 64, 1]);
  });

  it('skips hidden rows and columns', () => {
    const { texts } = draw({
      rowPx: [0, 20],
      colPx: [64],
      cells: [{ text: 'hidden' }, { text: 'shown' }],
    });
    expect(texts.map(({ text }) => text)).toEqual(['shown']);
  });

  it('draws text in its own colour, or the theme’s, in its direction', () => {
    const { texts } = draw({
      rowPx: [20, 20],
      colPx: [128],
      cells: [{ text: 'plain' }, { text: 'مرحبا', color: 0x00ff00ff, flags: KIND_TEXT | CELL_RTL }],
    });
    expect(texts[0]).toMatchObject({ text: 'plain', color: '#111111', direction: 'ltr' });
    expect(texts[1]).toMatchObject({ text: 'مرحبا', color: '#00ff00ff', direction: 'rtl' });
  });

  it('aligns by the style', () => {
    const { texts } = draw({
      rowPx: [20],
      colPx: [100, 100, 100],
      cells: [{ text: 'left' }, { text: 'mid', style: 1 }, { text: 'end', style: 2 }],
      styles: [{ horizontal: 0 }, { horizontal: 1 }, { horizontal: 2 }],
    });
    expect(texts[0]!.x).toBe(2);
    expect(texts[1]!.x).toBe(100 + (100 - 18) / 2);
    expect(texts[2]!.x).toBe(300 - 2 - 18 - 1);
  });

  it('replaces a number that does not fit with hashes, and shortens General first', () => {
    const { texts } = draw({
      rowPx: [20, 20, 20],
      colPx: [40],
      cells: [
        { text: '1234567890', flags: KIND_NUMBER, style: 0 },
        { text: '3.14159265', flags: KIND_GENERAL, style: 0 },
        { text: '', flags: KIND_NUMBER | CELL_HASHES, style: 0 },
      ],
      styles: [{ horizontal: 2 }],
    });
    expect(texts[0]!.text).toMatch(/^#+$/);
    expect(texts[1]!.text).toBe('3.142');
    expect(texts[2]!.text).toMatch(/^#+$/);
  });

  it('lets text overflow across empty cells, and stops at the next value', () => {
    const { clips, rects } = draw({
      rowPx: [20],
      colPx: [40, 40, 40, 40],
      cells: [{ text: 'a long piece of text' }, {}, { text: 'next' }, {}],
    });
    // The clip for the long text runs through the empty second cell, up to the third.
    expect(clips).toContainEqual([0, 0, 79, 20]);
    // The gridline between the first two cells is painted over.
    expect(rects).toContainEqual(['#ffffff', 39, 0, 1, 19]);
  });

  it('stops overflow at a value that displays as nothing', () => {
    const { clips } = draw({
      rowPx: [20],
      colPx: [40, 40],
      cells: [{ text: 'a long piece of text' }, { flags: CELL_OCCUPIED }],
    });
    expect(clips).toContainEqual([0, 0, 39, 20]);
  });

  it('carries overflow on to the next tile, whose extra draws the rest', () => {
    const edge = draw({
      rowPx: [20],
      colPx: [40],
      cells: [{ text: 'a long piece of text' }],
    });
    expect(edge.clips).toContainEqual([0, 0, 40, 20]);
    const next = draw({
      rowPx: [20],
      colPx: [40, 40],
      cells: [{}, { text: 'next' }],
      extras: [
        { kind: EXTRA_OVERFLOW, x: -40, y: 0, width: 40, height: 20, text: 'a long piece of text' },
      ],
    });
    const carried = next.texts.find(({ text }) => text === 'a long piece of text')!;
    expect(carried.x).toBe(-38);
    expect(next.clips).toContainEqual([0, 0, 40, 20]);
  });

  it('draws a merged area’s text over its whole rectangle', () => {
    const { texts } = draw({
      rowPx: [20, 20],
      colPx: [64, 64],
      extras: [{ kind: EXTRA_MERGE, x: 0, y: 0, width: 128, height: 40, text: 'mid', style: 0 }],
      styles: [{ horizontal: 1, vertical: 1 }],
    });
    expect(texts[0]!.x).toBe((128 - 18) / 2);
    expect(texts[0]!.y).toBe((40 - 13) / 2 + 10);
  });

  it('wraps text into lines', () => {
    const { texts } = draw({
      rowPx: [60],
      colPx: [40],
      cells: [{ text: 'one two three' }],
      styles: [{ layout: LAYOUT_WRAP, vertical: 0 }],
    });
    expect(texts.map(({ text }) => text)).toEqual(['one', 'two', 'three']);
    expect(texts[1]!.y - texts[0]!.y).toBe(13);
  });

  it('drops line breaks from text that does not wrap', () => {
    const { texts } = draw({ rowPx: [20], colPx: [200], cells: [{ text: 'one\ntwo' }] });
    expect(texts[0]!.text).toBe('onetwo');
  });

  it('draws borders inside the cell they belong to', () => {
    const { rects } = draw({
      rowPx: [20],
      colPx: [64],
      styles: [
        {
          rightLine: LINE_THIN,
          rightColor: 0x00ff00ff,
          bottomLine: LINE_MEDIUM,
          bottomColor: 0x0000ffff,
        },
      ],
    });
    expect(rects).toContainEqual(['#00ff00ff', 63, 0, 1, 20]);
    expect(rects).toContainEqual(['#0000ffff', 0, 18, 64, 2]);
  });
});

describe('drawTile on a right-to-left sheet', () => {
  const rtl = { ...palette, rightToLeft: true };
  function drawRtl(tile: Parameters<typeof encodeTile>[0]) {
    const recording = recorder();
    drawTile(recording.context, decodeViewBlock(encodeTile(tile)), rtl, 1);
    return recording;
  }

  it('puts the first column on the right, with gridlines on each cell’s left', () => {
    const { rects } = drawRtl({
      rowPx: [20],
      colPx: [64, 32],
      cells: [{ fill: 0xff0000ff }, {}],
    });
    expect(rects).toContainEqual(['#ff0000ff', 32, 0, 64, 20]);
    // The second column, on the left, has its line on its own left edge.
    expect(rects).toContainEqual(['#dddddd', 0, 0, 1, 20]);
    expect(rects).not.toContainEqual(['#dddddd', 31, 0, 1, 20]);
  });

  it('keeps alignment as it is, as Excel does', () => {
    const { texts } = drawRtl({
      rowPx: [20],
      colPx: [100, 100],
      cells: [{ text: 'left' }, { text: 'end', style: 1 }],
      styles: [{ horizontal: 0 }, { horizontal: 2 }],
    });
    // Column A is on the right: its text starts at its own left edge.
    const at = (text: string) => texts.find((drawn) => drawn.text === text)!.x;
    expect(at('left')).toBe(100 + 2 + 1);
    expect(at('end')).toBe(100 - 2 - 18);
  });

  it('lets text aligned left run on into earlier columns, to its right', () => {
    const { clips, rects } = drawRtl({
      rowPx: [20],
      colPx: [40, 40, 40],
      cells: [{}, { text: 'a long piece of text' }, {}],
    });
    // B is in the middle; the text runs right through A to the tile's edge.
    expect(clips).toContainEqual([41, 0, 79, 20]);
    // The line between B and A, which is A's, is painted over.
    expect(rects).toContainEqual(['#ffffff', 80, 0, 1, 19]);
  });

  it('draws a cell’s right border on its left, and a note’s corner at its top left', () => {
    const { rects } = drawRtl({
      rowPx: [20],
      colPx: [64, 64],
      cells: [{ style: 1 }, {}],
      styles: [{}, { rightLine: LINE_THIN, rightColor: 0x00ff00ff }],
    });
    expect(rects).toContainEqual(['#00ff00ff', 64, 0, 1, 20]);
  });

  it('places extras from the right', () => {
    const { texts } = drawRtl({
      rowPx: [20, 20],
      colPx: [64, 64, 64],
      extras: [{ kind: EXTRA_MERGE, x: 0, y: 0, width: 128, height: 40, text: 'mid', style: 0 }],
      styles: [{ horizontal: 1, vertical: 1 }],
    });
    expect(texts[0]!.x).toBe(64 + (128 - 18) / 2);
  });
});

describe('ERP cell states', () => {
  const erp = {
    light: {
      modified: 'amber',
      modifiedTint: 'amber-tint',
      invalid: 'red',
      formula: 'violet',
      readOnly: 'wash',
      conflict: 'orange',
      conflictHatch: 'orange-hatch',
      committed: 'green',
      goneHatch: 'grey-hatch',
    },
    dark: {
      modified: 'AMBER',
      modifiedTint: 'AMBER-TINT',
      invalid: 'RED',
      formula: 'VIOLET',
      readOnly: 'WASH',
      conflict: 'ORANGE',
      conflictHatch: 'ORANGE-HATCH',
      committed: 'GREEN',
      goneHatch: 'GREY-HATCH',
    },
  };
  const states = {
    rowPx: [20],
    colPx: [64, 64, 64, 64],
    cells: [
      { flags: CELL_ERP_MODIFIED },
      { flags: CELL_ERP_MODIFIED | CELL_ERP_INVALID },
      { flags: CELL_ERP_FORMULA },
      { flags: CELL_ERP_READ_ONLY },
    ],
  };
  const drawn = (
    tile: Parameters<typeof encodeTile>[0],
    more: Partial<TilePalette>,
    edges?: Parameters<typeof drawTile>[4],
  ) => {
    const recording = recorder();
    drawTile(
      recording.context,
      decodeViewBlock(encodeTile(tile)),
      { ...palette, erp, ...more },
      1,
      edges,
    );
    return recording.rects;
  };

  it('tints, bars and rings cells on their leading edge', () => {
    const rects = drawn(states, {});
    // Neighbouring cells of one tint are tinted as one rectangle on screen.
    expect(rects).toContainEqual(['amber-tint', 0, 0, 128, 20]);
    expect(rects).toContainEqual(['amber', 0, 0, 3, 19]);
    expect(rects).toContainEqual(['amber', 64, 0, 3, 19]);
    expect(rects).toContainEqual(['stroke red', 65, 1, 61, 17]);
    expect(rects).toContainEqual(['wash', 192, 0, 64, 20]);
    // A formula's bar is dotted, and its cell keeps its own background.
    expect(rects.filter(([color]) => color === 'violet')).toEqual([
      ['violet', 128, 0, 3, 3],
      ['violet', 128, 6, 3, 3],
      ['violet', 128, 12, 3, 3],
      ['violet', 128, 18, 3, 1],
    ]);
  });

  it('hatches a conflict with a "!" in its corner, and bars a committed field green', () => {
    const recording = recorder();
    drawTile(
      recording.context,
      decodeViewBlock(
        encodeTile({
          rowPx: [20],
          colPx: [64, 64],
          cells: [{ flags: CELL_ERP_MODIFIED | CELL_ERP_CONFLICT }, { flags: CELL_ERP_COMMITTED }],
        }),
      ),
      { ...palette, erp },
      1,
    );
    expect(recording.rects).toContainEqual(['amber', 0, 0, 3, 19]);
    expect(recording.clips).toContainEqual([0, 0, 63, 19]);
    expect(recording.texts).toContainEqual(
      expect.objectContaining({ text: '!', color: 'orange', x: 64 - 1 - 3 - 6 }),
    );
    expect(recording.rects).toContainEqual(['green', 64, 0, 3, 19]);
    // The committed cell has no amber of its own.
    expect(recording.rects.filter(([color, x]) => color.startsWith('amber') && x === 64)).toEqual(
      [],
    );
  });

  it('puts the bar on the right of a right-to-left sheet, its leading edge', () => {
    const rects = drawn(states, { rightToLeft: true });
    // Column A is the rightmost on screen.
    expect(rects).toContainEqual(['amber', 253, 0, 3, 19]);
  });

  it('takes the light colours on a pale fill in dark mode, and the dark ones elsewhere', () => {
    const rects = drawn(
      {
        rowPx: [20],
        colPx: [64, 64],
        cells: [{ fill: 0xd9e1f2ff, flags: CELL_ERP_MODIFIED }, { flags: CELL_ERP_MODIFIED }],
      },
      { dark: true, background: '#151b24' },
    );
    expect(rects).toContainEqual(['amber', 0, 0, 3, 19]);
    expect(rects).toContainEqual(['AMBER', 64, 0, 3, 19]);
  });

  it("leaves room for a read-only heading's lock before its text", () => {
    const plain = recorder();
    drawTile(
      plain.context,
      decodeViewBlock(encodeTile({ rowPx: [20], colPx: [64], cells: [{ text: 'Account' }] })),
      { ...palette, erp },
      1,
    );
    const locked = recorder();
    drawTile(
      locked.context,
      decodeViewBlock(
        encodeTile({
          rowPx: [20],
          colPx: [64],
          cells: [{ text: 'Account', flags: CELL_ERP_LOCK }],
        }),
      ),
      { ...palette, erp },
      1,
    );
    expect(locked.texts[0]!.x - plain.texts[0]!.x).toBe(11);
  });

  it('draws nothing without the palette’s colours, or on pages', () => {
    const none = drawn(states, { erp: undefined });
    expect(none.some(([color]) => /amber|violet|red|wash/i.test(color))).toBe(false);
    const page = drawn(
      states,
      {},
      {
        xs: [0, 64, 128, 192, 256],
        ys: [0, 20],
        perPixelX: 1,
        perPixelY: 1,
      },
    );
    expect(page.some(([color]) => /amber|violet|red|wash/i.test(color))).toBe(false);
  });
});
