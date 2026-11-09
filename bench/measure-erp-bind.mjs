// Phase 15's binding targets (plan B.6): 1,000 records of 20 fields from response to first paint
// in at most 300 ms, and 10,000 (the cap) in at most 1.5 s, in Chrome's WASM on the reference
// machine, median of 5 runs. The network is left out, as B.6 says: Query pages' bytes are made here
// and handed to the page, which stages them in the engine and binds them as a launch does
// (`cgsTest.bindPages`), then waits for the grid's first paint. It runs against `pnpm preview`, so
// build first.
// Usage: node bench/measure-erp-bind.mjs [--out=file] [--runs=5]
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const out = option('out') ?? join(root, 'bench/results/erp-bind.json');
const origin = option('origin') ?? 'http://127.0.0.1:4173';
const RUNS = Number(option('runs') ?? 5);
const SIZES = [1000, 10000];
const TARGETS = { 1000: 300, 10000: 1500 };

/** The vendors' 13 fields and 7 more text fields: 20 in all, as B.6 measures. */
const FIELDS = [
  ['vendorAccount', 'text'],
  ['vendorName', 'text'],
  ['vendorGroup', 'lookup'],
  ['mobile', 'text'],
  ['currency', 'lookup'],
  ['payMethod', 'lookup'],
  ['address', 'text'],
  ['taxFile', 'text'],
  ['blocked', 'boolean'],
  ['bankAccount', 'text'],
  ['debitLimit', 'decimal'],
  ['modifiedBy', 'text'],
  ['modifiedDate', 'dateTime'],
  ...Array.from({ length: 7 }, (_, i) => [`extra${i + 1}`, 'text']),
];

const schema = JSON.stringify({
  entity: 'vendors',
  schemaVersion: 'vendors.1',
  label: { en: 'Vendors', ar: 'الموردون' },
  keyField: 'vendorAccount',
  idField: 'recId',
  maxPageSize: 1000,
  maxRecords: 10000,
  maxCommitRecords: 500,
  fields: FIELDS.map(([name, type]) => ({
    name,
    label: { en: name, ar: name },
    type,
    editable: !['vendorAccount', 'debitLimit', 'modifiedDate'].includes(name),
    ...(type === 'decimal' ? { scale: 2 } : {}),
    ...(type === 'lookup' ? { lookup: 'codes' } : {}),
  })),
  lookups: { codes: [{ code: 'LOCAL', label: { en: 'Local', ar: 'محلي' } }] },
});

const session = JSON.stringify({
  company: 'Kit',
  userId: 'u17',
  userName: 'Sara',
  language: 'en',
  entity: 'vendors',
  entityRoute: 'api/payable/VendorSheet',
});

function value(type, i, name) {
  switch (type) {
    case 'boolean':
      return i % 11 === 0;
    case 'decimal':
      return `${(i * 37) % 100000}.${String(i % 100).padStart(2, '0')}`;
    case 'dateTime':
      return '2026-10-30T08:12:00+02:00';
    case 'lookup':
      return 'LOCAL';
    default:
      return name === 'vendorAccount'
        ? `V-${String(i).padStart(6, '0')}`
        : i % 3 === 0
          ? `شركة النيل ${i} ${name}`
          : `0${100000000 + i} ${name}`;
  }
}

/** Query pages of `n` records, 1,000 a page, as the ERP sends them. */
function pages(n) {
  const list = [];
  for (let start = 0; start < n; start += 1000) {
    const records = [];
    for (let i = start; i < Math.min(n, start + 1000); i++) {
      const values = Object.fromEntries(FIELDS.map(([name, type]) => [name, value(type, i, name)]));
      records.push({ id: i + 1, key: values.vendorAccount, version: 'AAAAAAAAB9E=', values });
    }
    const last = start + 1000 >= n;
    list.push(
      Buffer.from(
        JSON.stringify({
          records,
          nextAfterId: last ? null : records.at(-1).id,
          total: n,
          asOf: '2026-11-09T10:00:05Z',
        }),
      ).toString('base64'),
    );
  }
  return list;
}

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
          const started = performance.now();
          const steps = await window.cgsTest.bindPages(schema, bytes, session);
          // The grid's first paint of the bound sheet.
          await new Promise((done) => {
            const check = () => {
              const canvas = document.querySelector('[data-testid="sheet-canvas"]');
              if (canvas?.dataset.ready === 'true') requestAnimationFrame(() => done());
              else setTimeout(check, 1);
            };
            check();
          });
          return { ...steps, totalMs: performance.now() - started };
        },
        { schema, encoded, session },
      );
      const memory = await page.evaluate(() => window.cgsTest.memoryBytes());
      console.log(
        `${size} records: ${timing.totalMs.toFixed(0)} ms (stage ${timing.stageMs.toFixed(0)}, bind and paint ${(timing.totalMs - timing.stageMs).toFixed(0)}), ${timing.records} bound`,
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
  server?.kill();
}
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify({ measuredAt: new Date().toISOString(), results }, null, 2));
for (const r of results) {
  console.log(
    `${r.records} × ${r.fields}: median ${r.medianMs} ms (staging ${r.medianStageMs} ms), target ${r.targetMs} ms: ${r.met ? 'met' : 'NOT met'}`,
  );
}
