// Cash Gear Workspace's embed against the sheet's real `/embed` (Track S3 to S5): a parent page on
// Workspace's test origin (tests/embed-parent on 127.0.0.1:4273) frames the preview as Workspace
// does, sandboxed and with no COEP, and reads its workbooks by signed URL from the mock Workspace's
// Blob Storage (:4281). It runs the whole handshake, every message both ways, the errors, and the
// wrong-origin, wrong-version and malformed cases. Every message the embed sends is checked with
// the contract's validator.
import { expect, test, type Frame, type Page } from '@playwright/test';
import { PNG } from 'pngjs';
import { formattingBook, manyRowsBook, rightToLeftCopy, workbook } from '../fixtures/workbooks.mjs';
import { CONTRACT, parseEmbedMessage } from '../../packages/ui/src/embed/contract';
import { FONT_OVERRIDE } from './helpers.mjs';

const MOCK = 'http://127.0.0.1:4280';
const SHEET = 'http://127.0.0.1:4173';
const PARENT = 'http://127.0.0.1:4273';
const OTHER = 'http://127.0.0.1:4274';

type Message = Record<string, unknown> & { type: string };

async function mock(route: string, body: unknown = {}) {
  const response = await fetch(`${MOCK}/__mock/${route}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  expect(response.status).toBeLessThan(300);
  return response.status === 204 ? null : ((await response.json()) as Record<string, unknown>);
}

const blobReads = async () =>
  ((await (await fetch(`${MOCK}/__mock/state`)).json()) as { counts: { reads: number } }).counts
    .reads;

/** Puts a workbook in the mock as the budget's next version, and gives its signed URL. */
async function hosted(bytes: Uint8Array) {
  await mock('version', { workbook: 'budget', bytes: Buffer.from(bytes).toString('base64') });
  const content = (await mock('content', { workbook: 'budget' }))!;
  return { kind: 'url', url: content.readUrl as string, etag: content.etag as string };
}

const initOf = (source: unknown, fields: Record<string, unknown> = {}) => ({
  type: 'init',
  contract: CONTRACT,
  source,
  view: { kind: 'range', sheet: null, range: null },
  theme: 'light',
  direction: 'ltr',
  locale: 'en',
  ...fields,
});

/** Bytes kept as base64 by the parent, back as an `ArrayBuffer`. */
function revive(data: unknown): unknown {
  if (Array.isArray(data)) return data.map(revive);
  if (data && typeof data === 'object') {
    const fields = data as Record<string, unknown>;
    if (typeof fields.$bytes === 'string') {
      const buffer = Buffer.from(fields.$bytes, 'base64');
      return buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);
    }
    return Object.fromEntries(Object.entries(fields).map(([key, value]) => [key, revive(value)]));
  }
  return data;
}

const messages = async (page: Page) =>
  (await page.evaluate(() => window.cgsParent.state.messages)) as Message[];

/** The port's messages once there are `count`, each passing the contract's validator. */
async function answers(page: Page, count: number): Promise<Message[]> {
  await expect
    .poll(async () => (await messages(page)).length, { timeout: 45_000 })
    .toBeGreaterThanOrEqual(count);
  const seen = await messages(page);
  for (const message of seen)
    expect(parseEmbedMessage(revive(message)), message.type).not.toBeNull();
  return seen;
}

const last = async (page: Page, count: number) => (await answers(page, count))[count - 1]!;

async function embedFrame(page: Page): Promise<Frame> {
  await expect
    .poll(() => page.frames().some((f) => f.url().startsWith(SHEET + '/embed')))
    .toBe(true);
  return page.frames().find((f) => f.url().startsWith(SHEET + '/embed'))!;
}

/** Opens the parent and frames the embed, then waits for `ready`. */
async function framed(page: Page, options: { width?: number; height?: number } = {}) {
  await page.goto(`${PARENT}/parent.html`);
  await page.evaluate(
    ({ sheet, width, height }) => window.cgsParent.open({ sheet, width, height }),
    { sheet: SHEET, ...options },
  );
  await expect
    .poll(() => page.evaluate(() => window.cgsParent.state.ready.length), { timeout: 30_000 })
    .toBe(1);
  return embedFrame(page);
}

/** Frames the embed and sends `init`, then gives the frame and the first answer. */
async function started(page: Page, init: unknown) {
  const frame = await framed(page);
  await page.evaluate((message) => window.cgsParent.init(message), init);
  return { frame, first: await last(page, 1) };
}

const send = (page: Page, message: unknown) =>
  page.evaluate((data) => window.cgsParent.send(data), message);

/** Waits for the range's tiles, then counts the canvas's pixels unlike its first. */
async function drawnPixels(frame: Frame) {
  await expect(frame.locator('canvas[data-ready="true"]')).toBeVisible({ timeout: 30_000 });
  return frame.evaluate(() => {
    const canvas = document.querySelector('canvas')!;
    const data = canvas.getContext('2d')!.getImageData(0, 0, canvas.width, canvas.height).data;
    let differ = 0;
    for (let at = 4; at < data.length; at += 4) {
      if (data[at] !== data[0] || data[at + 1] !== data[1] || data[at + 2] !== data[2]) differ++;
    }
    return {
      differ,
      first: [data[0], data[1], data[2]],
      width: canvas.width,
      height: canvas.height,
    };
  });
}

function decodePng(message: Message) {
  const bytes = Buffer.from((message.bytes as { $bytes: string }).$bytes, 'base64');
  expect([...bytes.subarray(0, 8)]).toEqual([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  expect(bytes.length).toBeLessThanOrEqual(2 * 1024 * 1024);
  return PNG.sync.read(bytes);
}

/** The colour of a PNG's pixel. */
const pixel = (png: PNG, x: number, y: number) => {
  const at = (png.width * y + x) * 4;
  return [png.data[at]!, png.data[at + 1]!, png.data[at + 2]!];
};
const isLight = ([r, g, b]: number[]) => r! > 200 && g! > 200 && b! > 200;

// Headless Chrome hides scrollbars unless told not to; Workspace's readers have them, and a
// frame narrower than its range makes room for one (resize).
test.use({ launchOptions: { ignoreDefaultArgs: ['--hide-scrollbars'] } });

test.beforeEach(async ({ page }) => {
  await mock('reset');
  await mock('fault', { clear: true });
  await page.addInitScript(FONT_OVERRIDE);
});

test('the handshake: ready to any origin, init from Workspace with a port, then loaded', async ({
  page,
}) => {
  const source = await hosted(formattingBook());
  const frame = await framed(page);
  const ready = await page.evaluate(() => window.cgsParent.state.ready);
  expect(ready).toEqual([{ origin: SHEET, data: { type: 'ready', contracts: [CONTRACT] } }]);
  expect(parseEmbedMessage(ready[0]!.data)).not.toBeNull();
  // As in Workspace's spike S9: a sandboxed frame from a parent with no COEP isn't isolated, the
  // sheet runs without shared memory, and no referrer reached it.
  expect(await frame.evaluate(() => [crossOriginIsolated, document.referrer, origin])).toEqual([
    false,
    '',
    SHEET,
  ]);

  await page.evaluate((message) => window.cgsParent.init(message), initOf(source));
  const loaded = await last(page, 1);
  expect(loaded).toMatchObject({
    type: 'loaded',
    etag: source.etag,
    view: { kind: 'range', sheet: 'Formats', range: expect.stringMatching(/^A1:[A-Z]+\d+$/) },
    sheets: [
      { name: 'Formats', direction: 'ltr' },
      { name: 'Second', direction: 'ltr' },
    ],
    // A chart with one series and no title shows the series' name as its title, as in Excel.
    charts: [{ id: 'Sales chart', title: 'Series1', sheet: 'Formats' }],
    capped: false,
  });
  const size = loaded.size as { width: number; height: number };
  expect(size.width).toBeGreaterThan(100);
  expect(size.height).toBeGreaterThan(100);
  const drawn = await drawnPixels(frame);
  expect(drawn.differ).toBeGreaterThan(500);
  // The cells' text is there for screen readers; the canvas is only a picture.
  await expect(frame.locator('.cgs-embed-cells caption')).toHaveText(/^Cells of Formats, A1:/);
  await expect(frame.locator('.cgs-embed-cells td').first()).toHaveText('Header one');
  // Read in memory only: nothing was written to the device.
  expect(
    await frame.evaluate(async () => {
      const names: string[] = [];
      for await (const name of (await navigator.storage.getDirectory()).keys()) names.push(name);
      return names;
    }),
  ).toEqual([]);
});

test('setView draws another range or a chart, and says when one isn’t there', async ({ page }) => {
  const source = await hosted(formattingBook());
  const { frame } = await started(page, initOf(source));
  await send(page, { type: 'setView', view: { kind: 'range', sheet: 'second', range: 'B2:A1' } });
  expect(await last(page, 2)).toMatchObject({
    type: 'loaded',
    view: { kind: 'range', sheet: 'Second', range: 'A1:B2' },
    size: { width: 128, height: 40 },
    capped: false,
  });
  expect((await drawnPixels(frame)).width).toBeGreaterThan(0);

  await send(page, {
    type: 'setView',
    view: { kind: 'chart', sheet: null, chartId: 'Sales chart' },
  });
  const chart = await last(page, 3);
  expect(chart).toMatchObject({
    type: 'loaded',
    view: { kind: 'chart', sheet: 'Formats', chartId: 'Sales chart' },
  });
  await expect(frame.getByTestId('embed-chart').locator('svg').first()).toBeVisible();
  await expect(frame.locator('canvas')).toBeHidden();

  for (const view of [
    { kind: 'range', sheet: 'Hidden', range: 'A1' },
    { kind: 'range', sheet: 'Gone', range: null },
    { kind: 'chart', sheet: 'Second', chartId: 'Sales chart' },
  ]) {
    const count = (await messages(page)).length + 1;
    await send(page, { type: 'setView', view });
    expect(await last(page, count)).toMatchObject({ type: 'error', code: 'view.notFound' });
  }
  await expect(frame.getByTestId('embed-chart')).toBeHidden();
  // Nothing is drawn, so a picture answers the same error, with its id.
  await send(page, {
    type: 'requestPreview',
    id: 'after-error',
    maxWidth: 400,
    maxHeight: 300,
    scale: 1,
  });
  expect(await last(page, 7)).toMatchObject({
    type: 'error',
    code: 'view.notFound',
    id: 'after-error',
  });

  await send(page, {
    type: 'setView',
    view: { kind: 'range', sheet: 'Formats', range: 'A1:Z200' },
  });
  expect(await last(page, 8)).toMatchObject({ type: 'error', code: 'range.tooLarge' });
});

test('a used range over 5,000 cells is cut and says so', async ({ page }) => {
  const source = await hosted(manyRowsBook(2000, 6));
  const { first } = await started(page, initOf(source));
  expect(first).toMatchObject({
    type: 'loaded',
    view: { kind: 'range', sheet: 'Rows', range: 'A1:F833' },
    capped: true,
  });
});

test('setTheme draws again in dark, right to left and Arabic, with no answer', async ({ page }) => {
  const source = await hosted(formattingBook());
  const { frame } = await started(
    page,
    initOf(source, { view: { kind: 'range', sheet: 'Second', range: 'A1:C5' } }),
  );
  const light = await drawnPixels(frame);
  expect(isLight(light.first)).toBe(true);
  await send(page, { type: 'setTheme', theme: 'dark', direction: 'rtl', locale: 'ar' });
  await expect
    .poll(() =>
      frame.evaluate(() => [
        document.documentElement.dataset.cgTheme,
        document.dir,
        document.documentElement.lang,
      ]),
    )
    .toEqual(['dark', 'rtl', 'ar']);
  await expect.poll(async () => isLight((await drawnPixels(frame)).first)).toBe(false);
  await expect(frame.locator('.cgs-embed-cells caption')).toHaveText(/^خلايا Second/);
  // setTheme has no answer.
  await page.waitForTimeout(300);
  expect(await messages(page)).toHaveLength(1);
});

test('refresh with the version drawn reads nothing; a new version draws again', async ({
  page,
}) => {
  const source = await hosted(formattingBook());
  await started(page, initOf(source));
  const reads = await blobReads();
  await send(page, { type: 'refresh', source });
  expect(await last(page, 2)).toMatchObject({ type: 'loaded', etag: source.etag });
  expect(await blobReads()).toBe(reads);

  const next = await hosted(workbook([{ name: 'Formats', rows: [[1, [[1, 'New version']]]] }]));
  expect(next.etag).not.toBe(source.etag);
  await send(page, { type: 'refresh', source: next });
  expect(await last(page, 3)).toMatchObject({
    type: 'loaded',
    etag: next.etag,
    // Its text runs on into B1, which the used range takes too.
    view: { kind: 'range', sheet: 'Formats', range: 'A1:B1' },
    sheets: [{ name: 'Formats', direction: 'ltr' }],
    charts: [],
  });
  expect(await blobReads()).toBe(reads + 1);
});

test('requestPreview answers a PNG of the view as drawn, in the file’s own colours', async ({
  page,
}) => {
  const source = await hosted(formattingBook());
  await started(page, initOf(source, { theme: 'dark' }));
  await send(page, { type: 'setView', view: { kind: 'range', sheet: 'Formats', range: 'A1:F12' } });
  const loaded = await last(page, 2);
  const size = loaded.size as { width: number; height: number };
  await send(page, {
    type: 'requestPreview',
    id: 'range-1',
    maxWidth: 200,
    maxHeight: 2000,
    scale: 2,
  });
  const preview = await last(page, 3);
  expect(preview).toMatchObject({
    type: 'preview',
    id: 'range-1',
    etag: source.etag,
    view: loaded.view,
    width: 200,
    height: size.height,
  });
  const png = decodePng(preview);
  expect(png.width).toBe(400);
  expect(png.height).toBe(size.height * 2);
  // Dark mode is for the live view only.
  expect(isLight(pixel(png, png.width - 2, png.height - 2))).toBe(true);

  await send(page, {
    type: 'setView',
    view: { kind: 'chart', sheet: 'Formats', chartId: 'Sales chart' },
  });
  const chart = await last(page, 4);
  await send(page, {
    type: 'requestPreview',
    id: 'chart_1',
    maxWidth: 4096,
    maxHeight: 4096,
    scale: 1.5,
  });
  const picture = await last(page, 5);
  const chartSize = chart.size as { width: number; height: number };
  expect(picture).toMatchObject({
    type: 'preview',
    id: 'chart_1',
    view: { kind: 'chart', sheet: 'Formats', chartId: 'Sales chart' },
    width: chartSize.width,
    height: chartSize.height,
  });
  const chartPng = decodePng(picture);
  expect(chartPng.width).toBe(Math.round(chartSize.width * 1.5));
  expect(isLight(pixel(chartPng, 2, 2))).toBe(true);
});

test('a right-to-left sheet draws mirrored, column A on the right', async ({ page }) => {
  const source = await hosted(rightToLeftCopy(formattingBook()));
  await started(
    page,
    initOf(source, { view: { kind: 'range', sheet: 'Formats', range: 'A1:F1' } }),
  );
  const loaded = await last(page, 1);
  expect(loaded.sheets).toContainEqual({ name: 'Formats', direction: 'rtl' });
  await send(page, { type: 'requestPreview', id: 'rtl', maxWidth: 1000, maxHeight: 100, scale: 1 });
  const png = decodePng(await last(page, 2));
  // A1:E1 are filled and F1 isn't: F is at the left.
  expect(isLight(pixel(png, 4, Math.floor(png.height / 2)))).toBe(true);
  expect(isLight(pixel(png, png.width - 4, Math.floor(png.height / 2)))).toBe(false);
});

test('the source’s errors: forbidden, unavailable, too large, not a workbook', async ({ page }) => {
  const source = await hosted(formattingBook());
  // The signed URL expired: Blob Storage answers 403.
  await mock('clock', { advanceMs: 6 * 60_000 });
  const { frame } = await started(page, initOf(source));
  expect(await last(page, 1)).toMatchObject({ type: 'error', code: 'source.forbidden' });
  expect(JSON.stringify(await messages(page))).not.toContain('sig=');
  // Workspace asks for a new URL and refreshes.
  const fresh = (await mock('content', { workbook: 'budget' }))!;
  await send(page, {
    type: 'refresh',
    source: { kind: 'url', url: fresh.readUrl, etag: fresh.etag },
  });
  expect(await last(page, 2)).toMatchObject({ type: 'loaded', etag: fresh.etag });

  await mock('fault', { match: '^blobs/', status: 404 });
  await send(page, { type: 'refresh', source: { ...source, etag: '"another"' } });
  expect(await last(page, 3)).toMatchObject({ type: 'error', code: 'source.unavailable' });
  await expect(frame.locator('canvas')).toBeHidden();

  await mock('fault', { match: '^blobs/', status: 503 });
  await send(page, { type: 'refresh', source: { ...source, etag: '"again"' } });
  expect(await last(page, 4)).toMatchObject({ type: 'error', code: 'source.unavailable' });

  await send(page, {
    type: 'refresh',
    source: {
      kind: 'bytes',
      bytes: { $bytes: Buffer.from('not a workbook').toString('base64') },
      etag: '"text"',
    },
  });
  expect(await last(page, 5)).toMatchObject({ type: 'error', code: 'source.unavailable' });

  await send(page, {
    type: 'refresh',
    source: { kind: 'bytes', bytes: { $zeros: 20 * 1024 * 1024 + 1 }, etag: '"big"' },
  });
  expect(await last(page, 6)).toMatchObject({ type: 'error', code: 'workbook.tooLarge' });

  // The bytes themselves, transferred, as a desktop with them would send.
  await send(page, {
    type: 'refresh',
    source: {
      kind: 'bytes',
      bytes: { $bytes: Buffer.from(formattingBook()).toString('base64') },
      etag: '"bytes"',
    },
  });
  expect(await last(page, 7)).toMatchObject({ type: 'loaded', etag: '"bytes"' });
});

test('an init of another contract is answered contract.unsupported, and nothing else happens', async ({
  page,
}) => {
  const source = await hosted(formattingBook());
  await framed(page);
  await page.evaluate(
    (message) => window.cgsParent.init(message),
    initOf(source, { contract: 'sheet-embed.2' }),
  );
  expect(await last(page, 1)).toMatchObject({ type: 'error', code: 'contract.unsupported' });
  await send(page, { type: 'setView', view: { kind: 'range', sheet: null, range: 'A1' } });
  await page.evaluate((message) => window.cgsParent.init(message), initOf(source));
  await page.waitForTimeout(1500);
  expect(await messages(page)).toHaveLength(1);
});

test('init is taken from the parent alone, once, and malformed messages are dropped unlogged', async ({
  page,
}) => {
  const logged: string[] = [];
  page.on('console', (message) => logged.push(message.text()));
  const source = await hosted(formattingBook());
  const frame = await framed(page);
  // Without a port, aimed at another origin, or from a frame beside it: nothing.
  await page.evaluate((message) => window.cgsParent.post(message, 0), initOf(source));
  await page.evaluate(
    (message) => window.cgsParent.init(message, { origin: 'http://127.0.0.1:4999' }),
    initOf(source),
  );
  await page.evaluate((url) => window.cgsParent.openOther(url), `${OTHER}/rogue.html`);
  const rogue = page.frames().find((f) => f.url().startsWith(OTHER))!;
  await rogue.evaluate(
    ([message, sheet]) =>
      (window as unknown as { rogue: { tryInit: (m: unknown, s: string) => void } }).rogue.tryInit(
        message,
        sheet as string,
      ),
    [initOf(source), SHEET] as const,
  );
  // An init that fails validation doesn't count.
  await page.evaluate(
    (message) => window.cgsParent.init(message),
    initOf(source, { locale: 'SECRET-MARKER-1' }),
  );
  await page.waitForTimeout(1500);
  expect(await messages(page)).toHaveLength(0);
  expect(
    await rogue.evaluate(
      () => (window as unknown as { rogue: { answers: unknown[] } }).rogue.answers,
    ),
  ).toEqual([]);

  await page.evaluate((message) => window.cgsParent.init(message), initOf(source));
  expect(await last(page, 1)).toMatchObject({ type: 'loaded' });
  // A second init, on its own port, is ignored, and malformed messages on the port are dropped.
  await page.evaluate((message) => window.cgsParent.init(message, { spare: true }), initOf(source));
  for (const bad of [
    'SECRET-MARKER-2',
    { type: 'setView', view: { kind: 'range', sheet: 'SECRET-MARKER-3/', range: null } },
    { type: 'requestPreview', id: 'SECRET MARKER 4', maxWidth: 100, maxHeight: 100, scale: 1 },
    { type: 'save', what: 'SECRET-MARKER-5' },
    {
      type: 'refresh',
      source: { kind: 'url', url: 'http://example.com/SECRET-MARKER-6', etag: '"x"' },
    },
    initOf(source),
  ]) {
    await send(page, bad);
  }
  // The port still answers what is well formed, in order, after all that.
  await send(page, { type: 'setView', view: { kind: 'range', sheet: 'Second', range: 'A1' } });
  expect(await last(page, 2)).toMatchObject({
    type: 'loaded',
    view: { range: 'A1', sheet: 'Second' },
  });
  await page.waitForTimeout(500);
  expect(await messages(page)).toHaveLength(2);
  expect(await page.evaluate(() => window.cgsParent.state.spare)).toEqual([]);
  expect(logged.filter((line) => line.includes('SECRET'))).toEqual([]);
  expect(await frame.evaluate(() => document.title)).toBe('CashGear Sheet');
});

test('only Workspace’s origins may frame /embed; the rest of the sheet is never framed', async ({
  page,
  request,
}) => {
  const embed = await request.get(`${SHEET}/embed`);
  expect(embed.headers()['content-security-policy']).toContain(
    'frame-ancestors https://workspace.cashgearerp.com tauri://localhost http://tauri.localhost http://127.0.0.1:5273 http://127.0.0.1:4273 http://localhost:5273 http://localhost:4273',
  );
  expect(embed.headers()['cross-origin-embedder-policy']).toBe('require-corp');
  expect(await embed.text()).toContain('/assets/embed');
  const app = await request.get(`${SHEET}/`);
  expect(app.headers()['content-security-policy']).toContain("frame-ancestors 'none'");

  // From an origin Workspace doesn't have, the frame never loads and never says ready.
  await page.goto(`${OTHER}/parent.html`);
  await page.evaluate((sheet) => window.cgsParent.open({ sheet }), SHEET);
  await page.waitForTimeout(2500);
  expect(await page.evaluate(() => window.cgsParent.state.ready)).toEqual([]);

  // From Workspace's, the app's own page is refused all the same.
  await page.goto(`${PARENT}/parent.html`);
  await page.evaluate((sheet) => window.cgsParent.open({ sheet, path: '/' }), SHEET);
  await page.waitForTimeout(2500);
  expect(await page.evaluate(() => window.cgsParent.state.ready)).toEqual([]);
});

test('resize says when a narrower frame needs room for its scrollbar', async ({ page }) => {
  const source = await hosted(formattingBook());
  await started(
    page,
    initOf(source, { view: { kind: 'range', sheet: 'Formats', range: 'A1:C5' } }),
  );
  const loaded = await last(page, 1);
  const size = loaded.size as { width: number; height: number };
  await page.evaluate(() => window.cgsParent.resize(300, 600));
  const resize = await last(page, 2);
  expect(resize.type).toBe('resize');
  const next = resize.size as { width: number; height: number };
  expect(next.width).toBe(size.width);
  expect(next.height).toBeGreaterThan(size.height);
});

test('/embed on its own says what it is for', async ({ page }) => {
  await page.goto(`${SHEET}/embed`);
  await expect(page.locator('.cgs-embed-alone')).toContainText('Cash Gear Workspace');
  await expect(page.getByRole('link', { name: 'Open Cash Gear Sheet' })).toHaveAttribute(
    'href',
    '/',
  );
});
