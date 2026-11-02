// Collects the distinct number format codes used by the public test workbooks into
// crates/cgs-numfmt/tests/data/format-codes.txt. Only the code strings are taken, and the
// files are read as plain zips: none of them is ever opened in Excel.
import { mkdir, readdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { inflateRawSync } from 'node:zlib';
import { root } from './lib.mjs';

const inputRoot = join(root, 'tests/workbooks/public');
const output = join(root, 'crates/cgs-numfmt/tests/data/format-codes.txt');

async function* walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) yield* walk(path);
    else if (/\.xl[st][xm]$/i.test(entry.name)) yield path;
  }
}

/** The bytes of one named zip entry, found through the central directory, or null. */
function readEntry(zip, wanted) {
  let end = -1;
  for (let at = zip.length - 22; at >= Math.max(0, zip.length - 65557); at--) {
    if (zip.readUInt32LE(at) === 0x06054b50) {
      end = at;
      break;
    }
  }
  if (end < 0) return null;
  const count = zip.readUInt16LE(end + 10);
  let at = zip.readUInt32LE(end + 16);
  for (let index = 0; index < count; index++) {
    if (zip.readUInt32LE(at) !== 0x02014b50) return null;
    const method = zip.readUInt16LE(at + 10);
    const packed = zip.readUInt32LE(at + 20);
    const nameLength = zip.readUInt16LE(at + 28);
    const extraLength = zip.readUInt16LE(at + 30);
    const commentLength = zip.readUInt16LE(at + 32);
    const local = zip.readUInt32LE(at + 42);
    const name = zip.toString('utf8', at + 46, at + 46 + nameLength);
    if (name.replace(/\\/g, '/').toLowerCase() === wanted) {
      const start = local + 30 + zip.readUInt16LE(local + 26) + zip.readUInt16LE(local + 28);
      const body = zip.subarray(start, start + packed);
      if (method === 0) return body;
      if (method === 8) return inflateRawSync(body);
      return null;
    }
    at += 46 + nameLength + extraLength + commentLength;
  }
  return null;
}

const decode = (text) =>
  text
    .replace(/_x([0-9a-fA-F]{4})_/g, (_, hex) => String.fromCharCode(parseInt(hex, 16)))
    .replace(/&#x([0-9a-fA-F]+);/g, (_, hex) => String.fromCodePoint(parseInt(hex, 16)))
    .replace(/&#(\d+);/g, (_, digits) => String.fromCodePoint(Number(digits)))
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&');

const codes = new Set();
let files = 0;
let skipped = 0;
for await (const path of walk(inputRoot)) {
  try {
    const styles = readEntry(await readFile(path), 'xl/styles.xml');
    if (!styles) {
      skipped++;
      continue;
    }
    const xml = styles.toString('utf8');
    for (const tag of xml.matchAll(/<(?:\w+:)?numFmt\b[^>]*>/g)) {
      const value = /\bformatCode\s*=\s*(?:"([^"]*)"|'([^']*)')/.exec(tag[0]);
      if (!value) continue;
      const code = decode(value[1] ?? value[2] ?? '');
      // One line per code in the output, so a code with a line break can't be kept.
      if (code && !/[\r\n]/.test(code)) codes.add(code);
    }
    files++;
  } catch {
    skipped++;
  }
}

// A code's UTF-16 order is stable across platforms, unlike a locale-aware sort.
const sorted = [...codes].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
await mkdir(dirname(output), { recursive: true });
await writeFile(output, sorted.join('\n') + '\n');
console.log(`${files} files read, ${skipped} skipped, ${sorted.length} distinct codes`);
