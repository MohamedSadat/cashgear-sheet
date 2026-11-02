// Measures Phase 2's speed targets in the real app, in Chrome: how long a workbook takes to
// open, and how smoothly a million rows scroll. It runs against `pnpm preview`, so build first.
// Timings mean something only on the reference machine.
// Usage: node bench/measure-speed.mjs [--open=path,...] [--scroll=path] [--out=file]
import { spawn } from 'node:child_process';
import { readdirSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const speedFolder = join(root, 'tests/workbooks/generated/speed');
const openList = option('open')
  ? option('open').split(',')
  : [
      join(root, 'tests/workbooks/generated/row-model-333k.xlsx'),
      join(root, 'tests/workbooks/public/exceljs/spec__integration__data__huge.xlsx'),
      ...readdirSync(speedFolder)
        .filter((name) => name.endsWith('.xlsx'))
        .map((name) => join(speedFolder, name)),
    ];
const scrollFile = option('scroll') ?? join(speedFolder, 'rows-1m.xlsx');
const out = option('out') ?? join(root, 'bench/results/speed.json');
const origin = option('origin') ?? 'http://127.0.0.1:4173';

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

// Frame times need a real window; opening times don't.
const browser = await chromium.launch({
  channel: 'chrome',
  headless: process.argv.includes('--headless'),
});

/** A fresh page with this file chosen through the picker's fallback, as a user would. */
async function openFile(path) {
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const page = await context.newPage();
  await page.addInitScript(() => {
    delete window.showOpenFilePicker;
  });
  await page.goto(origin);
  await page.waitForFunction(
    () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
  );
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', { name: 'Open a workbook…' }).click();
  const started = Date.now();
  await (await chooser).setFiles(path);
  return { context, page, started };
}

async function measureOpen(path) {
  const { context, page, started } = await openFile(path);
  await page.waitForFunction(
    () => document.querySelector('[data-testid="sheet-canvas"]')?.dataset.ready === 'true',
    undefined,
    { timeout: 600000, polling: 50 },
  );
  const showing = Date.now() - started;
  // Is the page still answering while the graph builds?
  const longest = await page.evaluate(
    () =>
      new Promise((resolve) => {
        let last = performance.now();
        let worst = 0;
        const stop = performance.now() + 2000;
        const tick = () => {
          const now = performance.now();
          worst = Math.max(worst, now - last);
          last = now;
          if (now < stop) requestAnimationFrame(tick);
          else resolve(worst);
        };
        requestAnimationFrame(tick);
      }),
  );
  await page.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', undefined, {
    timeout: 600000,
    polling: 200,
  });
  const memory = await page.evaluate(() => window.cgsTest.memoryBytes());
  const result = {
    file: path.split(/[\\/]/).pop(),
    megabytes: Number((statSync(path).size / 1048576).toFixed(1)),
    showingMs: showing,
    readyMs: Date.now() - started,
    longestFrameWhilePreparingMs: Math.round(longest),
    wasmMemoryMiB: Math.round(memory / 1048576),
  };
  await context.close();
  console.log(JSON.stringify(result));
  return result;
}

function percentile(sorted, share) {
  return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * share))];
}

/** Records the gap between frames while `action` runs. */
async function frames(page, action) {
  await page.evaluate(() => {
    window.frameGaps = [];
    window.recording = true;
    let last = performance.now();
    const tick = () => {
      const now = performance.now();
      window.frameGaps.push(now - last);
      last = now;
      if (window.recording) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await action();
  const gaps = await page.evaluate(() => {
    window.recording = false;
    return window.frameGaps;
  });
  const sorted = [...gaps].sort((a, b) => a - b);
  return {
    frames: gaps.length,
    p50Ms: Number(percentile(sorted, 0.5).toFixed(1)),
    p95Ms: Number(percentile(sorted, 0.95).toFixed(1)),
    p99Ms: Number(percentile(sorted, 0.99).toFixed(1)),
    maxMs: Number(sorted[sorted.length - 1].toFixed(1)),
    within17Ms: Number(
      ((sorted.filter((gap) => gap <= 17).length / sorted.length) * 100).toFixed(1),
    ),
  };
}

async function measureScroll(path) {
  const { context, page, started } = await openFile(path);
  await page.waitForFunction(
    () => document.querySelector('[data-testid="sheet-canvas"]')?.dataset.ready === 'true',
    undefined,
    { timeout: 600000, polling: 50 },
  );
  console.log('scrolling file showing after ' + (Date.now() - started) + ' ms');
  const canvas = page.getByTestId('sheet-canvas');
  const box = await canvas.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  const results = { file: path.split(/[\\/]/).pop() };

  // Steady wheel scrolling, about three notches a second, as someone reading down a sheet.
  results.wheel = await frames(page, async () => {
    for (let tick = 0; tick < 240; tick++) {
      await page.mouse.wheel(0, 40);
      await page.waitForTimeout(16);
    }
  });
  results.topRowAfterWheel = await page.evaluate(() => window.cgsTest.grid().state().topRow);

  // A long drag of the scrollbar, top to bottom and back.
  const x = box.x + box.width - 7;
  const top = box.y + 40;
  const bottom = box.y + box.height - 30;
  await page.evaluate(() => window.cgsTest.grid().scrollTo(0, 0));
  await page.mouse.move(x, top + 2);
  await page.mouse.down();
  results.drag = await frames(page, async () => {
    for (let step = 0; step <= 200; step++) {
      await page.mouse.move(x, top + ((bottom - top) * step) / 200);
      await page.waitForTimeout(16);
    }
    for (let step = 200; step >= 0; step--) {
      await page.mouse.move(x, top + ((bottom - top) * step) / 200);
      await page.waitForTimeout(16);
    }
  });
  await page.mouse.up();

  // How long the cells stay empty once a fast drag stops.
  await page.mouse.move(x, top + 2);
  await page.mouse.down();
  await page.mouse.move(x, (top + bottom) / 2, { steps: 4 });
  await page.mouse.up();
  const stopped = Date.now();
  await page.waitForFunction(
    () => document.querySelector('[data-testid="sheet-canvas"]')?.dataset.ready === 'true',
    undefined,
    { timeout: 60000, polling: 'raf' },
  );
  results.filledAfterDragStopsMs = Date.now() - stopped;
  await context.close();
  console.log(JSON.stringify(results));
  return results;
}

const output = { chrome: browser.version(), open: [], scroll: null };
for (const path of openList) output.open.push(await measureOpen(path));
if (!process.argv.includes('--no-scroll')) output.scroll = await measureScroll(scrollFile);
writeFileSync(out, JSON.stringify(output, null, 2) + '\n');
await browser.close();
server?.kill();
process.exit(0);
