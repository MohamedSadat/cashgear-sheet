// Phase 15's binding targets (plan B.6): 1,000 records of 20 fields from response to first paint
// in at most 300 ms, and 10,000 (the cap) in at most 1.5 s, in Chrome's WASM on the reference
// machine, median of 5 runs. The network is left out, as B.6 says: Query pages' bytes are made here
// and handed to the page, which stages them in the engine and binds them as a launch does
// (`cgsTest.bindPages`), then waits for the grid's first paint. It runs against `pnpm preview`, so
// build first.
// Usage: node bench/measure-erp-bind.mjs [--out=file] [--runs=5]
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';
import { FIELDS, pages, schema, session } from './erp-pages.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const out = option('out') ?? join(root, 'bench/results/erp-bind.json');
const origin = option('origin') ?? 'http://127.0.0.1:4173';
const RUNS = Number(option('runs') ?? 5);
const SIZES = [1000, 10000];
const TARGETS = { 1000: 300, 10000: 1500 };

async function serverIsUp() {
  try {
    return (await fetch(origin, { signal: AbortSignal.timeout(1000) })).ok;
  } catch {
    return false;
  }
}

const median = (list) => [...list].sort((a, b) => a - b)[Math.floor(list.length / 2)];

const server = (await serverIsUp())
  ? null
  : spawn('pnpm', ['preview'], { cwd: root, stdio: 'ignore', shell: true });
for (let tries = 0; !(await serverIsUp()); tries++) {
  if (tries > 60) throw new Error('The preview server did not start.');
  await new Promise((resolve) => setTimeout(resolve, 500));
}
const browser = await chromium.launch({ channel: 'chrome', headless: true });
const results = [];
try {
  for (const size of SIZES) {
    const encoded = pages(size);
    const runs = [];
    for (let run = 0; run < RUNS; run++) {
      const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
      page.on('pageerror', (error) => console.log('page error:', error.message));
      await page.goto(origin);
      await page.waitForFunction(
        () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
      );
      const timing = await page.evaluate(
        async ({ schema, encoded, session }) => {
          const bytes = encoded.map((text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0)));
          const steps = await window.cgsTest.bindPages(schema, bytes, session);
          // From the response (the records handed over) to the first paint, as plan B.6 measures;
          // the new workbook opened meanwhile, as a launch opens it while the records download.
          const started = steps.startedAt;
          // The grid's first paint of the bound sheet.
          await new Promise((done) => {
            const check = () => {
              const canvas = document.querySelector('[data-testid="sheet-canvas"]');
              if (canvas?.dataset.ready === 'true') requestAnimationFrame(() => done());
              else setTimeout(check, 1);
            };
            // A frame first, so the flag says what the bound sheet's tiles are, not the blank's.
            requestAnimationFrame(check);
          });
          const at = (name) => performance.getEntriesByName(`erp:${name}`).at(-1)?.startTime ?? NaN;
          return {
            ...steps,
            totalMs: performance.now() - started,
            openMs: at('opened') - at('begin'),
            engineBindMs: at('bound') - at('opened'),
            fitMs: at('fitted') - at('bound'),
            showMs: at('shown') - at('fitted'),
            paintMs: performance.now() - started - steps.stageMs - steps.bindMs,
            overlappedOpenMs: steps.openMs,
          };
        },
        { schema, encoded, session },
      );
      const memory = await page.evaluate(() => window.cgsTest.memoryBytes());
      const ms = (n) => n.toFixed(0);
      console.log(
        `${size} records: ${ms(timing.totalMs)} ms (stage ${ms(timing.stageMs)}, open while downloading ${ms(timing.overlappedOpenMs)}, bind ${ms(timing.engineBindMs)}, fit ${ms(timing.fitMs)}, show ${ms(timing.showMs)}, paint ${ms(timing.paintMs)}), ${timing.records} bound`,
      );
      if (timing.records !== size) throw new Error(`Bound ${timing.records} of ${size} records.`);
      runs.push({ ...timing, memoryBytes: memory });
      await page.close();
    }
    const total = median(runs.map((r) => r.totalMs));
    results.push({
      records: size,
      fields: FIELDS.length,
      runs,
      medianMs: Math.round(total),
      medianStageMs: Math.round(median(runs.map((r) => r.stageMs))),
      targetMs: TARGETS[size],
      met: total <= TARGETS[size],
    });
  }
} finally {
  await browser.close();
  // The server is a shell's grandchild, which killing the shell would leave running.
  if (server) spawnSync('taskkill', ['/pid', String(server.pid), '/t', '/f'], { stdio: 'ignore' });
}
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify({ measuredAt: new Date().toISOString(), results }, null, 2));
for (const r of results) {
  console.log(
    `${r.records} × ${r.fields}: median ${r.medianMs} ms (staging ${r.medianStageMs} ms), target ${r.targetMs} ms: ${r.met ? 'met' : 'NOT met'}`,
  );
}
