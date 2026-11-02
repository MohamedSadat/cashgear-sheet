// Compares our display text with Excel's own, cell by cell, over a folder of workbooks. Only
// generated and private workbooks go through here: public files never open in Excel.
// Usage: node bench/run-excel-text.mjs <folder> <out.json> [--private]
// With `--private`, the output holds counts only: no file names and no cell values.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { root } from '../scripts/lib.mjs';

const [folder, out] = process.argv.slice(2).filter((arg) => !arg.startsWith('--'));
const isPrivate = process.argv.includes('--private');
const tool = join(root, 'target/fast/exceltext.exe');
// Run as a command, like the other Excel scripts here, which needs no change to script policy.
const script = readFileSync(join(root, 'bench/excel-text.ps1'), 'utf8');

function walk(dir, found = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    // The format zoo has its own test in cgs-numfmt, and the speed books aren't for Excel.
    if (statSync(path).isDirectory()) {
      if (entry !== 'format-zoo' && entry !== 'speed') walk(path, found);
    } else if (/\.(xlsx|xlsm)$/i.test(entry) && !entry.startsWith('~$')) found.push(path);
  }
  return found;
}

const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";
const scratch = mkdtempSync(join(tmpdir(), 'cgs-excel-text-'));
const totals = { files: 0, failed: 0, compared: 0, matched: 0, skippedWidth: 0, kinds: {} };
const perFile = [];
try {
  for (const path of walk(folder)) {
    if (statSync(path).size > 40 * 1024 * 1024) continue;
    const seen = join(scratch, 'seen.json');
    const excel = spawnSync(
      'powershell',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        '& {' + script + '} -Workbook ' + quoted(resolve(path)) + ' -Out ' + quoted(seen),
      ],
      { encoding: 'utf8', timeout: 900000, windowsHide: true },
    );
    if (excel.status !== 0) {
      totals.failed++;
      if (!isPrivate) console.error('Excel failed on ' + path + ': ' + excel.stderr.slice(0, 300));
      continue;
    }
    const ours = spawnSync(tool, [path, seen, ...(isPrivate ? ['--private'] : [])], {
      encoding: 'utf8',
      maxBuffer: 256 * 1024 * 1024,
      windowsHide: true,
    });
    if (ours.status !== 0) {
      totals.failed++;
      if (!isPrivate)
        console.error('Our side failed on ' + path + ': ' + ours.stderr.slice(0, 300));
      continue;
    }
    const report = JSON.parse(ours.stdout);
    totals.files++;
    totals.compared += report.compared;
    totals.matched += report.matched;
    totals.skippedWidth += report.skippedWidth;
    for (const [kind, count] of Object.entries(report.kinds)) {
      totals.kinds[kind] = (totals.kinds[kind] ?? 0) + count;
    }
    if (!isPrivate) perFile.push({ file: path.slice(folder.length + 1), ...report });
    console.log(
      (isPrivate ? 'file ' + totals.files : path.split(/[\\/]/).pop()) +
        ': ' +
        report.matched +
        ' of ' +
        report.compared,
    );
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
const share = totals.compared ? (totals.matched / totals.compared) * 100 : 100;
const result = { ...totals, matchPercent: Number(share.toFixed(3)), perFile };
writeFileSync(out, JSON.stringify(result, null, 1) + '\n');
console.log(
  `${totals.files} files, ${totals.failed} failed. ${totals.matched} of ${totals.compared} cells match Excel's text (${share.toFixed(3)}%). ${totals.skippedWidth} cells left out: Excel was showing ### because of its column width.`,
);
console.log(totals.kinds);
