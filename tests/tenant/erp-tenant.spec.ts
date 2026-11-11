// The scripted e2e against Cash Gear ERP's test tenant (Phase 19): the deployed sheet
// (`E2E_BASE_URL`, https://sheet.cashgearerp.com) against the live UnifiedAPI (`ERP_BASE_URL`),
// as Kit's test users, on Kit's own test vendors (`SHT-…`, from `Seed_SheetTestVendors.sql`) only.
// `pnpm erp:tenant e2e` runs it with the keys from `%USERPROFILE%\.cashgear\sheet-erp-test.env`;
// it is never part of `pnpm test:e2e`. Failures the mock injects are made here by the browser's
// own routes, and every value a test changes is put back after it.
import { expect, test, type BrowserContext, type Page, type Route } from '@playwright/test';
import { FONT_OVERRIDE, waitForGrid } from '../e2e/helpers.mjs';

const env = process.env;
const API = (env.ERP_BASE_URL ?? '').replace(/\/+$/, '');
const SHEET = new URL(env.E2E_BASE_URL ?? 'https://sheet.cashgearerp.com').origin;
const KEYS = { editor: env.ERP_TEST_KEY_EDITOR, adv: env.ERP_TEST_KEY_ADV };
/** Kit's test vendors, and the only ones a test changes. */
const FILTER = { search: env.ERP_TEST_WRITE_SEARCH ?? 'SHT-' };
const COL = { name: 1, mobile: 3, address: 6 };

test.skip(!API || !KEYS.editor || !KEYS.adv, 'Needs ERP_BASE_URL and the editor and AdvEdit keys.');

type Wire = string | number | boolean | null;
interface Snapshot {
  id: number;
  key: string;
  version: string;
  values: Record<string, Wire>;
}
interface Session {
  token: string;
  company: string;
  userId: string;
  userName: string;
  entityRoute: string;
}

// ---------------------------------------------------------------------------------------------
// The ERP, from Node, with the sheet's Origin

/** One request, waiting out a rate limit's 429 as the sheet does. */
async function call(
  route: string,
  init: { method?: string; body?: unknown; key?: string; session?: string } = {},
): Promise<{ status: number; json: unknown }> {
  const headers: Record<string, string> = { Origin: SHEET };
  if (init.body !== undefined) headers['Content-Type'] = 'application/json';
  if (init.key) headers['X-API-Key'] = init.key;
  if (init.session) headers['X-Sheet-Session'] = init.session;
  for (let waits = 0; ; waits++) {
    const response = await fetch(`${API}/${route}`, {
      method: init.method ?? (init.body === undefined ? 'GET' : 'POST'),
      headers,
      body: init.body === undefined ? undefined : JSON.stringify(init.body),
    });
    const raw = await response.text();
    if (response.status === 429 && waits < 3) {
      const seconds = Number(response.headers.get('Retry-After'));
      await new Promise((done) => setTimeout(done, (seconds > 0 ? seconds : 5) * 1000));
      continue;
    }
    return { status: response.status, json: raw === '' ? null : JSON.parse(raw) };
  }
}

async function ticket(key: string, language: 'en' | 'ar' = 'en') {
  const answer = await call('api/system/SheetLaunch/CreateTicket', {
    key,
    body: { entity: 'vendors', filter: FILTER, language },
  });
  expect(answer.status, 'CreateTicket').toBe(200);
  return answer.json as { ticket: string; url: string };
}

const sessions = new Map<string, Session>();

/** A session of the ERP's own for `key`, opened once and kept for the run. */
async function session(key: string): Promise<Session> {
  const known = sessions.get(key);
  if (known) return known;
  const answer = await call('api/sheet/Session/Redeem', {
    body: { ticket: (await ticket(key)).ticket },
  });
  expect(answer.status, 'Redeem').toBe(200);
  const opened = answer.json as Session;
  sessions.set(key, opened);
  return opened;
}

/** The test vendors as the ERP holds them now, in id order. */
async function records(): Promise<Snapshot[]> {
  const s = await session(KEYS.editor!);
  const answer = await call(`${s.entityRoute}/Query`, {
    session: s.token,
    body: { filter: FILTER, ids: null, afterId: 0, pageSize: 1000 },
  });
  expect(answer.status, 'Query').toBe(200);
  return (answer.json as { records: Snapshot[] }).records;
}

/** A commit made elsewhere in the ERP, as another user would, with the AdvEdit key. */
async function commitElsewhere(record: Snapshot, to: Record<string, Wire>) {
  const s = await session(KEYS.adv!);
  const schema = (await call(`${s.entityRoute}/Schema`, { session: s.token })).json as {
    schemaVersion: string;
  };
  const changes = Object.fromEntries(
    Object.entries(to).map(([field, value]) => [
      field,
      { from: record.values[field] ?? null, to: value },
    ]),
  );
  const answer = await call(`${s.entityRoute}/Commit`, {
    session: s.token,
    body: {
      commitId: crypto.randomUUID(),
      company: s.company,
      schemaVersion: schema.schemaVersion,
      dryRun: false,
      records: [{ id: record.id, key: record.key, version: record.version, changes }],
    },
  });
  expect(answer.status, JSON.stringify(answer.json)).toBe(200);
  return s.userId;
}

const EDITABLE = [
  'vendorName',
  'vendorGroup',
  'mobile',
  'currency',
  'payMethod',
  'address',
  'taxFile',
  'blocked',
];

/** Writes every test vendor back to `baseline`, at its version now. */
async function putBack(baseline: Snapshot[]) {
  const s = await session(KEYS.adv!);
  const schema = (await call(`${s.entityRoute}/Schema`, { session: s.token })).json as {
    schemaVersion: string;
  };
  for (let round = 0; round < 3; round++) {
    const now = new Map((await records()).map((r) => [r.id, r]));
    const back = baseline.flatMap((was) => {
      const current = now.get(was.id);
      if (!current) return [];
      const changes = Object.fromEntries(
        EDITABLE.filter((f) => (current.values[f] ?? null) !== (was.values[f] ?? null)).map((f) => [
          f,
          { from: current.values[f] ?? null, to: was.values[f] ?? null },
        ]),
      );
      return Object.keys(changes).length === 0
        ? []
        : [{ id: was.id, key: was.key, version: current.version, changes }];
    });
    if (back.length === 0) return;
    const answer = await call(`${s.entityRoute}/Commit`, {
      session: s.token,
      body: {
        commitId: crypto.randomUUID(),
        company: s.company,
        schemaVersion: schema.schemaVersion,
        dryRun: false,
        records: back,
      },
    });
    if (answer.status === 200) return;
    expect(answer.status, `put back: ${JSON.stringify(answer.json)}`).toBe(409);
  }
  throw new Error('The test vendors could not be put back.');
}

let baseline: Snapshot[] = [];

test.beforeAll(async () => {
  baseline = await records();
  // 12 on the tenant (Seed_SheetTestVendors.sql); any handful where another server has others.
  expect(baseline.length, "the tenant's test vendors").toBeGreaterThanOrEqual(5);
  expect(baseline.every((r) => r.key.startsWith(FILTER.search))).toBe(true);
});

test.afterEach(async () => {
  await putBack(baseline);
});

test.afterAll(async () => {
  for (const s of sessions.values()) {
    await call('api/sheet/Session/End', { session: s.token, body: {} });
  }
});

// ---------------------------------------------------------------------------------------------
// The sheet

const plain = (text: string | null) => (text ?? '').replace(/[⁦-⁩]/g, '');
const badgeOf = async (page: Page) => plain(await page.getByTestId('erp-status').textContent());
const status = (page: Page) => page.getByTestId('engine-status');

async function launch(page: Page, url: string) {
  page.on('pageerror', (error) => {
    throw error;
  });
  await page.addInitScript(FONT_OVERRIDE);
  await page.goto(url);
}

async function bound(page: Page, badge: string) {
  await expect.poll(() => badgeOf(page), { timeout: 90_000 }).toBe(badge);
  await waitForGrid(page);
}

async function select(page: Page, row: number, col: number) {
  await page.evaluate((area) => window.cgsTest.grid()!.selectArea(area), {
    top: row,
    left: col,
    bottom: row,
    right: col,
  });
  await page.getByTestId('sheet-canvas').focus();
}

const cellText = (page: Page, row: number, col: number) =>
  page.evaluate(([r, c]) => window.cgsTest.cellText(r!, c!), [row, col]);

async function typeInto(page: Page, row: number, col: number, text: string) {
  await select(page, row, col);
  await page.keyboard.type(text);
  await page.keyboard.press('Enter');
  await expect.poll(() => cellText(page, row + 1, col + 1)).toBe(text);
}

async function erpTab(page: Page) {
  await page.getByTestId('ribbon').getByRole('tab', { name: 'ERP Data', exact: true }).click();
}

async function ribbonButton(page: Page, name: string) {
  await erpTab(page);
  await page.getByTestId('ribbon').getByRole('button', { name, exact: true }).click();
}

async function commitAll(page: Page) {
  await ribbonButton(page, 'Commit▾');
  await page.getByText('Commit All Changes…', { exact: true }).click();
  await page.getByTestId('erp-commit-send').click();
}

/** The table's data rows as they show, by the first column. */
async function shownRows(page: Page) {
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
  return { headings: lines[0]!, data: lines.slice(1) };
}

/** Answers the next request a route matches with `status`, as the ERP would, then lets go. */
async function answerOnce(page: Page, glob: string, status: number, code: string) {
  let done = false;
  await page.route(glob, async (route: Route) => {
    if (done) return route.continue();
    done = true;
    await route.fulfill({
      status,
      headers: {
        'Access-Control-Allow-Origin': SHEET,
        'Access-Control-Expose-Headers': 'X-Correlation-Id, X-Company-Id, Retry-After',
        'Cache-Control': 'no-store',
        'Content-Type': 'application/json',
        'X-Correlation-Id': 'tenant-e2e',
      },
      body: JSON.stringify({
        message: 'From the test.',
        correlationId: 'tenant-e2e',
        errors: [{ code, message: 'From the test.' }],
      }),
    });
  });
}

/** A second tab of the sheet, opened as Cash Gear ERP's button opens one. */
async function launchTab(context: BrowserContext, url: string) {
  const tab = await context.newPage();
  await tab.addInitScript(FONT_OVERRIDE);
  await tab.goto(url);
  return tab;
}

// ---------------------------------------------------------------------------------------------

test('opens the test vendors from a launch, cross-origin isolated, each row its own record', async ({
  page,
}) => {
  const me = await session(KEYS.editor!);
  await launch(page, (await ticket(KEYS.editor!)).url);
  await bound(page, `Vendors · ${baseline.length} · ${me.userName} @ ${me.company}`);
  expect(new URL(page.url()).hash).toBe('');
  expect(await page.evaluate(() => crossOriginIsolated)).toBe(true);
  const { headings, data } = await shownRows(page);
  expect(headings.slice(0, 4)).toEqual(['Account', 'Name', 'Group', 'Mobile']);
  expect(data.map((row) => row[0])).toEqual(baseline.map((r) => r.key));
  for (const [i, row] of data.entries()) {
    expect(row[COL.name], row[0]).toBe(String(baseline[i]!.values['vendorName'] ?? ''));
    expect(row[COL.mobile], row[0]).toBe(String(baseline[i]!.values['mobile'] ?? ''));
  }
});

test('edits, validates and commits, and the ERP holds what was committed', async ({ page }) => {
  const me = await session(KEYS.editor!);
  await launch(page, (await ticket(KEYS.editor!)).url);
  await bound(page, `Vendors · ${baseline.length} · ${me.userName} @ ${me.company}`);
  const mobile = `010${String(Date.now() % 1e8).padStart(8, '0')}`;
  await typeInto(page, 2, COL.mobile, mobile);
  await typeInto(page, 3, COL.address, '12 Tahrir St, Cairo');
  await ribbonButton(page, 'Validate');
  await expect(status(page)).toHaveText(
    'Cash Gear ERP checked 2 changes to 2 vendors and found no problems.',
    { timeout: 60_000 },
  );
  await commitAll(page);
  await expect(status(page)).toHaveText('Committed 2 changes to 2 vendors.', { timeout: 60_000 });
  const after = await records();
  expect(after[1]!.values['mobile']).toBe(mobile);
  expect(after[2]!.values['address']).toBe('12 Tahrir St, Cairo');
  for (const i of [1, 2]) {
    expect(after[i]!.version).not.toBe(baseline[i]!.version);
    expect(after[i]!.values['modifiedBy']).toBe(me.userId);
  }
  for (const i of [0, 3, 4]) expect(after[i]!.version).toBe(baseline[i]!.version);
});

test('an edit made elsewhere conflicts on Refresh, Keep Mine resolves it, and the commit saves it', async ({
  page,
}) => {
  const me = await session(KEYS.editor!);
  await launch(page, (await ticket(KEYS.editor!)).url);
  await bound(page, `Vendors · ${baseline.length} · ${me.userName} @ ${me.company}`);
  await typeInto(page, 1, COL.mobile, '01022222222');
  // Another user changes the same field, and another field of the next vendor.
  const [first, second] = await records();
  const them = await commitElsewhere(first!, { mobile: '01033333333' });
  await commitElsewhere(second!, { address: 'Elsewhere 7' });
  await ribbonButton(page, 'Refresh');
  await expect(status(page)).toContainText('1 in conflict', { timeout: 60_000 });
  await expect.poll(() => cellText(page, 3, COL.address + 1)).toBe('Elsewhere 7');
  await ribbonButton(page, 'Resolve Conflicts');
  const grid = page.getByTestId('erp-resolve-grid');
  await expect(grid).toContainText('01033333333');
  await expect(grid).toContainText(them);
  await grid
    .getByRole('row', { name: new RegExp(first!.key) })
    .getByRole('checkbox')
    .check();
  await page.getByTestId('erp-resolve-mine').click();
  await expect(page.getByTestId('erp-resolve-intro')).toHaveText('No conflicts are left.');
  await page.getByTestId('erp-resolve-close').click();
  await commitAll(page);
  await expect(status(page)).toHaveText('Committed 1 change to 1 vendor.', { timeout: 60_000 });
  const [a, b] = await records();
  expect(a!.values['mobile']).toBe('01022222222');
  expect(b!.values['address']).toBe('Elsewhere 7');
});

test('a commit whose answers are all lost is found by CommitStatus and written once', async ({
  page,
}) => {
  const me = await session(KEYS.editor!);
  await launch(page, (await ticket(KEYS.editor!)).url);
  await bound(page, `Vendors · ${baseline.length} · ${me.userName} @ ${me.company}`);
  await typeInto(page, 1, COL.mobile, '01044444444');
  await typeInto(page, 2, COL.mobile, '01055555555');
  // Every Commit reaches the ERP, and every answer is lost on the way back.
  const ids = new Set<string>();
  await page.route('**/VendorSheet/Commit', async (route) => {
    ids.add((JSON.parse(route.request().postData() ?? '{}') as { commitId: string }).commitId);
    await route.fetch();
    await route.abort('connectionreset');
  });
  await commitAll(page);
  await expect(page.getByTestId('erp-commit-status')).toContainText("isn't known", {
    timeout: 90_000,
  });
  expect(ids.size).toBe(1);
  await page.unroute('**/VendorSheet/Commit');
  await page.getByTestId('erp-commit-retry').click();
  await expect(status(page)).toHaveText('Committed 2 changes to 2 vendors.', { timeout: 60_000 });
  expect(ids.size).toBe(1);
  // The ERP stored the first attempt and answered the rest from it: the versions now are the
  // stored ones, so nothing was written twice.
  const s = await session(KEYS.editor!);
  const stored = (await call(`${s.entityRoute}/CommitStatus/${[...ids][0]}`, { session: s.token }))
    .json as { status: string; records: Snapshot[] };
  expect(stored.status).toBe('committed');
  const now = await records();
  for (const record of stored.records) {
    expect(now.find((r) => r.id === record.id)!.version, record.key).toBe(record.version);
  }
  expect(now[0]!.values['mobile']).toBe('01044444444');
  expect(now[1]!.values['mobile']).toBe('01055555555');
});

test('a session that ends shows Reconnect, and a new launch hands over to the waiting tab', async ({
  page,
  context,
}) => {
  const me = await session(KEYS.editor!);
  await launch(page, (await ticket(KEYS.editor!)).url);
  await bound(page, `Vendors · ${baseline.length} · ${me.userName} @ ${me.company}`);
  await typeInto(page, 1, COL.address, 'Kept while reconnecting');
  await answerOnce(page, '**/VendorSheet/Query', 401, 'session.expired');
  await ribbonButton(page, 'Refresh');
  await expect
    .poll(() => badgeOf(page), { timeout: 60_000 })
    .toBe(
      `Vendors · ${baseline.length} · 1 changed · Session ended · ${me.userName} @ ${me.company}`,
    );
  await ribbonButton(page, 'Reconnect');
  await expect(page.getByTestId('erp-reconnect')).toBeVisible();
  const tab = await launchTab(context, (await ticket(KEYS.editor!)).url);
  await expect
    .poll(
      async () =>
        tab.isClosed() ||
        (await tab
          .getByTestId('erp-load-text')
          .textContent()
          .catch(() => '')) ===
          'This connection went to the sheet that was waiting for it. You can close this tab.',
      { timeout: 60_000 },
    )
    .toBe(true);
  if (!tab.isClosed()) await tab.close();
  await expect(page.getByTestId('erp-reconnect')).toBeHidden({ timeout: 60_000 });
  await expect
    .poll(() => badgeOf(page), { timeout: 60_000 })
    .toBe(`Vendors · ${baseline.length} · 1 changed · ${me.userName} @ ${me.company}`);
  await commitAll(page);
  await expect(status(page)).toHaveText('Committed 1 change to 1 vendor.', { timeout: 60_000 });
  expect((await records())[0]!.values['address']).toBe('Kept while reconnecting');
});

test('Arabic sessions open right to left, with the ERP user’s headings and the badge in Arabic', async ({
  page,
}) => {
  const me = await session(KEYS.editor!);
  await launch(page, (await ticket(KEYS.editor!, 'ar')).url);
  await bound(page, `الموردون · ${baseline.length} · ${me.userName} @ ${me.company}`);
  const { headings, data } = await shownRows(page);
  expect(headings.slice(0, 4)).toEqual(['الحساب', 'الاسم', 'المجموعة', 'الجوال']);
  expect(await page.evaluate(() => window.cgsTest.grid()!.rightToLeft)).toBe(true);
  expect(data.map((row) => row[0])).toEqual(baseline.map((r) => r.key));
});
