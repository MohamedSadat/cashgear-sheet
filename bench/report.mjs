// Turns the JSONL results of bench/run-*.mjs into the per-function report. Files count
// only when Excel saved them and left their values current, so stored values are
// Excel's. Usage: node bench/report.mjs <results.jsonl>... [--out=report.md]
import { readFile, writeFile } from 'node:fs/promises';

const args = process.argv.slice(2);
const inputs = args.filter((arg) => !arg.startsWith('--'));
const outArg = args.find((arg) => arg.startsWith('--out='));
const detail = args.includes('--detail');
const CAP = 0.1;

const percentile = (sorted, q) =>
  sorted.length ? sorted[Math.max(0, Math.ceil(sorted.length * q) - 1)] : 0;
const pct = (n, d) => (d ? ((100 * n) / d).toFixed(3) + '%' : 'n/a');
const ms = (value) => (value >= 100 ? value.toFixed(0) : value.toFixed(1));

/**
 * Function test suites: workbooks written to probe edge cases, which are not what real
 * workbooks look like. The exit target pools everything; this split shows how much of a
 * miss rate comes from each kind.
 */
export const conformance = (path) =>
  /calc_tests|calc_test_no_export|^xlcalculator|^pycel|^formulas\/|^koala|FormulaEval|Format[A-Za-z]*Tests|FormatApproxTests|ElapsedFormat|FloorCeiling|functions__|Matrix|functions-excel|Trimfunctions|SubtotalFilters|FormulaTest|TextFormatTests|ceiling-floor/i.test(
    path,
  );

export const usable = (row) =>
  row.ok &&
  row.provenance?.excelSaved &&
  !row.provenance.fullCalcOnLoad &&
  !row.provenance.manualCalc;

/** Weights cells so that no file holds more than `cap` of the pool. */
export function cappedRate(files, cap) {
  const live = files.filter((file) => file.scored > 0);
  if (!live.length) return { rate: 0, cells: 0 };
  const weights = new Map(live.map((file) => [file, 1]));
  for (let pass = 0; pass < 32; pass++) {
    const total = live.reduce((sum, file) => sum + file.scored * weights.get(file), 0);
    let changed = false;
    for (const file of live) {
      const limit = cap * total;
      if (file.scored * weights.get(file) > limit * (1 + 1e-9)) {
        weights.set(file, limit / file.scored);
        changed = true;
      }
    }
    if (!changed) break;
  }
  let matched = 0;
  let total = 0;
  for (const file of live) {
    total += file.scored * weights.get(file);
    matched += file.matched * weights.get(file);
  }
  return { rate: matched / total, cells: total };
}

function summarize(rows, label) {
  const files = rows.filter(usable);
  const scored = files.filter((row) => row.score.scored > 0);
  const fileScores = scored.map((row) => ({
    scored: row.score.scored,
    matched: row.score.matched,
  }));
  const totals = { scored: 0, matched: 0, wrong: 0, type: 0, unsupported: 0, near: 0, seeded: 0 };
  for (const row of files) {
    const score = row.score;
    totals.scored += score.scored;
    totals.matched += score.matched;
    totals.wrong += score.wrongValue;
    totals.type += score.wrongType;
    totals.unsupported += score.unsupported;
    totals.near += score.nearZeroOnly;
    totals.seeded += score.seeded;
  }
  const capped = cappedRate(fileScores, CAP);
  return {
    label,
    files: scored.length,
    ...totals,
    pooled: totals.scored ? totals.matched / totals.scored : 0,
    capped: capped.rate,
    perfect: scored.filter((row) => row.score.matched === row.score.scored).length,
  };
}

const lines = [];
for (const input of inputs) {
  const rows = (await readFile(input, 'utf8'))
    .trim()
    .split('\n')
    .map((line) => JSON.parse(line));
  const engine = rows[0]?.engine ?? input;
  const run = rows.length;
  const ok = rows.filter((row) => row.ok).length;
  const failed = rows.filter((row) => !row.ok);
  const reasons = {};
  for (const row of failed) {
    const key = (row.error ?? '').slice(0, 60);
    reasons[key] = (reasons[key] ?? 0) + 1;
  }
  lines.push('## ' + engine, '');
  lines.push(`${run} files run, ${ok} finished, ${failed.length} failed.`);
  if (failed.length) lines.push('Failures: ' + JSON.stringify(reasons));
  lines.push('');
  const real = rows.filter((row) => row.source !== 'generated');
  const sets = [
    summarize(real, 'All Excel-saved public files'),
    summarize(
      real.filter((row) => !row.engineFixture),
      'Without either engine’s own fixtures',
    ),
    summarize(
      real.filter((row) => !row.engineFixture && !conformance(row.path)),
      'Documents (without fixtures or function test suites)',
    ),
    summarize(
      real.filter((row) => !row.engineFixture && conformance(row.path)),
      'Function test suites (without fixtures)',
    ),
    ...(rows.some((row) => row.source === 'private')
      ? [
          summarize(
            rows.filter((row) => row.source === 'private'),
            'Private workbooks',
          ),
        ]
      : []),
    ...[...new Set(real.map((row) => row.source))].sort().map((source) =>
      summarize(
        real.filter((row) => row.source === source),
        '  ' + source,
      ),
    ),
  ];
  lines.push(
    '| Set | Files | Formula cells | Match (pooled) | Match (10% cap) | Wrong value | Wrong type | Unsupported | Near zero | All cells match |',
    '| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |',
  );
  for (const set of sets) {
    lines.push(
      `| ${set.label} | ${set.files} | ${set.scored} | ${pct(set.matched, set.scored)} | ${(100 * set.capped).toFixed(3)}% | ${set.wrong} | ${set.type} | ${set.unsupported} | ${set.near} | ${set.perfect} |`,
    );
  }
  lines.push('');

  const functions = {};
  for (const row of real.filter(usable)) {
    for (const [name, score] of Object.entries(row.score.byFunction)) {
      const entry = (functions[name] ??= { cells: 0, matched: 0, unsupported: 0, files: 0 });
      entry.cells += score.cells;
      entry.matched += score.matched;
      entry.unsupported += score.unsupported;
      entry.files += 1;
    }
  }
  const names = Object.keys(functions).sort(
    (a, b) =>
      functions[b].cells - functions[b].matched - (functions[a].cells - functions[a].matched) ||
      functions[b].cells - functions[a].cells,
  );
  lines.push(
    `### Per function (${names.length} functions, misses first)`,
    '',
    'A cell counts for every function in its formula, so a miss can belong to another function in the same formula or to a precedent.',
    '',
    '| Function | Files | Cells | Matched | Match | Unsupported |',
    '| --- | --- | --- | --- | --- | --- |',
  );
  for (const name of names) {
    const entry = functions[name];
    lines.push(
      `| ${name} | ${entry.files} | ${entry.cells} | ${entry.matched} | ${pct(entry.matched, entry.cells)} | ${entry.unsupported} |`,
    );
  }
  lines.push('');

  const slices = [];
  for (const row of rows.filter((candidate) => candidate.ok)) {
    for (const sample of row.sliceSamples ?? []) slices.push(sample);
  }
  slices.sort((a, b) => a - b);
  lines.push(
    '### Slices',
    '',
    `${slices.length} slices: p50 ${ms(percentile(slices, 0.5))} ms, p99 ${ms(percentile(slices, 0.99))} ms, max ${ms(slices.at(-1) ?? 0)} ms.`,
    '',
  );
  const generated = rows.filter((row) => row.source === 'generated');
  if (generated.length) {
    lines.push(
      '### Scale workbooks',
      '',
      '| Workbook | Formulas | Load | First edit after a cached load | Its longest slice | First calc | Slices | Slice p99 | Slice max | Edit p50 | Edit p99 | Edit slice p99 | Edit slice max | Top-row insert | Then settle | Peak memory | Match |',
      '| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |',
    );
    for (const row of generated) {
      if (!row.ok) {
        lines.push(`| ${row.path} | | ${row.error} | | | | | | | | | | | | | | |`);
        continue;
      }
      const mib = row.wasmMemoryBytes ? (row.wasmMemoryBytes / 1048576).toFixed(0) + ' MiB' : '';
      lines.push(
        `| ${row.path} | ${row.formulas} | ${ms(row.loadMs)} ms | ${ms(row.firstEditMs)} ms | ${ms(row.firstEditMaxSliceMs)} ms | ${ms(row.firstCalcMs)} ms | ${row.firstCalcSlices.count} | ${ms(row.firstCalcSlices.p99Ms)} | ${ms(row.firstCalcSlices.maxMs)} | ${ms(row.edit.p50Ms)} | ${ms(row.edit.p99Ms)} | ${ms(row.editSlices.p99Ms)} | ${ms(row.editSlices.maxMs)} | ${ms(row.topRowInsertMs)} ms | ${ms(row.topRowSettleMs)} ms | ${mib} | ${pct(row.score.matched, row.score.scored)} |`,
      );
    }
    lines.push('');
  }
  if (detail) {
    const worst = rows
      .filter(usable)
      .filter((row) => row.score.scored && row.score.matched < row.score.scored)
      .sort((a, b) => b.score.scored - b.score.matched - (a.score.scored - a.score.matched))
      .slice(0, 30);
    lines.push('### Worst files', '');
    for (const row of worst) {
      lines.push(
        `- ${row.path}: ${row.score.scored - row.score.matched} of ${row.score.scored} miss`,
      );
      for (const example of row.score.examples.slice(0, 3)) {
        lines.push(
          `    - ${example.sheet}!${example.cell} \`${example.formula}\` stored ${example.stored} got ${example.got}`,
        );
      }
    }
    lines.push('');
  }
}
const text = lines.join('\n') + '\n';
if (outArg) await writeFile(outArg.slice(6), text);
else console.log(text);
