// Measures how Excel scales chart axes on its own, for `cgs-engine`'s axis scaling to match.
// Makes cases, has bench/excel-chart-axes.ps1 build each chart in Excel, and writes what Excel
// chose. Opens no file.
// Usage: node bench/run-chart-axes.mjs <out.json>
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../scripts/lib.mjs';

const [out] = process.argv.slice(2);
if (!out) throw new Error('usage: node bench/run-chart-axes.mjs <out.json>');

// A small fixed generator, so the cases are the same every run.
let seed = 7;
const random = () => {
  seed = (seed * 1103515245 + 12345) % 2147483648;
  return seed / 2147483648;
};

const ranges = [
  [0, 10],
  [1, 9],
  [3, 7],
  [95, 100],
  [-5, 5],
  [-100, -20],
  [0.001, 0.009],
  [1234, 5678],
  [50, 51],
  [1e6, 3e6],
  [5, 5],
  [-7, -7],
  [0, 0.5],
  [12, 97],
  [-3, 47],
  [-47, 3],
  [0, 1],
  [0, 100],
  [0, 1000],
  [10, 20],
  [83, 100],
  [84, 100],
  [0.5, 0.6],
  [-0.2, 0.9],
  [7, 1300],
  [1, 2],
  [2, 3],
  [1500, 1600],
  [0, 7],
  [0, 13],
  [0, 23],
  [0, 37],
  [0, 61],
  [0, 0.07],
  [-1e5, 2e5],
  [-2.5, -0.5],
  [0, 0],
  [9.5, 10.5],
  [100, 120],
  [33, 34],
];
for (let i = 0; i < 30; i++) {
  const scale = 10 ** Math.floor(random() * 8 - 3);
  const a = (random() * 2 - 0.6) * scale;
  const b = a + random() * scale * (random() < 0.3 ? 0.1 : 2);
  ranges.push([Number(a.toPrecision(4)), Number(b.toPrecision(4))]);
}

const cases = [];
let id = 0;
const sizes = process.argv.includes('--sizes');
const sizeRanges = [
  [0, 10],
  [1, 9],
  [3, 7],
  [95, 100],
  [-5, 5],
  [0, 1000],
  [1e6, 3e6],
  [0.001, 0.009],
];
for (const [low, high] of sizes ? sizeRanges : []) {
  const values = [low, (low * 2 + high) / 3, high, (low + high) / 2].map((v) =>
    Number(v.toPrecision(6)),
  );
  for (let height = 100; height <= 250; height += 10)
    cases.push({ id: id++, type: 51, width: 360, height, values });
  for (let width = 140; width <= 720; width += 20)
    cases.push({ id: id++, type: 57, width, height: 216, values });
}
for (const [low, high] of sizes ? [] : ranges) {
  const values = [low, (low * 2 + high) / 3, high, (low + high) / 2].map((v) =>
    Number(v.toPrecision(6)),
  );
  for (const height of [120, 216, 320, 480]) {
    // 51 clustered column: the value axis runs up the chart's height.
    cases.push({ id: id++, type: 51, width: 360, height, values });
  }
  for (const width of [200, 360, 600]) {
    // 57 clustered bar: the value axis runs across.
    cases.push({ id: id++, type: 57, width, height: 216, values });
  }
  // 52 stacked column, three series: stacks add up.
  cases.push({ id: id++, type: 52, width: 360, height: 216, values, series: 3 });
  // -4169 scatter: both axes scale.
  const x = [low * 3, high, low + high, high * 2].map((v) => Number(v.toPrecision(6)));
  cases.push({ id: id++, type: -4169, width: 360, height: 216, values, x });
}

const scratch = mkdtempSync(join(tmpdir(), 'cgs-chart-axes-'));
try {
  const input = join(scratch, 'cases.json');
  const seen = join(scratch, 'seen.json');
  writeFileSync(input, JSON.stringify(cases));
  const script = readFileSync(join(root, 'bench/excel-chart-axes.ps1'), 'utf8');
  const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";
  const run = spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      '& {' + script + '} -Cases ' + quoted(input) + ' -Out ' + quoted(seen),
    ],
    { encoding: 'utf8', timeout: 1800000, windowsHide: true },
  );
  if (run.status !== 0) throw new Error('Excel failed: ' + run.stderr.slice(0, 2000));
  const results = JSON.parse(readFileSync(seen, 'utf8'));
  const byId = new Map(results.map((row) => [row.id, row]));
  writeFileSync(
    out,
    JSON.stringify(
      cases.map((c) => ({ ...c, excel: byId.get(c.id) })),
      null,
      0,
    ),
  );
  console.log(results.length + ' cases measured');
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
