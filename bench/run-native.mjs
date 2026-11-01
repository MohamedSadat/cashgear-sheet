// Runs the native harness over the corpus in parallel, for quick iteration. The
// bake-off's numbers come from Chrome, through bench/run-chrome.mjs.
// Usage: node bench/run-native.mjs <engine> <out.jsonl> [--filter=text] [--edits=N] [--own]
import { spawn } from 'node:child_process';
import { createWriteStream } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { root } from '../scripts/lib.mjs';

const [engine, out, ...rest] = process.argv.slice(2);
const option = (name, fallback) => {
  const found = rest.find((arg) => arg.startsWith('--' + name + '='));
  return found ? found.split('=')[1] : fallback;
};
const filter = option('filter', '');
const edits = option('edits', '10');
const timeoutMs = Number(option('timeout', '120000'));
const concurrency = Number(option('jobs', '8'));
const own = rest.includes('--own');
const bin = join(root, 'target', option('profile', 'fast'), own ? 'own-loader.exe' : 'run.exe');
const manifest = JSON.parse(
  await readFile(join(root, 'tests/workbooks', option('set', 'public'), 'manifest.json'), 'utf8'),
);
const files = manifest.files.filter((file) => file.path.includes(filter));
const stream = createWriteStream(out);

function runOne(file) {
  return new Promise((done) => {
    const path = join(root, 'tests/workbooks', option('set', 'public'), file.path);
    const child = spawn(bin, own ? [engine, path] : [engine, path, edits, '1']);
    let stdout = '';
    const timer = setTimeout(() => child.kill(), timeoutMs);
    child.stdout.on('data', (chunk) => (stdout += chunk));
    child.stderr.on('data', () => {});
    child.on('exit', (code, signal) => {
      clearTimeout(timer);
      let report;
      try {
        report = JSON.parse(stdout);
      } catch {
        report = { ok: false, error: signal ? 'timeout' : 'crash ' + code };
      }
      done({
        source: file.source,
        engineFixture: file.engineFixture,
        path: file.path,
        engine,
        ...report,
      });
    });
  });
}

let next = 0;
let finished = 0;
await Promise.all(
  Array.from({ length: concurrency }, async () => {
    while (next < files.length) {
      const result = await runOne(files[next++]);
      stream.write(JSON.stringify(result) + '\n');
      if (++finished % 500 === 0) console.log(finished + ' of ' + files.length);
    }
  }),
);
stream.end();
console.log('done ' + files.length);
