// Builds the Windows installer (pnpm desktop:installer, after pnpm build):
// target/release/bundle/nsis/CashGear Sheet_<version>_x64-setup.exe, installing per user, with the
// third-party notices beside the app, and registering the app for its file types
// (apps/desktop/src-tauri/windows/hooks.nsh). Every file it builds is signed through
// scripts/sign-windows.mjs.
//
// Settings, from the environment:
//   CGS_VERSION        build this version instead of tauri.conf.json's (tests)
//   CGS_TEST_PRODUCT, CGS_TEST_IDENTIFIER: build as another product (the installer test). These
//                      and the publisher, which takes the product's name, decide the install's
//                      registry keys, ProgIDs, shortcuts and data folders, so that it shares none
//                      with the real app.
//   TAURI_SIGNING_PRIVATE_KEY (and _PASSWORD): the updater's private key. With it the build also
//                      signs the update (the installer's .sig) for latest.json.
//   CGS_UPDATER_PUBKEY the updater's public key, when it is not the config's (tests)
//   CGS_UPDATER_URL    the update feed, when it is not the config's (tests: http)
//   CGS_SIGN_*         code signing: see scripts/sign-windows.mjs
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
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
const config = {
  ...(env.CGS_VERSION ? { version: env.CGS_VERSION } : {}),
  ...(env.CGS_TEST_PRODUCT
    ? { productName: env.CGS_TEST_PRODUCT, identifier: env.CGS_TEST_IDENTIFIER }
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
