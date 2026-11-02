// Builds the Chrome harness, and measures the engine's release size the same way as
// scripts/build-wasm.mjs: binaryen, then gzip. Usage: node bench/build-wasm.mjs [cargo args]
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { bindgenPath, root, run } from '../scripts/lib.mjs';

const engine = 'formualizer';
const cargoArgs = process.argv.slice(2);
const targetDir = join(root, 'target/bench-wasm-' + engine);
const env = {
  ...process.env,
  CARGO_TARGET_DIR: targetDir,
  // The default 1 MB stack is tight for deep chains.
  RUSTFLAGS: '-C link-arg=-zstack-size=8388608',
};

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

async function buildCrate(crate, outDir) {
  await run(
    'cargo',
    [
      'build',
      '-p',
      crate,
      '--lib',
      '--target',
      'wasm32-unknown-unknown',
      '--release',
      ...cargoArgs,
    ],
    { env },
  );
  await mkdir(outDir, { recursive: true });
  const name = crate.replaceAll('-', '_');
  await run(
    bindgenPath(),
    [
      join(targetDir, 'wasm32-unknown-unknown/release', name + '.wasm'),
      '--target',
      'web',
      '--out-dir',
      outDir,
      '--out-name',
      name,
    ],
    { env },
  );
  return optimize(join(outDir, name + '_bg.wasm'));
}

const outRoot = join(root, 'bench/results/wasm');
await buildCrate('cgs-bench', join(outRoot, engine));
const size = await buildCrate('cgs-size-probe', join(outRoot, engine + '-size'));
await writeFile(join(outRoot, engine + '-size.json'), JSON.stringify(size, null, 2) + '\n');
console.log(
  `${engine}: ${size.wasmBytes} bytes, ${size.gzippedBytes} gzipped (engine and adapter only)`,
);
