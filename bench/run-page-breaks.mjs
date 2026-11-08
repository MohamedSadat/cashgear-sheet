// Phase 10's page breaks against Excel's. Generated books covering every page setup option go
// through Excel over COM, with Microsoft Print to PDF as its printer, and through the engine;
// every sheet's row and column breaks and page count must be the same. With `--public`, the
// public test files that hold page setup go through as well, as locked-down temporary copies
// (read-only, macros forced off, links not updated), and their page counts are compared. They go
// through one Excel session at a time in chunks, and the results are written after each chunk.
// With `--rtl` (Phase 11), the generated books are right to left, and only public sheets that are
// right to left count.
// Usage: node bench/run-page-breaks.mjs [out.json] [--public] [--rtl] [--only=name]
import { execFile, spawnSync } from 'node:child_process';
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
const rightToLeft = process.argv.includes('--rtl');
const only = process.argv.find((arg) => arg.startsWith('--only='))?.slice(7);
const tool = join(root, 'target/fast/pagebreaks.exe');
const script = readFileSync(join(root, 'bench/excel-page-breaks.ps1'), 'utf8');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";

/** As `ours`, without waiting, for scanning many books at once. */
function oursLater(path) {
  return new Promise((done, fail) => {
    execFile(
      tool,
      [path],
      { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, timeout: 120000, windowsHide: true },
      (error, stdout) => (error ? fail(error) : done(JSON.parse(stdout))),
    );
  });
}

function ours(path) {
  const run = spawnSync(tool, [path], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    timeout: 120000,
  });
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

/** Page counts for several workbooks from one Excel session, keyed by path. */
function excelCounts(paths, scratch) {
  const list = join(scratch, 'list.txt');
  const target = join(scratch, 'counts.jsonl');
  writeFileSync(list, paths.join('\n'), 'utf8');
  rmSync(target, { force: true });
  const run = spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `& {${script}} -List ${quoted(list)} -Out ${quoted(target)} -CountsOnly`,
    ],
    { encoding: 'utf8', timeout: 60000 * paths.length, windowsHide: true },
  );
  const found = new Map();
  let text = '';
  try {
    text = readFileSync(target, 'utf8');
  } catch {
    // Nothing came back: every book in the chunk is reported as failed below.
  }
  for (const line of text.split(/\r?\n/)) {
    if (!line.trim()) continue;
    const entry = JSON.parse(line.replace(/^\uFEFF/, ''));
    found.set(entry.book, entry);
  }
  if (found.size < paths.length && run.status !== 0) {
    const reason = 'Excel: ' + (run.error?.message ?? run.stderr.slice(0, 300));
    for (const path of paths) if (!found.has(path)) found.set(path, { book: path, error: reason });
  }
  return found;
}

const same = (a, b) => JSON.stringify(a ?? []) === JSON.stringify(b ?? []);

/** Each visible sheet compared: everything for generated books, page counts for public ones. */
function compare(mine, theirs, countsOnly) {
  const sheets = [];
  for (const sheet of theirs.sheets) {
    const ownSheet = mine.find((entry) => entry.sheet === sheet.sheet);
    if (!ownSheet) continue;
    if (countsOnly && !ownSheet.hasSetup) continue;
    if (countsOnly && rightToLeft && !ownSheet.rightToLeft) continue;
    const pagesMatch = ownSheet.pages === sheet.pages;
    // GET.DOCUMENT also lists where the printed range starts and ends, and breaks past it, so
    // only breaks inside what prints are compared.
    const [top, left, bottom, right] = ownSheet.extent;
    const inside = (list, first, last) => (list ?? []).filter((at) => at > first && at <= last);
    const breaksMatch =
      same(inside(ownSheet.rows, top, bottom), inside(sheet.rows, top, bottom)) &&
      same(inside(ownSheet.cols, left, right), inside(sheet.cols, left, right));
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
const tally = (list) => {
  const sheets = list.flatMap((book) => book.sheets ?? []);
  return {
    books: list.length,
    failed: list.filter((book) => book.error).length,
    skipped: list.filter((book) => book.skipped).length,
    sheets: sheets.length,
    matched: sheets.filter((sheet) => sheet.match).length,
  };
};
function save() {
  results.totals = { generated: tally(results.generated), public: tally(results.public) };
  writeFileSync(out, JSON.stringify(results, null, 2));
}
try {
  const folder = join(
    root,
    rightToLeft ? 'bench/results/print-books-rtl' : 'bench/results/print-books',
  );
  mkdirSync(folder, { recursive: true });
  for (const [name, bytes] of Object.entries(printBooks({ rightToLeft }))) {
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
    const books = [];
    const paths = walk(publicFolder).filter((path) => statSync(path).size <= 40 * 1024 * 1024);
    const scanned = new Array(paths.length);
    let next = 0;
    // Six at a time: the engine's side of a scan takes most of an hour one by one.
    await Promise.all(
      Array.from({ length: 6 }, async () => {
        while (next < paths.length) {
          const at = next++;
          scanned[at] = await oursLater(paths[at]).catch(() => null);
        }
      }),
    );
    for (const [at, path] of paths.entries()) {
      try {
        const mine = scanned[at];
        if (!mine) continue;
        if (!mine.some((sheet) => sheet.hasSetup && (!rightToLeft || sheet.rightToLeft))) continue;
        // Excel takes hours over a sheet styled to its last cell, which the engine counts in
        // millions of pages; such books are listed, not compared.
        if (mine.some((sheet) => sheet.pages > 10000)) {
          results.public.push({
            book: relative(join(root, 'tests/workbooks/public'), path),
            skipped: 'over 10,000 pages',
          });
          continue;
        }
        books.push({ path, mine });
      } catch {
        // A file the engine can't read has no pages to compare.
      }
    }
    console.log(books.length, 'public files hold page setup');
    const chunk = 25;
    for (let at = 0; at < books.length; at += chunk) {
      const part = books.slice(at, at + chunk);
      // Copies, so nothing Excel does can touch the test set.
      const copies = part.map(({ path }, index) => {
        const extension = path.toLowerCase().endsWith('.xlsm') ? '.xlsm' : '.xlsx';
        const copy = join(scratch, `public-${at + index}${extension}`);
        copyFileSync(path, copy);
        return copy;
      });
      const counts = excelCounts(copies, scratch);
      part.forEach(({ path, mine }, index) => {
        const name = relative(publicFolder, path);
        const theirs = counts.get(copies[index]);
        if (!theirs || theirs.error) {
          const error = theirs?.error ?? 'Excel gave nothing back';
          results.public.push({ book: name, error });
          console.log(name, 'failed:', error.slice(0, 200));
          return;
        }
        const sheets = compare(mine, theirs.result, true);
        results.public.push({ book: name, sheets });
        console.log(name, sheets.every((s) => s.match) ? 'pages match' : 'PAGES DIFFER');
      });
      for (const copy of copies) rmSync(copy, { force: true });
      save();
    }
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

save();
console.log(JSON.stringify(results.totals));
