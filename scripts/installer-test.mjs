// Installs, updates and uninstalls the Windows installer as a user would (Phase 9's exit
// criterion; pnpm desktop:installer-test, after pnpm build). It builds the installer twice, a
// version apart, signed with a throwaway certificate and a throwaway updater key, serves the
// second as an update on 127.0.0.1, then:
//   1. installs the first silently into a folder of its own, and checks that its programs are
//      signed with the certificate, that the notices are beside it, and that it is registered
//      for .xlsx, .xlsm, .xltx, .xltm and .csv;
//   2. opens an .xlsx through the shell as a double-click does once the app is the type's
//      default: Windows runs the ProgID's open command, and the app shows the workbook;
//   3. starts it, has the status bar offer the update, and installs it from the Update dialog;
//   4. checks that the app the installer starts again is the new version, still registered;
//   5. uninstalls it silently and checks that nothing is left behind, and that the user's own
//      keys for those file types are exactly as they were.
// The test builds install as a product of their own, so they can run beside a real install of
// CashGear Sheet and never touch its registry keys, ProgIDs, shortcuts or data.
// Needs the WebView2 Runtime and the Windows SDK (signtool).
import { spawn, spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  realpathSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import { createServer as createNetServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { chromium } from '@playwright/test';
import { defaultStyledBook } from '../tests/fixtures/workbooks.mjs';
import { pnpmCommand, root, run, windowsPowerShell } from './lib.mjs';
import { makeTestCertificate } from './test-certificate.mjs';

if (process.platform !== 'win32') throw new Error('The installer test runs on Windows.');
const product = 'CGS Installer Test';
// ProgIDs are made from it (hooks.nsh), and a ProgID has no hyphens.
const identifier = 'com.cashgear.sheet.installertest';
const extensions = ['xlsx', 'xlsm', 'xltx', 'xltm', 'csv'];
const feedPort = 47812;
const started = Date.now();
const log = (line) => console.log(`[${((Date.now() - started) / 1000).toFixed(0)} s] ${line}`);
// The temp folder's long name: the uninstaller removes the Start menu shortcut only when its
// target matches the install folder exactly, and CI's temp folder is an 8.3 short name.
const work = join(realpathSync.native(tmpdir()), `cgs-installer-${randomBytes(4).toString('hex')}`);
const feed = join(work, 'feed');
const installDir = join(work, 'app');
// The installers, uninstallers and app keep their temporary files in here too: the updater
// never removes its download, nor NSIS its copy of the uninstaller.
const temp = join(work, 'temp');
mkdirSync(feed, { recursive: true });
mkdirSync(temp, { recursive: true });
const tempEnv = { ...process.env, TEMP: temp, TMP: temp };
const results = { versions: [], installer: {}, steps: {} };

const reg = (args) => spawnSync('reg.exe', args, { encoding: 'utf8', windowsHide: true });
const uninstallRoot = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall';
const uninstallKey = `${uninstallRoot}\\${product}`;
// A silent uninstall keeps this key, which records the install folder for the next install to
// restore. The test's publisher is its product's name, and Tauri's key for the product is
// Software\<publisher>\<product>, where the capabilities for Default apps go (hooks.nsh).
const publisherKey = `HKCU\\Software\\${product}`;
const capabilities = `${publisherKey}\\${product}\\Capabilities`;
const classes = 'HKCU\\Software\\Classes';
const progIds = extensions.map((ext) => `${classes}\\${identifier}.${ext}`);
const registeredApps = 'HKCU\\Software\\RegisteredApplications';
const desktop = windowsPowerShell("[Environment]::GetFolderPath('Desktop')");
const shortcuts = [
  join(process.env.APPDATA, 'Microsoft/Windows/Start Menu/Programs', `${product}.lnk`),
  join(desktop, `${product}.lnk`),
];
const dataFolders = [process.env.APPDATA, process.env.LOCALAPPDATA].map((folder) =>
  join(folder, identifier),
);

function uninstallEntry() {
  const query = reg(['query', uninstallKey]);
  if (query.status !== 0) return null;
  return {
    version: query.stdout.match(/DisplayVersion\s+REG_SZ\s+(\S+)/)?.[1] ?? null,
    uninstaller: query.stdout.match(/UninstallString\s+REG_SZ\s+"?([^"\r\n]+)/)?.[1] ?? null,
  };
}

/** A key and everything under it, as reg prints it, or '' when it isn't there. */
const keyDump = (key) => {
  const query = reg(['query', key, '/s']);
  return query.status === 0 ? query.stdout.trim() : '';
};

/** What the user's own setup holds for these types and for a real install, which must not change. */
const usersOwn = () =>
  [
    ...extensions.map((ext) => `${classes}\\.${ext}`),
    ...extensions.map((ext) => `${classes}\\com.cashgear.sheet.${ext}`),
    'HKCU\\Software\\CashGear\\CashGear Sheet',
    `${uninstallRoot}\\CashGear Sheet`,
  ]
    .map((key) => `${key}\n${keyDump(key)}`)
    .join('\n') +
  '\n' +
  (reg(['query', registeredApps]).stdout ?? '').replace(new RegExp(`.*${product}.*\\r?\\n`), '');
const usersOwnBefore = usersOwn();

/** What an install under the test's name has left on this machine. */
const leftBehind = () => [
  ...[uninstallKey, publisherKey, ...progIds].filter((key) => reg(['query', key]).status === 0),
  ...(reg(['query', registeredApps, '/v', product]).status === 0
    ? [`${registeredApps} ${product}`]
    : []),
  ...extensions
    .filter(
      (ext) =>
        reg(['query', `${classes}\\.${ext}\\OpenWithProgids`, '/v', `${identifier}.${ext}`])
          .status === 0,
    )
    .map((ext) => `.${ext} OpenWithProgids ${identifier}.${ext}`),
  ...[...shortcuts, ...dataFolders].filter((path) => existsSync(path)),
];

async function until(what, check, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = await check();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`Timed out: ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}

async function freePort() {
  const listener = createNetServer();
  await new Promise((resolve) => listener.listen(0, '127.0.0.1', resolve));
  const { port } = listener.address();
  await new Promise((resolve) => listener.close(resolve));
  return port;
}

const appExe = () =>
  join(
    installDir,
    readdirSync(installDir).find((name) => /\.exe$/i.test(name) && !/^uninstall/i.test(name)),
  );

function stopApp() {
  windowsPowerShell(
    `Get-Process | Where-Object { $_.Path -and $_.Path.StartsWith('${work}') } | Stop-Process -Force -ErrorAction SilentlyContinue`,
  );
}

const forceRemove = (path) =>
  rm(path, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 });

/** Takes out an install under the test's name, finished or not, this run's or a killed one's. */
async function removeInstall() {
  const uninstaller = uninstallEntry()?.uninstaller;
  stopApp();
  if (uninstaller && existsSync(uninstaller)) {
    spawnSync(`"${uninstaller}" /S`, { shell: true, windowsHide: true, env: tempEnv });
    await until(
      'the uninstaller to finish',
      () => !uninstallEntry() && !existsSync(uninstaller),
      120_000,
    ).catch((error) => log(error.message));
  }
  for (const key of [uninstallKey, publisherKey, ...progIds]) reg(['delete', key, '/f']);
  reg(['delete', registeredApps, '/v', product, '/f']);
  for (const ext of extensions) {
    reg(['delete', `${classes}\\.${ext}\\OpenWithProgids`, '/v', `${identifier}.${ext}`, '/f']);
  }
  for (const path of [...shortcuts, ...dataFolders]) await forceRemove(path);
}

/** A killed run's folders. */
async function removeKilledRuns() {
  const parent = dirname(work);
  for (const name of readdirSync(parent)) {
    const path = join(parent, name);
    if (name.startsWith('cgs-installer-') && path !== work) await forceRemove(path);
  }
}

/** Attaches to the app's WebView2 and returns the page once the app has started. */
async function attach(port) {
  const endpoint = `http://127.0.0.1:${port}`;
  await until(
    'the app to accept a debugger',
    () =>
      fetch(`${endpoint}/json/version`, { signal: AbortSignal.timeout(1000) }).then(
        (r) => r.ok,
        () => false,
      ),
    60_000,
  );
  const browser = await chromium.connectOverCDP(endpoint);
  // On a slow machine the webview answers before it has opened its page.
  const context = await until('the webview', () => browser.contexts()[0], 30_000);
  const page = context.pages()[0] ?? (await context.waitForEvent('page', { timeout: 60_000 }));
  await until('the app to load', () => page.url().startsWith('http'), 30_000);
  await page.waitForFunction(
    () => document.querySelector('[data-testid="engine-status"]')?.textContent === 'Ready',
    undefined,
    { timeout: 60_000 },
  );
  return { browser, page };
}

/** The registry's open command for a type, through the install's ProgID. */
const openCommand = (ext) =>
  reg(['query', `${classes}\\${identifier}.${ext}\\shell\\open\\command`, '/ve']).stdout.match(
    /REG_SZ\s+(.+)/,
  )?.[1];

function checkRegistered(exe) {
  const expected = `"${exe}" "%1"`;
  for (const ext of extensions) {
    if (openCommand(ext)?.trim() !== expected)
      throw new Error(`.${ext} opens with ${openCommand(ext)}, not ${expected}.`);
    const listed = reg([
      'query',
      `${classes}\\.${ext}\\OpenWithProgids`,
      '/v',
      `${identifier}.${ext}`,
    ]);
    if (listed.status !== 0) throw new Error(`.${ext} doesn't list the app in Open with.`);
    const capability = reg(['query', `${capabilities}\\FileAssociations`, '/v', `.${ext}`]);
    if (!capability.stdout.includes(`${identifier}.${ext}`))
      throw new Error(`Default apps doesn't know the app opens .${ext}.`);
  }
  if (reg(['query', registeredApps, '/v', product]).status !== 0)
    throw new Error('The app is not in Default apps.');
}

/**
 * Opens a file as Explorer does for a double-click once the app is the type's default: through
 * ShellExecuteEx with the type's ProgID, so Windows reads the open command from the registry and
 * runs it. The user's default for .xlsx stays theirs, so that is how the test can pick the app.
 */
function shellOpen(file, ext, env) {
  const script = `
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
[CgsShell]::Open('${file.replace(/'/g, "''")}', '${identifier}.${ext}')`;
  const childEnv = { ...env };
  for (const name of Object.keys(childEnv))
    if (name.toLowerCase() === 'psmodulepath') delete childEnv[name];
  const ran = spawnSync(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-Command', "$ErrorActionPreference = 'Stop'; " + script],
    { encoding: 'utf8', env: childEnv, windowsHide: true },
  );
  if (ran.status !== 0 || ran.stdout.trim() !== '0')
    throw new Error(`ShellExecuteEx failed: ${ran.stdout} ${ran.stderr}`);
}

const policyKey = 'HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge\\WebView2\\AdditionalBrowserArguments';
const policyName = 'cgs-desktop.exe';
const hostedRunner =
  process.env.GITHUB_ACTIONS === 'true' && process.env.RUNNER_ENVIRONMENT === 'github-hosted';
let policySet = false;
let server;
try {
  await removeInstall();
  await removeKilledRuns();

  // Two installers a version apart, signed with a throwaway certificate and updater key.
  const certificate = makeTestCertificate(join(work, 'certificate'));
  const keyPath = join(work, 'updater.key');
  const keyPassword = randomBytes(12).toString('hex');
  await run(
    ...pnpmCommand([
      '--filter',
      '@cgs/desktop',
      'tauri',
      'signer',
      'generate',
      '--ci',
      '-w',
      keyPath,
      '-p',
      keyPassword,
    ]),
    { stdio: 'ignore' },
  );
  const base = JSON.parse(
    readFileSync(join(root, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'),
  ).version;
  const [major, minor, patch] = base.split('.').map(Number);
  const versions = [base, `${major}.${minor}.${patch + 1}`];
  results.versions = versions;
  const env = {
    ...process.env,
    CGS_TEST_PRODUCT: product,
    CGS_TEST_IDENTIFIER: identifier,
    CGS_SIGN_PFX: certificate.pfx,
    CGS_SIGN_PASSWORD: certificate.password,
    CGS_SIGN_TIMESTAMP: '0',
    TAURI_SIGNING_PRIVATE_KEY: readFileSync(keyPath, 'utf8'),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: keyPassword,
    CGS_UPDATER_PUBKEY: readFileSync(keyPath + '.pub', 'utf8').trim(),
    CGS_UPDATER_URL: `http://127.0.0.1:${feedPort}/latest.json`,
  };
  const bundle = join(root, 'target/release/bundle/nsis');
  const setups = [];
  for (const version of versions) {
    log(`Building the ${version} installer`);
    const building = Date.now();
    await run(process.execPath, [join(root, 'scripts/desktop-installer.mjs')], {
      env: { ...env, CGS_VERSION: version },
      stdio: ['ignore', 'ignore', 'inherit'],
    });
    const name = `${product}_${version}_x64-setup.exe`;
    copyFileSync(join(bundle, name), join(feed, name));
    copyFileSync(join(bundle, name + '.sig'), join(feed, name + '.sig'));
    setups.push(join(feed, name));
    results.installer[version] = {
      bytes: statSync(join(feed, name)).size,
      buildSeconds: Math.round((Date.now() - building) / 1000),
    };
  }

  // The update feed.
  const update = setups[1].split(/[\\/]/).pop();
  const manifest = {
    version: versions[1],
    notes: 'The installer test update.',
    pub_date: new Date().toISOString(),
    platforms: Object.fromEntries(
      ['windows-x86_64', 'windows-x86_64-nsis'].map((platform) => [
        platform,
        {
          signature: readFileSync(setups[1] + '.sig', 'utf8'),
          url: `http://127.0.0.1:${feedPort}/${encodeURIComponent(update)}`,
        },
      ]),
    ),
  };
  writeFileSync(join(feed, 'latest.json'), JSON.stringify(manifest));
  server = createServer((request, response) => {
    const file = join(
      feed,
      decodeURIComponent(new URL(request.url, 'http://feed').pathname.slice(1)),
    );
    if (!file.startsWith(feed) || !existsSync(file)) return response.writeHead(404).end();
    response.writeHead(200, { 'Content-Length': statSync(file).size });
    response.end(readFileSync(file));
  });
  await new Promise((resolve) => server.listen(feedPort, '127.0.0.1', resolve));

  // 1. Install silently, and check the signatures, the notices and the file types.
  log(`Installing ${versions[0]} into ${installDir}`);
  let step = Date.now();
  const installed = spawnSync(`"${setups[0]}" /S /D=${installDir}`, {
    shell: true,
    windowsHide: true,
    env: tempEnv,
  });
  if (installed.status !== 0) throw new Error(`The installer failed (${installed.status}).`);
  results.steps.installSeconds = (Date.now() - step) / 1000;
  if (uninstallEntry()?.version !== versions[0])
    throw new Error('The install left no uninstall entry.');
  const files = JSON.parse(
    windowsPowerShell(
      `@(Get-ChildItem -Path '${installDir}' -Recurse -Include *.exe,*.dll | ForEach-Object { ` +
        `$s = Get-AuthenticodeSignature $_.FullName; ` +
        `[pscustomobject]@{ file = $_.FullName.Substring(${installDir.length + 1}); signer = $s.SignerCertificate.Thumbprint } }) | ConvertTo-Json`,
    ),
  );
  const unsigned = [files].flat().filter((file) => file.signer !== certificate.thumbprint);
  results.signedFiles = [files].flat().length;
  if (unsigned.length)
    throw new Error('Unsigned files: ' + unsigned.map((file) => file.file).join(', '));
  const notices = join(installDir, 'licences/THIRD-PARTY-NOTICES.txt');
  if (!existsSync(notices) || !readFileSync(notices, 'utf8').includes('Rust crates ('))
    throw new Error('The install lacks its third-party notices.');
  checkRegistered(appExe());
  results.steps.registeredTypes = extensions.map((ext) => '.' + ext);
  log(`Installed: ${results.signedFiles} programs, all signed, registered for its file types`);

  const port = await freePort();
  const profile = join(work, 'webview2');
  const dataDir = join(work, 'data');
  const browserArguments = `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`;
  // Elevated WebView2 hosts ignore browser arguments from the environment. On GitHub's
  // disposable runners, which run elevated, the documented policy passes them to our
  // executable only, as the desktop smoke test does.
  if (hostedRunner) {
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
      throw new Error('Could not set the WebView2 test policy: ' + added.stderr);
    policySet = true;
  }
  const appEnv = {
    ...tempEnv,
    // Keeps the app's recent files and working copies in the test's folder.
    CGS_DESKTOP_SMOKE: dataDir,
    WEBVIEW2_USER_DATA_FOLDER: profile,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: browserArguments,
  };

  // 2. A double-click on a workbook, through the shell and the install's registration.
  const book = join(work, 'Double click test.xlsx');
  writeFileSync(book, defaultStyledBook());
  step = Date.now();
  shellOpen(book, 'xlsx', appEnv);
  let { browser, page } = await attach(port);
  await page.waitForFunction(() => window.cgsTest?.loadPhase() === 'ready', undefined, {
    timeout: 60_000,
  });
  const first = await page.evaluate(() => window.cgsTest.cellText(1, 1));
  if (first !== 'Item 1') throw new Error(`The double-clicked workbook shows ${first} in A1.`);
  results.steps.doubleClick = { seconds: (Date.now() - step) / 1000, a1: first };
  log('A double-clicked workbook opened in the app');
  await browser.close().catch(() => {});
  stopApp();

  // 3. Start it, and update it from the Update dialog the status bar offers.
  const start = () =>
    spawn(appExe(), [], { windowsHide: true, env: appEnv, stdio: 'ignore', detached: false });
  const app = start();
  const exited = new Promise((resolve) => app.once('exit', resolve));
  ({ browser, page } = await attach(port));
  const before = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('native_info'));
  if (before.version !== versions[0]) throw new Error(`The installed app is ${before.version}.`);
  // The first check is 10 s after start.
  await page.getByTestId('update-status').waitFor({ timeout: 60_000 });
  const offered = await page.getByTestId('update-status').textContent();
  if (offered !== `Update to ${versions[1]}`) throw new Error(`The status bar says ${offered}.`);
  log(`Updating to ${versions[1]}`);
  step = Date.now();
  await page.getByTestId('update-status').click();
  await page.getByTestId('update-install').click();
  await until('the app to close for the installer', () => exited.then(() => true), 120_000);
  await browser.close().catch(() => {});
  await until('the update to install', () => uninstallEntry()?.version === versions[1], 180_000);
  results.steps.updateSeconds = (Date.now() - step) / 1000;

  // 4. The installer starts the new version, with the same settings. It starts it as the
  // desktop's user, which a CI runner's service session may not have: then the test starts it,
  // and records that the installer did not.
  try {
    ({ browser, page } = await attach(port));
    results.steps.restartedByInstaller = true;
  } catch (error) {
    log(`The installer did not start the app again (${error.message}): starting it`);
    results.steps.restartedByInstaller = false;
    start();
    ({ browser, page } = await attach(port));
  }
  const after = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('native_info'));
  if (after.version !== versions[1])
    throw new Error(`After the update the app is ${after.version}.`);
  results.steps.restartedVersion = after.version;
  checkRegistered(appExe());
  log(`Updated: the app runs as ${after.version}, still registered`);
  await browser.close().catch(() => {});
  stopApp();

  // 5. Uninstall silently: nothing may be left.
  step = Date.now();
  spawnSync(`"${join(installDir, 'uninstall.exe')}" /S`, {
    shell: true,
    windowsHide: true,
    env: tempEnv,
  });
  await until(
    'the uninstaller to finish',
    () => !uninstallEntry() && !existsSync(join(installDir, 'uninstall.exe')),
    120_000,
  );
  const left = existsSync(installDir) ? readdirSync(installDir) : [];
  if (left.length) throw new Error('The uninstaller left: ' + left.join(', '));
  // Kept by design, and taken by the test afterwards: the install-folder key (the next install
  // goes back there), and the app's data, which only the uninstaller's "Delete the application
  // data" box removes, so recovery data and settings outlive a reinstall.
  const kept = dataFolders.filter((path) => existsSync(path));
  results.steps.keptAppData = kept.map((path) => ({
    folder: path.replace(identifier, '<identifier>'),
    holds: readdirSync(path),
  }));
  const leftKeys = leftBehind().filter((item) => item !== publisherKey && !kept.includes(item));
  if (leftKeys.length) throw new Error('The uninstaller left: ' + leftKeys.join(', '));
  if (keyDump(capabilities)) throw new Error('The uninstaller left the Default apps capabilities.');
  if (usersOwn() !== usersOwnBefore)
    throw new Error("The user's own keys for these file types changed.");
  results.steps.uninstallSeconds = (Date.now() - step) / 1000;
  log('Uninstalled: nothing left behind, and the file types are as they were');
  mkdirSync(join(root, 'bench/results'), { recursive: true });
  writeFileSync(join(root, 'bench/results/installer-test.json'), JSON.stringify(results, null, 2));
  console.log(JSON.stringify(results, null, 2));
} finally {
  server?.close();
  if (policySet) reg(['delete', policyKey, '/v', policyName, '/f', '/reg:64']);
  await removeInstall().catch((error) => log(`Removing the install failed: ${error.message}`));
  await forceRemove(work).catch((error) =>
    log(`Removing the temp folder failed: ${error.message}`),
  );
  const left = leftBehind();
  if (existsSync(work)) left.push(work);
  if (usersOwn() !== usersOwnBefore) left.push("changes to the user's own registry keys");
  if (left.length) {
    console.error('The test left behind: ' + left.join(', '));
    process.exitCode = 1;
  }
}
