/// <reference lib="webworker" />
// Runs the WASM engine. Started from the main thread.
import { expose, transfer } from 'comlink';
import initWasm, { Engine, version } from './pkg/cgs_wasm';
import type { EngineApi, EngineTilePort } from './types';

let engine: Promise<Engine> | undefined;

function ensureEngine() {
  engine ??= initWasm({ module_or_path: new URL('./pkg/cgs_wasm_bg.wasm', import.meta.url) }).then(
    () => new Engine(),
  );
  return engine;
}

const tilePort: EngineTilePort = {
  async viewBlock(firstRow, firstCol) {
    const { buffer } = (await ensureEngine()).view_block(firstRow, firstCol);
    return transfer(buffer as ArrayBuffer, [buffer as ArrayBuffer]);
  },
};

const api: EngineApi = {
  async init() {
    await ensureEngine();
    return { version: version() };
  },
  attachTileWorker(port) {
    expose(tilePort, port);
  },
  touchSharedMemory(buffer, value) {
    Atomics.store(new Int32Array(buffer), 0, value);
  },
};

expose(api);
