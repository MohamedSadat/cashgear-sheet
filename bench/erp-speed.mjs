// Phase 19: every row of plan B.6's performance targets for ERP data, in Chrome's WASM, against
// `pnpm preview` (run `pnpm build` first). It runs measure-erp-bind.mjs (binding 1,000 and
// 10,000 records of 20 fields, from the response to the first paint) and measure-erp-edit.mjs
// (the changeset, local rules after a 1,000-row paste, and key to paint and tiles on a bound
// sheet against the same sheet unbound), then measures what the binding keeps in memory, and
// writes one report. The network is left out, as B.6 says: `pnpm erp:tenant e2e` measures it
// against the test tenant. Normal sheets are the existing benches' (input, filter, scroll).
// Timings mean something only on the reference machine.
// Usage: node bench/erp-speed.mjs [--runs=5] [--rounds=3] [--out=file]
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';
import { pages, schema, session } from './erp-pages.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const out = option('out') ?? join(root, 'bench/results/erp-speed.json');
const runs = option('runs') ?? '5';
const rounds = option('rounds') ?? '3';
const origin = option('origin') ?? 'http://127.0.0.1:4173';
const folder = dirname(out);
mkdirSync(folder, { recursive: true });

/** Runs one of the benches as its own process, and reads what it wrote. */
function bench(script, args, file) {
  const result = spawnSync(process.execPath, [join(root, 'bench', script), ...args], {
    cwd: root,
    stdio: 'inherit',
  });
  if (result.status !== 0) throw new Error(`${script} failed.`);
  return JSON.parse(readFileSync(file, 'utf8'));
}

async function serverIsUp() {
  try {
    return (await fetch(origin, { signal: AbortSignal.timeout(1000) })).ok;
  } catch {
    return false;
  }
}

/** What the binding keeps for each editable value, after binding `n` records. */
async function memory(n) {
  const server = (await serverIsUp())
    ? null
    : spawn('pnpm', ['preview'], { cwd: root, stdio: 'ignore', shell: true });
  try {
    for (let tries = 0; !(await serverIsUp()); tries++) {
      if (tries > 60) throw new Error('The preview server did not start.');
      await new Promise((done) => setTimeout(done, 500));
    }
    const browser = await chromium.launch({ channel: 'chrome', headless: true });
    try {
      const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
      await page.goto(origin);
      await page.waitForFunction(
        () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
      );
      return await page.evaluate(
        async ({ schema, encoded, session }) => {
          const bytes = encoded.map((text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0)));
          await window.cgsTest.bindPages(schema, bytes, session);
          return window.cgsTest.erpMemory();
        },
        { schema, encoded: pages(n), session },
      );
    } finally {
      await browser.close();
    }
  } finally {
    if (server)
      spawnSync('taskkill', ['/pid', String(server.pid), '/t', '/f'], { stdio: 'ignore' });
  }
}

const bind = bench(
  'measure-erp-bind.mjs',
  [`--runs=${runs}`, `--out=${join(folder, 'erp-bind.json')}`],
  join(folder, 'erp-bind.json'),
);
const edit = bench(
  'measure-erp-edit.mjs',
  [`--rounds=${rounds}`, `--out=${join(folder, 'erp-edit.json')}`],
  join(folder, 'erp-edit.json'),
);
const memories = [];
for (const n of [1000, 10000]) {
  const found = await memory(n);
  if (!found) throw new Error('No binding to measure.');
  memories.push({
    records: n,
    ...found,
    originalsPerValue: Number((found.originalsBytes / found.editableValues).toFixed(1)),
    bindingPerValue: Number((found.bindingBytes / found.editableValues).toFixed(1)),
  });
}

const c = edit.comparison;
const within = (bound, unbound) => bound <= unbound * 1.1;
const sized = (n) => bind.results.find((r) => r.records === n);
const changeset = (n) => edit.changeset.find((r) => r.records === n);
const rows = [
  {
    measure: 'Bind 1,000 records × 20 fields, response to first paint',
    target: '≤ 300 ms',
    result: `${sized(1000).medianMs} ms`,
    met: sized(1000).medianMs <= 300,
  },
  {
    measure: 'Bind 10,000 records, the cap',
    target: '≤ 1.5 s',
    result: `${sized(10000).medianMs} ms`,
    met: sized(10000).medianMs <= 1500,
  },
  {
    measure: 'changeset, 1,000 × 20 / 10,000 × 20',
    target: '≤ 10 ms / ≤ 100 ms',
    result: `${changeset(1000).oneField.medianMs} ms / ${changeset(10000).oneField.medianMs} ms`,
    met: changeset(1000).met && changeset(10000).met,
  },
  {
    measure: 'Local rules after pasting 1,000 rows',
    target: '≤ 50 ms',
    result: `${c.rulesAfter1000RowPasteMs} ms`,
    met: c.rulesAfter1000RowPasteMs <= 50,
  },
  {
    measure: 'Key to paint on a bound sheet, p99',
    target: 'within 10% of unbound',
    result: `${c.keyP99.bound} ms bound, ${c.keyP99.unbound} ms unbound`,
    met: within(c.keyP99.bound, c.keyP99.unbound),
  },
  {
    measure: 'Tile build with states, p99',
    target: 'within 10% of unbound',
    result: `${c.blockP99.bound} ms bound, ${c.blockP99.unbound} ms unbound (drawn tiles ${c.tileP99.bound} / ${c.tileP99.unbound})`,
    met: within(c.blockP99.bound, c.blockP99.unbound) && within(c.tileP99.bound, c.tileP99.unbound),
  },
  {
    measure: 'Memory for originals',
    target: '≤ 64 bytes an editable value',
    result: memories
      .map(
        (m) =>
          `${m.originalsPerValue} B (${m.bindingPerValue} B with keys and maps) at ${m.records}`,
      )
      .join('; '),
    met: memories.every((m) => m.originalsPerValue <= 64),
  },
];
writeFileSync(
  out,
  JSON.stringify(
    { measuredAt: new Date().toISOString(), rows, bind: bind.results, edit, memory: memories },
    null,
    2,
  ) + '\n',
);
console.log('\nPlan B.6');
for (const row of rows) {
  console.log(
    `${row.met ? 'met    ' : 'NOT met'}  ${row.measure}: ${row.result} (target ${row.target})`,
  );
}
process.exit(rows.every((row) => row.met) ? 0 : 1);
