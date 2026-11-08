// Opens a workbook too large for WASM's 4 GB, and checks that the app says so and then opens
// the next file. Slow, so it isn't one of the browser specs.
// Usage: node bench/measure-out-of-memory.mjs [--origin=http://127.0.0.1:4173]
import { join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const origin = option('origin') ?? 'http://127.0.0.1:4173';
const speed = join(root, 'tests/workbooks/generated/speed');

const browser = await chromium.launch({ channel: 'chrome', headless: true });
const page = await browser.newPage();
await page.addInitScript(() => {
  delete window.showOpenFilePicker;
});
await page.goto(origin);
const status = page.getByTestId('engine-status');
await status.filter({ hasText: 'Ready' }).waitFor();

async function choose(path) {
  const chooser = page.waitForEvent('filechooser');
  // The welcome button is there until a workbook opens, and Ctrl+O always is.
  await page.keyboard.press('Control+O');
  await (await chooser).setFiles(path);
}

const started = Date.now();
await choose(join(speed, 'formulas-past-memory.xlsx'));
await page.waitForFunction(
  () =>
    /ran out of memory|too large to open/.test(
      document.querySelector('[data-testid="engine-status"]')?.textContent ?? '',
    ),
  undefined,
  { timeout: 1200000, polling: 500 },
);
const message = await status.textContent();
const memory = await page.evaluate(() => window.cgsTest.memoryBytes().catch(() => 0));
console.log('After ' + (Date.now() - started) + ' ms: ' + message);
console.log('The engine holds ' + Math.round(memory / 1048576) + ' MiB');

// The next file opens: since Phase 12 the book is refused before reading, and the same engine
// takes it; a book that runs out while reading gets a new engine first.
await page.waitForFunction(() => document.querySelector('[data-testid="welcome"]') !== null);
await page.waitForTimeout(2000);
await choose(join(speed, 'values-25mb.xlsx'));
await page.waitForFunction(
  () => document.querySelector('[data-testid="sheet-canvas"]')?.dataset.ready === 'true',
  undefined,
  { timeout: 120000, polling: 100 },
);
console.log('The next file opened: ' + (await status.textContent()));
await browser.close();
