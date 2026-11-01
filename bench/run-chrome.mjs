// Runs the corpus through the WASM harness in Chrome, one fresh worker per workbook.
// These are the bake-off's numbers. Timings need --jobs=1.
// Usage: node bench/run-chrome.mjs <engine> <out.jsonl> [--set=public|generated|private]
//   [--filter=text] [--edits=N] [--jobs=N] [--timeout=ms]
// The private set reads CGS_PRIVATE_WORKBOOKS. Its results hold no file names or cell values.
import { createHash } from 'node:crypto';
import { createReadStream, createWriteStream, existsSync, readdirSync, statSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, join, resolve, sep } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';

const [engine, out, ...rest] = process.argv.slice(2);
const option = (name, fallback) => {
  const found = rest.find((arg) => arg.startsWith('--' + name + '='));
  return found ? found.split('=')[1] : fallback;
};
const set = option('set', 'public');
const filter = option('filter', '');
const edits = Number(option('edits', '100'));
const jobs = Number(option('jobs', '1'));
const timeoutMs = Number(option('timeout', '300000'));

function walk(dir, found = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) walk(path, found);
    else if (/\.(xlsx|xlsm)$/i.test(entry) && !entry.startsWith('~$')) found.push(path);
  }
  return found;
}

async function listFiles() {
  if (set === 'private') {
    const folder = process.env.CGS_PRIVATE_WORKBOOKS;
    if (!folder) throw new Error('Set CGS_PRIVATE_WORKBOOKS to the folder of private workbooks.');
    return walk(folder).map((path) => ({
      source: 'private',
      engineFixture: false,
      // A hash, so results can line up across engines without naming the file.
      path: createHash('sha256').update(path).digest('hex').slice(0, 16),
      absolute: path,
      examples: false,
    }));
  }
  const base = join(root, 'tests/workbooks', set);
  const manifest = JSON.parse(await readFile(join(base, 'manifest.json'), 'utf8'));
  return manifest.files
    .filter((file) => file.path.includes(filter))
    .map((file) => ({ ...file, absolute: join(base, file.path), examples: true }));
}

const files = await listFiles();
const byId = new Map(files.map((file, index) => [String(index), file]));
const mime = { '.js': 'text/javascript', '.html': 'text/html', '.wasm': 'application/wasm' };
const statics = {
  '/chrome/': join(root, 'bench/chrome'),
  '/wasm/': join(root, 'bench/results/wasm'),
};

const server = createServer((request, response) => {
  const url = new URL(request.url, 'http://localhost');
  response.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
  response.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
  let path;
  if (url.pathname === '/workbook') {
    path = byId.get(url.searchParams.get('id'))?.absolute;
  } else {
    for (const [prefix, dir] of Object.entries(statics)) {
      if (!url.pathname.startsWith(prefix)) continue;
      const candidate = resolve(dir, '.' + url.pathname.slice(prefix.length - 1));
      if (candidate === dir || candidate.startsWith(dir + sep)) path = candidate;
    }
  }
  if (!path || !existsSync(path) || statSync(path).isDirectory()) {
    response.statusCode = 404;
    response.end();
    return;
  }
  response.setHeader('Content-Type', mime[extname(path)] ?? 'application/octet-stream');
  createReadStream(path).pipe(response);
});
await new Promise((done) => server.listen(0, '127.0.0.1', done));
const origin = 'http://127.0.0.1:' + server.address().port;

const browser = await chromium.launch({ channel: 'chrome', headless: true });
const stream = createWriteStream(out);
const version = browser.version();
let next = 0;
let finished = 0;
await Promise.all(
  Array.from({ length: jobs }, async () => {
    const page = await browser.newPage();
    await page.goto(origin + '/chrome/index.html');
    while (next < files.length) {
      const id = next++;
      const file = files[id];
      const report = await page.evaluate(
        ([options, timeout]) => window.runWorkbook(options, timeout),
        [
          {
            engine,
            url: '/workbook?id=' + id,
            edits,
            topRowInsert: true,
            examples: file.examples,
            skipCalc: false,
          },
          timeoutMs,
        ],
      );
      stream.write(
        JSON.stringify({
          source: file.source,
          engineFixture: file.engineFixture === true,
          path: file.path,
          bytes: file.bytes,
          engine,
          chrome: version,
          ...report,
        }) + '\n',
      );
      if (++finished % 200 === 0) console.log(finished + ' of ' + files.length);
    }
    await page.close();
  }),
);
await browser.close();
server.close();
await new Promise((done) => stream.end(done));
console.log('done ' + files.length + ' in Chrome ' + version);
