// ERP data in the browser, against the mock ERP on its own origin (playwright.config.ts starts it
// on 127.0.0.1:4180, the sheet's preview on :4173). The page opens a launch link as Cash Gear ERP's
// button would, so the handoff, CORS, cross-origin isolation and the identity guard run as they
// will in production. Phase 15's read-only slice comes first, then Phase 16's editing, Phase 17's
// commits, and Phase 18's merge, Resolve, Reconnect, CommitStatus, recovery and Arabic.
import { expect, test, type BrowserContext, type Page } from '@playwright/test';
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
  await expect(page.getByTestId('engine-status')).toHaveText(
    `Refreshed ${expected.length.toLocaleString('en-US')} vendors. 1 changed in Cash Gear ERP.`,
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
  // The badge in the ERP user's language (Phase 18).
  await bound(page, 'الموردون · 2 · Sara @ Kit');
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
      await bound(
        page,
        language === 'ar' ? 'الموردون · 2 · Sara @ Kit' : 'Vendors · 2 · Sara @ Kit',
      );
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
        .toBe(
          language === 'ar'
            ? 'الموردون · 2 · معدّلة: 4 · غير صالحة: 2 · صيغ: 1 · Sara @ Kit'
            : 'Vendors · 2 · 4 changed · 2 invalid · 1 formula · Sara @ Kit',
        );
      // The selection away from the states, so that it covers none of them.
      await select(page, 9, 9);
      await waitForGrid(page, theme);
      await expectGolden(page, `erp-states-${name}`, 0.002, { fromRight: language === 'ar' });
    });
  }
});

// Phase 17: Validate, Review and Commit, against the mock's store as the oracle.

interface MockState {
  writes: number;
  commits: number;
  audit: number;
}

async function mockState(): Promise<MockState> {
  const response = await fetch(`${MOCK}/__mock/state?company=Kit`);
  const state = (await response.json()) as MockState;
  return { writes: state.writes, commits: state.commits, audit: state.audit };
}

async function mockVendor(key: string): Promise<Record<string, unknown>> {
  const response = await fetch(`${MOCK}/__mock/vendor?company=Kit&key=${key}`);
  return (await response.json()) as Record<string, unknown>;
}

const ERP_GREEN = '#079455';

async function erpTab(page: Page) {
  await page.getByTestId('ribbon').getByRole('tab', { name: 'ERP Data', exact: true }).click();
}

async function typeInto(page: Page, row: number, col: number, text: string) {
  await select(page, row, col);
  await page.keyboard.type(text);
  await page.keyboard.press('Enter');
  await expect.poll(() => cellText(page, row + 1, col + 1)).toBe(text);
}

async function commitAll(page: Page, visible = false) {
  await erpTab(page);
  await page.getByTestId('ribbon').getByRole('button', { name: 'Commit▾', exact: true }).click();
  await page
    .getByText(visible ? 'Commit Changes in Visible Rows…' : 'Commit All Changes…', { exact: true })
    .click();
}

const status = (page: Page) => page.getByTestId('engine-status');

/** The commit's confirmation, without the isolates around names. */
const confirmText = async (page: Page) =>
  plain(await page.getByTestId('erp-commit-text').textContent());

test.describe('committing ERP data', () => {
  // V-1001 to V-1008, the mock's fixed vendors, in id order: data rows 1 to 8 (0-based, the
  // heading at 0).
  const FILTER = { search: 'V-100' };
  const COL = { name: 1, group: 2, mobile: 3, pay: 5, address: 6 };

  test('validates, reviews and commits, leaving the ERP equal to the oracle', async ({ page }) => {
    const before = await records(FILTER);
    expect(before.map((r) => r.key)).toEqual(Array.from({ length: 8 }, (_, i) => `V-100${i + 1}`));
    await launch(page, (await ticket(FILTER)).url);
    await bound(page, 'Vendors · 8 · Sara @ Kit');
    await typeInto(page, 1, COL.name, 'Al Noor Trading Co');
    await typeInto(page, 2, COL.mobile, '01009876543');
    await typeInto(page, 3, COL.pay, 'NET60');
    await typeInto(page, 4, COL.address, '12 Tahrir St');
    const oracle = structuredClone(before);
    oracle[0]!.values['vendorName'] = 'Al Noor Trading Co';
    oracle[1]!.values['mobile'] = '01009876543';
    oracle[2]!.values['payMethod'] = 'NET60';
    oracle[3]!.values['address'] = '12 Tahrir St';
    const start = await mockState();

    // Validate: the dry run finds nothing and writes nothing.
    await erpTab(page);
    await page.getByTestId('ribbon').getByRole('button', { name: 'Validate', exact: true }).click();
    await expect(status(page)).toHaveText(
      'Cash Gear ERP checked 4 changes to 4 vendors and found no problems.',
      { timeout: 30_000 },
    );
    expect(await mockState()).toEqual(start);

    // Review lists the four, with their originals.
    await page
      .getByTestId('ribbon')
      .getByRole('button', { name: 'Review Changes', exact: true })
      .click();
    const review = page.getByTestId('erp-review');
    await expect(review).toBeVisible();
    await expect(page.getByTestId('erp-review-filters')).toContainText('All (4)');
    await expect(review).toContainText('Al Noor Trading Co');
    await expect(review).toContainText(String(before[1]!.values['mobile']));
    await expect(page.getByTestId('erp-review-commit')).toHaveText('Commit 4 changes to 4 vendors');

    // Commit from Review, through the confirmation.
    await page.getByTestId('erp-review-commit').click();
    await expect
      .poll(() => confirmText(page))
      .toBe(
        "Commit 4 changes to 4 vendors in Cash Gear ERP (Kit)?Cash Gear ERP checks them again and saves all of them or none. Undo is cleared once they're saved.",
      );
    await page.getByTestId('erp-commit-send').click();
    await expect(status(page)).toHaveText('Committed 4 changes to 4 vendors.', {
      timeout: 30_000,
    });
    const end = await mockState();
    expect(end.writes).toBe(start.writes + 4);
    expect(end.commits).toBe(start.commits + 1);
    expect(end.audit).toBe(start.audit + 4);

    // The ERP holds the oracle, and the sheet what the ERP persisted.
    const after = await records(FILTER);
    for (const [i, record] of after.entries()) {
      const expected = oracle[i]!;
      for (const field of FIELDS.slice(0, 11))
        expect(record.values[field], `${record.key} ${field}`).toEqual(expected.values[field]);
      const touched = i < 4;
      expect(record.version !== before[i]!.version, record.key).toBe(touched);
      if (touched) expect(record.values['modifiedBy']).toBe('u17');
    }
    await expectRecords(page, after);
    expect(await counts(page)).toEqual({
      changed: 0,
      changedRecords: 0,
      invalid: 0,
      formulas: 0,
      conflicts: 0,
      gone: 0,
    });
    // Committed fields show a green bar until they change; undo is cleared.
    await select(page, 9, 9);
    await expect.poll(() => leadingEdge(page, 1, COL.name)).toBe(ERP_GREEN);
    await expect.poll(() => leadingEdge(page, 3, COL.pay)).toBe(ERP_GREEN);
    await page.keyboard.press('Control+z');
    await expect.poll(() => cellText(page, 2, 2)).toBe('Al Noor Trading Co');
    await typeInto(page, 1, COL.name, 'Al Noor');
    await select(page, 9, 9);
    await expect.poll(() => leadingEdge(page, 1, COL.name)).toBe('#d97706');
  });

  test('a 400 marks cells and a 409 marks conflicts, and neither writes anything', async ({
    page,
  }) => {
    await launch(page, (await ticket(FILTER)).url);
    await bound(page, 'Vendors · 8 · Sara @ Kit');
    // V-1005 is locked in the mock: the ERP refuses its record, not a local rule.
    await typeInto(page, 5, COL.address, '5 Locked St');
    await typeInto(page, 6, COL.mobile, '01000000006');
    const start = await mockState();
    await commitAll(page);
    await page.getByTestId('erp-commit-send').click();
    await expect(page.getByTestId('erp-commit-status')).toHaveText(
      'Cash Gear ERP refused 1 change to 1 vendor, so nothing was saved. The cells are marked; Next Error goes through them.',
      { timeout: 30_000 },
    );
    expect(await mockState()).toEqual(start);
    expect((await counts(page)).invalid).toBe(1);
    const locked = await page.evaluate(() =>
      window.cgsTest.query({ kind: 'erpCell', sheet: 0, row: 6, col: 7 }),
    );
    expect(locked!.server!.code).toBe('record.locked');
    await page.getByTestId('erp-commit-close').click();
    // Discarding it leaves V-1006, which someone else changes in the ERP meanwhile.
    await select(page, 5, COL.address);
    await erpTab(page);
    await page.getByRole('button', { name: 'Discard Changes' }).click();
    await page.getByText('Discard Changes in Selected Cells').click();
    await expect.poll(async () => (await counts(page)).changedRecords).toBe(1);
    await mock('edit', { company: 'Kit', key: 'V-1006', values: { mobile: '0111' }, by: 'omar' });
    const edited = await mockState();
    await commitAll(page);
    await page.getByTestId('erp-commit-send').click();
    await expect(page.getByTestId('erp-commit-status')).toHaveText(
      '1 vendor was changed in Cash Gear ERP since you opened it, so nothing was saved. 1 vendor has fields you both changed: choose Resolve Conflicts, then commit again.',
      { timeout: 30_000 },
    );
    await page.getByTestId('erp-commit-close').click();
    expect(await mockState()).toEqual(edited);
    expect((await mockVendor('V-1006'))['mobile']).toBe('0111');
    await expect
      .poll(async () => plain(await page.getByTestId('erp-status').textContent()))
      .toBe('Vendors · 8 · 1 changed · 1 conflict · Sara @ Kit');
    const info = await page.evaluate(() =>
      window.cgsTest.query({ kind: 'erpCell', sheet: 0, row: 7, col: 4 }),
    );
    expect(info!.state).toBe('conflict');
    expect(info!.conflict).toMatchObject({ by: 'omar', now: '0111' });
    // Commit waits until the conflict is dealt with.
    await commitAll(page);
    await expect(page.getByTestId('erp-commit-refused-text')).toHaveText(
      '1 vendor has fields you and Cash Gear ERP both changed. Choose Resolve Conflicts, then commit.',
    );
    await page.getByTestId('erp-commit-refused-close').click();
    expect(await mockState()).toEqual(edited);
    // A Validate that finds an error says nothing of versions: the conflict stays.
    await typeInto(page, 5, COL.address, '5 Locked St');
    await erpTab(page);
    await page.getByTestId('ribbon').getByRole('button', { name: 'Validate', exact: true }).click();
    await expect(status(page)).toHaveText(
      'Cash Gear ERP refuses 1 change to 1 vendor. The cells are marked.',
      { timeout: 30_000 },
    );
    await expect
      .poll(async () => plain(await page.getByTestId('erp-status').textContent()))
      .toBe('Vendors · 8 · 2 changed · 1 invalid · 1 conflict · Sara @ Kit');
    expect(await mockState()).toEqual(edited);
  });

  test('a response dropped after the write, then a retry, writes exactly once', async ({
    page,
  }) => {
    await launch(page, (await ticket(FILTER)).url);
    await bound(page, 'Vendors · 8 · Sara @ Kit');
    await typeInto(page, 1, COL.mobile, '01001111111');
    await typeInto(page, 2, COL.address, '2 Nile St');
    const start = await mockState();
    // Every answer is lost: the commit's, the HTTP layer's three retries' and any the browser
    // sends again itself, until the fault is cleared.
    await mock('fault', { match: 'VendorSheet/Commit$', drop: 'after', times: 0 });
    await commitAll(page);
    await page.getByTestId('erp-commit-send').click();
    await expect(page.getByTestId('erp-commit-status')).toHaveText(
      "Cash Gear ERP didn't answer, so it isn't known whether the changes were saved. Try again asks Cash Gear ERP first, and sends the same commit only if it wasn't saved.",
      { timeout: 60_000 },
    );
    expect((await mockState()).writes).toBe(start.writes + 2);
    // Writes stay held while the outcome isn't known.
    const held = await page.evaluate(() =>
      window.cgsTest.command({ kind: 'input', sheet: 0, row: 4, col: 2, text: 'x' }).then(
        () => 'ok',
        (error: unknown) => (error instanceof Error ? error.message : String(error)),
      ),
    );
    expect(held).toBe('Wait until the commit to Cash Gear ERP finishes.');
    // Try again asks CommitStatus first, which finds the commit saved: no Commit goes again,
    // as one that did would fail now.
    await mock('fault', { clear: true });
    await mock('fault', { match: 'VendorSheet/Commit$', status: 500, times: 0 });
    await page.getByTestId('erp-commit-retry').click();
    await expect(status(page)).toHaveText('Committed 2 changes to 2 vendors.', {
      timeout: 30_000,
    });
    await mock('fault', { clear: true });
    const end = await mockState();
    expect(end.writes).toBe(start.writes + 2);
    expect(end.commits).toBe(start.commits + 1);
    expect((await mockVendor('V-1001'))['mobile']).toBe('01001111111');
    expect((await counts(page)).changed).toBe(0);
    await typeInto(page, 4, COL.name, 'Free again');
  });

  test('past 500 records Commit is refused, and Review lists changes in hidden rows', async ({
    page,
  }) => {
    test.setTimeout(180_000);
    await launch(page, (await ticket({ blocked: false })).url);
    await expect
      .poll(async () => plain(await page.getByTestId('erp-status').textContent()), {
        timeout: 90_000,
      })
      .toBe('Vendors · 2,174 · Sara @ Kit');
    await waitForGrid(page);
    // One address typed into 601 records at once, as Ctrl+Enter does.
    await page.evaluate(() =>
      window.cgsTest.command({
        kind: 'input',
        sheet: 0,
        row: 2,
        col: 7,
        text: '1 Bulk St',
        areas: [{ top: 2, left: 7, bottom: 602, right: 7 }],
      }),
    );
    await expect.poll(async () => (await counts(page)).changedRecords).toBe(601);
    const start = await mockState();
    await commitAll(page);
    await expect(page.getByTestId('erp-commit-refused-text')).toHaveText(
      '601 vendors have changes, and Cash Gear ERP takes at most 500 in one commit. Filter the table to fewer of them, then choose Commit Changes in Visible Rows.',
    );
    await page.getByTestId('erp-commit-refused-close').click();
    // Filtered to one group, the rows shown can be committed; the rest are listed as hidden.
    await page.evaluate(() =>
      window.cgsTest.command({
        kind: 'setFilter',
        sheet: 0,
        table: 'Vendors',
        col: 3,
        criteria: { kind: 'values', values: ['LOCAL'] },
      }),
    );
    await page.evaluate(() => window.cgsTest.settled());
    const visible = (await page.evaluate(() =>
      window.cgsTest.query({ kind: 'erpChanges', visible: true }),
    ))!;
    const shownCount = visible.records.length;
    expect(shownCount).toBeGreaterThan(0);
    expect(shownCount).toBeLessThan(500);
    await erpTab(page);
    await page
      .getByTestId('ribbon')
      .getByRole('button', { name: 'Review Changes', exact: true })
      .click();
    await expect(page.getByTestId('erp-review-filters')).toContainText(
      `In hidden rows (${601 - shownCount})`,
      { timeout: 30_000 },
    );
    await page.getByTestId('erp-review-close').click();
    await commitAll(page, true);
    await expect
      .poll(() => confirmText(page))
      .toBe(
        `Commit ${shownCount} changes to ${shownCount} vendors in Cash Gear ERP (Kit)?Cash Gear ERP checks them again and saves all of them or none. Undo is cleared once they're saved.`,
      );
    await page.getByTestId('erp-commit-cancel').click();
    expect(await mockState()).toEqual(start);
  });
});

// Phase 18: the three-way merge, Resolve Conflicts, sessions, Reconnect, CommitStatus, recovery
// and the Arabic catalogue, against the mock's store as the oracle (plan G, "Phase 18").

/** A session for a company's editor, as Redeem answers it, for handing over by hand. */
async function redeemed(key: string): Promise<Record<string, unknown>> {
  const response = await fetch(`${MOCK}/api/system/SheetLaunch/CreateTicket`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-API-Key': key },
    body: JSON.stringify({ entity: 'vendors', filter: null, language: 'en' }),
  });
  const { ticket: value } = (await response.json()) as { ticket: string };
  return (await (
    await fetch(`${MOCK}/api/sheet/Session/Redeem`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Origin: SHEET_ORIGIN },
      body: JSON.stringify({ ticket: value }),
    })
  ).json()) as Record<string, unknown>;
}

/** Whether the ERP still takes a session's token. */
async function sessionOpen(token: string): Promise<boolean> {
  const response = await fetch(`${MOCK}/api/payable/VendorSheet/Schema`, {
    headers: { Origin: SHEET_ORIGIN, 'X-Sheet-Session': token },
  });
  return response.status === 200;
}

const badgeOf = async (page: Page) => plain(await page.getByTestId('erp-status').textContent());

async function refresh(page: Page) {
  await erpTab(page);
  await page.getByTestId('ribbon').getByRole('button', { name: 'Refresh', exact: true }).click();
}

/** A second tab of the sheet, opened as Cash Gear ERP's button opens one. */
async function launchTab(context: BrowserContext, url: string) {
  const tab = await context.newPage();
  await tab.addInitScript(FONT_OVERRIDE);
  await tab.goto(url);
  return tab;
}

/** The launch tab hands its session to the waiting one, says so, and closes if it may. */
async function handedOver(tab: Page) {
  await expect
    .poll(
      async () =>
        tab.isClosed() ||
        (await tab
          .getByTestId('erp-load-text')
          .textContent()
          .catch(() => '')) ===
          'This connection went to the sheet that was waiting for it. You can close this tab.',
      { timeout: 30_000 },
    )
    .toBe(true);
  if (!tab.isClosed()) await tab.close();
}

test.describe('conflicts, refresh, sessions and recovery', () => {
  const FILTER = { search: 'V-100' };
  const COL = { name: 1, group: 2, mobile: 3, pay: 5, address: 6 };

  test('a concurrent edit conflicts, Resolve keeps one field and takes another, and the commit saves it', async ({
    page,
  }) => {
    const before = await records(FILTER);
    await launch(page, (await ticket(FILTER)).url);
    await bound(page, 'Vendors · 8 · Sara @ Kit');
    await typeInto(page, 2, COL.mobile, '01022222222');
    await typeInto(page, 4, COL.pay, 'NET60');
    await typeInto(page, 3, COL.address, '3 Mine St');
    // Elsewhere in the ERP: Omar changes the same two fields to other values, and Mona another
    // field of the third record.
    await mock('edit', { company: 'Kit', key: 'V-1002', values: { mobile: '0133' }, by: 'omar' });
    expect(before[3]!.values['payMethod']).toBe('CASH');
    await mock('edit', {
      company: 'Kit',
      key: 'V-1004',
      values: { payMethod: 'NET30' },
      by: 'omar',
    });
    await mock('edit', {
      company: 'Kit',
      key: 'V-1003',
      values: { vendorName: 'Theirs' },
      by: 'mona',
    });
    const edited = await mockState();
    await refresh(page);
    await expect(status(page)).toHaveText(
      'Refreshed 8 vendors. 3 changed in Cash Gear ERP: 1 merged with your changes, 2 in conflict.',
      { timeout: 30_000 },
    );
    await expect
      .poll(() => badgeOf(page))
      .toBe('Vendors · 8 · 3 changed · 2 conflicts · Sara @ Kit');
    // Mona's name shows in the sheet, rebased under the address still pending.
    await expect.poll(() => cellText(page, 4, 2)).toBe('Theirs');
    await expect.poll(() => cellText(page, 4, 7)).toBe('3 Mine St');
    // Commit waits, and offers Resolve.
    await commitAll(page);
    await expect(page.getByTestId('erp-commit-refused-text')).toHaveText(
      '2 vendors have fields you and Cash Gear ERP both changed. Choose Resolve Conflicts, then commit.',
    );
    await page.getByTestId('erp-commit-refused-resolve').click();
    const dialog = page.getByTestId('erp-resolve');
    await expect(dialog).toBeVisible();
    await expect(page.getByTestId('erp-resolve-intro')).toHaveText(
      '2 vendors changed in Cash Gear ERP since you loaded them, in fields you changed too.',
    );
    const grid = page.getByTestId('erp-resolve-grid');
    await expect(grid).toContainText('01022222222');
    await expect(grid).toContainText('0133');
    await expect(grid).toContainText('omar');
    // Keep mine for the mobile, take the ERP's pay method.
    await grid
      .getByRole('row', { name: /V-1002/ })
      .getByRole('checkbox')
      .check();
    await page.getByTestId('erp-resolve-mine').click();
    await expect(status(page)).toHaveText(
      'Resolved 1 field. 1 vendor is ready to commit. 1 vendor still has conflicts.',
    );
    await grid
      .getByRole('row', { name: /V-1004/ })
      .getByRole('checkbox')
      .check();
    await page.getByTestId('erp-resolve-erp').click();
    await expect(status(page)).toHaveText('Resolved 1 field. 1 vendor is ready to commit.');
    await expect(page.getByTestId('erp-resolve-intro')).toHaveText('No conflicts are left.');
    await page.getByTestId('erp-resolve-close').click();
    await expect.poll(() => cellText(page, 5, 6)).toBe('NET30');
    await expect.poll(() => badgeOf(page)).toBe('Vendors · 8 · 2 changed · Sara @ Kit');
    expect(await mockState()).toEqual(edited);
    // The commit saves mine over Omar's mobile, keeps Mona's name and Omar's pay method.
    await commitAll(page);
    await page.getByTestId('erp-commit-send').click();
    await expect(status(page)).toHaveText('Committed 2 changes to 2 vendors.', { timeout: 30_000 });
    expect((await mockState()).writes).toBe(edited.writes + 2);
    expect(await mockVendor('V-1002')).toMatchObject({ mobile: '01022222222' });
    expect(await mockVendor('V-1003')).toMatchObject({
      vendorName: 'Theirs',
      address: '3 Mine St',
    });
    expect(await mockVendor('V-1004')).toMatchObject({ payMethod: 'NET30' });
    const after = await records(FILTER);
    await expectRecords(page, after);
    expect(after.map((r) => r.values['vendorName'])).toEqual(
      before.map((r) => (r.key === 'V-1003' ? 'Theirs' : r.values['vendorName'])),
    );
  });

  test('non-overlapping concurrent edits rebase without a conflict, and Next Conflict has none', async ({
    page,
  }) => {
    await launch(page, (await ticket(FILTER)).url);
    await bound(page, 'Vendors · 8 · Sara @ Kit');
    await typeInto(page, 1, COL.address, '1 Mine St');
    await typeInto(page, 6, COL.mobile, '01066666666');
    await mock('edit', { company: 'Kit', key: 'V-1001', values: { mobile: '0111' }, by: 'omar' });
    await mock('edit', {
      company: 'Kit',
      key: 'V-1006',
      values: { address: 'Their St' },
      by: 'mona',
    });
    // The 409 merges what it carries: the commit is refused, both records rebase, nothing clashes.
    const edited = await mockState();
    await commitAll(page);
    await page.getByTestId('erp-commit-send').click();
    await expect(page.getByTestId('erp-commit-status')).toHaveText(
      '2 vendors were changed in Cash Gear ERP since you opened them, so nothing was saved. 2 vendors merged with your changes: commit again to send them.',
      { timeout: 30_000 },
    );
    await page.getByTestId('erp-commit-close').click();
    expect(await mockState()).toEqual(edited);
    await expect.poll(() => badgeOf(page)).toBe('Vendors · 8 · 2 changed · Sara @ Kit');
    await expect.poll(() => cellText(page, 2, 4)).toBe('0111');
    await expect.poll(() => cellText(page, 7, 7)).toBe('Their St');
    await erpTab(page);
    await expect(
      page.getByTestId('ribbon').getByRole('button', { name: 'Next Conflict', exact: true }),
    ).toBeDisabled();
    await commitAll(page);
    await page.getByTestId('erp-commit-send').click();
    await expect(status(page)).toHaveText('Committed 2 changes to 2 vendors.', { timeout: 30_000 });
    expect(await mockVendor('V-1001')).toMatchObject({ mobile: '0111', address: '1 Mine St' });
    expect(await mockVendor('V-1006')).toMatchObject({
      mobile: '01066666666',
      address: 'Their St',
    });
    // Review's Discard Selected puts back the field chosen by its checkbox.
    await typeInto(page, 7, COL.name, 'To discard');
    await expect.poll(async () => (await counts(page)).changed).toBe(1);
    await page
      .getByTestId('ribbon')
      .getByRole('button', { name: 'Review Changes', exact: true })
      .click();
    await page
      .getByTestId('erp-review-grid')
      .getByRole('row', { name: /V-1007/ })
      .getByRole('checkbox')
      .check();
    await page.getByTestId('erp-review-discard').click();
    await expect.poll(async () => (await counts(page)).changed).toBe(0);
    await page.getByTestId('erp-review-close').click();
  });

  test('a 401 during a commit, then Reconnect, asks CommitStatus and writes nothing twice', async ({
    page,
    context,
  }) => {
    await launch(page, (await ticket(FILTER)).url);
    await bound(page, 'Vendors · 8 · Sara @ Kit');
    await typeInto(page, 1, COL.mobile, '01011111111');
    await typeInto(page, 2, COL.address, '2 Nile St');
    const start = await mockState();
    // The commit lands but its answer is lost, and the session has ended by the next request.
    await mock('fault', { match: 'VendorSheet/Commit$', drop: 'after', times: 1 });
    await mock('fault', { match: 'VendorSheet/Commit$', status: 401, times: 0 });
    await commitAll(page);
    await page.getByTestId('erp-commit-send').click();
    await expect(page.getByTestId('erp-commit-status')).toHaveText(
      'Your Cash Gear ERP session has ended, so it isn’t known whether the changes were saved. Choose Reconnect: the sheet then asks Cash Gear ERP before it sends them again.',
      { timeout: 60_000 },
    );
    expect((await mockState()).writes).toBe(start.writes + 2);
    await expect
      .poll(() => badgeOf(page))
      .toBe('Vendors · 8 · 2 changed · Session ended · Sara @ Kit');
    // From now on a Commit would fail: only CommitStatus can find out.
    await mock('fault', { clear: true });
    await mock('fault', { match: 'VendorSheet/Commit$', status: 500, times: 0 });
    await page.getByTestId('erp-commit-reconnect').click();
    await expect(page.getByTestId('erp-reconnect-text')).toHaveText(
      "Open Cash Gear ERP's vendors list and choose Edit in Cash Gear Sheet. The new tab hands its connection to this sheet and closes, and this sheet then refreshes. Your changes stay here meanwhile.",
    );
    await handedOver(await launchTab(context, (await ticket(FILTER)).url));
    await expect(page.getByTestId('erp-reconnect')).toBeHidden({ timeout: 30_000 });
    await expect.poll(() => counts(page), { timeout: 30_000 }).toMatchObject({ changed: 0 });
    await expect.poll(() => badgeOf(page)).toBe('Vendors · 8 · Sara @ Kit');
    const end = await mockState();
    expect(end.writes).toBe(start.writes + 2);
    expect(end.commits).toBe(start.commits + 1);
    expect(await mockVendor('V-1001')).toMatchObject({ mobile: '01011111111' });
    await mock('fault', { clear: true });
    // The new session works: an edit commits.
    await typeInto(page, 3, COL.mobile, '01033333333');
    await commitAll(page);
    await page.getByTestId('erp-commit-send').click();
    await expect(status(page)).toHaveText('Committed 1 change to 1 vendor.', { timeout: 30_000 });
  });

  test('a session handed over for another company is refused, and a launch for it opens its own sheet', async ({
    page,
    context,
  }) => {
    await launch(page, (await ticket(FILTER)).url);
    await bound(page, 'Vendors · 8 · Sara @ Kit');
    await typeInto(page, 1, COL.mobile, '01011111111');
    // The session ends: the ERP's clock passes its idle time, and Refresh meets the 401.
    await mock('clock', { advanceMs: 31 * 60_000 });
    await refresh(page);
    await expect(status(page)).toHaveText(
      'Your Cash Gear ERP session has ended. Your changes stay in this sheet: choose Reconnect to carry on.',
      { timeout: 30_000 },
    );
    await page
      .getByTestId('ribbon')
      .getByRole('button', { name: 'Reconnect', exact: true })
      .click();
    await expect(page.getByTestId('erp-reconnect-status')).toHaveText('Waiting for Cash Gear ERP…');
    // Another tab of this origin hands Dot's session to the tab waiting for Kit's.
    const dot = await redeemed('mock-dot-editor');
    const forger = await context.newPage();
    await forger.goto(`${SHEET_ORIGIN}/`);
    const reply = await forger.evaluate(async (session) => {
      const channel = new BroadcastChannel('cgs-erp-reconnect');
      const heard: { type: string; tab?: string }[] = [];
      channel.onmessage = (event) => heard.push(event.data as { type: string; tab?: string });
      const scope = { company: 'Kit', userId: 'u17', entity: 'vendors' };
      channel.postMessage({ type: 'waiting?', ask: 'forged', scope });
      for (let i = 0; i < 50 && !heard.some((m) => m.type === 'waiting'); i++)
        await new Promise((resolve) => setTimeout(resolve, 20));
      const waiting = heard.find((m) => m.type === 'waiting');
      if (!waiting) return 'no tab waited';
      channel.postMessage({ type: 'handover', ask: 'forged', tab: waiting.tab, session });
      for (let i = 0; i < 100 && !heard.some((m) => m.type !== 'waiting'); i++)
        await new Promise((resolve) => setTimeout(resolve, 20));
      return heard.find((m) => m.type !== 'waiting')?.type ?? 'no answer';
    }, dot);
    expect(reply).toBe('refused');
    await expect
      .poll(async () => plain(await status(page).textContent()))
      .toBe(
        'A connection for another company, user or list was refused: this sheet holds vendors of Kit for Sara.',
      );
    await expect
      .poll(() => badgeOf(page))
      .toBe('Vendors · 8 · 1 changed · Session ended · Sara @ Kit');
    // The refused session was ended, so nothing can use it.
    await expect.poll(() => sessionOpen(String(dot['token']))).toBe(false);
    // A real launch for Dot isn't offered to the Kit tab: it opens its own sheet.
    const dotLaunch = await fetch(`${MOCK}/api/system/SheetLaunch/CreateTicket`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-API-Key': 'mock-dot-editor' },
      body: JSON.stringify({ entity: 'vendors', filter: { search: 'V-100' }, language: 'en' }),
    });
    const dotTab = await launchTab(context, ((await dotLaunch.json()) as { url: string }).url);
    await expect
      .poll(async () => plain(await dotTab.getByTestId('erp-status').textContent()), {
        timeout: 60_000,
      })
      .toMatch(/^Vendors · \d+ · Karim @ Dot$/);
    await expect
      .poll(() => badgeOf(page))
      .toBe('Vendors · 8 · 1 changed · Session ended · Sara @ Kit');
    await dotTab.close();
    await forger.close();
  });

  test('a page that ends with changes leaves a draft that recovers disconnected and never commits by itself', async ({
    page,
    context,
  }) => {
    await launch(page, (await ticket(FILTER)).url);
    await bound(page, 'Vendors · 8 · Sara @ Kit');
    await typeInto(page, 1, COL.name, 'Al Noor Recovered');
    await typeInto(page, 2, COL.mobile, '01022222222');
    await page.evaluate(() => window.cgsTest.settled());
    const start = await mockState();
    // The page ends without a word: no commit, no save.
    await page.close({ runBeforeUnload: false });

    const again = await context.newPage();
    again.on('pageerror', (error) => {
      throw error;
    });
    await again.addInitScript(FONT_OVERRIDE);
    await again.goto(`${SHEET_ORIGIN}/`);
    const offer = again.getByTestId('recover');
    await expect(offer).toBeVisible({ timeout: 30_000 });
    await expect(again.getByTestId('recover-text')).toContainText(
      "has changes to Cash Gear ERP's vendors that weren't committed when its session ended. They open disconnected",
    );
    await offer.getByRole('button', { name: 'Recover', exact: true }).click();
    await expect
      .poll(async () => plain(await again.getByTestId('erp-status').textContent()), {
        timeout: 60_000,
      })
      .toBe('Vendors · 8 · 2 changed · Disconnected · Sara @ Kit');
    await expect.poll(() => cellText(again, 2, 2)).toBe('Al Noor Recovered');
    await expect.poll(() => cellText(again, 3, 4)).toBe('01022222222');
    await erpTab(again);
    await expect(
      again.getByTestId('ribbon').getByRole('button', { name: 'Commit▾', exact: true }),
    ).toBeDisabled();
    expect(await mockState()).toEqual(start);
    // A launch for the same company, user and entity reconnects it; it refreshes, and still
    // sends nothing by itself.
    await handedOver(await launchTab(context, (await ticket(FILTER)).url));
    await expect
      .poll(async () => plain(await again.getByTestId('erp-status').textContent()), {
        timeout: 30_000,
      })
      .toBe('Vendors · 8 · 2 changed · Sara @ Kit');
    await expect(again.getByTestId('engine-status')).toHaveText(
      'Refreshed 8 vendors. Nothing changed in Cash Gear ERP.',
      { timeout: 30_000 },
    );
    await again.waitForTimeout(1500);
    expect(await mockState()).toEqual(start);
    // Committed by hand, it saves.
    await commitAll(again);
    await again.getByTestId('erp-commit-send').click();
    await expect(again.getByTestId('engine-status')).toHaveText(
      'Committed 2 changes to 2 vendors.',
      { timeout: 30_000 },
    );
    expect((await mockState()).writes).toBe(start.writes + 2);
  });

  test('a launch with no tab waiting offers a matching draft, which recovers into its session', async ({
    page,
    context,
  }) => {
    await launch(page, (await ticket(FILTER)).url);
    await bound(page, 'Vendors · 8 · Sara @ Kit');
    await typeInto(page, 5, COL.address, '5 Draft St');
    await page.evaluate(() => window.cgsTest.settled());
    await page.close({ runBeforeUnload: false });
    await mock('edit', { company: 'Kit', key: 'V-1005', values: { mobile: '0155' }, by: 'omar' });
    const start = await mockState();
    const tab = await launchTab(context, (await ticket(FILTER)).url);
    await expect
      .poll(async () => plain(await tab.getByTestId('erp-draft-text').textContent()), {
        timeout: 30_000,
      })
      .toBe(
        'You have changes to Vendors – Kit from a session that ended before they were committed. Recover them into this connection, which then refreshes, or open the data afresh.',
      );
    await tab.getByTestId('erp-draft-recover').click();
    await expect
      .poll(async () => plain(await tab.getByTestId('erp-status').textContent()), {
        timeout: 60_000,
      })
      .toBe('Vendors · 8 · 1 changed · Sara @ Kit');
    await expect(tab.getByTestId('engine-status')).toHaveText(
      'Refreshed 8 vendors. 1 changed in Cash Gear ERP: 1 merged with your changes.',
      { timeout: 30_000 },
    );
    await expect.poll(() => cellText(tab, 6, 7)).toBe('5 Draft St');
    await expect.poll(() => cellText(tab, 6, 4)).toBe('0155');
    expect(await mockState()).toEqual(start);
    await tab.close();
  });

  test('Arabic sessions word the ERP tab, the badge and Resolve in Arabic, right to left', async ({
    page,
  }) => {
    await launch(page, (await ticket(FILTER, 'ar')).url);
    await bound(page, 'الموردون · 8 · Sara @ Kit');
    await typeInto(page, 2, COL.mobile, '01022222222');
    await typeInto(page, 3, COL.address, '3 Mine St');
    await mock('edit', { company: 'Kit', key: 'V-1002', values: { mobile: '0133' }, by: 'omar' });
    await mock('edit', { company: 'Kit', key: 'V-1004', values: { taxFile: '0099' }, by: 'mona' });
    const tab = page.getByTestId('ribbon').getByRole('tab', { name: 'بيانات ERP', exact: true });
    await tab.click();
    await page.getByTestId('ribbon').getByRole('button', { name: 'تحديث', exact: true }).click();
    await expect(status(page)).toHaveText(
      'تم تحديث 8 موردين. عُدّل 2 في Cash Gear ERP: 1 في تعارض.',
      { timeout: 30_000 },
    );
    await expect
      .poll(() => badgeOf(page))
      .toBe('الموردون · 8 · معدّلة: 2 · تعارضات: 1 · Sara @ Kit');
    await select(page, 9, 9);
    await waitForGrid(page);
    await expectGolden(page, 'erp-merge-rtl', 0.002, { fromRight: true });
    await page
      .getByTestId('ribbon')
      .getByRole('button', { name: 'حل التعارضات', exact: true })
      .click();
    await expect(page.getByTestId('erp-resolve-intro')).toHaveText(
      'عُدّل مورد واحد في Cash Gear ERP منذ تحميلها، في حقول عدّلتها أنت أيضًا.',
    );
    expect(
      await page
        .getByTestId('erp-resolve')
        .locator('[dir="rtl"]')
        .first()
        .evaluate((element) => getComputedStyle(element).direction),
    ).toBe('rtl');
    await page.getByTestId('erp-resolve-all-mine').click();
    await expect(status(page)).toHaveText('تم حل حقل واحد. مورد واحد جاهز للاعتماد.');
    await page.getByTestId('erp-resolve-close').click();
    await expect.poll(() => badgeOf(page)).toBe('الموردون · 8 · معدّلة: 2 · Sara @ Kit');
  });
});
