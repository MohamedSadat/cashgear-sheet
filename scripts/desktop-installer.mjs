// Builds the Windows installer (pnpm desktop:installer, after pnpm build):
// target/release/bundle/nsis/CashGear Sheet_<version>_x64-setup.exe, installing per user, with the
// third-party notices beside the app, and registering the app for its file types
// (apps/desktop/src-tauri/windows/hooks.nsh). Every file it builds is signed through
// scripts/sign-windows.mjs.
//
// Settings, from the environment:
//   CGS_VERSION        build this version instead of tauri.conf.json's (tests)
//   CGS_TEST_PRODUCT, CGS_TEST_IDENTIFIER, CGS_TEST_SCHEME: build as another product (the
//                      installer test). These and the publisher, which takes the product's name,
//                      decide the install's registry keys, ProgIDs, link scheme, shortcuts and data
//                      folders, so that it shares none with the real app.
//   VITE_WORKSPACE_API_URL, VITE_WORKSPACE_BLOB_URL: the Cash Gear Workspace API and Blob Storage
//                      the web build was made for (pnpm build with the same variables), which
//                      join the desktop's connect-src. For a build against Workspace's
//                      development stack (docs/DEPLOYMENT.md).
//   TAURI_SIGNING_PRIVATE_KEY (and _PASSWORD): the updater's private key. With it the build also
//                      signs the update (the installer's .sig) for latest.json.
//   CGS_UPDATER_PUBKEY the updater's public key, when it is not the config's (tests)
//   CGS_UPDATER_URL    the update feed, when it is not the config's (tests: http)
//   CGS_SIGN_*         code signing: see scripts/sign-windows.mjs
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
import { widenConnectSrc, workspaceOrigins } from './desktop-config.mjs';
import { pnpmCommand, root, run } from './lib.mjs';

const notices = join(root, 'apps/web/dist/third-party-notices.txt');
if (!existsSync(join(root, 'apps/web/dist/index.html')) || !existsSync(notices)) {
  throw new Error('The web build is missing: run pnpm build first.');
}

// What goes beside the app.
const staging = join(root, 'target/installer');
rmSync(staging, { recursive: true, force: true });
const licences = join(staging, 'licences');
mkdirSync(licences, { recursive: true });
copyFileSync(notices, join(licences, 'THIRD-PARTY-NOTICES.txt'));

// The bundle's settings on top of tauri.conf.json.
const env = process.env;
const forward = (path) => path.replace(/\\/g, '/');
const shipped = JSON.parse(
  readFileSync(join(root, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'),
);
const origins = workspaceOrigins(env);
if (origins.length) console.log('connect-src adds ' + origins.join(' '));
const config = {
  ...(env.CGS_VERSION ? { version: env.CGS_VERSION } : {}),
  ...(env.CGS_TEST_PRODUCT
    ? { productName: env.CGS_TEST_PRODUCT, identifier: env.CGS_TEST_IDENTIFIER }
    : {}),
  ...(origins.length
    ? { app: { security: { csp: widenConnectSrc(shipped.app.security.csp, origins) } } }
    : {}),
  bundle: {
    ...(env.CGS_TEST_PRODUCT ? { publisher: env.CGS_TEST_PRODUCT } : {}),
    resources: { [forward(join(licences, '*'))]: 'licences/' },
    createUpdaterArtifacts: !!env.TAURI_SIGNING_PRIVATE_KEY,
    windows: {
      // Tauri splits the command at spaces, runs it in src-tauri, and passes the file as one
      // argument.
      signCommand: 'node ../../../scripts/sign-windows.mjs %1',
    },
  },
  plugins: {
    // Workspace's link (Track S6): the scheme the installer registers and the app answers.
    ...(env.CGS_TEST_SCHEME
      ? { 'deep-link': { desktop: { schemes: [env.CGS_TEST_SCHEME] } } }
      : {}),
    updater: {
      ...(env.CGS_UPDATER_PUBKEY ? { pubkey: env.CGS_UPDATER_PUBKEY } : {}),
      ...(env.CGS_UPDATER_URL
        ? {
            endpoints: [env.CGS_UPDATER_URL],
            dangerousInsecureTransportProtocol: env.CGS_UPDATER_URL.startsWith('http:'),
          }
        : {}),
    },
  },
};
// Only this build's installer is left in the bundle folder (an earlier test build there would
// end up in the release's files).
const out = join(root, 'target/release/bundle/nsis');
rmSync(out, { recursive: true, force: true });
const configFile = join(staging, 'tauri.installer.json');
writeFileSync(configFile, JSON.stringify(config, null, 2));
await run(...pnpmCommand(['--filter', '@cgs/desktop', 'tauri', 'build', '--config', configFile]));
for (const name of readdirSync(out)) console.log('Built ' + join(out, name));
