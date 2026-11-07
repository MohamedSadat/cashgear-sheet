// Checks the engine's memory budget in the real app, in Chrome against `pnpm preview` (build
// first), with the 10M-formula book from bench/generate-speed-books.mjs, which sits just under
// WASM's 4 GB:
//   1. once it is open, past 3 GB, the status bar warns while there is room to save;
//   2. filling more columns of formulas runs the engine out of memory mid-edit: the status bar
//      says so, the workbook closes, a new engine starts, and it offers the edits it kept;
//   3. the new engine takes work at once.
// Slow, so it isn't one of the browser specs.
// Usage: node bench/measure-memory-budget.mjs [--origin=http://127.0.0.1:4173]
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';
import { openBytes } from '../tests/e2e/helpers.mjs';
import { defaultStyledBook } from '../tests/fixtures/workbooks.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const origin = option('origin') ?? 'http://127.0.0.1:4173';
const book = join(root, 'tests/workbooks/generated/speed/formulas-10m.xlsx');

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

const browser = await chromium.launch({ channel: 'chrome', headless: false });
// The journal lives in OPFS, which lasts as long as the context, long enough for the offer.
const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
const page = await context.newPage();
await page.addInitScript(() => {
  delete window.showOpenFilePicker;
});
// What the browser does at the edge: a page or worker it stops, and what the console says.
const events = [];
const note = (event) => {
  events.push({ at: new Date().toISOString(), event });
  console.log('event: ' + event);
};
page.on('crash', () => note('the page crashed'));
page.on('worker', (worker) => {
  const name = worker.url().split('/').pop();
  note('worker started: ' + name);
  worker.on('close', () => note('worker closed: ' + name));
});
page.on('console', (message) => {
  if (message.type() === 'error') note('console: ' + message.text().slice(0, 300));
});
page.on('pageerror', (error) => note('page error: ' + error.message.slice(0, 300)));
await page.goto(origin);
const status = page.getByTestId('engine-status');
await status.filter({ hasText: 'Ready' }).waitFor();
const results = { events };
try {
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', { name: 'Open a workbook…' }).click();
  await (await chooser).setFiles(book);
  await page.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', undefined, {
    timeout: 1_200_000,
    polling: 500,
  });
  results.openMiB = Math.round((await page.evaluate(() => window.cgsTest.memoryBytes())) / 1048576);
  await page.waitForFunction(
    () =>
      /of the 4 GB/.test(
        document.querySelector('[data-testid="engine-status"]')?.textContent ?? '',
      ),
    undefined,
    { timeout: 30_000 },
  );
  results.warning = await status.textContent();
  console.log(`Open at ${results.openMiB} MiB: ${results.warning}`);

  // One edit, so the journal has something to keep, then four columns of 833,334 formulas at a
  // time, until there is no room left.
  await page.evaluate(() => window.cgsTest.edit(0, 1, 1, { number: 7 }));
  await page.evaluate(() => window.cgsTest.settled());
  const started = Date.now();
  const ranOut = () =>
    page.evaluate(() =>
      /ran out of memory/.test(
        document.querySelector('[data-testid="engine-status"]')?.textContent ?? '',
      ),
    );
  results.fills = [];
  for (let col = 6; col <= 60 && !(await ranOut()); col += 4) {
    const answer = await Promise.race([
      page.evaluate(
        (col) =>
          window.cgsTest
            .command({
              kind: 'input',
              sheet: 0,
              row: 1,
              col,
              text: '=A1+B1',
              areas: [{ top: 1, left: col, bottom: 833_334, right: col + 3 }],
            })
            .then(
              () => 'done',
              (error) => error.message,
            ),
        col,
      ),
      new Promise((resolve) => setTimeout(() => resolve('no answer in 10 minutes'), 600_000)),
    ]).catch((error) => 'the page failed: ' + error.message.split('\n')[0]);
    if (answer !== 'done' && !(await ranOut().catch(() => false))) {
      results.fills.push({ col, answer });
      console.log(`Columns ${col} to ${col + 3}: ${answer}`);
      break;
    }
    // Its calculation runs after the command, and can be what runs out.
    await page
      .waitForFunction(
        () =>
          !window.cgsTest.calculating() ||
          /ran out of memory/.test(
            document.querySelector('[data-testid="engine-status"]')?.textContent ?? '',
          ),
        undefined,
        { timeout: 600_000, polling: 500 },
      )
      .catch(() => undefined);
    const mib = (await ranOut())
      ? null
      : Math.round((await page.evaluate(() => window.cgsTest.memoryBytes())) / 1048576);
    results.fills.push({ col, answer, mib });
    console.log(`Columns ${col} to ${col + 3}: ${answer}, ${mib ?? 'ran out'} MiB`);
  }
  if (!(await ranOut())) throw new Error('The engine never ran out of memory.');
  results.ranOutAfterMs = Date.now() - started;
  results.message = await status.textContent();
  console.log(`Ran out after ${results.ranOutAfterMs} ms: ${results.message}`);
  // A new engine, an empty window, and the edits offered back.
  await page.getByTestId('welcome').waitFor({ timeout: 60_000 });
  await page.getByTestId('recover').waitFor({ timeout: 60_000 });
  results.offered = (await page.getByTestId('recover').textContent())?.trim();
  results.newEngineMiB = Math.round(
    (await page.evaluate(() => window.cgsTest.memoryBytes())) / 1048576,
  );
  console.log(`Offered: ${results.offered}`);
  await page.getByTestId('recover-discard').click();

  // The new engine takes work at once: nothing waits behind the command the old one never
  // answered. C1 is =B1*2.
  await openBytes(page, 'After.xlsx', defaultStyledBook());
  await page.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', undefined, {
    timeout: 60_000,
  });
  await page.evaluate(() => window.cgsTest.edit(0, 1, 2, { number: 21 }));
  await page.waitForFunction(
    async () => (await window.cgsTest.cellText(1, 3)) === '42',
    undefined,
    {
      timeout: 30_000,
    },
  );
  results.afterwards = 'a new workbook opened, and an edit calculated';
  console.log('Afterwards: ' + results.afterwards);
  writeFileSync(
    join(root, 'bench/results/memory-budget.json'),
    JSON.stringify(results, null, 2) + '\n',
  );
} finally {
  await browser.close();
  server?.kill();
}
