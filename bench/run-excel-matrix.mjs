// The COM-open matrix for Phase 3: saved files must open in Excel with no repair, and what a save
// kept must still work there. Only generated fixtures and, with `--private=<folder>`, your private
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
function save(source, extension, sets = [], extra = []) {
  const saved = join(scratch, `saved-${++serial}.${extension}`);
  const run = spawnSync(
    tool,
    ['--edit', source, saved, ...sets.map((set) => '--set=' + set), ...extra],
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
