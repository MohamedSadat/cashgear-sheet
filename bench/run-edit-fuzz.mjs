// Random edits against Excel: typing, formulas, fill, paste, moves, rows, columns and sheets, with
// undo and redo, on a seed Excel builds with names, a table and legacy arrays. The reference machine's Excel is 2016,
// which has no dynamic arrays, so those are held to other tests.
// 200 episodes of 5 edits from the same seed, so one divergence can't taint the others. Every step
// compares every cell and formula with Excel, and each episode's save is then opened in Excel and
// compared with what Excel made of the same edits.
//
// Excel's own undo doesn't hold edits made over COM, so an undo is judged by what it must return to:
// the state Excel showed before the edit it takes back.
//
// Usage: node bench/run-edit-fuzz.mjs [--seed=1] [--episodes=200] [--steps=5] [--fresh]
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../scripts/lib.mjs';

const option = (name, fallback) => {
  const found = process.argv.find((arg) => arg.startsWith('--' + name + '='));
  return found ? found.split('=')[1] : fallback;
};
const seedNumber = Number(option('seed', '1'));
const EPISODES = Number(option('episodes', '200'));
const STEPS = Number(option('steps', '5'));
const fresh = process.argv.includes('--fresh');

let state = BigInt(seedNumber) * 0x9e3779b97f4a7c15n + 1n;
function random(limit) {
  state ^= (state << 13n) & 0xffffffffffffffffn;
  state ^= state >> 7n;
  state ^= (state << 17n) & 0xffffffffffffffffn;
  return Number(state % BigInt(limit));
}
const pick = (list) => list[random(list.length)];

const col = (number) => {
  let out = '';
  for (let at = number; at > 0; at = Math.floor((at - 1) / 26)) {
    out = String.fromCharCode(65 + ((at - 1) % 26)) + out;
  }
  return out;
};
const cell = () => col(1 + random(9)) + (1 + random(16));
const range = () => {
  const [a, b] = [1 + random(9), 1 + random(9)].sort((x, y) => x - y);
  const [c, d] = [1 + random(16), 1 + random(16)].sort((x, y) => x - y);
  return col(a) + c + ':' + col(b) + d;
};
const quoted = (name) => (/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) ? name : "'" + name + "'");

const texts = ['hello', 'pen', 'a b', 'ورقة', '', '  spaced  ', 'x1'];
const typed = () =>
  pick([
    () => String(random(1000)),
    () => String(random(1000) / 8),
    () => '-' + random(50),
    () => pick(texts) || 'word',
    () => pick(['TRUE', 'FALSE']),
    () => pick(['50%', '12.5%', '1,234', '1,234.5', '$5', '1e3', '1 1/2']),
    () => pick(['1/2/2020', '2020-03-04', 'Jan 5', '12:30', '1:30 PM']),
    () => "'" + random(10),
  ])();

function formula(sheets, here) {
  const other = pick(sheets).name;
  const ref = other === here ? '' : quoted(other) + '!';
  return pick([
    () => `=${cell()}+${cell()}`,
    () => `=SUM(${range()})`,
    () => `=SUM(${ref}${range()})`,
    () => `=${ref}${cell()}*2`,
    () => `=$${col(1 + random(5))}$${1 + random(9)}+${cell()}`,
    () => `=IF(${cell()}>3,"big",${cell()})`,
    () => `=COUNTA(${range()})`,
    () => `=AVERAGE(1,${cell()},3)`,
    () => `=CONCATENATE("a",${cell()})`,
    () => `=SUM(${col(1 + random(4))}:${col(5 + random(3))})`,
    () => `=SUM(${1 + random(4)}:${5 + random(9)})`,
    () => `=ROW(${cell()})`,
    () => `=INDEX(${range()},1,1)`,
    () => `=SUM(Sales[Cost])*Rate`,
    () => `=SUM(Prices)`,
    () => `=${cell()}/0`,
    () => `=1+1`,
    () => `=TEXT(${cell()},"0.0")`,
  ])();
}

const area = (rows = 4, cols = 3) => {
  const top = 1 + random(14);
  const left = 1 + random(8);
  return { top, left, bottom: top + random(rows), right: left + random(cols) };
};

/** A plain edit, given the sheets as they stand. */
function randomOp(sheets, nextId) {
  const roll = random(100);
  const at = random(sheets.length);
  const target = sheets[at];
  const base = { sheet: target.id, pos: at + 1 };
  if (roll < 25) {
    return { ...base, kind: 'input', row: 1 + random(18), col: 1 + random(10), text: typed() };
  }
  if (roll < 45) {
    return {
      ...base,
      kind: 'input',
      row: 1 + random(18),
      col: 1 + random(10),
      text: formula(sheets, target.name),
    };
  }
  if (roll < 50) return { ...base, kind: 'clear', areas: [area()] };
  if (roll < 57) {
    const from = area(2, 1);
    const down = random(2) === 0;
    const to = down
      ? { ...from, bottom: from.bottom + 1 + random(4) }
      : { ...from, right: from.right + 1 + random(3) };
    return { ...base, kind: 'fill', from, to };
  }
  if (roll < 65) {
    const fromAt = random(sheets.length);
    const from = area();
    const top = 1 + random(20);
    const left = 1 + random(10);
    return {
      kind: 'paste',
      fromSheet: sheets[fromAt].id,
      fromPos: fromAt + 1,
      from,
      sheet: target.id,
      pos: at + 1,
      to: {
        top,
        left,
        bottom: top + (from.bottom - from.top),
        right: left + (from.right - from.left),
      },
    };
  }
  if (roll < 71) {
    const from = area(2, 1);
    return {
      kind: 'move',
      fromSheet: target.id,
      fromPos: at + 1,
      from,
      sheet: target.id,
      pos: at + 1,
      row: 1 + random(20),
      col: 1 + random(10),
    };
  }
  if (roll < 92) {
    const axis = random(2) === 0 ? 'row' : 'col';
    return {
      ...base,
      kind: 'axis',
      axis,
      at: 1 + random(axis === 'row' ? 20 : 11),
      count: 1 + random(3),
      delete: random(2) === 0,
    };
  }
  if (roll < 95) {
    const position = random(sheets.length + 1);
    return { kind: 'insertSheet', position, name: 'New ' + nextId };
  }
  if (roll < 97 && sheets.length > 1) return { ...base, kind: 'deleteSheet' };
  return {
    ...base,
    kind: 'renameSheet',
    name: pick(['Renamed', 'My Data', 'R2', 'New Name']) + nextId,
  };
}

const initialSheets = () => [
  { id: 0, name: 'Data' },
  { id: 1, name: 'Calc Sheet' },
  { id: 2, name: 'Spare' },
];

function applyToSheets(sheets, op, id) {
  if (op.kind === 'insertSheet') {
    sheets.splice(op.position, 0, { id, name: op.name });
  } else if (op.kind === 'deleteSheet') {
    sheets.splice(op.pos - 1, 1);
  } else if (op.kind === 'renameSheet') {
    sheets[op.pos - 1] = { ...sheets[op.pos - 1], name: op.name };
  }
}

// Each episode lists its steps. An edit runs in Excel as it stands. An undo or redo tells Excel to
// reopen the seed and replay what is left, and the engine side runs Undo or Redo itself.
const episodes = Array.from({ length: EPISODES }, (_, episode) => {
  const done = [];
  const undone = [];
  let nextId = 3;
  const steps = [];
  for (let step = 0; step < STEPS; step++) {
    const sheets = initialSheets();
    let id = 3;
    for (const op of done) {
      applyToSheets(sheets, op, op.kind === 'insertSheet' ? op.id : id);
    }
    const roll = random(100);
    if (roll < 12 && done.length > 0) {
      undone.push(done.pop());
      steps.push({ kind: 'undo', replay: done.map(strip) });
    } else if (roll < 17 && undone.length > 0) {
      done.push(undone.pop());
      steps.push({ kind: 'redo', replay: done.map(strip) });
    } else {
      const op = randomOp(sheets, nextId);
      if (op.kind === 'insertSheet') op.id = nextId;
      nextId += op.kind === 'insertSheet' ? 1 : 0;
      undone.length = 0;
      done.push(op);
      steps.push({ kind: 'edit', op: strip(op), ...(step === 0 ? { replay: [] } : {}) });
    }
  }
  return { episode, steps };
});

function strip(op) {
  return { ...op };
}

const outDir = join(root, 'bench/results/edits');
mkdirSync(outDir, { recursive: true });
const seedFile = join(root, 'tests/workbooks/generated/edit-seed.xlsx');
const script = readFileSync(join(root, 'bench/excel-edits.ps1'), 'utf8');
const quotePs = (text) => "'" + String(text).replace(/'/g, "''") + "'";
function excel(args) {
  const result = spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      '& {' +
        script +
        '} ' +
        Object.entries(args)
          .map(([key, value]) => `-${key} ${quotePs(value)}`)
          .join(' '),
    ],
    { encoding: 'utf8', timeout: 3_600_000, windowsHide: true, maxBuffer: 1 << 26 },
  );
  if (result.status !== 0) throw new Error('Excel run failed: ' + result.stderr + result.stdout);
}

if (!existsSync(seedFile) || process.argv.includes('--fresh-seed')) {
  rmSync(seedFile, { force: true });
  excel({ Mode: 'seed', Out: seedFile });
}
const opsFile = join(outDir, `ops-${seedNumber}.json`);
const excelFile = join(outDir, `excel-${seedNumber}.jsonl`);
writeFileSync(opsFile, JSON.stringify(episodes));
if (!existsSync(excelFile) || fresh) {
  const started = Date.now();
  excel({ Mode: 'edits', Seed: seedFile, Ops: opsFile, Out: excelFile });
  console.log('Excel finished in ' + Math.round((Date.now() - started) / 1000) + ' s');
}

const scratch = mkdtempSync(join(tmpdir(), 'cgs-edit-fuzz-'));
try {
  const tool = join(root, 'target/fast/editfuzz.exe');
  const run = spawnSync(tool, ['--check', seedFile, opsFile, excelFile, scratch], {
    encoding: 'utf8',
    maxBuffer: 1 << 28,
    windowsHide: true,
  });
  if (run.status !== 0) throw new Error('editfuzz failed: ' + run.stderr);
  const report = JSON.parse(run.stdout);
  const savedFile = join(outDir, `saved-${seedNumber}.jsonl`);
  excel({ Mode: 'saved', Folder: scratch, Out: savedFile });
  const saved = spawnSync(tool, ['--saved', excelFile, savedFile], {
    encoding: 'utf8',
    maxBuffer: 1 << 28,
    windowsHide: true,
  });
  if (saved.status !== 0) throw new Error('editfuzz --saved failed: ' + saved.stderr);
  const savedReport = JSON.parse(saved.stdout);
  writeFileSync(
    join(outDir, `report-${seedNumber}.json`),
    JSON.stringify({ edits: report, saved: savedReport }, null, 1),
  );
  console.log(
    `seed ${seedNumber}: ${report.stepsPassed} of ${report.stepsScheduled} steps match Excel, ${report.stepsDiverged} diverged, ${report.stepsSkipped} not reached; ${report.refusedByBoth} refused by both, ${report.refusedByExcelOnly} by Excel only, ${report.refusedByEngineOnly} by the engine only, ${report.ignoredByExcel} ignored by Excel and refused here; ${report.circular} episodes stopped at a circular reference; ${report.cellsCompared} cells compared`,
  );
  console.log(
    (run.stderr ? run.stderr.slice(0, 4000) + '\n' : '') +
      `saved: ${savedReport.opened} of ${savedReport.files} opened with no repair, ${savedReport.matched} match Excel's own result, ${savedReport.cellsCompared} cells compared`,
  );
  for (const sample of report.divergences.slice(0, Number(option('show', '12')))) {
    console.log(
      `  ep ${sample.episode} step ${sample.step} ${sample.op}: ${sample.sheet}!${sample.cell} ${sample.what} excel=${sample.excel} engine=${sample.engine} (${sample.differingCells} cells)`,
    );
  }
  for (const sample of savedReport.problems.slice(0, 8)) console.log('  saved: ' + sample);
} finally {
  if (process.argv.includes('--keep')) console.log('saved files are in ' + scratch);
  else rmSync(scratch, { recursive: true, force: true });
}
