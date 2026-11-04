// Runs Phase 3's save proof over folders of workbooks: `savecheck` saves each with and without
// edits, and the Open XML SDK compares the saved file's errors with its source's. Saved files live
// in a temporary folder that is always deleted. Public files are never opened in Excel here.
// Usage: node bench/run-save-check.mjs <out.json> <folder>... [--private]
// With `--private`, the output holds counts only: no file names, parts or cell values.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../scripts/lib.mjs';

const isPrivate = process.argv.includes('--private');
const [out, ...folders] = process.argv.slice(2).filter((arg) => !arg.startsWith('--'));
const scratch = mkdtempSync(join(tmpdir(), 'cgs-save-check-'));
const inherit = { stdio: 'inherit', cwd: root, windowsHide: true };

function run(command, args, options = inherit) {
  const result = spawnSync(command, args, { ...options, shell: false });
  if (result.status !== 0) throw new Error(`${command} ${args[0]} failed with ${result.status}`);
}

const add = (map, key, by = 1) => {
  map[key] = (map[key] ?? 0) + by;
};

try {
  run('cargo', ['build', '--profile', 'fast', '-p', 'cgs-bench', '--bin', 'savecheck']);
  run('dotnet', [
    'build',
    join(root, 'tests/validator/Validator.csproj'),
    '-c',
    'Release',
    '--nologo',
    '-v',
    'quiet',
    '-o',
    join(root, 'target/validator'),
  ]);

  const jsonl = join(scratch, 'save.jsonl');
  const kept = join(scratch, 'saved');
  run(join(root, 'target/fast/savecheck.exe'), [
    jsonl,
    ...folders,
    `--keep=${kept}`,
    ...(isPrivate ? ['--private'] : []),
  ]);
  const records = readFileSync(jsonl, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((line) => JSON.parse(line));

  const pairs = records.filter((record) => record.report.saved);
  const list = join(scratch, 'list.txt');
  writeFileSync(list, pairs.flatMap((pair) => [pair.source, pair.report.saved]).join('\n') + '\n');
  const validated = join(scratch, 'validated.jsonl');
  run(join(root, 'target/validator/Validator.exe'), [list, validated]);
  const verdicts = readFileSync(validated, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((line) => JSON.parse(line));

  const summary = {
    files: records.length,
    refused: 0,
    noEditPossible: 0,
    noEditReasons: {},
    refusedDamage: 0,
    failed: 0,
    noEditIdentical: 0,
    noEditEntriesMatch: 0,
    noEditDiffers: 0,
    edited: 0,
    entriesChecked: 0,
    entriesDifferent: 0,
    cellsCompared: 0,
    cellsMatched: 0,
    valueDifferences: {},
    resultsLeftToExcel: 0,
    savedBytes: 0,
    sourceBytes: 0,
    validated: pairs.length,
    sourceUnopenable: 0,
    savedUnopenable: 0,
    newlyUnopenable: 0,
    sourceErrors: 0,
    savedErrors: 0,
    filesWithNewErrors: 0,
    newErrors: {},
    failures: {},
  };
  const detail = [];
  for (const record of records) {
    const report = record.report;
    if (report.refused) {
      summary.refused++;
      continue;
    }
    if (!report.ok) {
      summary.failed++;
      add(summary.failures, String(report.error ?? '?').slice(0, 70));
      if (!isPrivate) detail.push({ file: record.file, error: report.error });
      continue;
    }
    if (report.no_edit_possible) {
      summary.noEditPossible++;
      // The engine's first words, which can name a sheet, so private runs keep only the count.
      if (!isPrivate) add(summary.noEditReasons, String(report.no_edit_possible).slice(0, 12));
    }
    if (report.refused_damage) summary.refusedDamage++;
    if (report.noedit_identical) summary.noEditIdentical++;
    else if (report.noedit_entries_match) summary.noEditEntriesMatch++;
    else summary.noEditDiffers++;
    if (report.edited) summary.edited++;
    summary.entriesChecked += report.entries_checked;
    summary.entriesDifferent += report.entries_different;
    summary.cellsCompared += report.compared;
    summary.cellsMatched += report.matched;
    summary.resultsLeftToExcel += report.results_left_to_excel;
    summary.savedBytes += report.saved_bytes;
    summary.sourceBytes += report.source_bytes;
    for (const [category, count] of Object.entries(report.categories)) {
      add(summary.valueDifferences, category, count);
    }
    if (!isPrivate && (report.compared !== report.matched || report.entries_different)) {
      detail.push({ file: record.file, examples: report.examples, categories: report.categories });
    }
  }
  pairs.forEach((pair, index) => {
    const before = verdicts[index * 2];
    const after = verdicts[index * 2 + 1];
    if (before.openError) summary.sourceUnopenable++;
    if (after.openError) summary.savedUnopenable++;
    if (after.openError && !before.openError) {
      summary.newlyUnopenable++;
      if (!isPrivate) detail.push({ file: pair.file, newlyUnopenable: after.openError });
    }
    const fresh = {};
    for (const count of Object.values(before.errors)) summary.sourceErrors += count;
    for (const [key, count] of Object.entries(after.errors)) {
      summary.savedErrors += count;
      const extra = count - (before.errors[key] ?? 0);
      if (extra > 0) fresh[key] = extra;
    }
    if (Object.keys(fresh).length) {
      summary.filesWithNewErrors++;
      for (const [key, count] of Object.entries(fresh)) {
        // The rule's id only: a part's name or a message can say something about the file.
        add(summary.newErrors, key.split('|')[0], count);
        if (!isPrivate) {
          detail.push({ file: pair.file, newError: key, count, sample: after.samples[key] });
        }
      }
    }
  });
  writeFileSync(out, JSON.stringify(isPrivate ? summary : { ...summary, detail }, null, 1) + '\n');
  console.log(summary);
  if (
    summary.failed ||
    summary.noEditDiffers ||
    summary.entriesDifferent ||
    summary.newlyUnopenable ||
    summary.filesWithNewErrors
  ) {
    process.exitCode = 1;
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
