// Phase 6's "rules match Excel": conditional formats, validation, filters and sorts made through the
// engine are judged by Excel 2016 over COM. `rulecheck` builds each case in a new workbook, saves it,
// and says what the engine made of it; `excel-rules.ps1` has Excel read the same file (or sort the
// unsorted copy itself), and this compares the two cell by cell. Generated workbooks only, in a
// temporary folder that is always deleted.
// Usage: node bench/run-excel-rules.mjs [out.json]
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../scripts/lib.mjs';

const out = process.argv[2] ?? join(root, 'bench/results/excel-rules.json');
const scratch = mkdtempSync(join(tmpdir(), 'cgs-excel-rules-'));
const tool = join(root, 'target/fast/rulecheck.exe');
const script = readFileSync(join(root, 'bench/excel-rules.ps1'), 'utf8');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";

// Today as Excel counts it, and dates around it typed as Excel reads them on en-US.
const now = new Date();
const midnight = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
const today = Math.round(midnight / 86400000) + 25569;
const day = (offset) => {
  const date = new Date(midnight + offset * 86400000);
  return `${date.getUTCMonth() + 1}/${date.getUTCDate()}/${date.getUTCFullYear()}`;
};

const NAMES = ['ann', 'Bob', 'cy', 'ann', 'dee', 'ED', 'fay', 'bob', 'gus', '', 'Hal', 'ann'];
const QTY = ['5', '12', '-3', '7', '0', '12', '2.5', 'x', '', '100', '7', '=1/0'];
const DATES = [-1, 0, 1, -5, -10, -40, 35, -370, 3, -2, -31, 0].map(day);
const NOTES = [
  'apple pie',
  'Banana',
  'cherry',
  'apple',
  '',
  'date',
  'Apple',
  'egg',
  'banana split',
  'fig',
  'grape',
  'Cherry',
];
const cells = [
  [1, 1, 'Name'],
  [1, 2, 'Qty'],
  [1, 3, 'Date'],
  [1, 4, 'Note'],
];
for (let at = 0; at < 12; at++) {
  for (const [col, list] of [NAMES, QTY, DATES, NOTES].entries()) {
    if (list[at] !== '') cells.push([at + 2, col + 1, list[at]]);
  }
}
const AREA = { top: 1, left: 1, bottom: 13, right: 4 };
const area = (top, left, bottom = top, right = left) => ({ top, left, bottom, right });
const COL = (col) => area(2, col, 13, col);
const red = { color: '9C0006', fill: 'FFC7CE' };
const green = { color: '006100', fill: 'C6EFCE' };
const yellow = { color: '9C5700', fill: 'FFEB9C' };
const cf = (areas, rule) => ({ kind: 'addCf', sheet: 0, areas: [areas], rule });
const dv = (areas, rule) => ({ kind: 'setValidation', sheet: 0, areas: [areas], rule });
const filterOn = { kind: 'toggleFilter', sheet: 0, area: AREA };
const filter = (col, criteria) => ({ kind: 'setFilter', sheet: 0, col, criteria });
const sort = (keys, extra = {}) => ({
  kind: 'sort',
  sheet: 0,
  area: AREA,
  header: true,
  keys,
  ...extra,
});
const yellowFill = (target) => ({
  kind: 'format',
  sheet: 0,
  areas: [target],
  change: { fill: { pattern: 'solid', color: { kind: 'rgb', rgb: 'FFFF00' } } },
});

const CASES = [
  // Conditional formats.
  [
    'cf-cell-value',
    'cf',
    [
      cf(COL(2), { type: 'cellIs', op: 'greaterThan', formulas: ['5'], format: red }),
      cf(COL(1), { type: 'cellIs', op: 'equal', formulas: ['"ann"'], format: green }),
      cf(COL(2), { type: 'cellIs', op: 'between', formulas: ['0', '7'], format: { bold: true } }),
      cf(COL(4), { type: 'cellIs', op: 'lessThan', formulas: ['"c"'], format: yellow }),
    ],
  ],
  [
    'cf-text',
    'cf',
    [
      cf(COL(4), { type: 'text', op: 'contains', text: 'apple', format: red }),
      cf(COL(4), { type: 'text', op: 'beginsWith', text: 'b', format: { bold: true } }),
      cf(COL(4), { type: 'text', op: 'endsWith', text: 'e', format: yellow }),
      cf(COL(1), { type: 'text', op: 'notContains', text: 'a', format: green }),
    ],
  ],
  [
    'cf-blanks-errors',
    'cf',
    [
      cf(COL(2), { type: 'blanks', format: { fill: 'EEEEEE' } }),
      cf(COL(2), { type: 'errors', format: red }),
      cf(COL(1), { type: 'blanks', not: true, format: { bold: true } }),
      cf(COL(4), { type: 'errors', not: true, format: { color: '0000FF' } }),
    ],
  ],
  [
    'cf-dates',
    'cf',
    [
      cf(COL(3), { type: 'timePeriod', period: 'yesterday', format: red }),
      cf(COL(3), { type: 'timePeriod', period: 'today', format: green }),
      cf(COL(3), { type: 'timePeriod', period: 'tomorrow', format: yellow }),
      cf(COL(3), { type: 'timePeriod', period: 'last7Days', format: { bold: true } }),
      cf(COL(3), { type: 'timePeriod', period: 'lastMonth', format: { color: '0000FF' } }),
    ],
  ],
  [
    'cf-dates-months',
    'cf',
    [
      cf(COL(3), { type: 'timePeriod', period: 'thisMonth', format: red }),
      cf(COL(3), { type: 'timePeriod', period: 'nextMonth', format: green }),
      cf(COL(3), { type: 'timePeriod', period: 'thisWeek', format: { bold: true } }),
      cf(COL(3), { type: 'timePeriod', period: 'lastWeek', format: yellow }),
    ],
  ],
  [
    'cf-top-bottom',
    'cf',
    [
      cf(COL(2), { type: 'top10', rank: 3, format: red }),
      cf(COL(2), { type: 'top10', bottom: true, rank: 2, format: green }),
      cf(COL(2), { type: 'top10', percent: true, rank: 25, format: { bold: true } }),
    ],
  ],
  [
    'cf-average',
    'cf',
    [
      cf(COL(2), { type: 'average', above: true, format: red }),
      cf(COL(2), { type: 'average', above: false, format: green }),
      cf(COL(3), { type: 'average', above: true, stdDev: 1, format: { bold: true } }),
    ],
  ],
  [
    'cf-duplicates',
    'cf',
    [
      cf(COL(1), { type: 'duplicates', format: red }),
      cf(COL(4), { type: 'duplicates', unique: true, format: green }),
      cf(COL(2), { type: 'duplicates', format: { bold: true } }),
    ],
  ],
  [
    'cf-formulas',
    'cf',
    [
      cf(area(2, 1, 13, 4), { type: 'expression', formula: '$B2>5', format: yellow }),
      cf(area(2, 1, 13, 4), {
        type: 'expression',
        formula: 'ISNUMBER(SEARCH("an",$A2))',
        format: { bold: true },
      }),
      cf(COL(4), { type: 'expression', formula: 'LEN(D2)>5', format: { color: '0000FF' } }),
    ],
  ],
  [
    'cf-color-scales',
    'cf',
    [
      cf(COL(2), {
        type: 'colorScale',
        points: [
          { kind: 'min', color: 'F8696B' },
          { kind: 'percentile', value: '50', color: 'FFEB84' },
          { kind: 'max', color: '63BE7B' },
        ],
      }),
      cf(COL(3), {
        type: 'colorScale',
        points: [
          { kind: 'min', color: 'FCFCFF' },
          { kind: 'max', color: '63BE7B' },
        ],
      }),
    ],
  ],
  // Validation.
  ['dv-whole', 'dv', [dv(COL(2), { kind: 'whole', op: 'between', formula1: '1', formula2: '10' })]],
  [
    'dv-decimal',
    'dv',
    [dv(COL(2), { kind: 'decimal', op: 'greaterThanOrEqual', formula1: '2.5' })],
  ],
  ['dv-list', 'dv', [dv(COL(1), { kind: 'list', formula1: '"ann,bob,cy"' })]],
  ['dv-list-range', 'dv', [dv(COL(4), { kind: 'list', formula1: '=$D$2:$D$5' })]],
  [
    'dv-text-length',
    'dv',
    [dv(COL(4), { kind: 'textLength', op: 'lessThanOrEqual', formula1: '5' })],
  ],
  [
    'dv-date',
    'dv',
    [
      dv(COL(3), {
        kind: 'date',
        op: 'between',
        formula1: String(today - 7),
        formula2: String(today),
      }),
    ],
  ],
  ['dv-custom', 'dv', [dv(COL(2), { kind: 'custom', formula1: '=AND(ISNUMBER(B2),B2<C2)' })]],
  ['dv-not-equal', 'dv', [dv(COL(2), { kind: 'whole', op: 'notEqual', formula1: '12' })]],
  // Filters, which Excel applies again itself.
  ['filter-values', 'filter', [filterOn, filter(1, { kind: 'values', values: ['ann', 'BOB'] })]],
  ['filter-blanks', 'filter', [filterOn, filter(1, { kind: 'values', values: [], blank: true })]],
  [
    'filter-custom-and',
    'filter',
    [
      filterOn,
      filter(2, {
        kind: 'custom',
        and: true,
        conditions: [
          { op: 'greaterThan', value: '2' },
          { op: 'lessThan', value: '100' },
        ],
      }),
    ],
  ],
  [
    'filter-wildcards',
    'filter',
    [
      filterOn,
      filter(4, {
        kind: 'custom',
        conditions: [
          { op: 'equal', value: 'apple*' },
          { op: 'equal', value: '*y' },
        ],
      }),
    ],
  ],
  [
    'filter-not-contains',
    'filter',
    [filterOn, filter(4, { kind: 'custom', conditions: [{ op: 'notEqual', value: '*an*' }] })],
  ],
  ['filter-top', 'filter', [filterOn, filter(2, { kind: 'top10', value: 3 })]],
  [
    'filter-bottom-percent',
    'filter',
    [filterOn, filter(2, { kind: 'top10', top: false, percent: true, value: 30 })],
  ],
  [
    'filter-above-average',
    'filter',
    [filterOn, filter(2, { kind: 'dynamic', name: 'aboveAverage' })],
  ],
  [
    'filter-below-average',
    'filter',
    [filterOn, filter(2, { kind: 'dynamic', name: 'belowAverage' })],
  ],
  [
    'filter-above-average-no-error',
    'filter',
    [
      { kind: 'clear', sheet: 0, areas: [area(13, 2)] },
      filterOn,
      filter(2, { kind: 'dynamic', name: 'aboveAverage' }),
    ],
  ],
  [
    'filter-below-average-no-error',
    'filter',
    [
      { kind: 'clear', sheet: 0, areas: [area(13, 2)] },
      filterOn,
      filter(2, { kind: 'dynamic', name: 'belowAverage' }),
    ],
  ],
  ['filter-this-month', 'filter', [filterOn, filter(3, { kind: 'dynamic', name: 'thisMonth' })]],
  ['filter-yesterday', 'filter', [filterOn, filter(3, { kind: 'dynamic', name: 'yesterday' })]],
  ['filter-last-month', 'filter', [filterOn, filter(3, { kind: 'dynamic', name: 'lastMonth' })]],
  [
    'filter-two-columns',
    'filter',
    [
      filterOn,
      filter(1, { kind: 'values', values: ['ann', 'bob', 'cy', 'dee'] }),
      filter(2, { kind: 'custom', conditions: [{ op: 'greaterThanOrEqual', value: '5' }] }),
    ],
  ],
  [
    'filter-table',
    'filter',
    [
      { kind: 'createTable', sheet: 0, area: AREA },
      {
        kind: 'setFilter',
        sheet: 0,
        table: 'Table1',
        col: 2,
        criteria: { kind: 'top10', value: 4, top: false },
      },
    ],
  ],
  // Sorts, which Excel does itself on the unsorted copy.
  ['sort-text', 'sort', [sort([{ index: 1 }])]],
  ['sort-mixed-descending', 'sort', [sort([{ index: 2, descending: true }])]],
  ['sort-mixed', 'sort', [sort([{ index: 2 }])]],
  ['sort-case', 'sort', [sort([{ index: 1 }], { caseSensitive: true })]],
  ['sort-two-keys', 'sort', [sort([{ index: 1 }, { index: 2, descending: true }])]],
  ['sort-dates', 'sort', [sort([{ index: 3, descending: true }])]],
  ['sort-list', 'sort', [sort([{ index: 4, list: ['cherry', 'apple', 'banana'] }])]],
  ['sort-notes', 'sort', [sort([{ index: 4 }])]],
];

// Colours and icons for sorts, put on before the unsorted copy is saved. Phase 13 added the
// conditional formats' colours and icons, which Excel 2016 sorts and filters by too.
const redText = (target) => ({
  kind: 'format',
  sheet: 0,
  areas: [target],
  change: { fontColor: { kind: 'rgb', rgb: 'FF0000' } },
});
const fills = [yellowFill(area(4, 2)), yellowFill(area(9, 2)), yellowFill(area(12, 2))];
const texts = [redText(area(3, 2)), redText(area(7, 2)), redText(area(11, 2))];
const overFive = cf(COL(2), { type: 'cellIs', op: 'greaterThan', formulas: ['5'], format: red });
const arrows = cf(COL(2), { type: 'iconSet', set: '3Arrows' });
const SETUP = {
  'sort-colour': fills,
  'sort-font-colour': texts,
  'sort-cf-colour': [overFive],
  'sort-icon': [arrows],
};
CASES.push(
  [
    'sort-colour',
    'sort',
    [sort([{ index: 2, by: { kind: 'cellColor', color: 'FFFF00' } }, { index: 1 }])],
  ],
  [
    'sort-font-colour',
    'sort',
    [sort([{ index: 2, descending: true, by: { kind: 'fontColor', color: 'FF0000' } }])],
  ],
  ['sort-cf-colour', 'sort', [sort([{ index: 2, by: { kind: 'cellColor', color: 'FFC7CE' } }])]],
  ['sort-icon', 'sort', [sort([{ index: 2, by: { kind: 'icon', set: '3Arrows', id: 2 } }])]],
  [
    'filter-cell-colour',
    'filter',
    [...fills, filterOn, filter(2, { kind: 'color', cell: true, color: 'FFFF00' })],
  ],
  ['filter-no-fill', 'filter', [...fills, filterOn, filter(2, { kind: 'color', cell: true })]],
  [
    'filter-font-colour',
    'filter',
    [...texts, filterOn, filter(2, { kind: 'color', cell: false, color: 'FF0000' })],
  ],
  [
    'filter-cf-colour',
    'filter',
    [overFive, filterOn, filter(2, { kind: 'color', cell: true, color: 'FFC7CE' })],
  ],
  ['filter-icon', 'filter', [arrows, filterOn, filter(2, { kind: 'icon', set: '3Arrows', id: 0 })]],
);

const cases = CASES.map(([name, kind, commands]) => ({
  name,
  kind,
  cells: [...cells, ...[]],
  setup: SETUP[name] ?? [],
  commands,
  area: AREA,
}));

function excel(list) {
  const input = join(scratch, 'excel-cases.json');
  const result = join(scratch, 'excel.json');
  writeFileSync(input, JSON.stringify(list));
  const run = spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `& {${script}} -Cases ${quoted(input)} -Out ${quoted(result)}`,
    ],
    { encoding: 'utf8', timeout: 1800000, windowsHide: true },
  );
  if (run.status !== 0) throw new Error('Excel script failed: ' + run.stderr.slice(0, 600));
  return JSON.parse(readFileSync(result, 'utf8').trimStart());
}

const near = (a, b) => {
  if (a === b) return true;
  if (!a || !b) return false;
  for (let at = 0; at < 6; at += 2) {
    if (Math.abs(parseInt(a.slice(at, at + 2), 16) - parseInt(b.slice(at, at + 2), 16)) > 2) {
      return false;
    }
  }
  return true;
};

try {
  const built = spawnSync(
    'cargo',
    ['build', '--profile', 'fast', '-p', 'cgs-bench', '--bin', 'rulecheck'],
    {
      stdio: 'inherit',
      cwd: root,
    },
  );
  if (built.status !== 0) throw new Error('rulecheck did not build.');
  const casesFile = join(scratch, 'cases.json');
  const oursFile = join(scratch, 'ours.json');
  writeFileSync(casesFile, JSON.stringify(cases));
  const ran = spawnSync(tool, [casesFile, scratch, oursFile, String(today)], { encoding: 'utf8' });
  if (ran.status !== 0) throw new Error('rulecheck failed: ' + ran.stderr.slice(0, 600));
  const ours = JSON.parse(readFileSync(oursFile, 'utf8'));
  const sortSpec = (name) => {
    const command = CASES.find(([n]) => n === name)[2].find((c) => c.kind === 'sort');
    return {
      keys: command.keys,
      header: command.header,
      caseSensitive: Boolean(command.caseSensitive),
      byColumns: Boolean(command.byColumns),
    };
  };
  const forExcel = ours
    .filter((entry) => !entry.error)
    .map((entry) => ({
      name: entry.name,
      kind: entry.kind,
      file: entry.file,
      before: entry.before,
      area: AREA,
      ...(entry.kind === 'sort' ? { sort: sortSpec(entry.name) } : {}),
    }));
  const theirs = excel(forExcel);
  const report = { today, cases: {}, totals: { cases: 0, matched: 0, compared: 0, agreed: 0 } };
  for (const entry of ours) {
    const seen = theirs.find((other) => other.name === entry.name);
    const result = { compared: 0, agreed: 0, differences: [] };
    if (entry.error || !seen || seen.error) {
      result.error = entry.error ?? seen?.error ?? 'Excel gave nothing';
    } else if (entry.kind === 'cf') {
      for (const cell of seen.cells) {
        if (cell.row === 1) continue;
        const mine = entry.cf.find((other) => other.row === cell.row && other.col === cell.col);
        const fill = mine?.fill ?? null;
        const color = mine?.color ?? '000000';
        const bold = Boolean(mine?.bold);
        result.compared++;
        if (near(fill, cell.fill) && near(color, cell.color) && bold === cell.bold) result.agreed++;
        else
          result.differences.push({
            row: cell.row,
            col: cell.col,
            ours: { fill, color, bold },
            excel: cell,
          });
      }
    } else if (entry.kind === 'dv') {
      const invalid = new Set(entry.invalid.map(([row, col]) => row + ':' + col));
      for (const cell of seen.valid) {
        result.compared++;
        const mine = !invalid.has(cell.row + ':' + cell.col);
        if (mine === cell.valid) result.agreed++;
        else
          result.differences.push({ row: cell.row, col: cell.col, ours: mine, excel: cell.valid });
      }
    } else if (entry.kind === 'filter') {
      result.compared = 13;
      const a = JSON.stringify(entry.hidden);
      const b = JSON.stringify(seen.hidden ?? []);
      result.agreed = a === b ? 13 : 0;
      if (a !== b) result.differences.push({ ours: entry.hidden, excel: seen.hidden });
    } else if (entry.kind === 'sort') {
      entry.values.forEach((row, r) =>
        row.forEach((text, c) => {
          result.compared++;
          const theirsText = seen.values[r]?.[c] ?? '';
          if (text === theirsText) result.agreed++;
          else result.differences.push({ row: r + 1, col: c + 1, ours: text, excel: theirsText });
        }),
      );
    }
    result.matches = !result.error && result.compared > 0 && result.agreed === result.compared;
    report.cases[entry.name] = result;
    report.totals.cases++;
    if (result.matches) report.totals.matched++;
    report.totals.compared += result.compared;
    report.totals.agreed += result.agreed;
    console.log(
      entry.name,
      result.matches ? 'match' : 'DIFFERS',
      result.error ?? '',
      result.differences.length ? JSON.stringify(result.differences.slice(0, 3)) : '',
    );
  }
  writeFileSync(out, JSON.stringify(report, null, 1) + '\n');
  console.log(JSON.stringify(report.totals));
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
