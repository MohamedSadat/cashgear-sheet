import { execFileSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { chromium, expect, test, type BrowserContext, type Page } from '@playwright/test';
import { defaultStyledBook } from '../fixtures/workbooks.mjs';
import { openBytes, waitForGrid } from './helpers.mjs';

// Phase 9: the web app installs as an app, starts offline once it has loaded online, and works
// there: the engine starts with shared memory, a workbook opens, and formulas calculate. A browser
// only installs apps in a profile of its own (Playwright's are incognito), so the install test
// makes one, and installs through the browser's own app commands. Every other spec runs with
// service workers blocked (playwright.config.ts).

const base = 'http://127.0.0.1:4173';

async function ready(page: Page) {
  await expect(page.getByTestId('engine-status')).toHaveText('Ready', { timeout: 60_000 });
  expect(await page.evaluate(() => crossOriginIsolated)).toBe(true);
}

/** Runs Windows PowerShell 5.1 with its own module path (see scripts/lib.mjs). */
function powershell(script: string): string {
  const env = { ...process.env };
  for (const name of Object.keys(env)) if (name.toLowerCase() === 'psmodulepath') delete env[name];
  return execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {
    encoding: 'utf8',
    env,
  }).trim();
}

/**
 * The apps Windows lists for an install from this profile, which outlive the profile unless they
 * are uninstalled: Chrome adds an entry under the Uninstall key, and Edge an MSIX package.
 */
function windowsApps(profile: string, channel: string | undefined, remove = false): string[] {
  if (process.platform !== 'win32') return [];
  const name = basename(profile);
  const key = `'HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall'`;
  // A fresh user, such as CI's, may have no such key.
  const entries =
    `if (Test-Path ${key}) { Get-ChildItem ${key} | ` +
    `Where-Object { $_.GetValue('UninstallString') -like '*${name}*' } | ` +
    (remove ? 'Remove-Item -Recurse };' : 'ForEach-Object { $_.PSChildName } };');
  const packages =
    `Get-AppxPackage -Name '127.0.0.1-*' | ` +
    `Where-Object { (Get-AppxPackageManifest $_).Package.Applications.Application.Parameters -like '*${name}*' } | ` +
    (remove ? 'Remove-AppxPackage;' : 'ForEach-Object { $_.PackageFullName };');
  return powershell(entries + (channel === 'msedge' ? packages : ''))
    .split(/\r?\n/)
    .filter(Boolean);
}

// In a hook, which runs after a timeout too, when the test's own code has stopped.
let cleanUp: (() => Promise<void>) | undefined;
test.afterEach(async () => {
  await cleanUp?.();
  cleanUp = undefined;
});

test('the web app installs, and the installed app works offline', async () => {
  const testInfo = test.info();
  test.setTimeout(180_000);
  const { channel } = testInfo.project.use;
  const profile = await mkdtemp(join(tmpdir(), 'cgs-pwa-'));
  const launch = () =>
    chromium.launchPersistentContext(profile, {
      channel,
      viewport: { width: 1280, height: 800 },
      // The config blocks service workers for every other test.
      serviceWorkers: 'allow',
    });
  const manifestId = base + '/';
  let running: BrowserContext | undefined;
  let installing = false;
  cleanUp = async () => {
    try {
      if (installing) {
        // A browser that has crashed can't uninstall; one started on the profile can.
        if (!running?.browser()?.isConnected()) running = await launch();
        const session = await running.browser()!.newBrowserCDPSession();
        await session.send('PWA.uninstall', { manifestId });
        await expect.poll(() => windowsApps(profile, channel), { timeout: 30_000 }).toEqual([]);
      }
    } finally {
      await running?.close();
      // Edge started again after a crash keeps its package through PWA.uninstall.
      if (installing) windowsApps(profile, channel, true);
      await rm(profile, { recursive: true, force: true, maxRetries: 20, retryDelay: 500 });
    }
  };
  const context = (running = await launch());
  const browser = await context.browser()!.newBrowserCDPSession();
  const page = context.pages()[0] ?? (await context.newPage());
  await page.goto(base + '/');
  await ready(page);
  await page.waitForFunction(() => !!navigator.serviceWorker.controller, undefined, {
    timeout: 60_000,
  });
  const cdp = await context.newCDPSession(page);
  const { installabilityErrors } = await cdp.send('Page.getInstallabilityErrors');
  expect(installabilityErrors).toEqual([]);
  const manifest = await cdp.send('Page.getAppManifest');
  expect(manifest.errors).toEqual([]);
  expect(JSON.parse(manifest.data!)).toMatchObject({
    name: 'CashGear Sheet',
    display: 'standalone',
  });

  // The app shell is cached once the worker has installed: the page, the engine and the font.
  const cached = await page.evaluate(async () => {
    const shell = (await caches.keys()).find((name) => name.startsWith('cgs-shell-'));
    return shell
      ? (await (await caches.open(shell)).keys()).map((r) => new URL(r.url).pathname)
      : [];
  });
  expect(cached).toContain('/');
  expect(cached.some((path) => /\/assets\/cgs_wasm_bg-.*\.wasm$/.test(path))).toBe(true);
  expect(cached.some((path) => /\/assets\/engine-worker-.*\.js$/.test(path))).toBe(true);
  expect(cached.some((path) => /\/assets\/tile-worker-.*\.js$/.test(path))).toBe(true);
  expect(cached.some((path) => /carlito-latin-400-normal-.*\.woff2$/.test(path))).toBe(true);

  // Install it as the browser's install button does, and start it, offline.
  installing = true;
  await browser.send('PWA.install', { manifestId, installUrlOrBundleUrl: base + '/' });
  // The system knows the installed app (this fails for one that is not installed).
  await browser.send('PWA.getOsAppState', { manifestId });
  await browser.send('PWA.changeAppUserSettings', { manifestId, displayMode: 'standalone' });
  await context.setOffline(true);
  let app: Page;
  if (channel === 'msedge') {
    // Edge starts installed apps through a host of its own, out of the test's reach: the app's
    // page, offline in the same profile, stands in for it.
    app = await context.newPage();
    await app.goto(base + '/');
  } else {
    const opened = context.waitForEvent('page');
    await browser.send('PWA.launch', { manifestId, url: base + '/' });
    app = await opened;
    await app.waitForLoadState();
    expect(await app.evaluate(() => matchMedia('(display-mode: standalone)').matches)).toBe(true);
  }
  await ready(app);
  expect(await app.evaluate(() => navigator.onLine)).toBe(false);

  // A workbook opens, draws and calculates offline.
  await openBytes(app, 'Offline.xlsx', defaultStyledBook());
  await app.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', undefined, {
    timeout: 60_000,
  });
  await waitForGrid(app);
  expect(await app.evaluate(() => window.cgsTest.cellText(1, 1))).toBe('Item 1');
  await app.evaluate(() => window.cgsTest.edit(0, 1, 2, { number: 10 }));
  await app.evaluate(() => window.cgsTest.settled());
  // C1 is =B1*2.
  await expect.poll(() => app.evaluate(() => window.cgsTest.cellText(1, 3))).toBe('20');
});

test('About lists the third-party software and its licences', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('menuitem', { name: 'File' }).click();
  await page.getByRole('menuitem', { name: 'About CashGear Sheet' }).click();
  await expect(page.getByTestId('about-version')).toContainText('CashGear Sheet 1.');
  await page.getByTestId('about-show-notices').click();
  const notices = page.getByTestId('about-notices');
  await expect(notices).toContainText('Rust crates (');
  await expect(notices).toContainText('formualizer-eval');
  await expect(notices).toContainText('react ');
  await expect(notices).toContainText('Carlito: OFL-1.1');
  await expect(notices).toContainText('@fluentui/svg-icons');
  await expect(notices).toContainText('Apache License');
  await page.getByTestId('about-close').click();
  await expect(page.getByTestId('about-dialog')).toHaveCount(0);
});
