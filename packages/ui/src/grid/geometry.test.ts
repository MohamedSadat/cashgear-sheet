import { describe, expect, it } from 'vitest';
import { visibleTiles } from './geometry';

describe('visibleTiles', () => {
  it('covers the view with whole tiles', () => {
    expect(visibleTiles(1280, 700, 1024, 1280)).toEqual([
      { firstRow: 0, firstCol: 0, x: 0, y: 0 },
      { firstRow: 0, firstCol: 16, x: 1024, y: 0 },
    ]);
    expect(visibleTiles(1024, 1281, 1024, 1280)).toEqual([
      { firstRow: 0, firstCol: 0, x: 0, y: 0 },
      { firstRow: 64, firstCol: 0, x: 0, y: 1280 },
    ]);
  });

  it('has nothing to cover in an empty view', () => {
    expect(visibleTiles(0, 700, 1024, 1280)).toEqual([]);
    expect(visibleTiles(1280, 0, 1024, 1280)).toEqual([]);
  });
});
