// Phase 16's editing targets (plan B.6, decisions 2026-11-10): on a bound sheet, key to paint and
// tile draws within 10% of the same sheet unbound (p99); `changeset` at most 10 ms for 1,000
// records of 20 fields and 100 ms for 10,000; local rules at most 50 ms after pasting 1,000 rows,
// measured as how much longer the paste takes bound than unbound. In Chrome's WASM, against
// `pnpm preview`, so build first. The unbound sheet is the bound one saved and opened again: the
// same table, values and formats, with no binding. Timings mean something only on the reference
// machine.
// Usage: node bench/measure-erp-edit.mjs [--out=file] [--rounds=3]
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { chromium } from '@playwright/test';
import { root } from '../scripts/lib.mjs';

const option = (name) =>
  process.argv.find((arg) => arg.startsWith('--' + name + '='))?.slice(name.length + 3);
const out = option('out') ?? join(root, 'bench/results/erp-edit.json');
const origin = option('origin') ?? 'http://127.0.0.1:4173';
const ROUNDS = Number(option('rounds') ?? 3);

/** The vendors' 13 fields and 7 more text fields, 20 in all, as B.6 measures; 17 editable. */
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
const READ_ONLY = ['vendorAccount', 'debitLimit', 'modifiedDate'];

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
    editable: !READ_ONLY.includes(name),
    ...(READ_ONLY.includes(name) ? { editReason: 'notEditable' } : {}),
    ...(type === 'text' ? { maxLength: 150 } : {}),
    ...(['vendorName', 'vendorGroup', 'payMethod'].includes(name) ? { required: true } : {}),
    ...(type === 'decimal' ? { scale: 2, min: '0' } : {}),
    ...(type === 'lookup' ? { lookup: 'codes' } : {}),
  })),
  lookups: {
    codes: [
      { code: 'LOCAL', label: { en: 'Local', ar: 'محلي' } },
      { code: 'IMPORT', label: { en: 'Import', ar: 'استيراد' } },
    ],
  },
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

/** Query pages of `n` records, 1,000 a page, as the ERP sends them, in base64. */
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

/** Tab-separated rows for columns `from` to `to` (1-based), as another program's copy gives them. */
function pasteText(rows, from, to, tag) {
  const lines = [];
  for (let r = 0; r < rows; r++) {
    const cells = [];
    for (let c = from; c <= to; c++) {
      const [name, type] = FIELDS[c - 1];
      cells.push(
        type === 'lookup'
          ? r % 2
            ? 'IMPORT'
            : 'local'
          : type === 'boolean'
            ? r % 5
              ? 'FALSE'
              : 'TRUE'
            : `00${r} ${tag} ${name}`,
      );
    }
    lines.push(cells.join('\t'));
  }
  return lines.join('\r\n') + '\r\n';
}

async function serverIsUp() {
  try {
    return (await fetch(origin, { signal: AbortSignal.timeout(1000) })).ok;
  } catch {
    return false;
  }
}

const summary = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  const at = (share) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * share))];
  return {
    count: sorted.length,
    p50Ms: Number((at(0.5) ?? 0).toFixed(2)),
    p95Ms: Number((at(0.95) ?? 0).toFixed(2)),
    p99Ms: Number((at(0.99) ?? 0).toFixed(2)),
    maxMs: Number((sorted.at(-1) ?? 0).toFixed(2)),
  };
};
const median = (list) => [...list].sort((a, b) => a - b)[Math.floor(list.length / 2)];

const server = (await serverIsUp())
  ? null
  : spawn('pnpm', ['preview'], { cwd: root, stdio: 'ignore', shell: true });
for (let tries = 0; !(await serverIsUp()); tries++) {
  if (tries > 60) throw new Error('The preview server did not start.');
  await new Promise((resolve) => setTimeout(resolve, 500));
}
// Headed, as measure-input.mjs is: a headless page's frames don't say what a person sees.
const browser = await chromium.launch({ channel: 'chrome', headless: false });

async function newPage() {
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const page = await context.newPage();
  page.on('pageerror', (error) => {
    console.error('Page error:', error.stack ?? error.message);
    process.exit(1);
  });
  await page.goto(origin);
  await page.waitForFunction(
    () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
  );
  return { context, page };
}

const ready = (page) =>
  page.waitForFunction(
    () => document.querySelector('[data-testid="sheet-canvas"]')?.dataset.ready === 'true',
    undefined,
    { timeout: 120000 },
  );

/** A bound sheet of `n` records, as a launch makes it. */
async function boundPage(n) {
  const { context, page } = await newPage();
  const encoded = pages(n);
  const records = await page.evaluate(
    async ({ schema, encoded, session }) => {
      const bytes = encoded.map((text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0)));
      return (await window.cgsTest.bindPages(schema, bytes, session)).records;
    },
    { schema, encoded, session },
  );
  if (records !== n) throw new Error(`Bound ${records} of ${n} records.`);
  await ready(page);
  return { context, page };
}

/** A page's workbook as saved, in base64. */
function savedBytes(page) {
  return page.evaluate(async () => {
    const bytes = await window.cgsTest.saveBytes();
    let text = '';
    for (let at = 0; at < bytes.length; at += 0x8000)
      text += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
    return btoa(text);
  });
}

/** The same sheet with no binding: the bound one's saved bytes, opened as any workbook. */
async function unboundCopy(base64) {
  const { context, page } = await newPage();
  await page.evaluate(async (base64) => {
    const binary = atob(base64);
    const data = new Uint8Array(binary.length);
    for (let at = 0; at < binary.length; at++) data[at] = binary.charCodeAt(at);
    await window.cgsTest.open({
      name: 'Vendors.xlsx',
      source: { kind: 'blob', blob: new File([data], 'Vendors.xlsx') },
    });
  }, base64);
  await page.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', undefined, {
    timeout: 120000,
  });
  if (await page.evaluate(() => window.cgsTest.binding())) throw new Error('The copy is bound.');
  await ready(page);
  return { context, page };
}

/** Milliseconds a command takes, from the call to its settling. */
async function commandMs(page, command) {
  return page.evaluate(async (command) => {
    await window.cgsTest.settled();
    const started = performance.now();
    await window.cgsTest.command(command);
    await window.cgsTest.settled();
    return performance.now() - started;
  }, command);
}

async function goTo(page, name) {
  await page.getByTestId('name-box').click();
  await page.keyboard.type(name);
  await page.keyboard.press('Enter');
  await page.waitForFunction(
    (name) => document.querySelector('[data-testid="name-box"]').value === name,
    name,
  );
}

/** Keys typed into Name and Address, each from when the browser got it to the frame showing it. */
async function keysToPaint(page, rows) {
  await page.evaluate(() => {
    window.keyDelays = [];
    document.addEventListener(
      'keydown',
      (event) => {
        const pressed = event.timeStamp;
        requestAnimationFrame(() => {
          const channel = new MessageChannel();
          channel.port1.onmessage = () => window.keyDelays.push(performance.now() - pressed);
          channel.port2.postMessage(0);
        });
      },
      { capture: true },
    );
  });
  for (let i = 0; i < 40; i++) {
    await goTo(page, (i % 2 ? 'G' : 'B') + (2 + ((i * 97) % rows)));
    await page.keyboard.type('0' + i);
    await page.keyboard.press('Enter');
    await page.evaluate(() => window.cgsTest.settled());
    await page.waitForTimeout(40);
  }
  await page.waitForTimeout(200);
  return page.evaluate(() => window.keyDelays.splice(0));
}

/** Each tile of the table drawn once, from the engine's bytes to the tile worker's bitmap. */
async function tileDraws(page, rows) {
  return page.evaluate(async (rows) => {
    const times = { tile: [], block: [] };
    for (let row = 1; row <= rows + 1; row += 64) {
      for (const col of [1, 17]) {
        times.block.push(await window.cgsTest.probe('block', row, col));
        times.tile.push(await window.cgsTest.probe('tile', row, col));
      }
    }
    return times;
  }, rows);
}

const results = { chrome: browser.version(), rounds: ROUNDS };
try {
  // Bound against unbound, on the cap's 10,000 records, after the same 1,000-row paste.
  const N = 10000;
  const rounds = [];
  const paste = {
    kind: 'pasteExternal',
    sheet: 0,
    row: 2,
    col: 2,
    text: pasteText(1000, 2, 10, 'p'),
  };
  /** The same paste, tile draws and typing on one sheet, with only its window open. */
  const measure = async ({ context, page }) => {
    const pasteMs = await commandMs(page, paste);
    const binding = await page.evaluate(() => window.cgsTest.binding());
    const tiles = await tileDraws(page, N);
    const keys = await keysToPaint(page, N);
    await context.close();
    return { pasteMs, changed: binding?.counts.changed ?? null, tiles, keys };
  };
  let base64 = null;
  for (let round = 0; round < ROUNDS; round++) {
    // Alternate which goes first, so drift falls on both.
    const sides = {};
    for (const side of round % 2 ? ['unbound', 'bound'] : ['bound', 'unbound']) {
      if (side === 'bound') {
        const opened = await boundPage(N);
        base64 ??= await savedBytes(opened.page);
        sides.bound = await measure(opened);
      } else {
        if (!base64) {
          const opened = await boundPage(N);
          base64 = await savedBytes(opened.page);
          await opened.context.close();
        }
        sides.unbound = await measure(await unboundCopy(base64));
      }
    }
    const entry = {
      pasteBoundMs: Math.round(sides.bound.pasteMs),
      pasteUnboundMs: Math.round(sides.unbound.pasteMs),
      rulesMs: Math.round(sides.bound.pasteMs - sides.unbound.pasteMs),
      changedAfterPaste: sides.bound.changed,
      keys: { bound: summary(sides.bound.keys), unbound: summary(sides.unbound.keys) },
      tiles: { bound: summary(sides.bound.tiles.tile), unbound: summary(sides.unbound.tiles.tile) },
      blocks: {
        bound: summary(sides.bound.tiles.block),
        unbound: summary(sides.unbound.tiles.block),
      },
    };
    console.log(JSON.stringify(entry));
    rounds.push(entry);
  }
  const ratio = (pick) => median(rounds.map(pick));
  results.comparison = {
    records: N,
    fields: FIELDS.length,
    rounds,
    keyP99: {
      bound: ratio((r) => r.keys.bound.p99Ms),
      unbound: ratio((r) => r.keys.unbound.p99Ms),
    },
    tileP99: {
      bound: ratio((r) => r.tiles.bound.p99Ms),
      unbound: ratio((r) => r.tiles.unbound.p99Ms),
    },
    blockP99: {
      bound: ratio((r) => r.blocks.bound.p99Ms),
      unbound: ratio((r) => r.blocks.unbound.p99Ms),
    },
    rulesAfter1000RowPasteMs: ratio((r) => r.rulesMs),
  };

  // The changeset: one changed field in every record, and every editable field of 1,000 records.
  results.changeset = [];
  for (const n of [1000, 10000]) {
    const { context, page } = await boundPage(n);
    const column = {
      kind: 'pasteExternal',
      sheet: 0,
      row: 2,
      col: 2,
      text: pasteText(n, 2, 2, 'c'),
    };
    await commandMs(page, column);
    const time = () =>
      page.evaluate(async () => {
        const started = performance.now();
        const set = await window.cgsTest.erpChanges();
        return { ms: performance.now() - started, records: set.records.length };
      });
    const runs = [];
    for (let run = 0; run < 7; run++) runs.push(await time());
    const oneField = {
      records: runs[0].records,
      medianMs: Number(median(runs.map((r) => r.ms)).toFixed(1)),
    };
    let everyField = null;
    if (n === 1000) {
      await commandMs(page, {
        kind: 'pasteExternal',
        sheet: 0,
        row: 2,
        col: 2,
        text: pasteText(n, 2, 10, 'a'),
      });
      await commandMs(page, {
        kind: 'pasteExternal',
        sheet: 0,
        row: 2,
        col: 12,
        text: pasteText(n, 12, 12, 'a'),
      });
      await commandMs(page, {
        kind: 'pasteExternal',
        sheet: 0,
        row: 2,
        col: 14,
        text: pasteText(n, 14, 20, 'a'),
      });
      const all = [];
      for (let run = 0; run < 7; run++) all.push(await time());
      everyField = {
        records: all[0].records,
        medianMs: Number(median(all.map((r) => r.ms)).toFixed(1)),
      };
    }
    const target = n === 1000 ? 10 : 100;
    const entry = {
      records: n,
      fields: FIELDS.length,
      oneField,
      everyField,
      targetMs: target,
      met: oneField.medianMs <= target,
    };
    console.log(JSON.stringify(entry));
    results.changeset.push(entry);
    await context.close();
  }
} finally {
  await browser.close();
  // The server is a shell's grandchild, which killing the shell would leave running.
  if (server) spawnSync('taskkill', ['/pid', String(server.pid), '/t', '/f'], { stdio: 'ignore' });
}
const c = results.comparison;
const within = (bound, unbound) => bound <= unbound * 1.1;
results.met = {
  keys: within(c.keyP99.bound, c.keyP99.unbound),
  tiles: within(c.tileP99.bound, c.tileP99.unbound),
  rules: c.rulesAfter1000RowPasteMs <= 50,
  changeset: results.changeset.every((entry) => entry.met),
};
mkdirSync(dirname(out), { recursive: true });
writeFileSync(
  out,
  JSON.stringify({ measuredAt: new Date().toISOString(), ...results }, null, 2) + '\n',
);
console.log(
  `keys p99 ${c.keyP99.bound} ms bound, ${c.keyP99.unbound} unbound; tiles p99 ${c.tileP99.bound} / ${c.tileP99.unbound}; blocks p99 ${c.blockP99.bound} / ${c.blockP99.unbound}; rules after a 1,000-row paste ${c.rulesAfter1000RowPasteMs} ms`,
);
for (const entry of results.changeset) {
  console.log(
    `changeset ${entry.records} × ${entry.fields}: ${entry.oneField.medianMs} ms (target ${entry.targetMs})${entry.everyField ? `, every field ${entry.everyField.medianMs} ms` : ''}`,
  );
}
console.log(JSON.stringify(results.met));
process.exit(0);
