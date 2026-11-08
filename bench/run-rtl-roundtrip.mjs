// Phase 11's round trip: Sheet Right-to-Left set or cleared in the app opens in Excel as the app
// meant it, with no repair and every drawing in the same cells, and Excel's own save of it reads
// back the same in the app. Generated workbooks and fixtures only.
// Usage: node bench/run-rtl-roundtrip.mjs [out.json]
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { root } from '../scripts/lib.mjs';
import { readZip } from '../tests/fixtures/workbooks.mjs';

const out = resolve(process.argv[2] ?? join(root, 'bench/results/rtl-roundtrip.json'));
const save = join(root, 'target/fast/savecheck.exe');
const pages = join(root, 'target/fast/pagebreaks.exe');
const script = readFileSync(join(root, 'bench/excel-direction.ps1'), 'utf8');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";
const fixtures = join(root, 'tests/workbooks/fixtures');
const flip = (sheet, on) => ({ kind: 'viewFlags', sheet, rightToLeft: on });

/** Each case: a source, the app's commands, and each worksheet's direction Excel must read. */
const CASES = {
  'a left-to-right sheet turned right to left, beside one left alone': {
    source: join(fixtures, 'formatting.xlsx'),
    commands: [flip(0, true)],
    expected: [true, false, false],
  },
  'a right-to-left sheet turned left to right': {
    source: join(fixtures, 'rtl.xlsx'),
    commands: [flip(0, false)],
    expected: [false],
  },
  'a right-to-left sheet edited and kept right to left': {
    source: join(fixtures, 'rtl.xlsx'),
    commands: [{ kind: 'input', sheet: 0, row: 12, col: 1, text: 'added' }],
    expected: [true],
  },
  'a new workbook with a new right-to-left sheet, and its first sheet turned': {
    source: 'blank',
    commands: [flip(0, true), { kind: 'insertSheet', position: 1, rightToLeft: true }],
    expected: [true, true],
  },
  'the Arabic sheet turned and turned back': {
    source: join(fixtures, 'arabic.xlsx'),
    commands: [flip(0, true), flip(0, false)],
    expected: [false],
  },
  'the moving parts turned right to left': {
    source: join(fixtures, 'moving-parts.xlsx'),
    commands: [flip(0, true), flip(1, true)],
    expected: [true, true],
  },
};

function directions(path) {
  const run = spawnSync(pages, [path], { encoding: 'utf8' });
  if (run.status !== 0) throw new Error('pagebreaks: ' + run.stderr.slice(0, 300));
  return JSON.parse(run.stdout).map((sheet) => sheet.rightToLeft);
}

/** Every drawing part's anchors, which a change of direction must leave as they were. */
function anchors(path) {
  return readZip(readFileSync(path))
    .filter(([name]) => /^xl\/drawings\/drawing\d+\.xml$/.test(name))
    .map(([name, body]) => [name, body.toString('utf8').match(/<xdr:from>.*?<\/xdr:from>/gs)]);
}

spawnSync('node', [join(root, 'scripts/write-fixtures.mjs')], { stdio: 'inherit' });
for (const bin of ['savecheck', 'pagebreaks']) {
  const built = spawnSync(
    'cargo',
    ['build', '--profile', 'fast', '-p', 'cgs-bench', '--bin', bin],
    {
      stdio: 'inherit',
      cwd: root,
    },
  );
  if (built.status !== 0) throw new Error(bin + ' did not build.');
}

const scratch = mkdtempSync(join(tmpdir(), 'cgs-rtl-'));
const results = [];
try {
  for (const [name, { source, commands, expected }] of Object.entries(CASES)) {
    const saved = join(scratch, 'saved.xlsx');
    const copy = join(scratch, 'excel-copy.xlsx');
    const target = join(scratch, 'excel.json');
    rmSync(copy, { force: true });
    const made = spawnSync(
      save,
      ['--edit', source, saved, ...commands.map((c) => '--command=' + JSON.stringify(c))],
      { encoding: 'utf8' },
    );
    if (made.status !== 0) throw new Error(`${name}: ${made.stderr.slice(0, 300)}`);
    const run = spawnSync(
      'powershell',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        `& {${script}} -Workbook ${quoted(saved)} -Copy ${quoted(copy)} -Out ${quoted(target)}`,
      ],
      { encoding: 'utf8', timeout: 300000, windowsHide: true },
    );
    if (run.status !== 0) throw new Error(`${name}: Excel: ${run.stderr.slice(0, 300)}`);
    const excel = JSON.parse(readFileSync(target, 'utf8').replace(/^﻿/, ''));
    const seen = [excel.sheets ?? []].flat().map((sheet) => sheet.rightToLeft);
    const ours = directions(saved);
    const back = excel.error ? null : directions(copy);
    // Anchors in the cells they were in, in the app's file and in Excel's own save.
    const sourceAnchors = source === 'blank' ? [] : anchors(source);
    const keptAnchors = JSON.stringify(anchors(saved)) === JSON.stringify(sourceAnchors);
    const same = (list) => JSON.stringify(list) === JSON.stringify(expected);
    const pass =
      excel.opened && !excel.repaired && !excel.error && same(seen) && same(ours) && same(back);
    results.push({
      case: name,
      pass: pass && keptAnchors,
      opened: excel.opened,
      repaired: excel.repaired,
      error: excel.error,
      expected,
      app: ours,
      excel: seen,
      appReadsExcelsCopy: back,
      anchorsKept: keptAnchors,
      excelDrawings: [excel.sheets ?? []].flat().map((sheet) => sheet.drawings),
    });
    console.log(name, pass && keptAnchors ? 'pass' : 'FAIL', JSON.stringify(results.at(-1)));
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
const passed = results.filter((result) => result.pass).length;
writeFileSync(out, JSON.stringify({ passed, cases: results.length, results }, null, 2));
console.log(`${passed} of ${results.length} cases pass`);
