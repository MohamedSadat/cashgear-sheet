// Launches the release desktop build, attaches Playwright to its WebView2 over CDP
// and checks the same things the browser specs check: real workbooks read through the
// desktop's file protocol, the same golden images as the web in both themes, a CashGear
// control, cross-origin isolation and shared memory. It also checks the native command,
// the window's own theme, and that the choice survives a restart. Then Cash Gear Workspace's
// link (Track S6) against the mock of its routes: the app started for a link, a link while it
// runs, Reconnect across windows, End on close, and neither the ticket nor the token left on disk.
import { spawn, spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { chromium, expect } from '@playwright/test';
import { controlColor, expectGolden, FONT_OVERRIDE, waitForGrid } from '../tests/e2e/helpers.mjs';
import {
  arabicBook,
  defaultStyledBook,
  formattingBook,
  readZip,
  structuresBook,
} from '../tests/fixtures/workbooks.mjs';
import { startMockWorkspace } from '../tests/workspace-mock/server.mjs';
import { root, windowsPowerShell } from './lib.mjs';

const appVersion = JSON.parse(
  await readFile(join(root, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'),
).version;

if (process.platform !== 'win32') throw new Error('Desktop smoke currently targets Windows.');
const runtimeKeys = [
  'HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
  'HKLM\\SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
  'HKCU\\Software\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
];
const runtimeVersion = runtimeKeys
  .map(
    (key) =>
      spawnSync('reg', ['query', key, '/v', 'pv'], {
        encoding: 'utf8',
        windowsHide: true,
      }).stdout?.match(/REG_SZ\s+([\d.]+)/)?.[1],
  )
  .find((version) => version && version !== '0.0.0.0');
if (!runtimeVersion)
  throw new Error('WebView2 Runtime is missing. Run scripts/setup-webview2.ps1 first.');
console.log('WebView2 Runtime: ' + runtimeVersion);

const listener = createServer();
await new Promise((resolve) => listener.listen(0, '127.0.0.1', resolve));
const port = listener.address().port;
await new Promise((resolve) => listener.close(resolve));
const profilePrefix = join(tmpdir(), 'cgs-webview2-smoke-');
const profile = await mkdtemp(profilePrefix);
const browserArguments =
  '--remote-debugging-port=' + port + ' --remote-debugging-address=127.0.0.1';
// Elevated WebView2 hosts ignore environment browser arguments. Use the documented
// HKLM override only on disposable GitHub-hosted runners, scoped to our executable.
// https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security#for-an-elevated-host-app-use-appropriate-override-flags
const policyKey = 'HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge\\WebView2\\AdditionalBrowserArguments';
const policyName = 'cgs-desktop.exe';
const hostedRunner =
  process.env.GITHUB_ACTIONS === 'true' && process.env.RUNNER_ENVIRONMENT === 'github-hosted';
let previousPolicy;
function reg(args) {
  return spawnSync('reg.exe', args, { encoding: 'utf8', windowsHide: true });
}
if (hostedRunner) {
  const existing = reg(['query', policyKey, '/v', policyName, '/reg:64']);
  if (existing.status === 0) {
    previousPolicy = existing.stdout.match(/REG_SZ\s+(.+)/)?.[1];
    if (previousPolicy === undefined) throw new Error('Unexpected existing WebView2 test policy');
  }
  const added = reg([
    'add',
    policyKey,
    '/v',
    policyName,
    '/t',
    'REG_SZ',
    '/d',
    browserArguments,
    '/f',
    '/reg:64',
  ]);
  if (added.status !== 0)
    throw new Error('Could not configure WebView2 test policy: ' + added.stderr);
}

/** What the app's CSP refused, from the console of every launch. The smoke fails on any. */
const policyViolations = [];

/** What every launch's process gets, plus `extra`. */
const appEnv = (extra = {}) => ({
  ...process.env,
  WEBVIEW2_USER_DATA_FOLDER: profile,
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: browserArguments,
  CGS_DESKTOP_SMOKE: profile,
  ...extra,
});
const exe = join(root, 'target/release/cgs-desktop.exe');

/**
 * Starts the desktop app and attaches to its WebView2. Launches share the profile. `args` are
 * what Explorer passes: a double-clicked workbook's path, or a link the system starts it for.
 */
async function launch(args = [], extraEnv = {}) {
  const child = spawn(exe, args, {
    cwd: root,
    windowsHide: true,
    env: appEnv(extraEnv),
    stdio: 'inherit',
  });
  let launchError;
  child.on('error', (error) => {
    launchError = error;
  });
  const endpoint = 'http://127.0.0.1:' + port;
  const deadline = Date.now() + 45000;
  while (true) {
    if (launchError) throw launchError;
    if (child.exitCode !== null)
      throw new Error('Desktop exited before debugging was ready: ' + child.exitCode);
    try {
      const response = await fetch(endpoint + '/json/version', {
        signal: AbortSignal.timeout(1000),
      });
      if (response.ok) break;
    } catch {
      /* Wait for WebView2 to create its debugging endpoint. */
    }
    if (Date.now() > deadline) throw new Error('WebView2 debugging endpoint timed out');
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  // The endpoint can answer and then go, if the browser it belongs to is on its way out.
  let browser;
  while (!browser) {
    try {
      browser = await chromium.connectOverCDP(endpoint);
    } catch (error) {
      if (Date.now() > deadline) throw error;
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
  }
  const context = browser.contexts()[0];
  const page = context.pages()[0] ?? (await context.waitForEvent('page'));
  // Page errors show in the smoke's output, so a failure says what went wrong inside.
  page.on('pageerror', (error) => console.error('Page error: ' + error.message));
  page.on('console', (message) => {
    if (message.type() !== 'error') return;
    const { url, lineNumber } = message.location();
    console.error('Console error: ' + message.text() + (url ? ` (${url}:${lineNumber})` : ''));
    if (message.text().includes('Content Security Policy')) policyViolations.push(message.text());
  });
  return { child, browser, page };
}

/** Kills the app, unless it has gone already, and waits for WebView2 to let go of the profile. */
async function kill(app) {
  if (app.child.exitCode === null && app.child.signalCode === null) {
    const exited = new Promise((resolve) => app.child.once('exit', resolve));
    app.child.kill();
    await exited;
  }
  await app.browser.close().catch(() => {});
  const deadline = Date.now() + 20000;
  while (Date.now() < deadline) {
    try {
      await fetch('http://127.0.0.1:' + port + '/json/version', {
        signal: AbortSignal.timeout(500),
      });
    } catch (error) {
      // A timeout means it is only busy. Only a refused connection means it has gone.
      if (error.name !== 'TimeoutError') break;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}

/** The theme the window itself has, as Tauri reports it. */
const windowTheme = (page) =>
  page.evaluate(() => window.__TAURI_INTERNALS__.invoke('plugin:window|theme', { label: 'main' }));
const systemTheme = (page) =>
  page.evaluate(() => (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'));

async function expectWindowTheme(page, theme) {
  await expect.poll(() => windowTheme(page), { timeout: 10000 }).toBe(theme);
}

/** Opens a file on disk the way the dialog does, through the native side and its protocol. */
async function openPath(page, path) {
  await page.evaluate(async (path) => {
    const file = await window.cgsOpenPath(path);
    await window.cgsTest.open(file);
  }, path);
}

/**
 * Sends WM_CLOSE to this app's windows whose titles start with `prefix`, as their close button
 * does, and says how many there were.
 */
function closeWindows(pid, prefix) {
  const script = `
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CgsWindows {
  public delegate bool EnumProc(IntPtr hwnd, IntPtr data);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, IntPtr data);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
  public static int Close(uint pid, string prefix) {
    int closed = 0;
    EnumWindows(delegate (IntPtr hwnd, IntPtr data) {
      uint owner;
      GetWindowThreadProcessId(hwnd, out owner);
      if (owner != pid || !IsWindowVisible(hwnd)) return true;
      var text = new StringBuilder(512);
      GetWindowText(hwnd, text, 512);
      if (text.ToString().StartsWith(prefix)) {
        PostMessage(hwnd, 0x0010, IntPtr.Zero, IntPtr.Zero);
        closed++;
      }
      return true;
    }, IntPtr.Zero);
    return closed;
  }
}
"@
[CgsWindows]::Close(${pid}, '${prefix.replace(/'/g, "''")}')`;
  return Number(windowsPowerShell(script));
}

/** The files under `folder` that hold any of `secrets`, as UTF-8 or as UTF-16. */
async function filesHolding(folder, secrets) {
  const found = [];
  const needles = secrets.flatMap((secret) => [
    Buffer.from(secret, 'utf8'),
    Buffer.from(secret, 'utf16le'),
  ]);
  const walk = async (path) => {
    for (const entry of await readdir(path, { withFileTypes: true }).catch(() => [])) {
      const child = join(path, entry.name);
      if (entry.isDirectory()) await walk(child);
      else {
        const bytes = await readFile(child).catch(() => null);
        if (bytes && needles.some((needle) => bytes.includes(needle))) found.push(child);
      }
    }
  };
  await walk(folder);
  return found;
}

/**
 * Cash Gear Workspace's "Open in Cash Gear Sheet" on the desktop (Track S6), against the mock of
 * its routes in this process. The app runs as shipped, from its WebView's own origin, with the
 * mock's origins named by the test-only variables.
 */
async function workspaceLinks() {
  const workspace = await startMockWorkspace();
  const tokens = [];
  const origins = new Set();
  // The tokens the mock gives out, to look for afterwards, and the origins redeems came from.
  const redeem = workspace.mock.redeem.bind(workspace.mock);
  workspace.mock.redeem = (ctx) => {
    origins.add(ctx.origin);
    const reply = redeem(ctx);
    if (reply.status === 200) tokens.push(reply.body.token);
    return reply;
  };
  const extra = { CGS_WORKSPACE_API: workspace.url, CGS_WORKSPACE_BLOB: workspace.blobUrl };
  const mock = async (route, body = {}) => {
    const response = await fetch(`${workspace.url}/__mock/${route}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (response.status >= 300) throw new Error(`The mock refused ${route}: ${response.status}`);
    return response.status === 204 ? null : response.json();
  };
  const state = async () => (await fetch(`${workspace.url}/__mock/state`)).json();
  const tickets = [];
  const link = async (workbook) => {
    const answer = await mock('launch', { user: 'editor', workbook, target: 'desktop' });
    if (answer.url !== `cashgear-sheet://workspace/launch#ticket=${answer.ticket}`)
      throw new Error('The mock gave another link: ' + answer.url);
    tickets.push(answer.ticket);
    return answer.url;
  };
  // The badge puts isolates around names.
  const plain = (text) => (text ?? '').replace(/[⁦-⁩]/g, '');
  const badge = (page) =>
    page
      .getByTestId('cloud-badge')
      .textContent({ timeout: 1000 })
      .then(plain, () => '');
  const title = (page) =>
    page.evaluate(() => window.__TAURI_INTERNALS__.invoke('plugin:window|title', {}));
  const budget = 'Budget 2027 – Cash Gear Workspace';
  const empty = 'New workbook – Cash Gear Workspace';
  /** Starts the app for a link while it runs: that process hands the link over and exits. */
  const forward = async (text) => {
    const child = spawn(exe, [text], { cwd: root, windowsHide: true, env: appEnv(extra) });
    const code = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('A forwarding app did not exit')), 30000);
      child.once('exit', (exitCode) => {
        clearTimeout(timer);
        resolve(exitCode);
      });
    });
    if (code !== 0) throw new Error('A forwarding app exited with ' + code);
  };
  const found = {};
  let running;
  try {
    // A malformed link opens the app with nothing in it.
    running = await launch(['cashgear-sheet://workspace/launch#ticket=short'], extra);
    await expect(running.page.getByTestId('engine-status')).toHaveText('Ready', {
      timeout: 30000,
    });
    await running.page.waitForTimeout(1000);
    if (await badge(running.page)) throw new Error('A malformed link opened a workbook.');
    await kill(running);
    running = undefined;

    // Started for a link: the app redeems its ticket from its own origin and opens the workbook.
    const seedEtag = (await state()).workbooks.find((w) => w.name === 'Budget 2027').etag;
    running = await launch([await link('budget')], extra);
    const first = running.page;
    await expect.poll(() => badge(first), { timeout: 60000 }).toBe(budget);
    found.cold = {
      b4: await first.evaluate(() => window.cgsTest.cellText(4, 2)),
      crossOriginIsolated: await first.evaluate(() => crossOriginIsolated),
    };
    await expect.poll(() => title(first)).toBe('Budget 2027 – CashGear Sheet');
    found.cold.title = await title(first);
    if (found.cold.b4 !== '1500') throw new Error('The launched workbook shows ' + found.cold.b4);
    if (!found.cold.crossOriginIsolated) throw new Error('The window is not cross-origin isolated');
    if (await first.evaluate(() => window.__TAURI_INTERNALS__.invoke('workspace_launch', {})))
      throw new Error('The ticket was offered twice.');

    // Saving: a new version with If-Match on the version it read.
    await first.evaluate(() => window.cgsTest.edit(0, 2, 2, { number: 999 }));
    await first.keyboard.press('Control+s');
    await expect.poll(() => badge(first), { timeout: 30000 }).toMatch(/· Saved \d/);
    let { saves } = await state();
    if (saves.length !== 1 || saves[0].ifMatch !== seedEtag)
      throw new Error('The save was not made from the version read: ' + JSON.stringify(saves));
    const savedEtag = (await state()).workbooks.find((w) => w.name === 'Budget 2027').etag;

    // While it runs: another link opens a window of its own in the same app.
    const context = running.browser.contexts()[0];
    let opening = context.waitForEvent('page', { timeout: 60000 });
    await forward(await link('empty'));
    const second = await opening;
    await expect.poll(() => badge(second), { timeout: 60000 }).toBe(empty);
    await expect.poll(() => title(second)).toBe('New workbook – CashGear Sheet');
    found.whileRunning = { windows: context.pages().length, title: await title(second) };
    await second.evaluate(() => window.cgsTest.edit(0, 1, 1, { text: 'first' }));
    await second.keyboard.press('Control+s');
    await expect.poll(async () => (await state()).saves.length, { timeout: 30000 }).toBe(2);
    ({ saves } = await state());
    if (saves[1].ifNoneMatch !== '*')
      throw new Error('The first save of an empty workbook: ' + JSON.stringify(saves[1]));

    // A malformed link while it runs opens nothing.
    const before = context.pages().length;
    await forward('cashgear-sheet://workspace/launch#ticket=' + 'A'.repeat(42));
    await first.waitForTimeout(2000);
    if (context.pages().length !== before) throw new Error('A malformed link opened a window.');

    // Reconnect across windows: the session ends with edits unsaved, the window waits, and the
    // next link's window hands its session over and closes.
    await first.evaluate(() => window.cgsTest.edit(0, 3, 2, { number: 1 }));
    await mock('clock', { advanceMs: 31 * 60_000 });
    await first.keyboard.press('Control+s');
    await expect(first.getByTestId('cloud-dialog')).toContainText(
      'Your Cash Gear Workspace session has ended',
      { timeout: 30000 },
    );
    await first.getByTestId('cloud-dialog-reconnect').click();
    await expect.poll(() => badge(first)).toBe('Waiting for Cash Gear Workspace…');
    await first.getByTestId('cloud-panel-close').click();
    opening = context.waitForEvent('page', { timeout: 60000 });
    await forward(await link('budget'));
    const third = await opening;
    await expect.poll(() => third.isClosed(), { timeout: 60000 }).toBe(true);
    // Open again, still showing when it last saved.
    await expect
      .poll(() => badge(first), { timeout: 30000 })
      .toMatch(/^Budget 2027 – Cash Gear Workspace · Saved \d/);
    await first.keyboard.press('Control+s');
    await expect.poll(async () => (await state()).saves.length, { timeout: 30000 }).toBe(3);
    ({ saves } = await state());
    if (saves[2].ifMatch !== savedEtag)
      throw new Error('The reconnected save was not from the version it had.');
    found.reconnect = { handedOver: true, windows: context.pages().length };

    // Closing a window ends its session in Workspace first, and the app goes with its last.
    const open = (await state()).sessions.filter((s) => !s.ended).length;
    if (closeWindows(running.child.pid, 'Budget 2027') !== 1)
      throw new Error('The workbook’s window was not found by its title.');
    await expect
      .poll(async () => (await state()).sessions.filter((s) => !s.ended).length, {
        timeout: 15000,
      })
      .toBe(open - 1);
    found.endedOnClose = true;
    closeWindows(running.child.pid, 'New workbook');
    const exited = await Promise.race([
      new Promise((resolve) => running.child.once('exit', () => resolve(true))),
      new Promise((resolve) => setTimeout(() => resolve(false), 20000)),
    ]);
    if (!exited) throw new Error('The app did not exit with its last window.');
    await kill(running).catch(() => {});
    running = undefined;

    found.redeemOrigins = [...origins];
    if (found.redeemOrigins.join() !== 'http://tauri.localhost')
      throw new Error('Redeems came from ' + found.redeemOrigins.join(', '));
    // Neither a ticket nor a token anywhere in the app's data or WebView2's profile, nor in the
    // user's registry (exported once, which takes seconds where a search per secret takes a
    // minute or more).
    const secrets = [...tickets, ...tokens];
    if (tickets.length !== 3 || tokens.length !== 3)
      throw new Error(`Expected three tickets and tokens, not ${tickets.length}, ${tokens.length}`);
    const left = await filesHolding(profile, secrets);
    if (left.length) throw new Error('A ticket or token is on disk: ' + left.join(', '));
    const hive = join(fixtures, 'hkcu.reg');
    const exported = spawnSync('reg.exe', ['export', 'HKCU', hive, '/y'], { windowsHide: true });
    if (exported.status !== 0) throw new Error('Could not export the user’s registry.');
    const inRegistry = await filesHolding(fixtures, secrets);
    await rm(hive, { force: true });
    if (inRegistry.length) throw new Error('A ticket or token is in the user’s registry.');
    found.secretsLookedFor = { tickets: tickets.length, tokens: tokens.length, found: 0 };
    return found;
  } finally {
    if (running) await kill(running).catch(() => {});
    await workspace.close();
  }
}

const fixtures = await mkdtemp(join(tmpdir(), 'cgs-smoke-books-'));
const books = {
  formatting: join(fixtures, 'formatting.xlsx'),
  arabic: join(fixtures, 'arabic.xlsx'),
  defaults: join(fixtures, 'defaults.xlsx'),
};
await writeFile(books.formatting, formattingBook());
await writeFile(books.arabic, arabicBook());
await writeFile(books.defaults, defaultStyledBook());

let app;
try {
  app = await launch();
  let page = app.page;
  await expect(page.getByTestId('engine-status')).toHaveText('Ready', { timeout: 30000 });
  if (!(await page.evaluate(() => crossOriginIsolated)))
    throw new Error('Desktop is not cross-origin isolated');
  await expect(page.locator('.cgs-shell')).toHaveAttribute('data-shared-memory', 'true');
  await expect(page.getByTestId('host-info')).toHaveText(`Desktop ${appVersion}`);
  await expect(page.getByRole('radiogroup', { name: 'Theme' })).toBeVisible();
  await expect(page.getByRole('radio', { name: 'System' })).toBeChecked();
  // Playwright doesn't list workers that started before it attached, but the page
  // records the scripts it loaded. WebView2 never loads a worker started by a worker,
  // so the tiles drawn below also prove the tile worker started from the page.
  const scripts = await page.evaluate(() =>
    performance.getEntriesByType('resource').map((entry) => entry.name),
  );
  for (const script of ['engine-worker', 'tile-worker'])
    if (!scripts.some((url) => url.includes(script)))
      throw new Error(`The ${script} did not load: ${scripts.join(', ')}`);

  await mkdir(join(root, 'bench/results'), { recursive: true });
  const results = { runtimeVersion, browser: app.browser.version(), url: page.url() };

  // A fresh profile follows the system.
  const system = await systemTheme(page);
  await expectWindowTheme(page, system);
  results.system = system;

  // The same fonts as the web's golden images, whatever this machine has.
  await page.evaluate(FONT_OVERRIDE);
  await page.reload();
  await expect(page.getByTestId('engine-status')).toHaveText('Ready', { timeout: 30000 });

  // Real files, through the dialog's path: the native side and the read protocol.
  const controls = {};
  const goldens = {};
  for (const theme of ['light', 'dark']) {
    await page.getByRole('radio', { name: theme === 'light' ? 'Light' : 'Dark' }).click();
    await expectWindowTheme(page, theme);
    controls[theme] = await controlColor(page);
    for (const [name, path] of Object.entries(books)) {
      await openPath(page, path);
      await waitForGrid(page, theme);
      goldens[name + '-' + theme] = await expectGolden(page, name + '-' + theme);
      if (name === 'formatting') {
        await page.screenshot({ path: join(root, `bench/results/desktop-${theme}-page.png`) });
      }
    }
  }
  if (controls.light === controls.dark)
    throw new Error('The CashGear control did not change with the theme: ' + controls.light);
  results.controlColors = controls;
  results.goldenDifference = goldens;

  // A value read through the protocol, and the formula behind it.
  await openPath(page, books.formatting);
  await waitForGrid(page, 'dark');
  results.cellText = await page.evaluate(() => window.cgsTest.cellText(4, 2));
  if (results.cellText !== '12.50%') throw new Error('Cell B4 shows ' + results.cellText);

  // Saving through the native side: the bytes go by protocol, and the file changes in one step.
  const savePath = join(fixtures, 'saved.xlsx');
  const sourceBook = structuresBook();
  await writeFile(savePath, sourceBook);
  await openPath(page, savePath);
  await waitForGrid(page, 'dark');
  await page.evaluate(() => window.cgsTest.edit(0, 5, 1, { number: 100 }));
  const saveTo = (path) =>
    page.evaluate(async (path) => {
      await window.cgsTest.writeTo(await window.cgsSaveTarget(path));
      return window.cgsTest.problem();
    }, path);
  const savedProblem = await saveTo(savePath);
  if (savedProblem) throw new Error('The save failed: ' + savedProblem);
  const contents = async (path) =>
    new Map(readZip(await readFile(path)).map(([name, body]) => [name, body]));
  const before = new Map(readZip(sourceBook));
  let after = await contents(savePath);
  if (!after.get('xl/worksheets/sheet1.xml').toString().includes('<c r="A5"><v>100</v></c>'))
    throw new Error('The saved file does not hold the edit.');
  if (after.has('xl/calcChain.xml')) throw new Error('The calculation chain is still there.');
  if (!after.get('xl/tables/table1.xml').equals(before.get('xl/tables/table1.xml')))
    throw new Error('A part the edit did not touch changed.');
  const leftovers = (await readdir(fixtures)).filter((name) => name.includes('cgs-saving'));
  if (leftovers.length) throw new Error('A save left files behind: ' + leftovers.join(', '));

  // A second save reads from the file the first one wrote.
  await page.evaluate(() => window.cgsTest.edit(0, 30, 1, { text: 'second' }));
  const secondProblem = await saveTo(savePath);
  if (secondProblem) throw new Error('The second save failed: ' + secondProblem);
  after = await contents(savePath);
  const secondSheet = after.get('xl/worksheets/sheet1.xml').toString();
  if (!secondSheet.includes('<c r="A5"><v>100</v></c>') || !secondSheet.includes('<c r="A30"'))
    throw new Error('The second save lost something.');

  // Save As writes somewhere new, and the old file stays.
  const copyPath = join(fixtures, 'copy.xlsx');
  await page.evaluate(() => window.cgsTest.edit(0, 31, 1, { number: 1 }));
  const copyProblem = await saveTo(copyPath);
  if (copyProblem) throw new Error('Save As failed: ' + copyProblem);
  if ((await contents(savePath)).get('xl/worksheets/sheet1.xml').toString().includes('A31'))
    throw new Error('Save As changed the file it came from.');
  if (!(await contents(copyPath)).get('xl/worksheets/sheet1.xml').toString().includes('A31'))
    throw new Error('Save As did not write the edit.');

  // A file that changed before the first edit is refused, rather than read at old offsets.
  const changedPath = join(fixtures, 'changed.xlsx');
  await writeFile(changedPath, sourceBook);
  await openPath(page, changedPath);
  await waitForGrid(page, 'dark');
  await writeFile(changedPath, Buffer.concat([sourceBook, Buffer.from('more')]));
  const refused = await page.evaluate(() =>
    window.cgsTest.edit(0, 2, 1, { number: 9 }).then(
      () => 'edited',
      (error) => error.message ?? String(error),
    ),
  );
  if (!/changed since it was opened/.test(refused))
    throw new Error('A changed file was not refused: ' + refused);
  // And a working copy protects a file that changes after the first edit.
  await writeFile(changedPath, sourceBook);
  await openPath(page, changedPath);
  await waitForGrid(page, 'dark');
  await page.evaluate(() => window.cgsTest.edit(0, 2, 1, { number: 9 }));
  await writeFile(changedPath, Buffer.from('something else entirely'));
  const protectedProblem = await saveTo(changedPath);
  if (protectedProblem) throw new Error('The working copy did not help: ' + protectedProblem);
  const rescued = await contents(changedPath);
  if (!rescued.get('xl/worksheets/sheet1.xml').toString().includes('<c r="A2"><v>9</v></c>'))
    throw new Error('The rescued file does not hold the edit.');
  if (!rescued.get('xl/tables/table1.xml').equals(before.get('xl/tables/table1.xml')))
    throw new Error('The rescued file lost a part.');
  results.saving = 'overwrite, second save, save as, changed file refused, working copy used';

  // The file just opened is on the recent list, and the list survives a restart.
  await openPath(page, books.formatting);
  await waitForGrid(page, 'dark');
  const recent = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('recent_files', {}));
  if (!recent.some((file) => file.name === 'formatting.xlsx'))
    throw new Error('The recent list is missing a file: ' + JSON.stringify(recent));

  // The last choice, dark, survives a restart: the page and the window both start in it.
  await kill(app);
  app = await launch();
  page = app.page;
  await expect(page.getByTestId('engine-status')).toHaveText('Ready', { timeout: 30000 });
  await expect(page.getByRole('radio', { name: 'Dark' })).toBeChecked();
  await expectWindowTheme(page, 'dark');
  results.remembered = 'dark';
  const remembered = await page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke('recent_files', {}),
  );
  if (!remembered[0] || remembered[0].name !== 'formatting.xlsx')
    throw new Error('The recent list did not survive a restart: ' + JSON.stringify(remembered));
  await page.getByRole('menuitem', { name: 'File' }).click();
  await page.getByRole('menuitem', { name: 'Recent' }).click();
  await page.getByRole('menuitem', { name: 'formatting.xlsx' }).click();
  await waitForGrid(page, 'dark');
  results.reopenedFromRecent = await page.getByRole('heading', { level: 1 }).textContent();

  // Started with a workbook, as a double-click in Explorer starts it: it opens by itself, and a
  // reload doesn't open it again.
  await kill(app);
  app = await launch([books.defaults]);
  page = app.page;
  await waitForGrid(page, 'dark');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('defaults.xlsx');
  if ((await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('launch_file', {}))) !== null)
    throw new Error('The workbook the app started with was offered twice.');
  results.startedWithFile = 'defaults.xlsx';

  // Back to the system: the window lets go of its override too.
  await page.getByRole('radio', { name: 'System' }).click();
  await expect.poll(() => windowTheme(page), { timeout: 10000 }).toBe(system);
  results.backToSystem = system;

  await kill(app);
  app = undefined;
  results.workspace = await workspaceLinks();

  if (policyViolations.length)
    throw new Error('The CSP refused: ' + [...new Set(policyViolations)].join(' | '));
  await writeFile(
    join(root, 'bench/results/desktop-smoke.json'),
    JSON.stringify(results, null, 2) + '\n',
  );
  console.log(JSON.stringify(results, null, 2));
} finally {
  await app?.browser.close().catch((error) => {
    console.error('Could not close the desktop browser: ' + error.message);
    process.exitCode = 1;
  });
  app?.child.kill();
  if (hostedRunner) {
    const restored = reg(
      previousPolicy === undefined
        ? ['delete', policyKey, '/v', policyName, '/f', '/reg:64']
        : [
            'add',
            policyKey,
            '/v',
            policyName,
            '/t',
            'REG_SZ',
            '/d',
            previousPolicy,
            '/f',
            '/reg:64',
          ],
    );
    if (restored.status !== 0) {
      console.error('Could not restore WebView2 test policy: ' + restored.stderr);
      process.exitCode = 1;
    }
  }
  await rm(fixtures, { recursive: true, force: true }).catch(() => {});
  if (profile.startsWith(profilePrefix))
    await rm(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 }).catch(
      (error) => console.warn('Temporary folder cleanup: ' + error.message),
    );
}
