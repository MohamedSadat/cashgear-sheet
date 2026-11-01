// Random structural edits against Excel: 200 episodes of 5 edits from the same seed, so one
// divergence can't taint the others. Excel runs once per seed and op list; its answers are
// kept, and each engine is then compared after every edit.
// Usage: node bench/run-structural.mjs [--seed=1] [--engines=formualizer,ironcalc] [--config=path]
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { root } from '../scripts/lib.mjs';

const option = (name, fallback) => {
  const found = process.argv.find((arg) => arg.startsWith('--' + name + '='));
  return found ? found.split('=')[1] : fallback;
};
const seedNumber = Number(option('seed', '1'));
const engines = option('engines', 'formualizer,ironcalc').split(',');
const profile = option('profile', 'fast');
const EPISODES = Number(option('episodes', '200'));
const STEPS = Number(option('steps', '5'));

// xorshift, so the same seed always gives the same edits.
let state = BigInt(seedNumber) * 0x9e3779b97f4a7c15n + 1n;
function random(limit) {
  state ^= (state << 13n) & 0xffffffffffffffffn;
  state ^= state >> 7n;
  state ^= (state << 17n) & 0xffffffffffffffffn;
  return Number(state % BigInt(limit));
}

const kinds = ['InsertRows', 'DeleteRows', 'InsertCols', 'DeleteCols'];
const episodes = Array.from({ length: EPISODES }, (_, episode) => ({
  episode,
  ops: Array.from({ length: STEPS }, () => {
    const kind = kinds[random(kinds.length)];
    const rows = kind.endsWith('Rows');
    return {
      kind,
      sheet: random(2),
      at: 1 + random(rows ? 45 : 12),
      count: 1 + random(3),
    };
  }),
}));

const outDir = join(root, 'bench/results/structural');
mkdirSync(outDir, { recursive: true });
const seedFile = join(root, 'tests/workbooks/generated/structural-seed.xlsx');
if (!existsSync(seedFile)) throw new Error('Run scripts/generate-test-workbooks.mjs first.');
const opsFile = join(outDir, `ops-${seedNumber}.json`);
const excelFile = join(outDir, `excel-${seedNumber}.jsonl`);
writeFileSync(opsFile, JSON.stringify(episodes));

if (!existsSync(excelFile) || process.argv.includes('--fresh')) {
  const started = Date.now();
  const result = spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-NonInteractive',
      '-ExecutionPolicy',
      'Bypass',
      '-File',
      join(root, 'bench/excel-fuzz.ps1'),
      '-Seed',
      seedFile,
      '-Ops',
      opsFile,
      '-Out',
      excelFile,
    ],
    { encoding: 'utf8', timeout: 3_600_000 },
  );
  if (result.status !== 0) throw new Error('Excel run failed: ' + result.stderr + result.stdout);
  console.log('Excel finished in ' + Math.round((Date.now() - started) / 1000) + ' s');
}

for (const engine of engines) {
  const result = spawnSync(
    join(root, 'target', profile, 'structural.exe'),
    [engine, seedFile, opsFile, excelFile],
    { encoding: 'utf8', maxBuffer: 1 << 28 },
  );
  if (result.status !== 0) throw new Error(engine + ' failed: ' + result.stderr);
  const report = JSON.parse(result.stdout);
  writeFileSync(join(outDir, `${engine}-${seedNumber}.json`), JSON.stringify(report, null, 1));
  console.log(
    `${engine}: ${report.opsPassed} of ${report.opsScheduled} edits match Excel, ${report.opsDiverged} diverged, ${report.opsSkipped} not reached after an earlier divergence; ${report.formulaOnlyDivergences} differ in formula text only`,
  );
  for (const sample of report.divergences.slice(0, 6)) {
    console.log(
      `  episode ${sample.episode} step ${sample.step} ${sample.op}: ${sample.sheet}!${sample.cell} ${sample.what} excel=${sample.excel} engine=${sample.engine} formula=${sample.formula}`,
    );
  }
}
