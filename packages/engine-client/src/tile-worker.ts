/// <reference lib="webworker" />
// Draws tiles from the engine's data. Started from the main thread, because WebView2
// can't load a worker started by a worker, and it reads the engine over a `MessageChannel`.
import { expose, transfer, wrap, type Remote } from 'comlink';
import { drawTile, tilePixels } from './tile-draw';
import type { EngineTilePort, TileWorkerApi } from './types';
import { decodeViewBlock, hashBytes } from './view-block';

let engine: Remote<EngineTilePort> | undefined;

const api: TileWorkerApi = {
  attachEngine(port) {
    engine = wrap<EngineTilePort>(port);
  },
  async draw({ firstRow, firstCol, scale, palette }) {
    if (!engine) throw new Error('The tile worker is not connected to the engine.');
    const buffer = await engine.viewBlock(firstRow, firstCol);
    const hash = hashBytes(new Uint8Array(buffer));
    const { width, height } = tilePixels(scale);
    const canvas = new OffscreenCanvas(width, height);
    const context = canvas.getContext('2d', { alpha: false });
    if (!context) throw new Error('The tile worker could not get a 2D canvas.');
    drawTile(context, decodeViewBlock(buffer), palette, scale);
    const bitmap = canvas.transferToImageBitmap();
    return transfer({ bitmap, hash }, [bitmap]);
  },
};

expose(api);
