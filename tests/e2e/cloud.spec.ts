// Cash Gear Workspace's workbooks in the browser (Track S1 and S2), against the mock Workspace on
// its own origins (playwright.config.ts starts its API on 127.0.0.1:4280 and its Blob Storage on
// :4281, the sheet's preview on :4173). The page opens a launch link as Workspace's "Open in Cash
// Gear Sheet" would, so the fragment's handling, CORS, the signed URL, cross-origin isolation and
// the content security policy run as they will in production.
import { expect, test, type BrowserContext, type Page } from '@playwright/test';
import { openXmlCopy, readZip, workbook } from '../fixtures/workbooks.mjs';
import { budgetBook } from '../workspace-mock/server.mjs';
import { FONT_OVERRIDE, waitForGrid } from './helpers.mjs';

const MOCK = 'http://127.0.0.1:4280';
const SHEET_ORIGIN = 'http://127.0.0.1:4173';

interface State {
  counts: Record<string, number>;
  saves: { workbookId: string; ifMatch: string | null; ifNoneMatch: string | null }[];
  tickets: { used: boolean }[];
  sessions: { userId: string; ended: boolean; workbookId: string }[];
  workbooks: { id: string; name: string; versions: number; etag: string | null }[];
}

async function mock(route: string, body: unknown = {}) {
  const response = await fetch(`${MOCK}/__mock/${route}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  expect(response.status).toBeLessThan(300);
  return response.status === 204 ? null : ((await response.json()) as Record<string, string>);
}

async function state(): Promise<State> {
  return (await (await fetch(`${MOCK}/__mock/state`)).json()) as State;
}

const book = async (name: string) => (await state()).workbooks.find((w) => w.name === name)!;

/** A launch link for a user and workbook, as Workspace's launch route answers it. */
async function launchUrl(user = 'editor', workbookName = 'budget') {
  const { url, ticket } = (await mock('launch', { user, workbook: workbookName }))!;
  return { url: url!, ticket: ticket! };
}

/** What a saved workbook's first sheet holds at a cell, read from its XML: the value, not the formula. */
async function savedCell(workbookId: string, at: string): Promise<string | null> {
  const bytes = Buffer.from(
    await (await fetch(`${MOCK}/__mock/bytes?workbook=${workbookId}`)).arrayBuffer(),
  );
  const parts = new Map(readZip(bytes).map(([name, data]) => [name, data.toString('utf8')]));
  const sheet = parts.get('xl/worksheets/sheet1.xml')!;
  const cell = new RegExp(`<c r="${at}"([^>]*)>(.*?)</c>`).exec(sheet);
  if (!cell) return null;
  const inner = cell[2]!.replace(/<f[^>]*>.*?<\/f>|<f[^>]*\/>/g, '');
  const text = inner.replace(/<[^>]+>/g, '');
  if (!/t="s"/.test(cell[1]!)) return text;
  const strings = [...(parts.get('xl/sharedStrings.xml') ?? '').matchAll(/<si>(.*?)<\/si>/g)];
  return strings[Number(text)]![1]!.replace(/<[^>]+>/g, '');
}

/** Strips the isolates the badge puts around names. */
const plain = (text: string | null) => (text ?? '').replace(/[⁦-⁩]/g, '');
const badgeOf = async (page: Page) => plain(await page.getByTestId('cloud-badge').textContent());

/** Content security policy violations the page reports. */
function violations(page: Page) {
  const seen: string[] = [];
  page.on('console', (message) => {
    if (/Content Security Policy/i.test(message.text())) seen.push(message.text());
  });
  return seen;
}

async function open(page: Page, url: string) {
  page.on('pageerror', (error) => {
    throw error;
  });
  await page.addInitScript(FONT_OVERRIDE);
  await page.goto(url);
}

async function opened(page: Page, badge: string) {
  await expect.poll(() => badgeOf(page), { timeout: 60_000 }).toBe(badge);
  await waitForGrid(page);
}

/** A second tab of the sheet, opened as Workspace's button opens one. */
async function launchTab(context: BrowserContext, url: string) {
  const tab = await context.newPage();
  await tab.addInitScript(FONT_OVERRIDE);
  await tab.goto(url);
  return tab;
}

const BUDGET = 'Budget 2027 – Cash Gear Workspace';

test.beforeEach(async () => {
  await mock('reset');
});

test('opens a Workspace workbook from a launch, leaving no trace of its ticket or token', async ({
  page,
}) => {
  const seen = violations(page);
  let token = '';
  page.on('response', async (response) => {
    if (response.url().endsWith('/api/v1/sheet-sessions/redeem') && response.ok()) {
      token = ((await response.json()) as { token: string }).token;
    }
  });
  const { url, ticket } = await launchUrl();
  expect(url).toBe(`${SHEET_ORIGIN}/cloud#launch=${ticket}`);
  await open(page, url);
  await opened(page, BUDGET);
  expect(await page.evaluate(() => window.cgsTest.cellText(2, 2))).toBe('1200');
  expect(await page.evaluate(() => window.cgsTest.cellText(4, 2))).toBe('1500');
  await expect(page).toHaveTitle('Budget 2027 – CashGear Sheet');
  expect(await page.evaluate(() => crossOriginIsolated)).toBe(true);

  // The fragment is gone from the address bar and from history, and the ticket is used up.
  expect(page.url()).toBe(`${SHEET_ORIGIN}/`);
  expect(await page.evaluate(() => [history.length, location.hash])).toEqual([2, '']);
  expect((await state()).tickets).toEqual([{ used: true, workbookId: expect.any(String) }]);

  // The token is in memory only: nowhere in storage, cookies, the history entry or OPFS.
  expect(token).toMatch(/^[A-Za-z0-9_-]{43}$/);
  // The first edit makes the working copy and the draft, which must not hold it either.
  await page.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 999 }));
  await page.evaluate(() => window.cgsTest.settled());
  const kept = await page.evaluate(async () => {
    const texts = [
      JSON.stringify({ ...localStorage }),
      JSON.stringify({ ...sessionStorage }),
      document.cookie,
      JSON.stringify(history.state),
      location.href,
      JSON.stringify(await indexedDB.databases()),
    ];
    const walk = async (directory: FileSystemDirectoryHandle) => {
      for await (const entry of (
        directory as unknown as { values(): AsyncIterable<FileSystemHandle> }
      ).values()) {
        if (entry.kind === 'directory') await walk(entry as FileSystemDirectoryHandle);
        else texts.push(await (await (entry as FileSystemFileHandle).getFile()).text());
      }
    };
    await walk(await navigator.storage.getDirectory());
    return texts;
  });
  expect(kept.length).toBeGreaterThan(6);
  for (const text of kept) expect(text).not.toContain(token);
  expect(seen).toEqual([]);
});

test('saves new versions with If-Match, each from the version before', async ({ page }) => {
  const seen = violations(page);
  const { url } = await launchUrl();
  const first = (await book('Budget 2027')).etag;
  await open(page, url);
  await opened(page, BUDGET);
  await page.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 999 }));
  await page.keyboard.press('Control+s');
  await expect.poll(() => badgeOf(page)).toMatch(/^Budget 2027 – Cash Gear Workspace · Saved \d/);
  await expect(page.getByTestId('save-status')).toBeVisible();
  expect(await page.evaluate(() => window.cgsTest.hasChanges())).toBe(false);
  const saved = await book('Budget 2027');
  expect(saved.versions).toBe(2);
  expect(await savedCell(saved.id, 'B2')).toBe('999');
  expect(await savedCell(saved.id, 'B4')).toBe('1299');

  await page.evaluate(() => window.cgsTest.edit(0, 3, 2, { number: 1 }));
  await page.keyboard.press('Control+s');
  await expect.poll(async () => (await state()).saves.length).toBe(2);
  const { saves } = await state();
  expect(saves.map((s) => s.ifMatch)).toEqual([first, saved.etag]);
  expect(await savedCell(saved.id, 'B4')).toBe('1000');
  expect(seen).toEqual([]);
});

test('opens and saves a workbook DocumentFormat.OpenXml wrote, every element prefixed', async ({
  page,
}) => {
  // As Cash Gear Workspace's server writes a workbook it exports from a database view.
  const source = openXmlCopy(Buffer.from(budgetBook()));
  await mock('version', { workbook: 'budget', user: 'other', bytes: source.toString('base64') });
  const { url } = await launchUrl();
  await open(page, url);
  await opened(page, BUDGET);
  expect(await page.evaluate(() => window.cgsTest.cellText(4, 2))).toBe('1500');
  await page.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 999 }));
  await page.keyboard.press('Control+s');
  await expect.poll(() => badgeOf(page)).toMatch(/^Budget 2027 – Cash Gear Workspace · Saved \d/);
  const saved = await book('Budget 2027');
  const bytes = Buffer.from(
    await (await fetch(`${MOCK}/__mock/bytes?workbook=${saved.id}`)).arrayBuffer(),
  );
  const parts = new Map(readZip(bytes).map(([name, data]) => [name, data]));
  const sheet = parts.get('xl/worksheets/sheet1.xml')!.toString('utf8');
  // The sheet keeps its prefix, with the edit and the new total in it. The workbook gains only
  // the request to calculate on opening, under the file's own prefix, and the styles are the
  // source's, byte for byte.
  expect(sheet).toMatch(/^<\?xml[^>]*\?><x:worksheet /);
  expect(sheet).toMatch(/<x:c r="B2"[^>]*><x:v>999<\/x:v><\/x:c>/);
  expect(sheet).toMatch(/<x:c r="B4"[^>]*><x:f>[^<]*<\/x:f><x:v>1299<\/x:v><\/x:c>/);
  const before = new Map(readZip(source).map(([name, data]) => [name, data.toString('utf8')]));
  expect(parts.get('xl/workbook.xml')!.toString('utf8')).toBe(
    before
      .get('xl/workbook.xml')!
      .replace('</x:workbook>', '<x:calcPr fullCalcOnLoad="1"/></x:workbook>'),
  );
  expect(parts.get('xl/styles.xml')!.toString('utf8')).toBe(before.get('xl/styles.xml'));
});

test('a newer version saved meanwhile offers Save a Copy, which keeps the changes beside it', async ({
  page,
}) => {
  const { url } = await launchUrl();
  await open(page, url);
  await opened(page, BUDGET);
  await mock('version', { workbook: 'budget', user: 'other' });
  await page.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 999 }));
  await page.keyboard.press('Control+s');
  const dialog = page.getByTestId('cloud-dialog');
  await expect(dialog).toContainText('A newer version was saved');
  await expect(page.getByTestId('cloud-dialog-text')).toContainText(
    /Omar Other.*saved a newer version of .*Budget 2027.* in Cash Gear Workspace on /,
  );
  expect((await book('Budget 2027')).versions).toBe(2);

  await page.getByTestId('cloud-save-copy').click();
  await expect
    .poll(() => badgeOf(page))
    .toMatch(/^Budget 2027 \(copy\) – Cash Gear Workspace · Saved \d/);
  const copy = await book('Budget 2027 (copy)');
  expect(copy.versions).toBe(1);
  expect(await savedCell(copy.id, 'B2')).toBe('999');
  await expect(page).toHaveTitle('Budget 2027 (copy) – CashGear Sheet');

  // The tab now edits the copy: its next save is a version of the copy.
  await page.evaluate(() => window.cgsTest.edit(0, 3, 2, { number: 7 }));
  await page.keyboard.press('Control+s');
  await expect.poll(async () => (await book('Budget 2027 (copy)')).versions).toBe(2);
  expect((await book('Budget 2027')).versions).toBe(2);
});

test('Reload discards the changes and opens the newer version', async ({ page }) => {
  const { url } = await launchUrl();
  await open(page, url);
  await opened(page, BUDGET);
  const theirs = workbook([
    {
      name: 'Budget',
      rows: [
        [
          1,
          [
            [1, 'Item'],
            [2, 'Amount'],
          ],
        ],
        [
          2,
          [
            [1, 'Rent'],
            [2, 5000],
          ],
        ],
      ],
    },
  ]);
  await mock('version', { workbook: 'budget', user: 'other', bytes: theirs.toString('base64') });
  await page.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 999 }));
  await page.keyboard.press('Control+s');
  await page.getByTestId('cloud-reload').click();
  await expect.poll(() => page.evaluate(() => window.cgsTest.cellText(2, 2))).toBe('5000');
  expect(await page.evaluate(() => window.cgsTest.hasChanges())).toBe(false);
  await expect(page.getByTestId('cloud-dialog')).toBeHidden();

  // Saving now goes from the newer version.
  await page.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 6000 }));
  await page.keyboard.press('Control+s');
  await expect.poll(async () => (await state()).saves.length).toBe(1);
  expect((await book('Budget 2027')).versions).toBe(3);
});

test('a viewer can change the workbook but only save a copy, and a refused copy offers the device', async ({
  page,
}) => {
  const { url } = await launchUrl('viewer');
  await open(page, url);
  await opened(page, `${BUDGET} · View only`);
  await page.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 999 }));
  await page.keyboard.press('Control+s');
  await expect(page.getByTestId('cloud-dialog')).toContainText('View only');
  await expect(page.getByTestId('cloud-dialog-text')).toContainText('but not save changes to it');
  await mock('copies', { allowed: false });
  await page.getByTestId('cloud-save-copy').click();
  await expect(page.getByTestId('cloud-dialog')).toContainText('No copy was made');
  await expect(page.getByTestId('cloud-save-device')).toBeVisible();
  await page.getByTestId('cloud-cancel').click();
  expect((await state()).counts.saves + (await state()).counts.copies).toBe(0);

  await mock('copies', { allowed: true });
  await page.keyboard.press('Control+s');
  await page.getByTestId('cloud-save-copy').click();
  await expect
    .poll(() => badgeOf(page))
    .toMatch(/^Budget 2027 \(copy\) – Cash Gear Workspace · Saved \d/);
  expect((await book('Budget 2027')).versions).toBe(1);
});

test('a session that ends keeps the edits, and Reconnect takes the next launch’s session', async ({
  page,
  context,
}) => {
  const { url } = await launchUrl();
  const first = (await book('Budget 2027')).etag;
  await open(page, url);
  await opened(page, BUDGET);
  await page.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 999 }));
  // Thirty-one idle minutes on Workspace's clock: the next request finds the session over.
  await mock('clock', { advanceMs: 31 * 60_000 });
  await page.keyboard.press('Control+s');
  await expect(page.getByTestId('cloud-dialog')).toContainText(
    'Your Cash Gear Workspace session has ended',
  );
  await page.getByTestId('cloud-dialog-reconnect').click();
  await expect.poll(() => badgeOf(page)).toBe('Waiting for Cash Gear Workspace…');
  await expect(page.getByTestId('cloud-panel-text')).toContainText(
    'again from Cash Gear Workspace and choose Open in Cash Gear Sheet',
  );
  await page.getByTestId('cloud-panel-close').click();
  expect(await page.evaluate(() => window.cgsTest.cellText(2, 2))).toBe('999');

  // The next launch's tab hands its session over and opens nothing itself.
  const next = await launchUrl();
  const tab = await launchTab(context, next.url);
  await expect
    .poll(
      async () =>
        tab.isClosed() ||
        (await tab
          .getByTestId('cloud-panel-text')
          .textContent()
          .catch(() => '')) ===
          'This connection went to the sheet that was waiting for it. You can close this tab.',
      { timeout: 30_000 },
    )
    .toBe(true);
  await expect.poll(() => badgeOf(page)).toBe(BUDGET);

  // The waiting tab saves in that session, from the version it had.
  await page.keyboard.press('Control+s');
  await expect.poll(async () => (await state()).saves.length).toBe(1);
  expect((await state()).saves[0]!.ifMatch).toBe(first);
  // The tab that handed over left the session open; closing the tab that uses it ends it.
  if (!tab.isClosed()) await tab.close({ runBeforeUnload: true });
  const sessions = (await state()).sessions;
  expect(sessions.at(-1)!.ended).toBe(false);
  await page.close({ runBeforeUnload: true });
  await expect.poll(async () => (await state()).sessions.at(-1)!.ended).toBe(true);
});

test('closing the tab ends its session in Workspace', async ({ page }) => {
  const { url } = await launchUrl();
  await open(page, url);
  await opened(page, BUDGET);
  expect((await state()).sessions[0]!.ended).toBe(false);
  await page.close({ runBeforeUnload: true });
  await expect.poll(async () => (await state()).sessions[0]!.ended).toBe(true);
});

test('a used or expired launch link opens nothing, and says why', async ({ page }) => {
  const used = await launchUrl();
  await fetch(`${MOCK}/api/v1/sheet-sessions/redeem`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Origin: SHEET_ORIGIN },
    body: JSON.stringify({ ticket: used.ticket }),
  });
  await open(page, used.url);
  const refused =
    'This link has expired or was already used. Go back to Cash Gear Workspace and choose Open in Cash Gear Sheet again.';
  await expect(page.getByTestId('cloud-panel-text')).toHaveText(refused, { timeout: 60_000 });
  expect(await badgeOf(page)).toBe('Cash Gear Workspace: nothing opened');
  expect(page.url()).toBe(`${SHEET_ORIGIN}/`);
  await page.getByTestId('cloud-panel-close').click();
  await expect(page.getByTestId('cloud-panel')).toBeHidden();

  const expired = await launchUrl();
  await mock('clock', { advanceMs: 121_000 });
  await page.goto(expired.url);
  await expect(page.getByTestId('cloud-panel-text')).toHaveText(refused, { timeout: 60_000 });
});

test('a new Workspace workbook opens blank, and its first save makes its first version', async ({
  page,
}) => {
  const { url } = await launchUrl('editor', 'empty');
  await open(page, url);
  await opened(page, 'New workbook – Cash Gear Workspace');
  expect(await page.evaluate(() => window.cgsTest.cellText(1, 1))).toBe('');
  await page.evaluate(() => window.cgsTest.edit(0, 1, 1, { text: 'Hello' }));
  await page.keyboard.press('Control+s');
  await expect.poll(async () => (await state()).saves.length).toBe(1);
  expect((await state()).saves[0]).toMatchObject({ ifMatch: null, ifNoneMatch: '*' });
  const created = await book('New workbook');
  expect(created.versions).toBe(1);
  expect(await savedCell(created.id, 'A1')).toBe('Hello');
});

test('an Arabic session words the badge and dialogs in Arabic, right to left', async ({ page }) => {
  const { url } = await launchUrl('arabic');
  await open(page, url);
  await opened(page, 'Budget 2027 – Cash Gear Workspace');
  await expect(page.getByTestId('cloud-status')).toHaveAttribute('dir', 'rtl');
  await mock('version', { workbook: 'budget', user: 'other' });
  await page.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 999 }));
  await page.keyboard.press('Control+s');
  await expect(page.getByTestId('cloud-dialog')).toContainText('حُفظ إصدار أحدث');
  const text = page.getByTestId('cloud-dialog-text');
  await expect(text).toContainText('Omar Other');
  await expect(text.locator('..')).toHaveAttribute('dir', 'rtl');
  await expect(page.getByTestId('cloud-save-copy')).toHaveText('حفظ نسخة');
  await page.getByTestId('cloud-save-copy').click();
  await expect
    .poll(() => badgeOf(page))
    .toMatch(/^Budget 2027 \(نسخة\) – Cash Gear Workspace · حُفظ/);
});
