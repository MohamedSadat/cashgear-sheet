// Pure grid geometry, in whole device pixels. No DOM, no engine.
import { TILE_COLS, TILE_ROWS } from '@cgs/engine-client';

export interface PlacedTile {
  firstRow: number;
  firstCol: number;
  x: number;
  y: number;
}

/** The tiles that cover a view of this size, from the top-left cell. */
export function visibleTiles(
  viewWidth: number,
  viewHeight: number,
  tileWidth: number,
  tileHeight: number,
): PlacedTile[] {
  const tiles: PlacedTile[] = [];
  for (let row = 0; row * tileHeight < viewHeight; row++) {
    for (let col = 0; col * tileWidth < viewWidth; col++) {
      tiles.push({
        firstRow: row * TILE_ROWS,
        firstCol: col * TILE_COLS,
        x: col * tileWidth,
        y: row * tileHeight,
      });
    }
  }
  return tiles;
}
