// Writes the fixture workbooks to tests/workbooks, which git ignores. `fixtures` holds good ones,
// and `broken` holds the ones the Excel open matrix uses to prove it can see a repair.
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { root } from './lib.mjs';
import {
  arabicBook,
  brokenBooks,
  defaultStyledBook,
  formattingBook,
  macroBook,
  movingPartsBook,
  prefixedBook,
  pivotBook,
  structuresBook,
} from '../tests/fixtures/workbooks.mjs';

const good = join(root, 'tests/workbooks/fixtures');
const broken = join(root, 'tests/workbooks/broken');
mkdirSync(good, { recursive: true });
mkdirSync(broken, { recursive: true });
for (const [name, bytes] of Object.entries({
  'arabic.xlsx': arabicBook(),
  'defaults.xlsx': defaultStyledBook(),
  'formatting.xlsx': formattingBook(),
  'pivot.xlsx': pivotBook(),
  'macro.xlsm': macroBook(),
  'structures.xlsx': structuresBook(),
  'moving-parts.xlsx': movingPartsBook(),
  'prefixed.xlsx': prefixedBook(),
})) {
  writeFileSync(join(good, name), bytes);
}
for (const [name, bytes] of Object.entries(brokenBooks())) {
  writeFileSync(join(broken, name + '.xlsx'), bytes);
}
console.log('Wrote the fixtures to tests/workbooks.');
