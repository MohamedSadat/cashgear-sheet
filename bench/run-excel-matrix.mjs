// The COM-open matrix: saved files must open in Excel with no repair, after cell edits and after
// structural ones, and what a save kept must still work there. Only generated fixtures and, with `--private=<folder>`, your private
// workbooks go through it, as public files never open in Excel. Saved files go to a temporary
// folder that is always deleted, and originals are only read.
// Usage: node bench/run-excel-matrix.mjs <out.json> [--private=<folder>]
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { root } from '../scripts/lib.mjs';
import { readZip } from '../tests/fixtures/workbooks.mjs';

const out = process.argv[2];
const privateFolder = process.argv.find((arg) => arg.startsWith('--private='))?.slice(10);
const scratch = mkdtempSync(join(tmpdir(), 'cgs-excel-matrix-'));
const tool = join(root, 'target/fast/savecheck.exe');
const openScript = readFileSync(join(root, 'bench/excel-open.ps1'), 'utf8');
const featureScript = readFileSync(join(root, 'bench/excel-features.ps1'), 'utf8');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";
const fixtures = join(root, 'tests/workbooks/fixtures');

function powershell(script, parameters) {
  const result = join(scratch, 'result.json');
  rmSync(result, { force: true });
  const args = Object.entries({ ...parameters, Out: result })
    .map(([key, value]) => `-${key} ${quoted(value)}`)
    .join(' ');
  const run = spawnSync(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-Command', `& {${script}} ${args}`],
    { encoding: 'utf8', timeout: 600000, windowsHide: true },
  );
  if (run.status !== 0) throw new Error('Excel script failed: ' + run.stderr.slice(0, 300));
  return JSON.parse(readFileSync(result, 'utf8'));
}

const opens = (path) => powershell(openScript, { Workbook: resolve(path) });
const feature = (path, mode) => powershell(featureScript, { Workbook: resolve(path), Mode: mode });

let serial = 0;
/** Saves `source` with these edits, and returns the saved path and the save's report. */
function save(source, extension, sets = [], extra = [], commands = []) {
  const saved = join(scratch, `saved-${++serial}.${extension}`);
  const run = spawnSync(
    tool,
    [
      '--edit',
      source,
      saved,
      ...sets.map((set) => '--set=' + set),
      ...commands.map((command) => '--command=' + JSON.stringify(command)),
      ...extra,
    ],
    { encoding: 'utf8', windowsHide: true },
  );
  if (run.status !== 0) throw new Error('Saving failed: ' + run.stderr.slice(0, 300));
  return { saved, report: JSON.parse(run.stdout) };
}

/** Entry contents, by name, with hashes, as a zip this repo's fixtures wrote them. */
const hashes = (path) =>
  Object.fromEntries(
    readZip(readFileSync(path)).map(([name, body]) => [
      name,
      createHash('sha256').update(body).digest('hex'),
    ]),
  );

const results = {
  controls: {},
  scenarios: {},
  structural: {},
  generated: { files: 0, sourcesOpened: 0, savedOpened: 0, failures: [] },
  private: null,
  features: {},
};

// Scenario edits on the fixture with a table, a shared formula, an array and a calculation chain.
const STRUCTURES = {
  'one value': ['0,2,1,n:10'],
  'a new row far away': ['0,1000,100,n:1'],
  'the last cell of the sheet': ['0,1048576,16384,n:1'],
  'text with XML, a line break, a control character and something that looks escaped': [
    '0,20,1,t:a<b>&"c"\\nd\\u0001e_x0041_f عربي',
  ],
  'a boolean and an error': ['0,21,1,b:true', '0,21,2,e:#N/A'],
  'a new formula': ['0,22,1,f:SUM(A2:A3)&"<x>"'],
  'a cell cleared': ['0,2,2,clear'],
  'a formula cleared': ['0,2,3,clear'],
  'table header renamed': ['0,1,1,t:Renamed'],
  'table header cleared': ['0,1,2,clear'],
  'table header duplicated': ['0,1,1,t:Second'],
  'table header set to a number': ['0,1,1,n:5'],
  'table header set to a formula': ['0,1,1,f:1+1'],
  'a shared formula master replaced': ['0,5,2,n:9'],
  'a shared formula master cleared': ['0,5,2,clear'],
  'a shared formula follower replaced': ['0,6,2,f:A6*10'],
  'an array anchor replaced': ['0,9,4,n:1'],
  'an array cell replaced': ['0,9,5,n:1'],
  'a merged cell written': ['0,2,4,n:7'],
  'a precedent, so results change': ['0,5,1,n:100'],
};

const rows = (sheet, at, count, remove = false) => ({
  kind: 'axis',
  sheet,
  axis: 'row',
  at,
  count,
  delete: remove,
});
const cols = (sheet, at, count, remove = false) => ({
  kind: 'axis',
  sheet,
  axis: 'col',
  at,
  count,
  delete: remove,
});
const input = (sheet, row, col, text) => ({ kind: 'input', sheet, row, col, text });

// Structural edits, each on every fixture in `MOVED`. Some land on what a fixture holds and some
// miss it, and a command a fixture refuses changes nothing.
const MOVES = {
  'rows in at the top': [rows(0, 1, 2)],
  'rows in the middle': [rows(0, 4, 3)],
  'rows out of the middle': [rows(0, 3, 2, true)],
  'rows out across a merge': [rows(0, 9, 2, true)],
  'rows out across an array': [rows(0, 12, 2, true)],
  'rows out below everything': [rows(0, 100, 5, true)],
  'columns in at the left': [cols(0, 1, 2)],
  'columns in the middle': [cols(0, 3, 1)],
  'columns out of the middle': [cols(0, 2, 1, true)],
  'columns out past everything': [cols(0, 50, 3, true)],
  'a table column out': [cols(0, 3, 1, true)],
  'a whole table out': [cols(0, 1, 3, true)],
  'a table out by its rows': [rows(0, 1, 5, true)],
  'a shared formula master out': [rows(0, 2, 1, true)],
  'a sparkline cell out': [cols(0, 7, 1, true)],
  'everything out': [rows(0, 1, 20, true)],
  'rows and columns both': [rows(0, 2, 1), cols(0, 2, 1), rows(0, 5, 2, true)],
  'a sheet added at the front': [{ kind: 'insertSheet', position: 0 }],
  'a sheet added at the end': [{ kind: 'insertSheet', position: 99 }],
  'the first sheet renamed': [{ kind: 'renameSheet', sheet: 0, name: "It's new" }],
  'the first sheet deleted': [{ kind: 'deleteSheet', sheet: 0 }],
  'a sheet added, then deleted': [{ kind: 'insertSheet', position: 0 }, { kind: 'undo' }],
  'rows in, then undone': [rows(0, 2, 1), { kind: 'undo' }],
  'rows in, undone, redone': [rows(0, 2, 1), { kind: 'undo' }, { kind: 'redo' }],
  'a cell, then rows, then a cell': [input(0, 3, 2, '77'), rows(0, 2, 1), input(0, 2, 2, '=1+1')],
  'cells moved': [
    {
      kind: 'move',
      fromSheet: 0,
      from: { top: 2, left: 1, bottom: 3, right: 2 },
      sheet: 0,
      row: 30,
      col: 5,
    },
  ],
  'cells filled and pasted': [
    {
      kind: 'fill',
      sheet: 0,
      from: { top: 2, left: 2, bottom: 2, right: 2 },
      to: { top: 2, left: 2, bottom: 2, right: 6 },
    },
    {
      kind: 'paste',
      fromSheet: 0,
      from: { top: 1, left: 1, bottom: 2, right: 2 },
      sheet: 0,
      to: { top: 40, left: 1, bottom: 41, right: 2 },
    },
  ],
  'a dynamic array typed, then rows in': [input(0, 60, 1, '=SORT(B2:B5)'), rows(0, 50, 1)],
  'a total of everything moved': [
    rows(0, 2, 1),
    cols(0, 1, 1),
    { kind: 'renameSheet', sheet: 0, name: 'Renamed' },
    { kind: 'insertSheet', position: 1 },
    rows(0, 6, 1, true),
  ],
};
const format = (areas, change) => ({ kind: 'format', sheet: 0, areas, change });
const cells = (top, left, bottom = top, right = left) => ({ top, left, bottom, right });
const lines = (axis, first, last, extra) => ({
  kind: 'lines',
  sheet: 0,
  axis,
  first,
  last,
  ...extra,
});
// Each of what Phase 5 can do to a file, some on what a fixture holds and some past it.
Object.assign(MOVES, {
  'a font, a fill and a border': [
    format([cells(1, 1, 4, 3)], {
      bold: true,
      fontName: 'Arial',
      fontSize: 14,
      underline: 'double',
      fontColor: { kind: 'theme', theme: 4, tint: 0.4 },
      fill: { pattern: 'solid', color: { kind: 'rgb', rgb: 'FFEEDD' } },
      border: {
        top: { style: 'thick', color: { kind: 'rgb', rgb: 'FF0000' } },
        bottom: { style: 'double' },
        left: { style: 'dashed' },
        right: { style: 'slantDashDot' },
        insideHorizontal: { style: 'hair' },
        insideVertical: { style: 'dotted' },
      },
    }),
  ],
  'a pattern and alignment': [
    format([cells(2, 2, 6, 5)], {
      fill: {
        pattern: 'darkTrellis',
        color: { kind: 'theme', theme: 6 },
        patternColor: { kind: 'rgb', rgb: '00FF00' },
      },
      horizontal: 'centerContinuous',
      vertical: 'distributed',
      wrap: true,
      indent: 2,
      rotation: 135,
      readingOrder: 2,
    }),
  ],
  'a number format, then fewer decimals': [
    format([cells(1, 1, 10, 4)], { numberFormat: '[Red]#,##0.00" x";-0.0_);@' }),
    format([cells(1, 1, 10, 4)], { decimals: -1 }),
  ],
  'a diagonal border': [
    format([cells(3, 3, 5, 5)], {
      border: { diagonal: { style: 'medium' }, diagonalUp: true, diagonalDown: true },
    }),
  ],
  'formats cleared': [format([cells(1, 1, 20, 10)], { clear: true })],
  'a whole column': [format([cells(1, 2, 1048576, 2)], { italic: true, fontSize: 18 })],
  'a whole row': [
    format([cells(2, 1, 2, 16384)], {
      fill: { pattern: 'solid', color: { kind: 'rgb', rgb: 'CCFFCC' } },
    }),
  ],
  'the whole sheet': [format([cells(1, 1, 1048576, 16384)], { fontName: 'Tahoma', fontSize: 12 })],
  'a column and a row that cross': [
    format([cells(1, 3, 1048576, 3)], { bold: true }),
    format([cells(3, 1, 3, 16384)], { italic: true }),
  ],
  'a format, undone': [format([cells(1, 1, 3, 3)], { bold: true }), { kind: 'undo' }],
  'a format, undone, redone': [
    format([cells(1, 1, 3, 3)], { bold: true }),
    { kind: 'undo' },
    { kind: 'redo' },
  ],
  'a merge and centre': [{ kind: 'merge', sheet: 0, areas: [cells(30, 2, 32, 4)], center: true }],
  'a merge across': [{ kind: 'merge', sheet: 0, areas: [cells(34, 2, 36, 4)], across: true }],
  'a merge over cells': [{ kind: 'merge', sheet: 0, areas: [cells(2, 1, 3, 2)] }],
  'every merge undone': [{ kind: 'unmerge', sheet: 0, areas: [cells(1, 1, 1048576, 16384)] }],
  'rows and columns sized': [
    lines('row', 2, 4, { size: 40 }),
    lines('col', 2, 3, { size: 120 }),
    lines('col', 40, 50, { size: 10 }),
    lines('row', 5000, 5000, { size: 80 }),
  ],
  'rows and columns hidden': [
    lines('row', 3, 4, { hidden: true }),
    lines('col', 2, 2, { hidden: true }),
    lines('col', 16384, 16384, { hidden: true }),
    lines('row', 1048576, 1048576, { hidden: true }),
  ],
  'rows and columns hidden, then shown': [
    lines('row', 3, 4, { hidden: true }),
    lines('row', 3, 4, { hidden: false }),
    lines('col', 2, 2, { size: 0 }),
    lines('col', 2, 2, { hidden: false }),
  ],
  'panes frozen': [{ kind: 'freeze', sheet: 0, rows: 2, cols: 1 }],
  'the top row frozen, scrolled': [{ kind: 'freeze', sheet: 0, rows: 1, cols: 0, top: 5, left: 3 }],
  'the first column frozen': [{ kind: 'freeze', sheet: 0, rows: 0, cols: 1 }],
  'panes frozen, then not': [
    { kind: 'freeze', sheet: 0, rows: 2, cols: 2 },
    { kind: 'freeze', sheet: 0, rows: 0, cols: 0 },
  ],
  'no gridlines or headings': [{ kind: 'viewFlags', sheet: 0, grid: false, headings: false }],
  'cells inserted down': [{ kind: 'shiftCells', sheet: 0, area: cells(40, 2, 41, 3), down: true }],
  'cells inserted across': [
    { kind: 'shiftCells', sheet: 0, area: cells(40, 2, 41, 3), down: false },
  ],
  'cells deleted upward': [
    { kind: 'shiftCells', sheet: 0, area: cells(40, 2, 41, 3), down: true, delete: true },
  ],
  'cells deleted leftward': [
    { kind: 'shiftCells', sheet: 0, area: cells(40, 2, 41, 3), down: false, delete: true },
  ],
  'cells in use inserted down': [
    { kind: 'shiftCells', sheet: 0, area: cells(1, 1, 1, 1), down: true },
  ],
  'cells in use deleted': [
    { kind: 'shiftCells', sheet: 0, area: cells(2, 2, 2, 2), down: false, delete: true },
  ],
  'cells shifted, then rows in': [
    { kind: 'shiftCells', sheet: 0, area: cells(40, 2, 41, 3), down: true },
    rows(0, 2, 1),
    format([cells(50, 1, 50, 3)], { bold: true }),
  ],
  'a replace': [
    {
      kind: 'replaceAll',
      query: { text: 'a', sheet: 0, formulas: true },
      with: 'b',
    },
  ],
  'a size, a format and a row out': [
    lines('row', 4, 4, { size: 50 }),
    format([cells(4, 1, 6, 3)], { bold: true }),
    rows(0, 5, 1, true),
    cols(0, 1, 1),
  ],
});

const MOVED = {
  'moving-parts.xlsx': 'xlsx',
  'prefixed.xlsx': 'xlsx',
  'formatting.xlsx': 'xlsx',
  'structures.xlsx': 'xlsx',
  'pivot.xlsx': 'xlsx',
  'arabic.xlsx': 'xlsx',
  'defaults.xlsx': 'xlsx',
  'macro.xlsm': 'xlsm',
};

try {
  spawnSync('node', [join(root, 'scripts/write-fixtures.mjs')], { stdio: 'inherit' });
  const built = spawnSync(
    'cargo',
    ['build', '--profile', 'fast', '-p', 'cgs-bench', '--bin', 'savecheck'],
    { stdio: 'inherit', cwd: root },
  );
  if (built.status !== 0) throw new Error('The check tool did not build.');

  // If these open, the check can't see a repair, and nothing below would mean anything.
  const brokenFolder = join(root, 'tests/workbooks/broken');
  for (const name of readdirSync(brokenFolder)) {
    const seen = opens(join(brokenFolder, name));
    results.controls[name] = seen.opened && !seen.repaired ? 'NOT SEEN' : 'refused';
    console.log('control', name, results.controls[name]);
  }

  for (const [name, sets] of Object.entries(STRUCTURES)) {
    const { saved, report } = save(join(fixtures, 'structures.xlsx'), 'xlsx', sets);
    const seen = opens(saved);
    results.scenarios[name] = {
      clean: seen.opened && !seen.repaired,
      recalculates: report.recalculatesOnOpen,
      removed: report.removedParts,
      headersFixed: report.tableHeadersFixed,
    };
    console.log('scenario', name, results.scenarios[name].clean ? 'clean' : 'REPAIR');
  }
  // A new blank workbook, as it is and with one of each kind of cell.
  for (const [name, sets] of Object.entries({
    'blank as it is': [],
    'blank with cells': ['0,1,1,n:1', '0,1,2,t:text', '0,1,3,f:A1+1', '0,2,1,b:false'],
  })) {
    const seen = opens(save('blank', 'xlsx', sets).saved);
    results.scenarios[name] = { clean: seen.opened && !seen.repaired };
    console.log('scenario', name, results.scenarios[name].clean ? 'clean' : 'REPAIR');
  }

  // Every structural edit over every fixture.
  for (const [file, extension] of Object.entries(MOVED)) {
    const source = join(fixtures, file);
    const before = opens(source);
    if (!before.opened || before.repaired) throw new Error(file + ' does not open cleanly.');
    results.structural[file] = { clean: 0, repaired: [] };
    for (const [name, commands] of Object.entries(MOVES)) {
      const { saved } = save(source, extension, [], [], commands);
      const seen = opens(saved);
      if (seen.opened && !seen.repaired) results.structural[file].clean++;
      else results.structural[file].repaired.push(name);
      rmSync(saved, { force: true });
    }
    console.log('structural', file, results.structural[file]);
  }

  // What a structural edit must leave working: the chart, the pivot and the macros.
  {
    const moved = save(join(fixtures, 'moving-parts.xlsx'), 'xlsx', [], [], [rows(0, 1, 2)]);
    results.features.chartAfterRowsIn = feature(moved.saved, 'chart');
    const pivot = save(join(fixtures, 'pivot.xlsx'), 'xlsx', [], [], [rows(0, 1, 2)]);
    results.features.pivotAfterRowsIn = {
      opens: opens(pivot.saved).opened,
      excel: feature(pivot.saved, 'pivot'),
      changedParts: pivot.report.changedParts,
    };
    const source = join(fixtures, 'macro.xlsm');
    const macro = save(source, 'xlsm', [], [], [rows(0, 50, 2)]);
    results.features.macroAfterRowsIn = {
      projectIdentical:
        hashes(macro.saved)['xl/vbaProject.bin'] === hashes(source)['xl/vbaProject.bin'],
      opens: opens(macro.saved).opened,
    };
  }

  // Each kept feature, after a save that changed cells around it.
  {
    const source = join(fixtures, 'formatting.xlsx');
    const original = hashes(source);
    const { saved, report } = save(source, 'xlsx', ['0,100,1,n:1']);
    const after = hashes(saved);
    const kept = Object.keys(original).filter(
      (name) => /charts|drawings|media/.test(name) && after[name] === original[name],
    );
    const all = Object.keys(original).filter((name) => /charts|drawings|media/.test(name));
    results.features.chart = {
      partsKept: `${kept.length} of ${all.length}`,
      opens: opens(saved).opened,
      excel: feature(saved, 'chart'),
      untouchedBy: report.changedParts,
    };
  }
  {
    const source = join(fixtures, 'pivot.xlsx');
    const original = hashes(source);
    const edited = ['0,2,2,n:1000'];
    const plain = save(source, 'xlsx', edited);
    const after = hashes(plain.saved);
    const all = Object.keys(original).filter((name) => /pivot/i.test(name));
    results.features.pivot = {
      partsKept: `${all.filter((name) => after[name] === original[name]).length} of ${all.length}`,
      opens: opens(plain.saved).opened,
      // Data changed, and nothing asked for a refresh: as Excel leaves a pivot after an edit.
      excel: feature(plain.saved, 'pivot'),
    };
    // The spike: is `refreshOnLoad` enough to bring a pivot up to date by itself?
    const refresh = save(source, 'xlsx', edited, ['--refresh-pivots']);
    const fresh = hashes(refresh.saved);
    results.features.pivotRefreshOnLoad = {
      changedParts: refresh.report.changedParts,
      recordsKept:
        fresh['xl/pivotCache/pivotCacheRecords1.xml'] ===
        original['xl/pivotCache/pivotCacheRecords1.xml'],
      opens: opens(refresh.saved).opened,
      excel: feature(refresh.saved, 'pivot'),
      readOnlyOpen: feature(refresh.saved, 'pivotReadOnly'),
    };
    // And with no cell edits at all, only the flag.
    const flagOnly = save(source, 'xlsx', [], ['--refresh-pivots']);
    results.features.pivotFlagOnly = {
      changedParts: flagOnly.report.changedParts,
      recalculates: flagOnly.report.recalculatesOnOpen,
      excel: feature(flagOnly.saved, 'pivot'),
    };
  }
  {
    const source = join(fixtures, 'macro.xlsm');
    const original = hashes(source);
    const untouched = save(source, 'xlsm');
    const edited = save(source, 'xlsm', ['0,1,1,n:100']);
    const after = hashes(edited.saved);
    results.features.macro = {
      projectIdentical: after['xl/vbaProject.bin'] === original['xl/vbaProject.bin'],
      source: feature(source, 'macro'),
      noEdit: feature(untouched.saved, 'macro'),
      edited: feature(edited.saved, 'macro'),
      expected: 'the source and no-edit saves give 42, and the edit gives 200',
    };
  }

  // Everything else this repo generates, with the checker's general test edits.
  const keep = join(scratch, 'kept');
  const jsonl = join(scratch, 'kept.jsonl');
  const generated = [fixtures, join(root, 'tests/workbooks/generated')];
  const checked = spawnSync(tool, [jsonl, ...generated, `--keep=${keep}`, '--jobs=4'], {
    stdio: 'inherit',
  });
  if (checked.status !== 0) throw new Error('The checker failed.');
  for (const line of readFileSync(jsonl, 'utf8').split('\n').filter(Boolean)) {
    const record = JSON.parse(line);
    if (!record.report.saved) {
      results.generated.failures.push({ file: record.file, error: record.report.error });
      continue;
    }
    if (statSync(record.report.saved).size > 40 * 1024 * 1024) continue;
    results.generated.files++;
    const before = opens(record.source);
    const after = opens(record.report.saved);
    if (before.opened && !before.repaired) results.generated.sourcesOpened++;
    if (after.opened && !after.repaired) results.generated.savedOpened++;
    else results.generated.failures.push({ file: record.file, opened: after.opened });
    console.log('generated', record.file.split('/').pop(), after.opened ? 'clean' : 'REPAIR');
    rmSync(record.report.saved, { force: true });
  }

  if (privateFolder) {
    // Counts only, from here on. Nothing names a file or says what is in one.
    const privateKeep = join(scratch, 'private');
    mkdirSync(privateKeep);
    const privateJsonl = join(scratch, 'private.jsonl');
    const run = spawnSync(
      tool,
      [privateJsonl, privateFolder, `--keep=${privateKeep}`, '--jobs=4', '--private'],
      { stdio: 'ignore' },
    );
    if (run.status !== 0) throw new Error('The checker failed on the private set.');
    const summary = {
      files: 0,
      sourcesOpened: 0,
      savedOpened: 0,
      entriesChecked: 0,
      entriesDifferent: 0,
      cellsCompared: 0,
      cellsMatched: 0,
      failed: 0,
    };
    const lines = readFileSync(privateJsonl, 'utf8').split('\n').filter(Boolean);
    for (const line of lines) {
      const record = JSON.parse(line);
      summary.files++;
      if (!record.report.saved) {
        summary.failed++;
        continue;
      }
      summary.entriesChecked += record.report.entries_checked;
      summary.entriesDifferent += record.report.entries_different;
      summary.cellsCompared += record.report.compared;
      summary.cellsMatched += record.report.matched;
      const before = opens(record.source);
      const after = opens(record.report.saved);
      if (before.opened && !before.repaired) summary.sourcesOpened++;
      if (after.opened && !after.repaired) summary.savedOpened++;
      // A row and a column in on the first sheet, a sheet added, the first one renamed.
      summary.structuralTried = (summary.structuralTried ?? 0) + 1;
      try {
        const extension = record.source.split('.').pop();
        const { saved } = save(
          record.source,
          extension,
          [],
          [],
          [
            rows(0, 2, 1),
            cols(0, 2, 1),
            { kind: 'insertSheet', position: 0 },
            { kind: 'renameSheet', sheet: 0, name: 'Renamed sheet' },
            { kind: 'format', sheet: 0, areas: [cells(1, 1, 3, 3)], change: { bold: true } },
            { kind: 'lines', sheet: 0, axis: 'col', first: 2, last: 2, size: 90 },
            { kind: 'freeze', sheet: 0, rows: 1, cols: 1 },
          ],
        );
        const seen = opens(saved);
        if (seen.opened && !seen.repaired) {
          summary.structuralOpened = (summary.structuralOpened ?? 0) + 1;
        }
        rmSync(saved, { force: true });
      } catch {
        summary.structuralFailed = (summary.structuralFailed ?? 0) + 1;
      }
    }
    results.private = summary;
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
  writeFileSync(out, JSON.stringify(results, null, 1) + '\n');
}
console.log(JSON.stringify(results.features, null, 1));
const scenarios = Object.values(results.scenarios);
console.log(
  `${scenarios.filter((entry) => entry.clean).length} of ${scenarios.length} scenarios clean. Generated: ${results.generated.savedOpened} of ${results.generated.files}. Private: ${JSON.stringify(results.private)}`,
);
