// Writes the third-party notices the app shows (File › About › Third-party notices) and the
// installer ships: every Rust crate built into the WASM engine and the desktop app, every npm
// package in the web bundle, what @cashgear/ui vendors, and the bundled Carlito font, each with
// its licence, followed by the licence texts their packages carry.
//   node scripts/third-party-notices.mjs [output]   (default apps/web/dist/third-party-notices.txt)
// pnpm build runs it after the web build.
import { spawnSync } from 'node:child_process';
import {
  existsSync,
  readdirSync,
  readFileSync,
  realpathSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join } from 'node:path';
import { pnpmCommand, root } from './lib.mjs';

const output = process.argv[2] ?? join(root, 'apps/web/dist/third-party-notices.txt');
const licenceFile = /^(licen[cs]e|copying|notice|copyright|ofl|third-party-notices)([-._].*)?$/i;

function licenceTexts(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((name) => licenceFile.test(name) && statSync(join(dir, name)).isFile())
    .sort()
    .map((name) => ({ name, text: readFileSync(join(dir, name), 'utf8').trim() }));
}

// Rust: the normal dependencies of what ships, for the platform it ships on.
function crates(roots, platform) {
  const result = spawnSync(
    'cargo',
    ['metadata', '--format-version', '1', '--locked', '--filter-platform', platform],
    { cwd: root, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
  );
  if (result.status !== 0) throw new Error('cargo metadata failed: ' + result.stderr);
  const metadata = JSON.parse(result.stdout);
  const packages = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  const seen = new Set();
  const queue = metadata.packages.filter((pkg) => roots.includes(pkg.name)).map((pkg) => pkg.id);
  while (queue.length) {
    const id = queue.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      if (dep.dep_kinds.some((kind) => kind.kind === null)) queue.push(dep.pkg);
    }
  }
  return (
    [...seen]
      .map((id) => packages.get(id))
      // Our own crates are not third-party. A local override of a dependency (.cargo/config.toml
      // `paths`) has no source either, so it goes by where the crate is.
      .filter(
        (pkg) => pkg.source || !pkg.manifest_path.toLowerCase().startsWith(root.toLowerCase()),
      )
      .map((pkg) => ({
        name: pkg.name,
        version: pkg.version,
        licence: pkg.license ?? pkg.license_file ?? 'see its files',
        home: pkg.repository ?? pkg.homepage ?? '',
        texts: licenceTexts(dirname(pkg.manifest_path)),
      }))
  );
}

const rust = new Map();
for (const pkg of [
  ...crates(['cgs-wasm'], 'wasm32-unknown-unknown'),
  ...crates(['cgs-desktop'], 'x86_64-pc-windows-msvc'),
]) {
  rust.set(`${pkg.name} ${pkg.version}`, pkg);
}

// npm: what the web bundle ships.
const [command, args] = pnpmCommand(['-r', '--prod', 'licenses', 'list', '--json']);
const listed = spawnSync(command, args, {
  cwd: root,
  encoding: 'utf8',
  maxBuffer: 64 * 1024 * 1024,
});
if (listed.status !== 0) throw new Error('pnpm licenses list failed: ' + listed.stderr);
const npm = Object.values(JSON.parse(listed.stdout))
  .flat()
  .filter((pkg) => !pkg.name.startsWith('@cgs/') && pkg.name !== '@cashgear/ui')
  .map((pkg) => ({
    name: pkg.name,
    version: pkg.versions.join(', '),
    licence: pkg.license,
    home: pkg.homepage ?? '',
    texts: licenceTexts(pkg.paths[0]),
  }));

// @cashgear/ui is first party, but carries others' work in its vendor folder.
const library = realpathSync(join(root, 'packages/ui/node_modules/@cashgear/ui'));
const vendorDir = join(library, 'src/vendor');
const vendored = readdirSync(vendorDir).map((dir) => {
  const path = join(vendorDir, dir);
  const read = (name) =>
    existsSync(join(path, name)) ? JSON.parse(readFileSync(join(path, name), 'utf8')) : {};
  const pkg = read('package.json');
  const integrity = read('integrity-manifest.json');
  const texts = licenceTexts(path);
  const firstLine = texts.find((text) => /^licen/i.test(text.name))?.text.split('\n')[0] ?? '';
  return {
    name: pkg.name ?? integrity.package ?? dir,
    version: pkg.version ?? integrity.version ?? '',
    licence:
      pkg.license ??
      (/^MIT/.test(firstLine) ? 'MIT' : texts.length ? 'see below' : 'see its files'),
    home: pkg.homepage ?? integrity.licenseSource ?? '',
    texts,
  };
});

const fontDir = join(root, 'packages/engine-client/src/fonts');
// The fonts PDFs fall back on, shipped as their authors made them.
const pdfFontDir = join(root, 'packages/engine-client/src/pdf-fonts');
const pdfLicence = (name) => [{ name, text: readFileSync(join(pdfFontDir, name), 'utf8').trim() }];
const fonts = [
  {
    name: 'Carlito',
    version: '',
    licence: 'OFL-1.1',
    home: 'https://github.com/googlefonts/carlito',
    texts: licenceTexts(fontDir),
  },
  {
    name: 'Caladea',
    version: '',
    licence: 'OFL-1.1',
    home: 'https://github.com/huertatipografica/Caladea',
    texts: pdfLicence('OFL-Caladea.txt'),
  },
  {
    name: 'Liberation Fonts',
    version: '2.1.5',
    licence: 'OFL-1.1',
    home: 'https://github.com/liberationfonts/liberation-fonts',
    texts: pdfLicence('LICENSE-Liberation.txt'),
  },
  {
    name: 'Noto Sans Arabic',
    version: '',
    licence: 'OFL-1.1',
    home: 'https://github.com/notofonts/arabic',
    texts: pdfLicence('OFL-Noto.txt'),
  },
];

const line = (pkg) =>
  `  ${[pkg.name, pkg.version].filter(Boolean).join(' ')}: ${pkg.licence}${pkg.home ? `  <${pkg.home}>` : ''}`;
const sorted = (list) => [...list].sort((a, b) => a.name.localeCompare(b.name));
const sections = [
  'CashGear Sheet: third-party notices',
  '',
  'CashGear Sheet is built with the open-source software below. Each is listed with its',
  'licence; the licence texts and notices its authors ship follow.',
  '',
  `Rust crates (${rust.size}):`,
  ...sorted(rust.values()).map(line),
  '',
  `JavaScript packages (${npm.length}):`,
  ...sorted(npm).map(line),
  '',
  `Inside the CashGear UI library (${vendored.length}):`,
  ...sorted(vendored).map(line),
  '',
  'Fonts, used where Calibri is not installed:',
  ...fonts.map(line),
  '',
  '',
  'Licence texts and notices',
  '=========================',
];
// Many packages ship the same text (Apache-2.0 above all): each text appears once, headed by
// every package that ships it.
const texts = new Map();
for (const pkg of [...sorted(rust.values()), ...sorted(npm), ...sorted(vendored), ...fonts]) {
  for (const text of pkg.texts) {
    const label = `${[pkg.name, pkg.version].filter(Boolean).join(' ')} (${text.name})`;
    texts.set(text.text, [...(texts.get(text.text) ?? []), label]);
  }
}
for (const [text, labels] of texts) sections.push('', `--- ${labels.join(', ')} ---`, '', text);
writeFileSync(output, sections.join('\n') + '\n');
console.log(
  `Third-party notices: ${rust.size} crates, ${npm.length} npm packages, ${vendored.length} ` +
    `vendored, ${fonts.length} font, ${texts.size} licence texts -> ${output}`,
);
