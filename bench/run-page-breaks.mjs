// Phase 10's page breaks against Excel's. Generated books covering every page setup option go
// through Excel over COM, with Microsoft Print to PDF as its printer, and through the engine;
// every sheet's row and column breaks and page count must be the same. With `--public`, the
// public test files that hold page setup go through as well, as locked-down temporary copies
// (read-only, macros forced off, links not updated), and their page counts are compared.
// Usage: node bench/run-page-breaks.mjs [out.json] [--public] [--only=name]
import { spawnSync } from 'node:child_process';
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, resolve } from 'node:path';
import { root } from '../scripts/lib.mjs';
import { printBooks } from './print-books.mjs';

const out = resolve(
  process.argv.slice(2).find((arg) => !arg.startsWith('--')) ??
    join(root, 'bench/results/page-breaks.json'),
);
const withPublic = process.argv.includes('--public');
const only = process.argv.find((arg) => arg.startsWith('--only='))?.slice(7);
const tool = join(root, 'target/fast/pagebreaks.exe');
const script = readFileSync(join(root, 'bench/excel-page-breaks.ps1'), 'utf8');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";

function ours(path) {
  const run = spawnSync(tool, [path], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (run.status !== 0) throw new Error('pagebreaks: ' + run.stderr.slice(0, 300));
  return JSON.parse(run.stdout);
}

function excels(path, scratch) {
  const target = join(scratch, 'excel.json');
  const run = spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `& {${script}} -Workbook ${quoted(path)} -Out ${quoted(target)}`,
    ],
    { encoding: 'utf8', timeout: 600000, windowsHide: true },
  );
  if (run.status !== 0) throw new Error('Excel: ' + run.stderr.slice(0, 300));
  return JSON.parse(readFileSync(target, 'utf8').replace(/^\uFEFF/, ''));
}

const same = (a, b) => JSON.stringify(a ?? []) === JSON.stringify(b ?? []);

/** Each visible sheet compared: everything for generated books, page counts for public ones. */
function compare(mine, theirs, countsOnly) {
  const sheets = [];
  for (const sheet of theirs.sheets) {
    const ownSheet = mine.find((entry) => entry.sheet === sheet.sheet);
    if (!ownSheet) continue;
    if (countsOnly && !ownSheet.hasSetup) continue;
    const pagesMatch = ownSheet.pages === sheet.pages;
    const breaksMatch = same(ownSheet.rows, sheet.rows) && same(ownSheet.cols, sheet.cols);
    sheets.push({
      sheet: sheet.name,
      match: !sheet.error && (countsOnly ? pagesMatch : pagesMatch && breaksMatch),
      excel: { pages: sheet.pages, rows: sheet.rows, cols: sheet.cols, error: sheet.error },
      ours: {
        pages: ownSheet.pages,
        rows: ownSheet.rows,
        cols: ownSheet.cols,
        zoom: ownSheet.zoom,
      },
    });
  }
  return sheets;
}

function walk(dir, found = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) walk(path, found);
    else if (/\.(xlsx|xlsm)$/i.test(entry) && !entry.startsWith('~$')) found.push(path);
  }
  return found;
}

const built = spawnSync(
  'cargo',
  ['build', '--profile', 'fast', '-p', 'cgs-bench', '--bin', 'pagebreaks'],
  {
    stdio: 'inherit',
    cwd: root,
  },
);
if (built.status !== 0) throw new Error('The pagebreaks tool did not build.');

const scratch = mkdtempSync(join(tmpdir(), 'cgs-page-breaks-'));
const results = { generated: [], public: [], totals: {} };
try {
  const folder = join(root, 'bench/results/print-books');
  mkdirSync(folder, { recursive: true });
  for (const [name, bytes] of Object.entries(printBooks())) {
    if (only && !name.includes(only)) continue;
    const path = join(folder, name);
    writeFileSync(path, bytes);
    try {
      const sheets = compare(ours(path), excels(path, scratch), false);
      results.generated.push({ book: name, sheets });
      console.log(name, sheets.every((s) => s.match) ? 'match' : 'DIFFERENT');
    } catch (error) {
      results.generated.push({ book: name, error: String(error) });
      console.log(name, 'failed:', String(error).slice(0, 200));
    }
  }
  if (withPublic) {
    const publicFolder = join(root, 'tests/workbooks/public');
    for (const path of walk(publicFolder)) {
      if (statSync(path).size > 40 * 1024 * 1024) continue;
      let mine;
      try {
        mine = ours(path);
      } catch {
        continue;
      }
      if (!mine.some((sheet) => sheet.hasSetup)) continue;
      // A copy, so nothing Excel does can touch the test set.
      const copy = join(
        scratch,
        'public' + (path.toLowerCase().endsWith('.xlsm') ? '.xlsm' : '.xlsx'),
      );
      copyFileSync(path, copy);
      const name = relative(publicFolder, path);
      try {
        const sheets = compare(mine, excels(copy, scratch), true);
        results.public.push({ book: name, sheets });
        console.log(name, sheets.every((s) => s.match) ? 'pages match' : 'PAGES DIFFER');
      } catch (error) {
        results.public.push({ book: name, error: String(error) });
        console.log(name, 'failed:', String(error).slice(0, 200));
      } finally {
        rmSync(copy, { force: true });
      }
    }
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

const tally = (list) => {
  const sheets = list.flatMap((book) => book.sheets ?? []);
  return {
    books: list.length,
    failed: list.filter((book) => book.error).length,
    sheets: sheets.length,
    matched: sheets.filter((sheet) => sheet.match).length,
  };
};
results.totals = { generated: tally(results.generated), public: tally(results.public) };
writeFileSync(out, JSON.stringify(results, null, 2));
console.log(JSON.stringify(results.totals));
