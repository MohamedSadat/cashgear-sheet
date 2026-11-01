import { TileCache, tilePixels, type EngineClient, type TilePalette } from '@cgs/engine-client';
import { visibleTiles } from './geometry';

/**
 * Composites tile pictures onto the visible canvas, outside React. The tile worker
 * draws them from the engine's data, so nothing here waits on the engine or sees a cell.
 */
export class GridController {
  private readonly context: CanvasRenderingContext2D;
  private readonly cache: TileCache;
  private palette: TilePalette;
  private scale = 1;
  private frame = 0;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    client: EngineClient,
    palette: TilePalette,
    onError: (error: unknown) => void,
  ) {
    const context = canvas.getContext('2d', { alpha: false });
    if (!context) throw new Error('This browser has no 2D canvas.');
    this.context = context;
    this.palette = palette;
    canvas.dataset.background = palette.background;
    this.cache = new TileCache((request) => client.drawTile(request), this.schedule, onError);
    this.cache.setStyle({ scale: this.scale, palette });
  }

  /** Redraws tiles with the new theme's colours; the old ones stay until they arrive. */
  setPalette(palette: TilePalette) {
    this.palette = palette;
    this.canvas.dataset.background = palette.background;
    this.canvas.dataset.ready = 'false';
    this.cache.setStyle({ scale: this.scale, palette });
    this.schedule();
  }

  /** Takes the canvas's size in CSS pixels. */
  resize(width: number, height: number, scale: number) {
    this.canvas.width = Math.round(width * scale);
    this.canvas.height = Math.round(height * scale);
    if (scale !== this.scale) {
      this.scale = scale;
      this.cache.setStyle({ scale, palette: this.palette });
    }
    this.schedule();
  }

  dispose() {
    cancelAnimationFrame(this.frame);
    this.frame = 0;
    this.cache.dispose();
  }

  private readonly schedule = () => {
    if (!this.frame) this.frame = requestAnimationFrame(this.composite);
  };

  private readonly composite = () => {
    this.frame = 0;
    const { canvas, context, cache } = this;
    context.fillStyle = this.palette.background;
    context.fillRect(0, 0, canvas.width, canvas.height);
    const { width, height } = tilePixels(this.scale);
    const tiles = visibleTiles(canvas.width, canvas.height, width, height);
    for (const { firstRow, firstCol, x, y } of tiles) {
      const bitmap = cache.get(firstRow, firstCol);
      if (bitmap) context.drawImage(bitmap, x, y);
    }
    // For tests: the picture is complete once every tile is drawn in the current style.
    const ready = tiles.every((tile) => cache.isCurrent(tile.firstRow, tile.firstCol));
    canvas.dataset.ready = String(ready && tiles.length > 0);
    canvas.dataset.tiles = String(tiles.length);
    canvas.dataset.firstTileHash = String(cache.hash(0, 0) ?? '');
  };
}
