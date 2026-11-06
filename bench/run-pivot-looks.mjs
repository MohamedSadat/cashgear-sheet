// Pivot styles against Excel 2016: every cell of every pivot table, as Excel shows it
// (DisplayFormat over COM) and as the app draws it, compared for fill, font colour and weight, and
// which edges have a line. The workbooks are the ones `bench/run-pivot-check.mjs` leaves: Excel's
// own, and the app's after it changed layouts and styles. Generated workbooks only.
// Usage: node bench/run-pivot-looks.mjs [folder] [workbook names…]
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { root } from '../scripts/lib.mjs';

const folder = resolve(process.argv[2] || join(root, 'bench/results/pivots'));
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";
const script = readFileSync(join(root, 'bench/excel-pivot-looks.ps1'), 'utf8');
const build = spawnSync(
  'cargo',
  ['build', '-p', 'cgs-bench', '--profile', 'fast', '--bin', 'pivotlooks'],
  {
    cwd: root,
    stdio: 'inherit',
  },
);
if (build.status !== 0) throw new Error('pivotlooks did not build');
const readJson = (path) => {
  const text = readFileSync(path, 'utf8');
  return JSON.parse(text.charCodeAt(0) === 0xfeff ? text.slice(1) : text);
};

const summary = {};
const names = process.argv.slice(3);
for (const name of names.length ? names : ['excel-pivots.xlsx', 'app-refreshed-changed.xlsx']) {
  const workbook = join(folder, name);
  const ours = join(folder, `${name}.looks-app.json`);
  const theirs = join(folder, `${name}.looks-excel.json`);
  const app = spawnSync(join(root, 'target/fast/pivotlooks'), [workbook, ours], {
    encoding: 'utf8',
  });
  if (app.status !== 0) throw new Error('pivotlooks failed: ' + app.stderr);
  const excel = spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-NonInteractive',
      '-STA',
      '-Command',
      `& {${script}} -Workbook ${quoted(workbook)} -Out ${quoted(theirs)}`,
    ],
    { encoding: 'utf8', timeout: 900000, windowsHide: true },
  );
  if (excel.status !== 0) throw new Error('Excel failed: ' + excel.stderr.slice(0, 600));
  const app_ = readJson(ours);
  const excel_ = readJson(theirs);
  if (excel_.error) throw new Error(excel_.error);
  const result = { pivots: 0, matching: 0, cells: 0, wrong: 0, byKind: {}, samples: [] };
  for (const theirsPivot of excel_.pivots) {
    const oursPivot = app_.find(
      (p) => p.sheet === theirsPivot.sheet && p.name === theirsPivot.name,
    );
    if (!oursPivot) continue;
    result.pivots++;
    const mine = new Map(oursPivot.cells.map((c) => [`${c.row}:${c.col}`, c]));
    const excelCells = new Map(theirsPivot.cells.map((c) => [`${c.row}:${c.col}`, c]));
    // A line on an edge, from either cell beside it.
    const edge = (cells, row, col, side) => {
      const opposite = {
        top: ['bottom', -1, 0],
        bottom: ['top', 1, 0],
        left: ['right', 0, -1],
        right: ['left', 0, 1],
      }[side];
      const here = cells.get(`${row}:${col}`)?.[side];
      const there = cells.get(`${row + opposite[1]}:${col + opposite[2]}`)?.[opposite[0]];
      return Boolean(here || there);
    };
    let ok = true;
    for (const cell of theirsPivot.cells) {
      const our = mine.get(`${cell.row}:${cell.col}`);
      if (!our) continue;
      result.cells++;
      const problems = [];
      // A tint can round a step differently: colours within 2 a channel are the same.
      const near = (x, y) =>
        x === y ||
        (x !== null &&
          y !== null &&
          [0, 2, 4].every(
            (i) => Math.abs(parseInt(x.slice(i, i + 2), 16) - parseInt(y.slice(i, i + 2), 16)) <= 2,
          ));
      if (!near(our.fill ?? null, cell.fill ?? null))
        problems.push(`fill app ${our.fill} Excel ${cell.fill}`);
      if (Boolean(our.bold) !== Boolean(cell.bold))
        problems.push(`bold app ${our.bold} Excel ${cell.bold}`);
      if (!near(our.color ?? '000000', cell.color))
        problems.push(`colour app ${our.color} Excel ${cell.color}`);
      for (const side of ['top', 'bottom', 'left', 'right']) {
        const a = edge(mine, cell.row, cell.col, side);
        const b = edge(excelCells, cell.row, cell.col, side);
        if (a !== b) problems.push(`${side} line app ${a} Excel ${b}`);
      }
      if (problems.length) {
        ok = false;
        result.wrong++;
        for (const p of problems) {
          const kind = p.split(' app ')[0];
          result.byKind[kind] = (result.byKind[kind] ?? 0) + 1;
        }
        if (result.samples.length < 400)
          result.samples.push(
            `${theirsPivot.sheet} ${theirsPivot.style} R${cell.row}C${cell.col}: ${problems.join('; ')}`,
          );
      }
    }
    if (ok) result.matching++;
  }
  summary[name] = result;
  console.log(
    `${name}: ${result.matching} of ${result.pivots} pivots look as Excel shows them; ${result.wrong} of ${result.cells} cells differ`,
    result.byKind,
  );
  for (const line of result.samples.slice(0, 25)) console.log('   ', line);
}
writeFileSync(join(folder, 'looks-summary.json'), JSON.stringify(summary, null, 2));
