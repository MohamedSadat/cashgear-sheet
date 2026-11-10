// ERP data in the browser, against the mock ERP on its own origin (playwright.config.ts starts it
// on 127.0.0.1:4180, the sheet's preview on :4173). The page opens a launch link as Cash Gear ERP's
// button would, so the handoff, CORS, cross-origin isolation and the identity guard run as they
// will in production. Phase 15's read-only slice comes first, then Phase 16's editing.
import { expect, test, type Page } from '@playwright/test';
import { readZip } from '../fixtures/workbooks.mjs';
import { expectGolden, FONT_OVERRIDE, waitForGrid } from './helpers.mjs';

const MOCK = 'http://127.0.0.1:4180';
const SHEET_ORIGIN = 'http://127.0.0.1:4173';
const ENGLISH = [
  'Account',
  'Name',
  'Group',
  'Mobile',
  'Currency',
  'Payment method',
  'Address',
  'Tax file',
  'Blocked',
  'Bank account',
  'Credit limit',
  'Modified by',
  'Modified',
];

type Wire = string | number | boolean | null;
interface Snapshot {
  id: number;
  key: string;
  version: string;
  values: Record<string, Wire>;
}

async function mock(route: string, body?: unknown) {
  const response = await fetch(`${MOCK}/__mock/${route}`, {
    method: body === undefined && route.startsWith('state') ? 'GET' : 'POST',
    headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  expect(response.status).toBeLessThan(300);
}

async function ticket(
  filter: Record<string, Wire> | null,
  language: 'en' | 'ar' = 'en',
): Promise<{ ticket: string; url: string }> {
  const response = await fetch(`${MOCK}/api/system/SheetLaunch/CreateTicket`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-API-Key': 'mock-kit-editor' },
    body: JSON.stringify({ entity: 'vendors', filter, language }),
  });
  expect(response.status).toBe(200);
  return (await response.json()) as { ticket: string; url: string };
}

/** The records a filter matches, read through the contract as the sheet reads them. */
async function records(filter: Record<string, Wire> | null): Promise<Snapshot[]> {
  const { ticket: value } = await ticket(filter);
  const session = (await (
    await fetch(`${MOCK}/api/sheet/Session/Redeem`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Origin: SHEET_ORIGIN },
      body: JSON.stringify({ ticket: value }),
    })
  ).json()) as { token: string; entityRoute: string };
  const out: Snapshot[] = [];
  let afterId = 0;
  for (;;) {
    const page = (await (
      await fetch(`${MOCK}/${session.entityRoute}/Query`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Origin: SHEET_ORIGIN,
          'X-Sheet-Session': session.token,
        },
        body: JSON.stringify({ filter, ids: null, afterId, pageSize: 1000 }),
      })
    ).json()) as { records: Snapshot[]; nextAfterId: number | null };
    out.push(...page.records);
    if (page.nextAfterId === null) return out;
    afterId = page.nextAfterId;
  }
}

const FIELDS = [
  'vendorAccount',
  'vendorName',
  'vendorGroup',
  'mobile',
  'currency',
  'payMethod',
  'address',
  'taxFile',
  'blocked',
  'bankAccount',
  'debitLimit',
  'modifiedBy',
  'modifiedDate',
];

/** What a cell shows for a wire value, by the column's number format. */
function shown(field: string, value: Wire): string {
  if (value === null) return '';
  if (typeof value === 'boolean') return value ? 'TRUE' : 'FALSE';
  if (field === 'debitLimit') {
    const [whole, fraction = ''] = String(value).split('.');
    return `${Number(whole).toLocaleString('en-US')}.${fraction.padEnd(2, '0')}`;
  }
  // The wall-clock time in the ERP's offset, as yyyy-mm-dd hh:mm.
  if (field === 'modifiedDate') return String(value).slice(0, 16).replace('T', ' ');
  return String(value);
}

/** Strips the isolates the badge puts around names. */
const plain = (text: string | null) => (text ?? '').replace(/[⁦-⁩]/g, '');

async function launch(page: Page, url: string) {
  page.on('pageerror', (error) => {
    throw error;
  });
  await page.addInitScript(FONT_OVERRIDE);
  await page.goto(url);
}

async function bound(page: Page, badge: string) {
  await expect
    .poll(async () => plain(await page.getByTestId('erp-status').textContent()), {
      timeout: 60_000,
    })
    .toBe(badge);
  await waitForGrid(page);
}

/** The table's rows as they show, keyed by their first column. */
async function rows(page: Page) {
  const binding = (await page.evaluate(() => window.cgsTest.binding()))!;
  const text = await page.evaluate((area) => window.cgsTest.shownText(area), {
    top: binding.top,
    left: binding.left,
    bottom: binding.bottom,
    right: binding.right,
  });
  const lines = text
    .replace(/\r?\n$/, '')
    .split(/\r?\n/)
    .map((line) => line.split('\t'));
  return { headings: lines[0]!, data: lines.slice(1), binding };
}

/** Every row holds exactly its own record, each key once, and every record has a row. */
async function expectRecords(page: Page, expected: Snapshot[]) {
  const { data } = await rows(page);
  const byKey = new Map(expected.map((r) => [r.key, r]));
  expect(data.length).toBe(expected.length);
  const seen = new Set<string>();
  for (const row of data) {
    const record = byKey.get(row[0]!);
    expect(record, `row ${row[0]}`).toBeDefined();
    expect(seen.has(row[0]!)).toBe(false);
    seen.add(row[0]!);
    expect(row).toEqual(FIELDS.map((field) => shown(field, record!.values[field] ?? null)));
  }
}

test.beforeEach(async () => {
  await mock('reset');
});

test('opens vendors from a launch, leaving no trace of its ticket, sorts, filters and refreshes them', async ({
  page,
}) => {
  const filter = { blocked: false };
  const expected = await records(filter);
  const { url, ticket: value } = await ticket(filter);
  await launch(page, url);
  await bound(page, `Vendors · ${expected.length.toLocaleString('en-US')} · Sara @ Kit`);

  // The fragment is gone from the address bar and from history.
  expect(page.url()).toBe(`${SHEET_ORIGIN}/`);
  expect(await page.evaluate(() => location.hash)).toBe('');
  await expect(page).toHaveTitle('Vendors – Kit – CashGear Sheet');
  // History holds the blank page the browser started on and the app's own page: the launch
  // entry was replaced, so no step back reaches the ticket.
  expect(await page.evaluate(() => [history.length, location.href])).toEqual([
    2,
    `${SHEET_ORIGIN}/`,
  ]);
  expect(page.url()).not.toContain(value);
});

test('opens every active vendor in English, and keeps each key with its record through sorts, filters and a refresh', async ({
  page,
}) => {
  const filter = { blocked: false };
  const expected = await records(filter);
  await launch(page, (await ticket(filter)).url);
  const badge = `Vendors · ${expected.length.toLocaleString('en-US')} · Sara @ Kit`;
  await bound(page, badge);
  await expect(page).toHaveTitle('Vendors – Kit – CashGear Sheet');
  const { headings, binding } = await rows(page);
  expect(headings).toEqual(ENGLISH);
  expect(binding).toMatchObject({ table: 'Vendors', sheet: 0, top: 1, left: 1, right: 13 });
  await expectRecords(page, expected);

  // Sort Z to A on the Name column from the Data tab.
  await page.evaluate(() =>
    window.cgsTest.grid()!.selectArea({ top: 4, left: 1, bottom: 4, right: 1 }),
  );
  await page.getByTestId('ribbon').getByRole('tab', { name: 'Data', exact: true }).click();
  await page
    .getByTestId('ribbon')
    .getByRole('button', { name: 'Sort Z to A', exact: true })
    .click();
  await page.evaluate(() => window.cgsTest.settled());
  const sorted = await rows(page);
  // Moved from id order, the Z names first.
  expect(sorted.data.map((row) => row[0])).not.toEqual(expected.map((r) => r.key));
  expect(sorted.data.findIndex((row) => row[1]!.startsWith('Z'))).toBeLessThan(
    sorted.data.findIndex((row) => row[1]!.startsWith('A')),
  );
  await expectRecords(page, expected);

  // A filter on the group hides rows; the rest still hold their own records.
  await page.evaluate(() =>
    window.cgsTest.command({
      kind: 'setFilter',
      sheet: 0,
      table: 'Vendors',
      col: 3,
      criteria: { kind: 'values', values: ['IMPORT'] },
    }),
  );
  await page.evaluate(() => window.cgsTest.settled());
  await expectRecords(page, expected);
  // The rows that show are IMPORT's, and only they show.
  const { data: filtered } = await rows(page);
  const heights = await page.evaluate(
    (count) =>
      Array.from({ length: count }, (_, i) => window.cgsTest.grid()!.boxForCell(i + 1, 0).height),
    filtered.length,
  );
  expect(heights.map((height) => height > 0)).toEqual(filtered.map((row) => row[2] === 'IMPORT'));
  expect(heights.some((height) => height > 0)).toBe(true);
  await page.evaluate(() =>
    window.cgsTest.command({ kind: 'clearFilter', sheet: 0, table: 'Vendors' }),
  );
  await page.evaluate(() => window.cgsTest.settled());
  await expectRecords(page, expected);

  // Someone changes a vendor in the ERP; Refresh finds it by id wherever the sort put it.
  await mock('edit', {
    company: 'Kit',
    key: 'V-1001',
    values: { vendorName: 'Al Noor Trading Co' },
  });
  await page.getByTestId('ribbon').getByRole('tab', { name: 'ERP Data', exact: true }).click();
  await page.getByTestId('ribbon').getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(page.getByTestId('engine-status')).toContainText(
    `Refreshed ${expected.length.toLocaleString('en-US')} vendors · 1 changed`,
  );
  const after = await records(filter);
  await expectRecords(page, after);
  expect(after.find((r) => r.key === 'V-1001')!.values.vendorName).toBe('Al Noor Trading Co');

  // Connection Info says where the data came from.
  await page
    .getByTestId('ribbon')
    .getByRole('button', { name: 'Connection Info', exact: true })
    .click();
  await expect(page.getByTestId('erp-info-company')).toHaveText('Kit');
  await expect(page.getByTestId('erp-info-user')).toHaveText('Sara (u17)');
  await expect(page.getByTestId('erp-info-server')).toHaveText(MOCK);
  await page.getByTestId('erp-info-close').click();
});

test('opens in Arabic, right to left, with the ERP user’s headings', async ({ page }) => {
  const filter = { blocked: false, search: 'noor' };
  const expected = await records(filter);
  expect(expected.map((r) => r.key)).toEqual(['V-1001', 'V-1006']);
  await launch(page, (await ticket(filter, 'ar')).url);
  await bound(page, 'Vendors · 2 · Sara @ Kit');
  const { headings } = await rows(page);
  expect(headings.slice(0, 4)).toEqual(['الحساب', 'الاسم', 'المجموعة', 'الجوال']);
  expect(await page.evaluate(() => window.cgsTest.grid()!.rightToLeft)).toBe(true);
  await expect(page.getByTestId('sheet-tabs')).toContainText('الموردون');
  await expectRecords(page, expected);
});

test('refuses edits to ERP data with its own message', async ({ page }) => {
  await launch(page, (await ticket({ search: 'noor' })).url);
  await bound(page, 'Vendors · 2 · Sara @ Kit');
  const message = page.getByTestId('protect-message-text');

  // Typing into a read-only cell, the key here, is refused at the first key.
  await page.evaluate(() =>
    window.cgsTest.grid()!.selectArea({ top: 1, left: 0, bottom: 1, right: 0 }),
  );
  await page.getByTestId('sheet-canvas').focus();
  await page.keyboard.press('x');
  await expect(message).toHaveText('This column is read-only in Cash Gear ERP.');
  await page.getByRole('button', { name: 'OK' }).click();

  // Delete clears through the engine, which refuses it.
  await page.getByTestId('sheet-canvas').focus();
  await page.keyboard.press('Delete');
  await expect(message).toHaveText('This column is read-only in Cash Gear ERP.');
  await page.getByRole('button', { name: 'OK' }).click();

  // Commands the guard refuses, each with its message; nothing changes.
  for (const [command, text] of [
    [
      { kind: 'axis', sheet: 0, axis: 'row', at: 3, count: 1, delete: true },
      "Rows can't be added or deleted in ERP data.",
    ],
    [
      { kind: 'convertToRange', table: 'Vendors' },
      "The table that holds ERP data can't be resized, renamed or reshaped.",
    ],
    [
      {
        kind: 'sort',
        sheet: 0,
        area: { top: 2, left: 1, bottom: 3, right: 13 },
        keys: [{ index: 2 }],
        byColumns: true,
      },
      'ERP data can only be sorted top to bottom.',
    ],
  ] as const) {
    const refused = await page.evaluate(
      (c) =>
        window.cgsTest.command(c).then(
          () => 'let through',
          (error: Error) => error.message,
        ),
      command,
    );
    expect(refused).toBe(text);
  }
  await expectRecords(page, await records({ search: 'noor' }));
});

test('refuses a replayed or expired launch link', async ({ page }) => {
  // Replayed: the ticket was redeemed before the sheet got it.
  const replayed = await ticket(null);
  await fetch(`${MOCK}/api/sheet/Session/Redeem`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Origin: SHEET_ORIGIN },
    body: JSON.stringify({ ticket: replayed.ticket }),
  });
  await launch(page, replayed.url);
  const text = page.getByTestId('erp-load-text');
  const refused =
    'This link has expired or was already used. Go back to Cash Gear ERP and choose Edit in Cash Gear Sheet again.';
  await expect(text).toHaveText(refused, { timeout: 60_000 });
  await expect(page.getByTestId('erp-load-retry')).toHaveCount(0);
  await page.getByTestId('erp-load-close').click();
  await expect(page.getByTestId('erp-status')).toHaveCount(0);
  expect(page.url()).toBe(`${SHEET_ORIGIN}/`);

  // Expired: two minutes passed on the ERP's clock.
  const expired = await ticket(null);
  await mock('clock', { advanceMs: 121_000 });
  await page.goto(expired.url);
  await expect(page.getByTestId('erp-load-text')).toHaveText(refused, { timeout: 60_000 });
});

test('Save As writes an ordinary workbook with no ids, versions or connection', async ({
  page,
}) => {
  const expected = await records({ search: 'noor' });
  await launch(page, (await ticket({ search: 'noor' })).url);
  await bound(page, 'Vendors · 2 · Sara @ Kit');
  const bytes = Buffer.from(
    await page.evaluate(async () => [...(await window.cgsTest.saveBytes())]),
  );
  const parts = readZip(bytes);
  expect(parts.some(([name]) => /customxml/i.test(name))).toBe(false);
  const all = parts.map(([, body]) => body.toString('utf8')).join('\n');
  expect(all).toContain('V-1006');
  for (const record of expected) {
    expect(all).not.toContain(record.version);
    expect(all).not.toMatch(new RegExp(`[>"]${record.id}[<"]`));
  }
  for (const secret of ['recId', 'api/payable', 'X-Sheet-Session', 'u17', '127.0.0.1:4180']) {
    expect(all).not.toContain(secret);
  }
});

// ---- Phase 16: editing and change tracking ----

/** What another program's copy looks like: a paste event with its own data. */
async function pasteFromElsewhere(page: Page, text: string) {
  await page.evaluate((text) => {
    const data = new DataTransfer();
    data.setData('text/plain', text);
    const target = document.querySelector('[data-testid="sheet-canvas"]')!;
    target.dispatchEvent(
      new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }),
    );
  }, text);
}

/** Selects a range by 0-based rows and columns, and gives the grid the keyboard. */
async function select(page: Page, top: number, left: number, bottom = top, right = left) {
  await page.evaluate((area) => window.cgsTest.grid()!.selectArea(area), {
    top,
    left,
    bottom,
    right,
  });
  await page.getByTestId('sheet-canvas').focus();
}

const cellText = (page: Page, row: number, col: number) =>
  page.evaluate(([r, c]) => window.cgsTest.cellText(r!, c!), [row, col]);

const counts = async (page: Page) => (await page.evaluate(() => window.cgsTest.binding()))!.counts;

/** The canvas's colour at a cell's leading edge, where Modified draws its bar. 0-based. */
async function leadingEdge(page: Page, row: number, col: number) {
  return page.evaluate(
    ([r, c]) => {
      const canvas = document.querySelector<HTMLCanvasElement>('[data-testid="sheet-canvas"]')!;
      const box = window.cgsTest.grid()!.cellBox(r!, c!);
      const rect = canvas.getBoundingClientRect();
      const ratio = canvas.width / rect.width;
      const x = Math.round((box.left - rect.left + 1) * ratio);
      const y = Math.round((box.top - rect.top + box.height / 2) * ratio);
      const [red, green, blue] = canvas.getContext('2d')!.getImageData(x, y, 1, 1).data;
      return '#' + [red, green, blue].map((v) => v!.toString(16).padStart(2, '0')).join('');
    },
    [row, col],
  );
}

test.describe('editing ERP data', () => {
  test('types, pastes and fills down in editable columns, keeping text as typed', async ({
    page,
  }) => {
    await launch(page, (await ticket({ search: 'noor' })).url);
    await bound(page, 'Vendors · 2 · Sara @ Kit');

    // Typing a mobile keeps its leading zero.
    await select(page, 1, 3);
    await page.keyboard.type('0100555');
    await page.keyboard.press('Enter');
    await expect.poll(() => cellText(page, 2, 4)).toBe('0100555');
    await expect
      .poll(async () => plain(await page.getByTestId('erp-status').textContent()))
      .toBe('Vendors · 2 · 1 changed · Sara @ Kit');

    // Text from another program keeps its zeros too.
    await select(page, 1, 6);
    await pasteFromElsewhere(page, '0012\t001\r\n');
    await expect.poll(() => cellText(page, 2, 7)).toBe('0012');
    await expect.poll(() => cellText(page, 2, 8)).toBe('001');

    // Fill down, Ctrl+D, from the first name to the second.
    await select(page, 1, 1, 2, 1);
    await page.keyboard.press('Control+d');
    const first = await cellText(page, 2, 2);
    await expect.poll(() => cellText(page, 3, 2)).toBe(first);

    // A lookup typed in lower case takes its list's spelling.
    await select(page, 2, 2);
    await page.keyboard.type('services');
    await page.keyboard.press('Enter');
    await expect.poll(() => cellText(page, 3, 3)).toBe('SERVICES');
    const changed = await counts(page);
    expect(changed.changed).toBeGreaterThanOrEqual(4);
    expect(changed.invalid).toBe(0);

    // The changeset holds each changed field with what the ERP sent.
    const set = await page.evaluate(() => window.cgsTest.erpChanges());
    const v1001 = set!.records.find((r) => r.key === 'V-1001')!;
    expect(v1001.changes['mobile']).toEqual({ from: '01001234567', to: '0100555' });
    expect(v1001.changes['taxFile']!.to).toBe('001');
  });

  test('refuses a paste into a read-only column with its message', async ({ page }) => {
    await launch(page, (await ticket({ search: 'noor' })).url);
    await bound(page, 'Vendors · 2 · Sara @ Kit');
    const message = page.getByTestId('protect-message-text');
    // From another program, into the key.
    await select(page, 2, 0);
    await pasteFromElsewhere(page, 'V-9999\r\n');
    await expect(message).toHaveText('This column is read-only in Cash Gear ERP.');
    await page.getByRole('button', { name: 'OK' }).click();
    // Copied in the sheet, from Name into Currency.
    await select(page, 1, 1);
    await page.keyboard.press('Control+c');
    await select(page, 1, 4);
    await page.keyboard.press('Control+v');
    await expect(message).toHaveText('This column is read-only in Cash Gear ERP.');
    await page.getByRole('button', { name: 'OK' }).click();
    await expectRecords(page, await records({ search: 'noor' }));
    expect((await counts(page)).changed).toBe(0);
  });

  test('undo back to the original clears Modified, and Discard Changes puts originals back', async ({
    page,
  }) => {
    await launch(page, (await ticket({ search: 'noor' })).url);
    await bound(page, 'Vendors · 2 · Sara @ Kit');
    const before = await leadingEdge(page, 1, 1);
    await select(page, 1, 1);
    await page.keyboard.type('Renamed');
    await page.keyboard.press('Enter');
    await expect.poll(() => leadingEdge(page, 1, 1)).toBe('#d97706');
    expect((await counts(page)).changed).toBe(1);
    await select(page, 5, 5);
    await page.keyboard.press('Control+z');
    await expect.poll(() => leadingEdge(page, 1, 1)).toBe(before);
    expect((await counts(page)).changed).toBe(0);
    // Discard Changes from the ERP Data tab writes the originals back, and undo returns them.
    await select(page, 1, 1);
    await page.keyboard.type('Again');
    await page.keyboard.press('Enter');
    await page.getByRole('tab', { name: 'ERP Data' }).click();
    await page.getByRole('button', { name: 'Discard Changes' }).click();
    await page.getByText('Discard All Changes').click();
    await expect.poll(async () => (await counts(page)).changed).toBe(0);
    await expectRecords(page, await records({ search: 'noor' }));
    await select(page, 5, 5);
    await page.keyboard.press('Control+z');
    await expect.poll(async () => (await counts(page)).changed).toBe(1);
  });

  for (const [name, theme, language] of [
    ['light', 'light', 'en'],
    ['dark', 'dark', 'en'],
    ['rtl', 'light', 'ar'],
  ] as const) {
    test(`draws cell states, in ${name}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await launch(page, (await ticket({ search: 'noor' }, language)).url);
      await bound(page, 'Vendors · 2 · Sara @ Kit');
      // Modified, invalid (a required name emptied, and an unknown group) and a formula.
      for (const command of [
        { kind: 'input', sheet: 0, row: 2, col: 2, text: 'Al Noor Trading Co' },
        { kind: 'input', sheet: 0, row: 2, col: 4, text: '0100' },
        { kind: 'clear', sheet: 0, areas: [{ top: 3, left: 2, bottom: 3, right: 2 }] },
        { kind: 'input', sheet: 0, row: 3, col: 3, text: 'NOPE' },
        { kind: 'input', sheet: 0, row: 3, col: 7, text: '=B2' },
      ] as const) {
        await page.evaluate((c) => window.cgsTest.command(c), command);
      }
      await expect
        .poll(async () => plain(await page.getByTestId('erp-status').textContent()))
        .toBe('Vendors · 2 · 4 changed · 2 invalid · 1 formula · Sara @ Kit');
      // The selection away from the states, so that it covers none of them.
      await select(page, 9, 9);
      await waitForGrid(page, theme);
      await expectGolden(page, `erp-states-${name}`, 0.002, { fromRight: language === 'ar' });
    });
  }
});
