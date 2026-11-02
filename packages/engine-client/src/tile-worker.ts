/// <reference lib="webworker" />
// Draws tiles from the engine's data. Started from the main thread, because WebView2
// can't load a worker started by a worker, and it reads the engine over a `MessageChannel`.
import { expose, transfer, wrap, type Remote } from 'comlink';
import { carlitoFaces } from './fonts';
import { drawTile, tileEdges } from './tile-draw';
import type { EngineTilePort, TileWorkerApi } from './types';
import { decodeViewBlock, hashBytes, TILE_NOT_READY } from './view-block';

let engine: Remote<EngineTilePort> | undefined;

/** Carlito stands in for Calibri, so it must be here before tiles are drawn with it. */
const fontsReady = Promise.allSettled(
  carlitoFaces().map(async (face) => {
    await face.load();
    self.fonts.add(face);
  }),
);

/** `#rrggbb` as a number. */
function rgb(css: string) {
  return parseInt(css.slice(1, 7), 16);
}

const api: TileWorkerApi = {
  attachEngine(port) {
    engine = wrap<EngineTilePort>(port);
  },
  async draw({ sheet, firstRow, firstCol, scale, palette }) {
    if (!engine) throw new Error('The tile worker is not connected to the engine.');
    await fontsReady;
    const buffer = await engine.viewBlock(
      sheet,
      firstRow,
      firstCol,
      palette.dark,
      rgb(palette.background),
      rgb(palette.text),
    );
    const hash = hashBytes(new Uint8Array(buffer));
    const block = decodeViewBlock(buffer);
    const width = tileEdges(block.colPx, scale)[block.cols]!;
    const height = tileEdges(block.rowPx, scale)[block.rows]!;
    const canvas = new OffscreenCanvas(Math.max(1, width), Math.max(1, height));
    const context = canvas.getContext('2d', { alpha: false });
    if (!context) throw new Error('The tile worker could not get a 2D canvas.');
    drawTile(context, block, palette, scale);
    const bitmap = canvas.transferToImageBitmap();
    return transfer({ bitmap, hash, ready: !(block.flags & TILE_NOT_READY) }, [bitmap]);
  },
};

expose(api);
