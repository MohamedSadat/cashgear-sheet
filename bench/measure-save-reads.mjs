// Phase 12: a save no longer holds the engine. In the release build in Chrome, against
// `pnpm preview` (build first), this opens the 25 MB formula book, edits it, and saves it while
// asking for cells, engine tiles and drawn tiles one after another, timing each. Partway through
// the save it makes another edit, which must wait for the save and then give the right answers.
// Excel then opens both saves: the first holds the first edit only, the second both.
// Usage: node bench/measure-save-reads.mjs [--book=formulas-25mb] [--out=file] [--no-excel]
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const bookName = option('book') ?? 'formulas-25mb';
const book = join(root, 'tests/workbooks/generated/speed', bookName + '.xlsx');
const folder = join(root, 'bench/results/save-slices');
const out = option('out') ?? join(folder, 'save-reads.json');
const origin = option('origin') ?? 'http://127.0.0.1:4173';
const withExcel = !process.argv.includes('--no-excel');
mkdirSync(folder, { recursive: true });
if (!existsSync(book)) throw new Error(`${book} is missing: run bench/generate-speed-books.mjs.`);

async function serverIsUp() {
  try {
    return (await fetch(origin, { signal: AbortSignal.timeout(1000) })).ok;
  } catch {
    return false;
  }
}
const server = (await serverIsUp())
  ? null
  : spawn('pnpm', ['preview'], { cwd: root, stdio: 'ignore', shell: true });
for (let tries = 0; !(await serverIsUp()); tries++) {
  if (tries > 60) throw new Error('The preview server did not start.');
  await new Promise((done) => setTimeout(done, 500));
}

const percentile = (values, p) => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return Number(
    sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)].toFixed(1),
  );
};
const summary = (probes) => {
  const ms = probes.map((probe) => probe.ms);
  return {
    count: ms.length,
    p50: percentile(ms, 50),
    p95: percentile(ms, 95),
    p99: percentile(ms, 99),
    max: ms.length ? Number(Math.max(...ms).toFixed(1)) : null,
    over100: ms.filter((value) => value > 100).length,
  };
};

const results = { measured: new Date().toISOString(), book: bookName };
const browser = await chromium.launch({ channel: 'chrome', headless: false });
results.browser = 'Chrome ' + browser.version();
const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
const page = await context.newPage();
const errors = [];
page.on('pageerror', (error) => errors.push(error.message));
await page.addInitScript(() => {
  delete window.showOpenFilePicker;
});

/** Runs a probe loop in the page until `window.cgsProbeStop` is set, and returns what it timed. */
const startProbes = () =>
  page.evaluate(async () => {
    const probes = [];
    const kinds = ['cell', 'block', 'tile'];
    window.cgsProbeStop = false;
    window.cgsProbes = probes;
    for (let index = 0; !window.cgsProbeStop; index++) {
      const kind = kinds[index % 3];
      // Anywhere in the book's used rows, so the engine can't answer from a tile it just drew.
      const row = 1 + Math.floor(Math.random() * 850_000);
      const col = 1 + Math.floor(Math.random() * 5);
      const at = performance.now();
      const ms = await window.cgsTest.probe(
        kind,
        kind === 'cell' ? row : row - (row % 64) + 1,
        kind === 'cell' ? col : 1,
      );
      probes.push({ kind, ms, at, saving: window.cgsSaving === true });
      await new Promise((done) => setTimeout(done, 10));
    }
    return probes;
  });
const stopProbes = () => page.evaluate(() => (window.cgsProbeStop = true));
const status = () =>
  page.evaluate(() => document.querySelector('[data-testid="engine-status"]')?.textContent ?? '');
const cells = (row) =>
  page.evaluate(
    async (row) => Promise.all([1, 2, 3, 4, 5].map((col) => window.cgsTest.cellText(row, col))),
    row,
  );

/** Reads a file the page saved to its private storage, onto disk. */
async function fetchSaved(name) {
  const base64 = await page.evaluate(async (name) => {
    const handle = await (await navigator.storage.getDirectory()).getFileHandle(name);
    const bytes = new Uint8Array(await (await handle.getFile()).arrayBuffer());
    let binary = '';
    for (let at = 0; at < bytes.length; at += 32768) {
      binary += String.fromCharCode(...bytes.subarray(at, at + 32768));
    }
    return btoa(binary);
  }, name);
  const path = join(folder, name);
  writeFileSync(path, Buffer.from(base64, 'base64'));
  return path;
}

/** Saves to a private file through the app's own Save path, with its command queue. */
const saveTo = (name) =>
  page.evaluate(async (name) => {
    const handle = await (
      await navigator.storage.getDirectory()
    ).getFileHandle(name, {
      create: true,
    });
    const target = {
      async write(blob) {
        const writable = await handle.createWritable();
        await writable.write(blob);
        await writable.close();
        return { name, source: { kind: 'blob', blob: await handle.getFile() }, recent: handle };
      },
    };
    window.cgsSaving = true;
    const started = performance.now();
    try {
      await window.cgsTest.writeTo(target);
    } finally {
      window.cgsSaving = false;
    }
    return { ms: performance.now() - started, problem: window.cgsTest.problem() };
  }, name);

try {
  await page.goto(origin);
  await page.waitForFunction(
    () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
  );
  const opened = Date.now();
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', { name: 'Open a workbook…' }).click();
  await (await chooser).setFiles(book);
  await page.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', undefined, {
    timeout: 900_000,
    polling: 200,
  });
  results.readyMs = Date.now() - opened;

  // The first edit: A1 = 7, so C1 = 7 * 7 = 49, D1 = 50, E1 = 50.
  await page.evaluate(() => window.cgsTest.edit(0, 1, 1, { number: 7 }));
  await page.evaluate(() => window.cgsTest.settled());
  await page.waitForFunction(() => !window.cgsTest.calculating(), undefined, { timeout: 600_000 });
  results.afterFirstEdit = await cells(1);

  // Reads with nothing else going on, to compare with.
  const idle = startProbes();
  await new Promise((done) => setTimeout(done, 5000));
  await stopProbes();
  results.idle = summary(await idle);

  // Reads during a save, and an edit partway through: A2 = 11, so C2 = 11, D2 = 12, E2 = 12.
  const during = startProbes();
  const saving = saveTo('saved-1.xlsx');
  await page.waitForFunction(
    () => {
      const text = document.querySelector('[data-testid="engine-status"]')?.textContent ?? '';
      const match = /Saving (\d+)%/.exec(text);
      return match && Number(match[1]) >= 25;
    },
    undefined,
    { timeout: 600_000, polling: 20 },
  );
  results.statusMidSave = await status();
  const editStarted = Date.now();
  const midEdit = page.evaluate(() => window.cgsTest.edit(0, 2, 1, { number: 11 }));
  const saved = await saving;
  await midEdit;
  results.midEditWaitedMs = Date.now() - editStarted;
  await stopProbes();
  const probes = await during;
  results.save = saved;
  const inSave = probes.filter((probe) => probe.saving);
  results.duringSave = {
    all: summary(inSave),
    cell: summary(inSave.filter((probe) => probe.kind === 'cell')),
    block: summary(inSave.filter((probe) => probe.kind === 'block')),
    tile: summary(inSave.filter((probe) => probe.kind === 'tile')),
  };
  await page.evaluate(() => window.cgsTest.settled());
  await page.waitForFunction(() => !window.cgsTest.calculating(), undefined, { timeout: 600_000 });
  results.afterSave = { row1: await cells(1), row2: await cells(2) };
  results.changesAfterSave = await page.evaluate(() => window.cgsTest.hasChanges());
  results.answersRight =
    results.afterSave.row1.join(',') === '7,7,49,50,50' &&
    results.afterSave.row2.join(',') === '11,1,11,12,12' &&
    results.changesAfterSave === true;

  // A second save holds the edit that waited.
  results.secondSave = await saveTo('saved-2.xlsx');
  const first = await fetchSaved('saved-1.xlsx');
  const second = await fetchSaved('saved-2.xlsx');
  results.files = { first, second };
} catch (error) {
  results.error = error.message.split('\n')[0];
} finally {
  if (errors.length) results.pageErrors = errors.slice(0, 5);
  await browser.close();
  server?.kill();
}

/** Runs one of the bench's Excel scripts. Never two at once. */
function excel(script, parameters) {
  const result = join(folder, 'excel-result.json');
  rmSync(result, { force: true });
  const args = Object.entries({ ...parameters, Out: result })
    .map(([key, value]) => `-${key} '${String(value).replace(/'/g, "''")}'`)
    .join(' ');
  const env = { ...process.env };
  for (const name of Object.keys(env)) if (name.toLowerCase() === 'psmodulepath') delete env[name];
  const run = spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `& {${readFileSync(join(root, script), 'utf8')}} ${args}`,
    ],
    { encoding: 'utf8', timeout: 900_000, windowsHide: true, env },
  );
  if (run.status !== 0) throw new Error('Excel script failed: ' + run.stderr.slice(0, 300));
  return JSON.parse(readFileSync(result, 'utf8'));
}

writeFileSync(out, JSON.stringify(results, null, 2) + '\n');
if (withExcel && results.files) {
  results.excel = {};
  try {
    for (const [which, path] of Object.entries(results.files)) {
      const opened = excel('bench/excel-open.ps1', { Workbook: resolve(path), Range: 'A1:E2' });
      const texts = opened.texts ?? [];
      results.excel[which] = {
        opened: opened.opened,
        repaired: opened.repaired,
        error: opened.error,
        cells: opened.cells,
        rows: [texts.slice(0, 5), texts.slice(5, 10)],
      };
    }
    const [first, second] = [results.excel.first, results.excel.second];
    results.excelRight =
      first.opened &&
      !first.repaired &&
      second.opened &&
      !second.repaired &&
      first.rows[0].join(',') === '7,7,49,50,50' &&
      first.rows[1].join(',') === '2,1,2,3,3' &&
      second.rows[0].join(',') === '7,7,49,50,50' &&
      second.rows[1].join(',') === '11,1,11,12,12';
  } catch (error) {
    results.excelError = error.message.split('\n')[0];
  }
}

writeFileSync(out, JSON.stringify(results, null, 2) + '\n');
console.log(JSON.stringify(results, null, 2));
