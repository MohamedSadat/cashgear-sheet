// Phase 10's round trip: page setup made in the app opens in Excel as the app meant it, with no
// repair, and Excel's own save of it reads back the same in the app. Generated workbooks only.
// Usage: node bench/run-page-setup-roundtrip.mjs [out.json]
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { root } from '../scripts/lib.mjs';

const out = resolve(process.argv[2] ?? join(root, 'bench/results/page-setup-roundtrip.json'));
const save = join(root, 'target/fast/savecheck.exe');
const pages = join(root, 'target/fast/pagebreaks.exe');
const script = readFileSync(join(root, 'bench/excel-page-setup.ps1'), 'utf8');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";
const fill = (rows) =>
  Array.from({ length: rows }, (_, r) => ({
    kind: 'input',
    sheet: 0,
    row: r + 1,
    col: 1,
    text: String(r + 1),
  }));

/** Each case: the app's commands, and what Excel must read. Margins are in points there. */
const CASES = {
  'landscape A4 fitted a page wide': {
    commands: [
      {
        kind: 'pageSetup',
        sheets: [0],
        spec: { orientation: 'landscape', paper: 9, fitToPage: true, fitWidth: 1, fitHeight: 0 },
      },
    ],
    excel: { orientation: 2, paper: 9, zoom: 0, fitWide: 1, fitTall: 0 },
  },
  'scale, order and first page': {
    commands: [
      {
        kind: 'pageSetup',
        sheets: [0],
        spec: { scale: 73, pageOrder: 'overThenDown', firstPage: 5, orientation: 'portrait' },
      },
    ],
    excel: { zoom: 73, order: 2, firstPage: '5', orientation: 1 },
  },
  'margins and centring': {
    commands: [
      {
        kind: 'pageSetup',
        sheets: [0],
        spec: {
          margins: { left: 0.25, right: 0.5, top: 1, bottom: 1.25, header: 0.4, footer: 0.6 },
          centerHorizontally: true,
          centerVertically: true,
        },
      },
    ],
    excel: { margins: [18, 36, 72, 90, 28.8, 43.2], centerH: true, centerV: true },
  },
  'sheet options': {
    commands: [
      {
        kind: 'pageSetup',
        sheets: [0],
        spec: {
          gridlines: true,
          headings: true,
          blackAndWhite: true,
          draft: true,
          comments: 'atEnd',
          errors: 'NA',
        },
      },
    ],
    excel: {
      gridlines: true,
      headings: true,
      blackAndWhite: true,
      draft: true,
      comments: 1,
      errors: 3,
    },
  },
  'headers and footers': {
    commands: [
      {
        kind: 'pageSetup',
        sheets: [0],
        spec: {
          headerFooter: {
            oddHeader: '&L&"Arial,Bold"&14Report&C&D&R&T',
            oddFooter: '&LConfidential&CPage &P of &N&R&F',
            evenFooter: '&CEven &P',
            firstHeader: '&CFirst page',
            differentOddEven: true,
            differentFirst: true,
          },
        },
      },
    ],
    excel: {
      header: ['&"Arial,Bold"&14Report', '&D', '&T'],
      footer: ['Confidential', 'Page &P of &N', '&F'],
      oddAndEven: true,
      differentFirst: true,
      evenFooter: 'Even &P',
      firstHeader: 'First page',
    },
  },
  'print area, titles and breaks': {
    commands: [
      ...fill(60),
      {
        kind: 'printArea',
        sheet: 0,
        areas: [{ top: 1, left: 1, bottom: 50, right: 4 }],
        action: 'set',
      },
      { kind: 'printTitles', sheet: 0, rows: [1, 2], cols: [1, 1] },
      { kind: 'pageBreak', sheet: 0, row: 21, action: 'insert' },
      { kind: 'pageBreak', sheet: 0, row: 31, col: 3, action: 'insert' },
    ],
    excel: {
      printArea: '$A$1:$D$50',
      titleRows: '$1:$2',
      titleCols: '$A:$A',
      rowBreaks: [21, 31],
      colBreaks: [3],
    },
  },
};

/** The fields Excel's copy must read back as the app wrote them. */
const KEYS = [
  'paper',
  'orientation',
  'scale',
  'fitToPage',
  'fitWidth',
  'fitHeight',
  'firstPage',
  'pageOrder',
  'blackAndWhite',
  'draft',
  'comments',
  'errors',
  'margins',
  'centerHorizontally',
  'centerVertically',
  'headings',
  'gridlines',
  'headerFooter',
  'rowBreaks',
  'colBreaks',
  'printArea',
  'titleRows',
  'titleCols',
];

function setupOf(path) {
  const run = spawnSync(pages, [path], { encoding: 'utf8' });
  if (run.status !== 0) throw new Error('pagebreaks: ' + run.stderr.slice(0, 300));
  return JSON.parse(run.stdout)[0].setup;
}

const close = (a, b) =>
  typeof a === 'number' && typeof b === 'number'
    ? Math.abs(a - b) < 0.01
    : JSON.stringify(a) === JSON.stringify(b);

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

const scratch = mkdtempSync(join(tmpdir(), 'cgs-page-setup-'));
const results = [];
try {
  for (const [name, { commands, excel }] of Object.entries(CASES)) {
    const saved = join(scratch, 'saved.xlsx');
    const copy = join(scratch, 'excel-copy.xlsx');
    const target = join(scratch, 'excel.json');
    rmSync(copy, { force: true });
    const made = spawnSync(
      save,
      ['--edit', 'blank', saved, ...commands.map((c) => '--command=' + JSON.stringify(c))],
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
      { encoding: 'utf8', timeout: 600000, windowsHide: true },
    );
    if (run.status !== 0) throw new Error(`${name}: Excel failed: ${run.stderr.slice(0, 300)}`);
    const seen = JSON.parse(readFileSync(target, 'utf8').replace(/^\uFEFF/, ''));
    const differences = [];
    for (const [key, wanted] of Object.entries(excel)) {
      const got = seen.setup?.[key];
      const okay = Array.isArray(wanted)
        ? Array.isArray(got) &&
          wanted.length === got.length &&
          wanted.every((w, i) => close(w, got[i]))
        : close(wanted, got);
      if (!okay) differences.push({ key, wanted, excel: got });
    }
    const ours = setupOf(saved);
    const back = seen.error ? null : setupOf(copy);
    const lost = back ? KEYS.filter((key) => !close(ours[key], back[key])) : KEYS;
    results.push({
      case: name,
      opened: seen.opened,
      repaired: seen.repaired,
      error: seen.error,
      excelReads: differences.length ? differences : 'as written',
      readsBack: lost.length
        ? lost.map((key) => ({ key, ours: ours[key], back: back?.[key] }))
        : 'the same',
    });
    console.log(
      name,
      seen.opened && !seen.repaired ? 'opens clean' : 'REPAIR',
      differences.length ? 'EXCEL READS DIFFERENTLY' : 'Excel reads it',
      lost.length ? 'DIFFERENT BACK' : 'same back',
    );
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
writeFileSync(out, JSON.stringify(results, null, 2));
const failures = results.filter(
  (r) => !r.opened || r.repaired || r.excelReads !== 'as written' || r.readsBack !== 'the same',
);
console.log(`${results.length - failures.length} of ${results.length} cases round-trip.`);
if (failures.length) process.exitCode = 1;
