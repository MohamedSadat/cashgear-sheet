// Phase 9's scale benchmark, in the real app in Chrome against `pnpm preview` (build first): a
// ladder of generated workbooks (bench/generate-speed-books.mjs) towards WASM's 4 GB. For each
// it records how long the first sheet takes to show, how long until everything is read and the
// graph is built, a full recalculation (Ctrl+Alt+F9), one edit with its dependants, and the
// engine's memory after each step. WASM's memory never shrinks, so each reading is also the peak
// so far. A book that runs out records where. Timings mean something only on the reference
// machine.
// Usage: node bench/measure-scale.mjs [--only=name,name] [--out=file] [--origin=...]
import { spawn } from 'node:child_process';
import { existsSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const speed = join(root, 'tests/workbooks/generated/speed');
const out = option('out') ?? join(root, 'bench/results/scale.json');
const origin = option('origin') ?? 'http://127.0.0.1:4173';
const only = option('only')?.split(',');
const MIB = 1048576;

// Formulas are three to a row (C = A*B, D = C+1, E = IF(D>100, D-1, D)); values eight to a row.
const ladder = [
  { name: 'formulas-1m', formulas: 1_000_002, cells: 1_666_670 },
  { name: 'formulas-2m', formulas: 2_000_001, cells: 3_333_335 },
  { name: 'formulas-25mb', formulas: 2_580_000, cells: 4_300_000 },
  { name: 'formulas-4m', formulas: 4_000_002, cells: 6_666_670 },
  { name: 'formulas-6m', formulas: 6_000_000, cells: 10_000_000 },
  { name: 'formulas-8m', formulas: 8_000_001, cells: 13_333_335 },
  { name: 'formulas-10m', formulas: 10_000_008, cells: 16_666_680 },
  { name: 'formulas-past-memory', formulas: 15_000_000, cells: 25_000_000 },
  { name: 'values-100mb', formulas: 0, cells: 15_988_608 },
  { name: 'values-50m', formulas: 0, cells: 50_331_648 },
  { name: 'values-100m', formulas: 0, cells: 100_663_296 },
].filter((book) => !only || only.includes(book.name));

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
  await new Promise((resolve) => setTimeout(resolve, 500));
}

const previous = existsSync(out) ? JSON.parse(readFileSync(out, 'utf8')) : { books: {} };
const results = {
  measured: new Date().toISOString(),
  machine: 'reference (Xeon E5-2680 v3, 32 GB)',
  browser: '',
  books: previous.books ?? {},
};
const browser = await chromium.launch({ channel: 'chrome', headless: false });
results.browser = 'Chrome ' + browser.version();
const save = () => writeFileSync(out, JSON.stringify(results, null, 2) + '\n');

const outOfMemory = (text) => /ran out of memory|too large/i.test(text ?? '');

for (const book of ladder) {
  const path = join(speed, book.name + '.xlsx');
  if (!existsSync(path)) {
    console.log(`${book.name}: not generated, skipped`);
    continue;
  }
  const result = {
    fileMB: Number((statSync(path).size / 1e6).toFixed(1)),
    formulas: book.formulas,
    cells: book.cells,
  };
  results.books[book.name] = result;
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.addInitScript(() => {
    delete window.showOpenFilePicker;
  });
  await page.goto(origin);
  await page.waitForFunction(
    () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
  );
  const status = () =>
    page.evaluate(() => document.querySelector('[data-testid="engine-status"]')?.textContent ?? '');
  const memory = async () =>
    Math.round((await page.evaluate(() => window.cgsTest.memoryBytes())) / MIB);
  /** Waits for `test`, or for the engine to say it ran out; true if it did. */
  const until = async (test, timeout = 1_800_000) => {
    const handle = await page.waitForFunction(
      (source) => {
        const text = document.querySelector('[data-testid="engine-status"]')?.textContent ?? '';
        if (/ran out of memory|too large/i.test(text)) return 'oom';
        return new Function('return (' + source + ')()')() ? 'done' : false;
      },
      test.toString(),
      { timeout, polling: 100 },
    );
    return (await handle.jsonValue()) === 'oom';
  };
  const failed = async (step, started) => {
    result.outOfMemory = { step, afterMs: Date.now() - started, message: await status() };
    console.log(`${book.name}: ran out of memory while ${step}, after ${Date.now() - started} ms`);
  };

  try {
    const started = Date.now();
    const chooser = page.waitForEvent('filechooser');
    await page.getByRole('button', { name: 'Open a workbook…' }).click();
    await (await chooser).setFiles(path);
    if (
      await until(
        () => document.querySelector('[data-testid="sheet-canvas"]')?.dataset.ready === 'true',
      )
    ) {
      await failed('reading', started);
      continue;
    }
    result.firstSheetMs = Date.now() - started;
    if (await until(() => window.cgsTest.loadPhase() === 'ready')) {
      await failed('reading or building the graph', started);
      continue;
    }
    result.readyMs = Date.now() - started;
    result.readyMiB = await memory();
    console.log(
      `${book.name}: shown ${result.firstSheetMs} ms, ready ${result.readyMs} ms, ${result.readyMiB} MiB`,
    );

    // Ctrl+Alt+F9: every formula, from scratch.
    let at = Date.now();
    const full = await page
      .evaluate(() => window.cgsTest.calculateNow(true).then(() => 'done'))
      .catch((error) => error.message);
    if (full !== 'done' || outOfMemory(await status())) {
      await failed('recalculating everything', at);
      continue;
    }
    result.fullRecalcMs = Date.now() - at;
    result.fullRecalcMiB = await memory();

    // One edit, the first time: this builds the graph if opening hadn't yet.
    at = Date.now();
    await page.evaluate(() => window.cgsTest.edit(0, 1, 1, { number: 7 }));
    await page.evaluate(() => window.cgsTest.settled());
    if (await until(() => !window.cgsTest.calculating())) {
      await failed('editing', at);
      continue;
    }
    result.firstEditMs = Date.now() - at;
    // A second edit, the cost a user feels from then on.
    at = Date.now();
    await page.evaluate(() => window.cgsTest.edit(0, 2, 1, { number: 9 }));
    await page.evaluate(() => window.cgsTest.settled());
    await until(() => !window.cgsTest.calculating());
    result.editMs = Date.now() - at;
    result.endMiB = await memory();
    if (book.formulas) {
      // Row 2: A2 = 9 and B2 = 2*7 % 13 = 1, so C2 = 9, D2 = 10, E2 = 10.
      const values = await page.evaluate(async () => [
        await window.cgsTest.cellText(2, 3),
        await window.cgsTest.cellText(2, 4),
        await window.cgsTest.cellText(2, 5),
      ]);
      result.checked = values.join(',') === '9,10,10';
      if (!result.checked) result.checkedValues = values;
    }
    result.bytesPerFormula = book.formulas
      ? Math.round((result.endMiB * MIB) / book.formulas)
      : undefined;
    result.bytesPerCell = Math.round((result.endMiB * MIB) / book.cells);
    console.log(
      `${book.name}: full recalc ${result.fullRecalcMs} ms, first edit ${result.firstEditMs} ms, ` +
        `edit ${result.editMs} ms, ${result.endMiB} MiB`,
    );
  } catch (error) {
    result.error = error.message.split('\n')[0];
    console.log(`${book.name}: ${result.error}`);
  } finally {
    if (errors.length) result.pageErrors = errors.slice(0, 3);
    save();
    await context.close();
  }
}
await browser.close();
server?.kill();
save();
console.log('Results in ' + out);
