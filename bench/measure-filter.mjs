// Phase 6's filter target: a value-list filter on a column of a million rows, from the command to
// the filtered view ready to draw, in Chrome on the reference machine, median of 5 runs. It runs
// against `pnpm preview`, so build first.
// Usage: node bench/measure-filter.mjs [--file=path] [--out=file]
import { spawn } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const file = option('file') ?? join(root, 'tests/workbooks/generated/speed/rows-1m.xlsx');
const out = option('out') ?? join(root, 'bench/results/filter-speed.json');
const origin = option('origin') ?? 'http://127.0.0.1:4173';
const RUNS = 5;

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
const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
  page.on('worker', (worker) => {
    worker.on('console', (message) => {
      if (message.type() === 'error') console.log('worker:', message.text());
    });
  });
  await page.goto(origin);
  await page.waitForFunction(
    () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
  );
  console.log('page ready');
  page.on('console', (message) => {
    if (message.type() === 'error') console.log('page:', message.text());
  });
  const bytes = readFileSync(file).toString('base64');
  await page.evaluate(async (base64) => {
    const data = Uint8Array.from(atob(base64), (ch) => ch.charCodeAt(0));
    const blob = new Blob([data]);
    await window.cgsTest.open({ name: 'rows-1m.xlsx', source: { kind: 'blob', blob } });
  }, bytes);
  console.log('opening');
  await page.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', undefined, {
    timeout: 600000,
    polling: 200,
  });
  console.log('opened');
  const rows = await page.evaluate(async () => {
    await window.cgsTest.command({
      kind: 'toggleFilter',
      sheet: 0,
      area: { top: 1, left: 1, bottom: 1, right: 1 },
    });
    await window.cgsTest.settled();
    return 1048576;
  });
  console.log('filter buttons on');
  const timings = [];
  for (let run = 0; run < RUNS; run++) {
    const ms = await page.evaluate(async (run) => {
      const started = performance.now();
      await window.cgsTest.command({
        kind: 'setFilter',
        sheet: 0,
        col: 8,
        criteria: { kind: 'values', values: [String(run * 7), String(300 + run)] },
      });
      await window.cgsTest.settled();
      // Ready to draw: the grid has the sheet's new rows.
      await new Promise((resolve) => requestAnimationFrame(() => resolve()));
      const took = performance.now() - started;
      await window.cgsTest.command({ kind: 'clearFilter', sheet: 0 });
      await window.cgsTest.settled();
      return took;
    }, run);
    timings.push(Math.round(ms));
    console.log('run', run + 1, Math.round(ms), 'ms');
  }
  const sorted = [...timings].sort((a, b) => a - b);
  const result = {
    file: file.split(/[\\/]/).pop(),
    rows,
    runsMs: timings,
    medianMs: sorted[Math.floor(sorted.length / 2)],
    targetMs: 200,
  };
  writeFileSync(out, JSON.stringify(result, null, 1) + '\n');
  console.log(JSON.stringify(result));
} finally {
  await browser.close();
  server?.kill();
}
