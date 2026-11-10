import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
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

/**
 * The ProgIDs the browser registered for `.ext` for the app installed from this profile, as
 * Explorer lists them in Open with. Chrome registers a launcher inside the profile, and Edge an
 * MSIX package whose ProgIDs name its package family.
 */
function openWithProgIds(profile: string, ext: string): string[] {
  if (process.platform !== 'win32') return [];
  const name = basename(profile);
  const script = `
$classes = 'HKCU:/Software/Classes'
$families = @(Get-AppxPackage -Name '127.0.0.1-*' |
  Where-Object { (Get-AppxPackageManifest $_).Package.Applications.Application.Parameters -like '*${name}*' } |
  ForEach-Object { $_.PackageFamilyName })
$list = Join-Path $classes '.${ext}/OpenWithProgids'
if (Test-Path $list) {
  foreach ($id in (Get-Item $list).GetValueNames()) {
    if (-not $id) { continue }
    $command = Join-Path $classes "$id/shell/open/command"
    $text = if (Test-Path $command) { [string](Get-Item $command).GetValue('') } else { '' }
    $application = Join-Path $classes "$id/Application"
    $model = if (Test-Path $application) { [string](Get-Item $application).GetValue('AppUserModelId') } else { '' }
    if ($text -like '*${name}*') { $id; continue }
    foreach ($family in $families) { if ($model -like "$family*") { $id } }
  }
}`;
  return powershell(script).split(/\r?\n/).filter(Boolean);
}

/**
 * Opens a file through a ProgID as Explorer's Open with does: ShellExecuteEx with the ProgID as
 * the class, so Windows reads how to open it from the registry and runs it.
 */
function shellOpen(file: string, progId: string) {
  const quote = (text: string) => text.replace(/'/g, "''");
  const result = powershell(`
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class CgsShell {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct Info {
    public int cbSize; public uint fMask; public IntPtr hwnd;
    public string lpVerb; public string lpFile; public string lpParameters; public string lpDirectory;
    public int nShow; public IntPtr hInstApp; public IntPtr lpIDList; public string lpClass;
    public IntPtr hkeyClass; public uint dwHotKey; public IntPtr hIcon; public IntPtr hProcess;
  }
  [DllImport("shell32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern bool ShellExecuteEx(ref Info info);
  public static int Open(string file, string progId) {
    var info = new Info();
    info.cbSize = Marshal.SizeOf(info);
    info.fMask = 0x00000001 | 0x00000400; // SEE_MASK_CLASSNAME, SEE_MASK_FLAG_NO_UI
    info.lpVerb = "open"; info.lpFile = file; info.lpClass = progId; info.nShow = 1;
    return ShellExecuteEx(ref info) ? 0 : Marshal.GetLastWin32Error();
  }
}
"@
[CgsShell]::Open('${quote(file)}', '${quote(progId)}')`);
  if (result !== '0') throw new Error(`ShellExecuteEx failed with ${result}`);
}

/**
 * Presses Open in the browser's own dialog that asks, the first time, whether the installed app
 * may open these file types. Returns whether it found the dialog.
 */
function answerFilePrompt(profile: string, channel: string | undefined): boolean {
  const browser = channel === 'msedge' ? 'msedge.exe' : 'chrome.exe';
  const result = powershell(`
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$scope = [System.Windows.Automation.TreeScope]
$ids = @(Get-CimInstance Win32_Process -Filter "Name='${browser}'" |
  Where-Object { $_.CommandLine -like '*${basename(profile)}*' -and $_.CommandLine -notlike '*--type=*' } |
  ForEach-Object { $_.ProcessId })
$found = $false
foreach ($id in $ids) {
  $mine = New-Object System.Windows.Automation.PropertyCondition($A::ProcessIdProperty, [int]$id)
  foreach ($window in $A::RootElement.FindAll($scope::Children, $mine)) {
    $box = $window.FindFirst($scope::Descendants,
      (New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::CheckBox)))
    if (-not $box -or $box.Current.Name -notlike '*XLSX*') { continue }
    $open = $window.FindFirst($scope::Descendants,
      (New-Object System.Windows.Automation.PropertyCondition($A::NameProperty, 'Open')))
    if ($open) {
      $open.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
      $found = $true
    }
  }
}
$found`);
  return result.trim() === 'True';
}

/** The names in the app's recent files, which it adds to once a workbook has opened. */
const recentNames = (page: Page) =>
  page.evaluate(
    () =>
      new Promise<string[]>((resolve) => {
        const request = indexedDB.open('cgs-recent', 1);
        request.onupgradeneeded = () =>
          request.result.createObjectStore('files', { keyPath: 'id' });
        request.onerror = () => resolve([]);
        request.onsuccess = () => {
          const all = request.result.transaction('files').objectStore('files').getAll();
          all.onsuccess = () => {
            resolve((all.result as { name: string }[]).map((entry) => entry.name));
            request.result.close();
          };
          all.onerror = () => resolve([]);
        };
      }),
  );

test('the installed web app opens a workbook from Open with', async () => {
  test.skip(process.platform !== 'win32', 'Open with is Windows only');
  const testInfo = test.info();
  test.setTimeout(240_000);
  const { channel } = testInfo.project.use;
  const profile = await mkdtemp(join(tmpdir(), 'cgs-pwa-'));
  // Not in %LOCALAPPDATA%\Temp: from Chrome 157 an installed app can't read a launched file in a
  // folder File System Access counts as the system's, and getFile() throws a SecurityError.
  const files = join(testInfo.outputDir, 'launch');
  await mkdir(files, { recursive: true });
  // A headless browser takes the system's launch and opens no window for it.
  const launch = () =>
    chromium.launchPersistentContext(profile, {
      channel,
      headless: false,
      viewport: { width: 1280, height: 800 },
      serviceWorkers: 'allow',
    });
  const manifestId = base + '/';
  let running: BrowserContext | undefined;
  let installing = false;
  cleanUp = async () => {
    try {
      if (installing) {
        if (!running?.browser()?.isConnected()) running = await launch();
        const session = await running.browser()!.newBrowserCDPSession();
        await session.send('PWA.uninstall', { manifestId });
        await expect.poll(() => windowsApps(profile, channel), { timeout: 30_000 }).toEqual([]);
        await expect.poll(() => openWithProgIds(profile, 'xlsx'), { timeout: 30_000 }).toEqual([]);
      }
    } finally {
      await running?.close();
      if (installing) windowsApps(profile, channel, true);
      await rm(profile, { recursive: true, force: true, maxRetries: 20, retryDelay: 500 });
      await rm(files, { recursive: true, force: true, maxRetries: 20, retryDelay: 500 });
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

  // The manifest offers the five types, and the installed app takes them from the system.
  const cdp = await context.newCDPSession(page);
  const manifest = JSON.parse((await cdp.send('Page.getAppManifest')).data!) as {
    file_handlers: { accept: Record<string, string[]> }[];
  };
  const offered = manifest.file_handlers.flatMap((handler) => Object.values(handler.accept).flat());
  expect(offered.sort()).toEqual(['.csv', '.xlsm', '.xlsx', '.xltm', '.xltx']);
  installing = true;
  await browser.send('PWA.install', { manifestId, installUrlOrBundleUrl: base + '/' });
  await browser.send('PWA.changeAppUserSettings', { manifestId, displayMode: 'standalone' });
  const state = (await browser.send('PWA.getOsAppState', { manifestId })) as {
    fileHandlers: { accepts: { fileExtensions: string[] }[] }[];
  };
  const taken = state.fileHandlers.flatMap((handler) =>
    handler.accepts.flatMap((accept) => accept.fileExtensions),
  );
  expect(taken).toContain('.xlsx');

  // Windows lists the app in Open with for .xlsx.
  let progIds: string[] = [];
  await expect
    .poll(() => (progIds = openWithProgIds(profile, 'xlsx')).length, { timeout: 60_000 })
    .toBeGreaterThan(0);

  // Open with: Windows starts the installed app with the file, which opens it.
  const file = join(files, 'Launched.xlsx');
  await writeFile(file, defaultStyledBook());
  /** The app's window shows the workbook, in a window of its own. */
  const showsWorkbook = async (app: Page) => {
    await expect(app.getByRole('heading', { name: 'Launched.xlsx' })).toBeVisible();
    await app.waitForFunction(() => window.cgsTest.loadPhase() === 'ready', undefined, {
      timeout: 60_000,
    });
    expect(await app.evaluate(() => window.cgsTest.cellText(1, 1))).toBe('Item 1');
    expect(await app.evaluate(() => matchMedia('(display-mode: standalone)').matches)).toBe(true);
  };
  const appeared = context.waitForEvent('page', { timeout: 60_000 }).catch(() => null);
  shellOpen(file, progIds[0]);
  if (channel !== 'msedge') {
    // The first launch with files asks whether the app may open them, as it asks anyone. The app
    // adds a workbook to its recent files once it has opened it.
    let asked = false;
    await expect
      .poll(
        async () => {
          if (!asked) asked = answerFilePrompt(profile, channel);
          return recentNames(page);
        },
        { timeout: 90_000 },
      )
      .toContain('Launched.xlsx');
    const app = await appeared;
    expect(app?.url().startsWith(base)).toBe(true);
    await showsWorkbook(app!);
    return;
  }
  // Edge: Open with starts the installed app through its package, in a window of its own. Under
  // automation Edge settles its one-time question about opening files by itself, as a no, and
  // then turns away files even from its own DevTools launch, so the file isn't checked here.
  const app = await appeared;
  expect(app?.url().startsWith(base)).toBe(true);
  expect(await app!.evaluate(() => matchMedia('(display-mode: standalone)').matches)).toBe(true);
});
