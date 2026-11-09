// Phase 13's Excel check: each case makes changes through the engine and saves, Excel 2016 opens
// the file over COM with no repair and reads back what the case expects, and the app reads Excel's
// own save of it back the same. Generated workbooks and fixtures only.
// Usage: node bench/run-parity-check.mjs [--only=<text>] [out.json]
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { root } from '../scripts/lib.mjs';

const args = process.argv.slice(2);
const only = args.find((arg) => arg.startsWith('--only='))?.slice(7);
const out = resolve(
  args.find((arg) => !arg.startsWith('--')) ?? join(root, 'bench/results/parity-check.json'),
);
const save = join(root, 'target/fast/savecheck.exe');
const dump = join(root, 'target/fast/paritydump.exe');
const script = readFileSync(join(root, 'bench/excel-parity.ps1'), 'utf8');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";
const fixtures = join(root, 'tests/workbooks/fixtures');
// Fixed salts, so a run is repeatable; the app picks random ones.
const SALT = 'mZgorKXCxX0pnb7n04x0bA==';
const cell = (row, col) => ({ top: row, left: col, bottom: row, right: col });
const NOTHING_ALLOWED = {
  formatCells: false,
  formatColumns: false,
  formatRows: false,
  insertColumns: false,
  insertRows: false,
  insertHyperlinks: false,
  deleteColumns: false,
  deleteRows: false,
  sort: false,
  autoFilter: false,
  pivotTables: false,
};
const EVERYTHING_ALLOWED = Object.fromEntries(Object.keys(NOTHING_ALLOWED).map((k) => [k, true]));

/**
 * Each case: a source, the app's commands, what Excel must read (`expect`: per sheet name, the
 * fields to compare; `cells` likewise; `book`), and passwords Excel must or mustn't take.
 */
const CASES = {
  'hiding and showing sheets, with the active one moving as Excel moves it': {
    source: 'blank',
    commands: [
      { kind: 'insertSheet', position: 1 },
      { kind: 'insertSheet', position: 2 },
      { kind: 'insertSheet', position: 3 },
      { kind: 'activeSheet', sheet: 1 },
      // The active sheet hidden: the next one to its right shows.
      { kind: 'sheetState', sheet: 1, hidden: true },
      { kind: 'sheetState', sheet: 3, hidden: true },
      { kind: 'sheetState', sheet: 3, hidden: false },
      { kind: 'sheetState', sheet: 0, hidden: true },
    ],
    expect: {
      Sheet1: { visible: 0, active: false },
      Sheet2: { visible: 0, active: false },
      Sheet3: { visible: -1, active: false },
      Sheet4: { visible: -1, active: true },
    },
  },
  'zoom saved per sheet, with nothing else changed': {
    source: join(fixtures, 'formatting.xlsx'),
    commands: [
      { kind: 'zoom', sheet: 0, zoom: 150 },
      { kind: 'zoom', sheet: 1, zoom: 55 },
    ],
    expectSheets: (sheets) => sheets[0].zoom === 150 && sheets[1].zoom === 55,
  },
  'a sheet of the fixture hidden beside its charts': {
    source: join(fixtures, 'moving-parts.xlsx'),
    commands: [{ kind: 'sheetState', sheet: 1, hidden: true }],
    expectSheets: (sheets) => sheets[1].visible === 0 && sheets[0].visible === -1,
  },
  'Protect Sheet with a password and some options, and Protect Workbook': {
    source: 'blank',
    commands: [
      { kind: 'input', sheet: 0, row: 1, col: 1, text: '1' },
      { kind: 'input', sheet: 0, row: 1, col: 2, text: '2' },
      { kind: 'input', sheet: 0, row: 1, col: 3, text: '=A1+B1' },
      { kind: 'format', sheet: 0, areas: [cell(1, 2)], change: { locked: false } },
      { kind: 'format', sheet: 0, areas: [cell(1, 3)], change: { formulaHidden: true } },
      { kind: 'insertSheet', position: 1 },
      {
        kind: 'protectSheet',
        sheet: 0,
        password: 'abc',
        salt: SALT,
        allow: { formatCells: true, insertRows: true, sort: true, autoFilter: true },
      },
      { kind: 'protectWorkbook', structure: true, password: 'wb', salt: SALT },
    ],
    expect: {
      Sheet1: {
        protected: true,
        objects: true,
        scenarios: true,
        selection: 0,
        allow: {
          ...NOTHING_ALLOWED,
          formatCells: true,
          insertRows: true,
          sort: true,
          autoFilter: true,
        },
      },
      Sheet2: { protected: false },
    },
    cells: [
      { sheet: 'Sheet1', cell: 'A1', locked: true, hidden: false },
      { sheet: 'Sheet1', cell: 'B1', locked: false, hidden: false },
      { sheet: 'Sheet1', cell: 'C1', locked: true, hidden: true },
    ],
    book: { structure: true },
    unprotect: [
      { sheet: 'Sheet1', password: 'ABC', unprotected: false },
      { sheet: 'Sheet1', password: 'abc', unprotected: true },
      { password: 'nope', unprotected: false },
      { password: 'wb', unprotected: true },
    ],
  },
  'Protect Sheet with no password, objects editable and only unlocked cells selectable': {
    source: 'blank',
    commands: [
      {
        kind: 'protectSheet',
        sheet: 0,
        allow: { selectLocked: false, editObjects: true, ...EVERYTHING_ALLOWED },
      },
    ],
    expect: {
      Sheet1: { protected: true, objects: false, selection: 1, allow: EVERYTHING_ALLOWED },
    },
    unprotect: [{ sheet: 'Sheet1', password: '', unprotected: true }],
  },
  'Protect Sheet with nothing selectable, then the fixture edited around it': {
    source: join(fixtures, 'structures.xlsx'),
    commands: [
      { kind: 'protectSheet', sheet: 0, allow: { selectLocked: false, selectUnlocked: false } },
      { kind: 'input', sheet: 1, row: 1, col: 1, text: 'beside' },
    ],
    expectSheets: (sheets) => sheets[0].protected && sheets[0].selection === -4142,
  },
  'notes written, shown and edited, in the box Excel 2016 gives them': {
    source: 'blank',
    commands: [
      {
        kind: 'setNote',
        sheet: 0,
        row: 2,
        col: 2,
        text: 'Mohamed Sadat:\nhello',
        author: 'Mohamed Sadat',
      },
      { kind: 'setNote', sheet: 0, row: 6, col: 4, text: 'plain', author: 'Ann' },
      { kind: 'noteVisible', sheet: 0, row: 6, col: 4, visible: true },
      { kind: 'setNote', sheet: 0, row: 6, col: 4, text: 'plain, edited', author: 'Bob' },
    ],
    cells: [
      {
        sheet: 'Sheet1',
        cell: 'B2',
        note: { author: 'Mohamed Sadat', text: 'Mohamed Sadat:\nhello', visible: false },
      },
      {
        sheet: 'Sheet1',
        cell: 'D6',
        note: { author: 'Ann', text: 'plain, edited', visible: true },
      },
    ],
    // The size Excel gives a new note, in points.
    excelCells: [
      { sheet: 'Sheet1', cell: 'B2', note: { width: 108, height: 59.25 } },
      { sheet: 'Sheet1', cell: 'D6', note: { width: 108, height: 59.25 } },
    ],
  },
  'Remove Hyperlink takes the look off, and Clear Hyperlinks keeps it': {
    source: 'blank',
    commands: [
      {
        kind: 'setLink',
        sheet: 0,
        area: cell(8, 2),
        link: { address: 'https://example.com/', text: 'site' },
      },
      {
        kind: 'setLink',
        sheet: 0,
        area: cell(9, 2),
        link: { address: 'https://example.com/', text: 'site' },
      },
      { kind: 'setLink', sheet: 0, area: cell(8, 2), plain: true },
      { kind: 'setLink', sheet: 0, area: cell(9, 2) },
    ],
    cells: [
      { sheet: 'Sheet1', cell: 'B8', links: 0, underline: -4142 },
      { sheet: 'Sheet1', cell: 'B9', links: 0, underline: 2 },
    ],
  },
  'error checking, with errors ignored here, as Excel 2016 finds them': {
    source: 'blank',
    commands: [
      ...[
        ['A1', '1'],
        ['A2', '2'],
        ['A3', '3'],
        ['A4', '4'],
        ['B1', '2'],
        ['B2', '4'],
        ['B3', '6'],
        ['B4', '8'],
        ['C1', '3'],
        ['C2', '6'],
        ['C3', '9'],
        ['C4', '12'],
        ['A5', '=SUM(A1:A4)'],
        ['B5', '=SUM(B1:B3)'],
        ['C5', '=SUM(C1:C4)'],
        ['F1', '=1/0'],
        ['F4', "'456"],
        ['F5', "'1/1/25"],
        ['F6', '=DATEVALUE("1/1/25")'],
        ['H1', '=A1+1'],
        ['J1', '1'],
        ['J2', '2'],
        ['J3', '3'],
        ['J4', '=SUM(J2:J3)'],
      ].map(([at, text]) => {
        const col = at.charCodeAt(0) - 64;
        return { kind: 'input', sheet: 0, row: Number(at.slice(1)), col, text };
      }),
      { kind: 'format', sheet: 0, areas: [cell(1, 8)], change: { locked: false } },
      { kind: 'ignoreError', sheet: 0, row: 4, col: 6 },
      { kind: 'ignoreError', sheet: 0, row: 4, col: 10 },
    ],
    excelCells: [
      { sheet: 'Sheet1', cell: 'B5', errors: 'inconsistentFormula,omittedCells', ignored: '' },
      { sheet: 'Sheet1', cell: 'F1', errors: 'evaluateToError', ignored: '' },
      { sheet: 'Sheet1', cell: 'F4', errors: 'numberAsText', ignored: 'numberAsText' },
      { sheet: 'Sheet1', cell: 'F5', errors: 'textDate', ignored: '' },
      { sheet: 'Sheet1', cell: 'F6', errors: 'textDate', ignored: '' },
      { sheet: 'Sheet1', cell: 'H1', errors: 'unlockedFormula', ignored: '' },
      { sheet: 'Sheet1', cell: 'J4', errors: 'omittedCells', ignored: 'omittedCells' },
      { sheet: 'Sheet1', cell: 'A5', errors: '', ignored: '' },
    ],
  },
  'rich text typed with runs, pasted and moved, as Excel reads each character': {
    source: 'blank',
    commands: [
      {
        kind: 'richInput',
        sheet: 0,
        row: 1,
        col: 1,
        text: 'H2O is wet',
        runs: [
          { start: 0 },
          { start: 1, font: { vertAlign: 'subscript' } },
          { start: 2 },
          { start: 7, font: { bold: true, italic: true } },
        ],
      },
      {
        kind: 'richInput',
        sheet: 0,
        row: 2,
        col: 1,
        text: 'x2',
        runs: [{ start: 0 }, { start: 1, font: { vertAlign: 'superscript' } }],
      },
      { kind: 'paste', fromSheet: 0, from: cell(1, 1), sheet: 0, to: cell(4, 3) },
      { kind: 'move', fromSheet: 0, from: cell(2, 1), sheet: 0, row: 6, col: 2 },
      { kind: 'format', sheet: 0, areas: [cell(8, 1)], change: { vertAlign: 'superscript' } },
      { kind: 'input', sheet: 0, row: 8, col: 1, text: 'up' },
    ],
    excelCells: [
      { sheet: 'Sheet1', cell: 'A1', chars: '|_||||||bi|bi|bi' },
      { sheet: 'Sheet1', cell: 'C4', chars: '|_||||||bi|bi|bi' },
      { sheet: 'Sheet1', cell: 'B6', chars: '|^' },
      { sheet: 'Sheet1', cell: 'A8', chars: '^|^' },
    ],
  },
  'protection taken off again': {
    source: 'blank',
    commands: [
      { kind: 'protectSheet', sheet: 0, password: 'abc', salt: SALT },
      { kind: 'protectWorkbook', password: 'wb', salt: SALT },
      { kind: 'unprotectSheet', sheet: 0, password: 'abc' },
      { kind: 'unprotectWorkbook', password: 'wb' },
      { kind: 'input', sheet: 0, row: 1, col: 1, text: 'free' },
    ],
    expect: { Sheet1: { protected: false } },
    book: { structure: false },
  },
};

/** Whether `actual` has every field `expected` names, deeply. */
function matches(actual, expected) {
  if (expected === null || typeof expected !== 'object') return actual === expected;
  return Object.entries(expected).every(([key, value]) => matches(actual?.[key], value));
}

function judge(reading, { expect, expectSheets, cells, excelCells, book }) {
  const problems = [];
  for (const [name, fields] of Object.entries(expect ?? {})) {
    const sheet = reading.sheets.find((entry) => entry.name === name);
    if (!matches(sheet, fields)) problems.push(`${name}: ${JSON.stringify(sheet)}`);
  }
  if (expectSheets && !expectSheets(reading.sheets)) {
    problems.push('sheets: ' + JSON.stringify(reading.sheets));
  }
  for (const want of [...(cells ?? []), ...(excelCells ?? [])]) {
    const got = reading.cells.find((one) => one.sheet === want.sheet && one.cell === want.cell);
    const fields = Object.fromEntries(
      Object.entries(want).filter(([key]) => key !== 'sheet' && key !== 'cell'),
    );
    if (!matches(got, fields)) problems.push(`${want.sheet}!${want.cell}: ${JSON.stringify(got)}`);
  }
  if (book && !matches(reading.book, book)) problems.push('book: ' + JSON.stringify(reading.book));
  return problems;
}

spawnSync('node', [join(root, 'scripts/write-fixtures.mjs')], { stdio: 'inherit' });
for (const bin of ['savecheck', 'paritydump']) {
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

const scratch = mkdtempSync(join(tmpdir(), 'cgs-parity-'));
const results = [];
try {
  for (const [name, spec] of Object.entries(CASES)) {
    if (only && !name.includes(only)) continue;
    const saved = join(scratch, 'saved.xlsx');
    const copy = join(scratch, 'excel-copy.xlsx');
    const probe = join(scratch, 'probe.json');
    const target = join(scratch, 'excel.json');
    rmSync(copy, { force: true });
    const made = spawnSync(
      save,
      ['--edit', spec.source, saved, ...spec.commands.map((c) => '--command=' + JSON.stringify(c))],
      { encoding: 'utf8' },
    );
    if (made.status !== 0) throw new Error(`${name}: ${made.stderr.slice(0, 400)}`);
    writeFileSync(
      probe,
      JSON.stringify({
        cells: [...(spec.cells ?? []), ...(spec.excelCells ?? [])].map(
          ({ sheet, cell, chars }) => ({ sheet, cell, chars: chars !== undefined }),
        ),
        unprotect: (spec.unprotect ?? []).map(({ sheet, password }) => ({ sheet, password })),
      }),
    );
    const run = spawnSync(
      'powershell',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        `& {${script}} -Workbook ${quoted(saved)} -Probe ${quoted(probe)} -Copy ${quoted(copy)} -Out ${quoted(target)}`,
      ],
      { encoding: 'utf8', timeout: 300000, windowsHide: true },
    );
    if (run.status !== 0) throw new Error(`${name}: Excel: ${run.stderr.slice(0, 400)}`);
    // PowerShell writes UTF-8 with a byte order mark, which goes with anything before the JSON.
    const excel = JSON.parse(readFileSync(target, 'utf8').replace(/^[^{[]*/, ''));
    excel.sheets = [excel.sheets ?? []].flat();
    excel.cells = [excel.cells ?? []].flat();
    if (process.env.PARITY_DEBUG) console.log(JSON.stringify(excel));
    const problems = excel.error ? [excel.error] : judge(excel, spec);
    for (const want of spec.unprotect ?? []) {
      const got = [excel.book?.unprotected ?? []]
        .flat()
        .find((one) => (one.sheet || '') === (want.sheet ?? '') && one.password === want.password);
      if (got?.unprotected !== want.unprotected) {
        problems.push(`password ${JSON.stringify(want.password)}: ${JSON.stringify(got)}`);
      }
    }
    // The app reads Excel's own save the same way.
    let back = [];
    if (!excel.error) {
      const read = spawnSync(
        dump,
        [copy, ...(spec.cells ?? []).map(({ sheet, cell }) => `--cell=${sheet}!${cell}`)],
        { encoding: 'utf8' },
      );
      if (read.status !== 0) back = ['paritydump: ' + read.stderr.slice(0, 300)];
      else back = judge(JSON.parse(read.stdout), { ...spec, excelCells: [] });
    }
    const pass = excel.opened && !excel.repaired && problems.length === 0 && back.length === 0;
    results.push({
      case: name,
      pass,
      opened: excel.opened,
      repaired: excel.repaired,
      problems,
      readBack: back,
    });
    console.log(`${pass ? 'ok  ' : 'FAIL'} ${name}`);
    for (const problem of [...problems, ...back.map((text) => 'read back: ' + text)]) {
      console.log('     ' + problem);
    }
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
writeFileSync(out, JSON.stringify(results, null, 2) + '\n');
const passed = results.filter((result) => result.pass).length;
console.log(`${passed} of ${results.length} cases agree with Excel 2016.`);
if (passed !== results.length) process.exitCode = 1;
