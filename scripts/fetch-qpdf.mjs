// qpdf, for checking the PDFs the app writes. It is a test tool only and never ships: it lives in
// .tools, fetched at a pinned version, and its SHA-256 is checked before it is unpacked.
// Usage: node scripts/fetch-qpdf.mjs, or import `qpdfPath` from it.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { root } from './lib.mjs';

const VERSION = '12.4.2';
const URL_ = `https://github.com/qpdf/qpdf/releases/download/v${VERSION}/qpdf-${VERSION}-msvc64.zip`;
const SHA256 = 'db87077e683630c1217e0e8f9a20a9749d952ab676e881c3689187763a5de25d';
const folder = join(root, '.tools', 'qpdf-' + VERSION);

function find(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      const found = find(path);
      if (found) return found;
    } else if (entry.name.toLowerCase() === 'qpdf.exe') return path;
  }
  return null;
}

/** Where qpdf is, fetching it the first time. `QPDF` names one already installed. */
export async function qpdfPath() {
  if (process.env.QPDF) return process.env.QPDF;
  if (existsSync(folder)) {
    const found = find(folder);
    if (found) return found;
  }
  if (process.platform !== 'win32') return 'qpdf';
  const response = await fetch(URL_);
  if (!response.ok) throw new Error(`qpdf ${VERSION} couldn't be fetched: ${response.status}`);
  const zip = Buffer.from(await response.arrayBuffer());
  const digest = createHash('sha256').update(zip).digest('hex');
  if (digest !== SHA256) throw new Error(`qpdf's SHA-256 is ${digest}, not ${SHA256}`);
  rmSync(folder, { recursive: true, force: true });
  mkdirSync(folder, { recursive: true });
  const archive = join(folder, 'qpdf.zip');
  writeFileSync(archive, zip);
  // Windows' own tar reads zip files; a GNU tar earlier on PATH doesn't.
  const tar = join(process.env.SystemRoot ?? 'C:/Windows', 'System32', 'tar.exe');
  execFileSync(tar, ['-xf', archive, '-C', folder]);
  rmSync(archive);
  const found = find(folder);
  if (!found) throw new Error('qpdf.exe is not in the release archive.');
  console.log(`qpdf ${VERSION} (sha256 ${digest}) is at ${found}`);
  return found;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  console.log(await qpdfPath());
}
