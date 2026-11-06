// Pivot refresh against Excel 2016. Excel makes a workbook of pivot tables over COM
// (bench/pivot-cases.mjs), then edits their source and refreshes them, saving both. The app:
//   1. refreshes each pivot from its own cache records and must give the cells, location and
//      lines Excel saved, in both of Excel's files (`pivotcheck`);
//   2. makes the same source edits, refreshes through the engine and must give what Excel's
//      refresh gave (`pivotrefresh`), then saves;
//   3. rearranges and creates pivots and saves again.
// Excel opens both saved files with no repair, refreshes every pivot in them, and must show what
// the app wrote. Only generated workbooks open in Excel; public test files are checked by
// `pivotcheck` alone.
// Usage: node bench/run-pivot-check.mjs [out-folder]
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { root } from '../scripts/lib.mjs';
import { pivotSpec } from './pivot-cases.mjs';

const folder = resolve(
  process.argv.slice(2).find((a) => !a.startsWith('--')) ?? join(root, 'bench/results/pivots'),
);
mkdirSync(folder, { recursive: true });
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";
const powershell = (script, parameters) => {
  const args = Object.entries(parameters)
    .map(([key, value]) => `-${key} ${quoted(value)}`)
    .join(' ');
  const run = spawnSync(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-STA', '-Command', `& {${script}} ${args}`],
    { encoding: 'utf8', timeout: 900000, windowsHide: true },
  );
  if (run.status !== 0) throw new Error('Excel failed: ' + run.stderr.slice(0, 800));
};
const cargo = (bin, args) => {
  const build = spawnSync(
    'cargo',
    ['build', '-p', 'cgs-bench', '--profile', 'fast', '--bin', bin],
    {
      cwd: root,
      stdio: 'inherit',
    },
  );
  if (build.status !== 0) throw new Error(`building ${bin} failed`);
  const run = spawnSync(join(root, 'target/fast', bin), args, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 1 << 26,
  });
  if (run.status !== 0) throw new Error(`${bin} failed: ${run.stderr.slice(0, 800)}`);
  return run.stdout;
};
const file = (name) => join(folder, name);

// 1. Excel makes the pivots, edits their source, and refreshes them.
const spec = pivotSpec();
writeFileSync(file('cases.json'), JSON.stringify(spec));
// `--reuse` keeps the two files Excel made last time, for working on the app's side.
if (!process.argv.includes('--reuse')) {
  for (const name of ['excel-pivots.xlsx', 'excel-refreshed.xlsx'])
    rmSync(file(name), { force: true });
  powershell(readFileSync(join(root, 'bench/excel-make-pivots.ps1'), 'utf8'), {
    Cases: file('cases.json'),
    Out: file('excel-pivots.xlsx'),
    Refreshed: file('excel-refreshed.xlsx'),
    Captions: file('captions.json'),
  });
}

// 2. Each pivot refreshed from its own records, against what Excel saved.
const fromRecords = {};
for (const name of ['excel-pivots', 'excel-refreshed']) {
  const out = file(`${name}-records.json`);
  const text = cargo('pivotcheck', [file(`${name}.xlsx`), '--arabic', '--json', out]);
  process.stdout.write(text.split('\n').slice(-2).join('\n'));
  fromRecords[name] = JSON.parse(readFileSync(out, 'utf8'));
}

// 3. The engine's refresh after the same edits, against Excel's; then changes and new pivots.
const report = file('refresh-report.json');
const saved = file('app-refreshed.xlsx');
process.stdout.write(
  cargo('pivotrefresh', [
    file('excel-pivots.xlsx'),
    file('excel-refreshed.xlsx'),
    file('cases.json'),
    saved,
    report,
  ]),
);
const refresh = JSON.parse(readFileSync(report, 'utf8'));

// 4. Excel opens what the app saved, refreshes each pivot, and shows what the app wrote.
const ERRORS = {
  '-2146826281': '#DIV/0!',
  '-2146826246': '#N/A',
  '-2146826259': '#NAME?',
  '-2146826288': '#NULL!',
  '-2146826252': '#NUM!',
  '-2146826265': '#REF!',
  '-2146826273': '#VALUE!',
};
const same = (ours, theirs, emptyText) => {
  if (ours === null || ours === '') return theirs === null || theirs === '';
  // Excel reads 0 where it shows the caption for empty cells.
  if (emptyText && ours === emptyText && theirs === 0) return true;
  if (typeof ours === 'object' && ours.error)
    return theirs?.e !== undefined && ERRORS[theirs.e] === ours.error;
  if (typeof ours === 'number') {
    return (
      typeof theirs === 'number' && Math.abs(ours - theirs) <= 1e-9 * Math.max(1, Math.abs(ours))
    );
  }
  if (typeof ours === 'boolean') return theirs === ours;
  return String(ours) === String(theirs);
};
function compare(name, values, verify) {
  const result = {
    file: name,
    opened: verify.opened,
    repaired: verify.repaired,
    error: verify.error,
    pivots: 0,
    matching: 0,
    cells: 0,
    wrong: [],
  };
  for (const theirs of verify.pivots ?? []) {
    result.pivots++;
    const ours = values.find((p) => p.sheet === theirs.sheet && p.name === theirs.name);
    if (!ours || theirs.error) {
      result.wrong.push(`${theirs.sheet} ${theirs.name}: ${theirs.error ?? 'not in the app'}`);
      continue;
    }
    let ok = theirs.top === ours.top && theirs.left === ours.left;
    if (!ok)
      result.wrong.push(
        `${theirs.sheet} ${theirs.name}: at R${theirs.top}C${theirs.left}, the app at R${ours.top}C${ours.left}`,
      );
    const rows = Math.max(theirs.rows.length, ours.rows.length);
    for (let r = 0; r < rows; r++) {
      const width = Math.max(theirs.rows[r]?.length ?? 0, ours.rows[r]?.length ?? 0);
      for (let c = 0; c < width; c++) {
        const a = ours.rows[r]?.[c] ?? null;
        const b = theirs.rows[r]?.[c] ?? null;
        if (a === null && b === null) continue;
        result.cells++;
        if (!same(a, b, ours.emptyText)) {
          ok = false;
          if (result.wrong.length < 40)
            result.wrong.push(
              `${theirs.sheet} ${theirs.name} R${theirs.top + r}C${theirs.left + c}: app ${JSON.stringify(a)} Excel ${JSON.stringify(b)}`,
            );
        }
      }
    }
    if (ok) result.matching++;
  }
  return result;
}
const verified = [];
for (const [name, values] of [
  ['app-refreshed.xlsx', refresh.refreshedValues],
  ['app-refreshed-changed.xlsx', refresh.changedValues],
]) {
  const out = file(`${name}.excel.json`);
  powershell(readFileSync(join(root, 'bench/excel-pivots-verify.ps1'), 'utf8'), {
    Workbook: file(name),
    Out: out,
  });
  // PowerShell writes UTF-8 with a byte order mark.
  const text = readFileSync(out, 'utf8');
  const verify = JSON.parse(text.charCodeAt(0) === 0xfeff ? text.slice(1) : text);
  const result = compare(name, values, verify);
  verified.push(result);
  console.log(
    `${name}: Excel opened it ${result.opened && !result.repaired ? 'clean' : 'WITH A REPAIR'}; ` +
      `${result.matching} of ${result.pivots} pivots show what the app wrote after Excel's refresh (${result.cells} cells)`,
  );
  for (const line of result.wrong.slice(0, 12)) console.log('   ', line);
}

const summary = {
  excelPivots: spec.cases.length,
  fromRecords: Object.fromEntries(
    Object.entries(fromRecords).map(([name, r]) => [
      name,
      {
        tables: r.tables,
        bothMatch: r.bothMatch,
        cellsCompared: r.cellsCompared,
        cellsWrong: r.cellsWrong,
      },
    ]),
  ),
  engineRefresh: {
    pivots: refresh.pivots,
    cellsMatch: refresh.cellsMatch,
    linesMatch: refresh.linesMatch,
    cellsCompared: refresh.cellsCompared,
    cellsWrong: refresh.cellsWrong,
  },
  changes: refresh.changes,
  excel: verified,
};
writeFileSync(file('summary.json'), JSON.stringify(summary, null, 2));
const clean = verified.every((v) => v.opened && !v.repaired && v.matching === v.pivots);
console.log(clean ? 'All pivot checks pass.' : 'Some pivot checks failed; see summary.json.');
process.exit(clean ? 0 : 1);
