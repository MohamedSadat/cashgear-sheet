// Types the same text into Excel and into `parse_input`, and lists where they read it differently:
// a number, a date, text or a formula, and the format it takes on. Excel works in a new empty
// workbook, which it never saves. The machine's own regional settings apply, so this is en-US here.
// Usage: node bench/run-excel-input.mjs
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../scripts/lib.mjs';

const CASES = [
  // Numbers.
  '1',
  '1.5',
  '.5',
  '5.',
  '-1',
  '+1',
  '(1)',
  '(1.5)',
  '-(1)',
  '00012',
  '1e3',
  '1E3',
  '1e+3',
  '1.5e-2',
  '1e',
  'e5',
  '1e400',
  '1E-400',
  '99999999999999999',
  '0.1234567890123456789',
  // Groups.
  '1,000',
  '1,000.5',
  '1,23',
  '12,34,56',
  '1,2345',
  ',5',
  '1,',
  '1.2.3',
  '1,000,000',
  '1000,000',
  '1 000',
  // Percent.
  '1%',
  '1.5%',
  '50 %',
  '-5%',
  '%5',
  '.5%',
  '1,000%',
  '1e3%',
  '%',
  // Money.
  '$5',
  '$5.50',
  '-$5',
  '$-5',
  '($5)',
  '$1,000',
  '5$',
  '$1e3',
  '$',
  '$ 5',
  // Fractions.
  '1 1/2',
  '1 1/20',
  '0 1/2',
  '1 1/0',
  '-1 1/2',
  '1 2/3',
  '1 100/200',
  // Dates.
  '1/2',
  '3/4/2021',
  '3/4/21',
  '2021-03-04',
  '2021/3/4',
  '3-4',
  '3/4',
  'Mar 4',
  '4 Mar',
  '4-Mar-21',
  'March 2021',
  'Mar-21',
  'mar 4 2021',
  'March 4, 2021',
  '13/13',
  '32/1',
  '1/0',
  '1900-01-01',
  '2/29/1900',
  '1/0/1900',
  '12/31/9999',
  '1/1/10000',
  '1/1/1899',
  '2/30/2021',
  '1-2-3',
  // Times.
  '12:30',
  '12:30:15',
  '1:30 PM',
  '1:30 pm',
  '1 PM',
  '13:00:00',
  '25:00',
  '1:60',
  '0:00',
  '24:00',
  '1:30:15 AM',
  '12:30.5',
  '3/4/2021 12:30',
  '3/4/2021 1:30 PM',
  // Words.
  'TRUE',
  'true',
  'True',
  'FALSE',
  '#N/A',
  '#div/0!',
  '#REF!',
  '#VALUE!',
  'abc',
  ' abc',
  'abc ',
  ' 1',
  '1 ',
  ' ',
  // Marks.
  "'abc",
  "'123",
  "'",
  '@',
  '-',
  '+',
  '--1',
  '1-',
  '1+',
  '(',
  '()',
  '-a',
  '+a',
  // Formulas.
  '1:61',
  '1:100',
  '1:30:60',
  '1:30:61',
  '60:00',
  '9999:00',
  '10000:00',
  '1:02:03.5',
  '12:30.55',
  '2/30',
  '1/32',
  '4/31',
  '1/99',
  '1/100',
  '4 Mar 2021',
  '1-2/3',
  '12 1/2',
  '-1:30',
  '$.5',
  '$5%',
  '5%$',
  '1,000e3',
  '(1%)',
  '(-1)',
  '- 1',
  '+ 1',
  '1E3.5',
  '0x10',
  '1_000',
  '=1+1',
  '= 1+1',
  '+1+1',
  '-1+1',
  '+A1',
  '-A1',
  '=',
  '==1',
  '=A1',
  '=SUM(1,2)',
  '+SUM(1,2)',
];

const scratch = mkdtempSync(join(tmpdir(), 'cgs-excel-input-'));
try {
  const inputs = join(scratch, 'inputs.json');
  const seen = join(scratch, 'seen.json');
  writeFileSync(inputs, JSON.stringify(CASES));
  const script = readFileSync(join(root, 'bench/excel-input.ps1'), 'utf8');
  const excel = spawnSync(
    'powershell',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `& {${script}} -Inputs '${inputs}' -Out '${seen}'`,
    ],
    { encoding: 'utf8', timeout: 300000, windowsHide: true },
  );
  if (excel.status !== 0) throw new Error('Excel failed: ' + excel.stderr.slice(0, 600));
  const now = new Date();
  const today =
    Math.floor(Date.UTC(now.getFullYear(), now.getMonth(), now.getDate()) / 864e5) + 25569;
  const ours = spawnSync(join(root, 'target/fast/inputcheck.exe'), [seen, String(today)], {
    encoding: 'utf8',
    windowsHide: true,
  });
  process.stdout.write(ours.stdout);
  if (ours.status !== 0) throw new Error(ours.stderr.slice(0, 600));
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
