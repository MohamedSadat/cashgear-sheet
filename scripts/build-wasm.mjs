import { spawnSync } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { bindgenPath, bindgenVersion, root, run, wasmOutput } from './lib.mjs';

const release = process.argv.includes('--release');
// Room for the published Formualizer (3.6 MB); Phase 1 sets the real budget.
const limitBytes = 4000000;
const bindgen = bindgenPath();
const check = spawnSync(bindgen, ['--version'], { encoding: 'utf8' });
if (check.status !== 0 || check.stdout.trim() !== 'wasm-bindgen ' + bindgenVersion) {
  throw new Error('wasm-bindgen ' + bindgenVersion + ' is missing. Run pnpm setup:wasm.');
}
await run('cargo', [
  'build',
  '-p',
  'cgs-wasm',
  '--target',
  'wasm32-unknown-unknown',
  ...(release ? ['--release'] : []),
]);
await mkdir(wasmOutput, { recursive: true });
await run(bindgen, [
  join(root, 'target/wasm32-unknown-unknown', release ? 'release' : 'debug', 'cgs_wasm.wasm'),
  '--target',
  'web',
  '--out-dir',
  wasmOutput,
  '--out-name',
  'cgs_wasm',
]);

/** Optimizes a release module with binaryen; returns its size and gzipped size. */
async function optimize(path) {
  const { default: binaryen } = await import('binaryen');
  const module = binaryen.readBinary(await readFile(path));
  module.setFeatures(binaryen.Features.All);
  binaryen.setOptimizeLevel(3);
  binaryen.setShrinkLevel(1);
  module.optimize();
  if (!module.validate()) throw new Error('Optimized WASM failed validation: ' + path);
  const bytes = module.emitBinary();
  module.dispose();
  await writeFile(path, bytes);
  return { wasmBytes: bytes.length, gzippedBytes: gzipSync(bytes, { level: 9 }).length };
}

if (release) {
  const { wasmBytes, gzippedBytes } = await optimize(join(wasmOutput, 'cgs_wasm_bg.wasm'));
  await mkdir(join(root, 'bench/results'), { recursive: true });
  await writeFile(
    join(root, 'bench/results/wasm-size.json'),
    JSON.stringify({ wasmBytes, gzippedBytes, limitBytes }, null, 2) + '\n',
  );
  console.log('Release WASM: ' + wasmBytes + ' bytes; gzip: ' + gzippedBytes + ' / ' + limitBytes);
  if (gzippedBytes > limitBytes) throw new Error('Release WASM gzip size exceeds 4 MB');
}
