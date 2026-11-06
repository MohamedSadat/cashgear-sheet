import { describe, expect, it } from 'vitest';
import {
  Axis,
  cellName,
  columnName,
  rangeName,
  scrollForThumb,
  sliderToZoom,
  thumbFor,
  tilesCovering,
  zoomToSlider,
} from './geometry';

describe('Axis', () => {
  it('needs nothing for a million default rows', () => {
    const axis = new Axis(1_048_576, 20);
    expect(axis.offset(1_000_000)).toBe(20_000_000);
    expect(axis.at(20_000_019)).toBe(1_000_000);
    expect(axis.total).toBe(20_971_520);
  });

  it('adds up sizes that differ', () => {
    // Line 2 is 40 px, line 3 is hidden.
    const axis = new Axis(10, 20, [2, 2, 40, 3, 3, 0]);
    expect(axis.size(1)).toBe(20);
    expect(axis.size(2)).toBe(40);
    expect(axis.size(3)).toBe(0);
    expect(axis.offset(2)).toBe(40);
    expect(axis.offset(3)).toBe(80);
    expect(axis.offset(4)).toBe(80);
    expect(axis.offset(5)).toBe(100);
    expect(axis.total).toBe(20 * 8 + 40);
  });

  it('finds the visible line at a position', () => {
    const axis = new Axis(10, 20, [2, 2, 40, 3, 3, 0]);
    expect(axis.at(0)).toBe(0);
    expect(axis.at(39)).toBe(1);
    expect(axis.at(40)).toBe(2);
    expect(axis.at(79)).toBe(2);
    expect(axis.at(80)).toBe(4);
    expect(axis.at(99_999)).toBe(9);
  });

  it('rounds each line alone, so a line always sits on whole pixels', () => {
    const axis = new Axis(4, 15, [1, 1, 21], 1.5);
    expect(axis.size(0)).toBe(23);
    expect(axis.size(1)).toBe(32);
    expect(axis.offset(2)).toBe(55);
    axis.setScale(1);
    expect(axis.offset(2)).toBe(36);
  });

  it('skips hidden lines when moving', () => {
    const axis = new Axis(6, 20, [1, 2, 0]);
    expect(axis.nextVisible(1)).toBe(3);
    expect(axis.previousVisible(2)).toBe(0);
  });

  it('keeps runs of lines as runs, inside and past them', () => {
    // Lines 2 to 5 are 30 px, and a filter hides 7 to a million.
    const axis = new Axis(1_048_576, 20, [2, 5, 30, 7, 1_048_574, 0]);
    expect(axis.size(4)).toBe(30);
    expect(axis.offset(4)).toBe(2 * 20 + 2 * 30);
    expect(axis.offset(6)).toBe(2 * 20 + 4 * 30);
    expect(axis.offset(1_048_575)).toBe(3 * 20 + 4 * 30);
    expect(axis.total).toBe(4 * 20 + 4 * 30);
    expect(axis.nextVisible(7)).toBe(1_048_575);
    expect(axis.previousVisible(1_048_574)).toBe(6);
    expect(axis.at(3 * 20 + 4 * 30)).toBe(1_048_575);
  });
});

describe('names', () => {
  it('names columns and cells', () => {
    expect(columnName(0)).toBe('A');
    expect(columnName(25)).toBe('Z');
    expect(columnName(26)).toBe('AA');
    expect(columnName(16383)).toBe('XFD');
    expect(cellName(0, 0)).toBe('A1');
    expect(rangeName({ top: 0, left: 0, bottom: 0, right: 0 })).toBe('A1');
    expect(rangeName({ top: 1, left: 1, bottom: 2, right: 3 })).toBe('B2:D3');
  });
});

describe('tilesCovering', () => {
  it('covers a range with whole tiles, nearest the middle first', () => {
    const tiles = tilesCovering({ top: 60, left: 0, bottom: 70, right: 20 });
    expect(tiles).toHaveLength(4);
    expect(tiles).toContainEqual({ firstRow: 0, firstCol: 16 });
    expect(tiles).toContainEqual({ firstRow: 64, firstCol: 0 });
  });
});

describe('scrollbars', () => {
  it('places a thumb in proportion, with a minimum length', () => {
    expect(thumbFor(0, 100, 1000, 100, 8)).toEqual({ position: 0, length: 10 });
    expect(thumbFor(900, 100, 1000, 100, 8)).toEqual({ position: 90, length: 10 });
    expect(thumbFor(0, 100, 10_000_000, 100, 8).length).toBe(8);
    expect(thumbFor(0, 100, 50, 100, 8)).toEqual({ position: 0, length: 100 });
  });

  it('turns a dragged thumb back into a scroll position', () => {
    expect(scrollForThumb(45, 10, 100, 1000, 100)).toBe(450);
    expect(scrollForThumb(-5, 10, 100, 1000, 100)).toBe(0);
    expect(scrollForThumb(999, 10, 100, 1000, 100)).toBe(900);
  });
});

describe('zoom slider', () => {
  it('puts 100% at the middle and round-trips', () => {
    expect(zoomToSlider(100)).toBe(50);
    expect(zoomToSlider(10)).toBe(0);
    expect(zoomToSlider(400)).toBe(100);
    for (const zoom of [10, 55, 100, 150, 400]) {
      expect(sliderToZoom(zoomToSlider(zoom))).toBe(zoom);
    }
  });
});
