// Do formats round-trip through Excel? Formats go on through the engine and are saved, Excel reads each
// back over COM and saves a copy its own way, and our reader reads that copy. All three have to agree,
// and a file's existing style indices have to stay as they were. Only generated workbooks and, with
// `--private=<folder>`, your private ones go through it: public files never open in Excel. Everything
// saved goes to a temporary folder that is always deleted, and originals are only read. For private
// files, only counts reach the output.
// Usage: node bench/run-excel-format.mjs <out.json> [--private=<folder>]
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import {
  DATE_CODES,
  FRACTION_CODES,
  SPECIAL_CODES,
  TIME_CODES,
} from '../packages/ui/src/number-formats.ts';
import { root } from '../scripts/lib.mjs';
import { inflateRawSync } from 'node:zlib';

const out = process.argv[2];
const privateFolder = process.argv.find((arg) => arg.startsWith('--private='))?.slice(10);
const scratch = mkdtempSync(join(tmpdir(), 'cgs-excel-format-'));
const tool = join(root, 'target/fast/savecheck.exe');
const script = readFileSync(join(root, 'bench/excel-format.ps1'), 'utf8');
const fixtures = join(root, 'tests/workbooks/fixtures');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";

const area = (row, col, bottom = row, right = col) => ({ top: row, left: col, bottom, right });

/** Everything the ribbon and the dialog can set, one to a cell. */
const FONT = [
  { fontName: 'Arial' },
  { fontName: 'Times New Roman', fontSize: 14 },
  { fontName: 'Courier New', bold: true },
  { fontName: 'Tahoma', italic: true },
  { fontName: 'Verdana', fontSize: 9 },
  { fontSize: 8 },
  { fontSize: 10.5 },
  { fontSize: 18, bold: true, italic: true },
  { fontSize: 36 },
  { bold: true },
  { italic: true },
  { strike: true },
  { underline: 'single' },
  { underline: 'double' },
  { underline: 'singleAccounting' },
  { underline: 'doubleAccounting' },
  { fontColor: { kind: 'rgb', rgb: 'FF0000' } },
  { fontColor: { kind: 'rgb', rgb: '00B050' } },
  { fontColor: { kind: 'theme', theme: 4 } },
  { fontColor: { kind: 'theme', theme: 5, tint: 0.4 } },
  { fontColor: { kind: 'theme', theme: 1, tint: 0.5 } },
  { fontColor: { kind: 'theme', theme: 9, tint: -0.25 } },
  { fontColor: { kind: 'auto' } },
];
const PATTERNS = [
  'gray125',
  'gray0625',
  'lightGray',
  'mediumGray',
  'darkGray',
  'darkHorizontal',
  'darkVertical',
  'darkDown',
  'darkUp',
  'darkGrid',
  'darkTrellis',
  'lightHorizontal',
  'lightVertical',
  'lightDown',
  'lightUp',
  'lightGrid',
  'lightTrellis',
];
const FILL = [
  { fill: { pattern: 'solid', color: { kind: 'rgb', rgb: 'FFFF00' } } },
  { fill: { pattern: 'solid', color: { kind: 'theme', theme: 4 } } },
  { fill: { pattern: 'solid', color: { kind: 'theme', theme: 6, tint: 0.6 } } },
  { fill: { pattern: 'solid', color: { kind: 'theme', theme: 0, tint: -0.15 } } },
  ...PATTERNS.map((pattern) => ({
    fill: {
      pattern,
      color: { kind: 'rgb', rgb: 'FFFFCC' },
      patternColor: { kind: 'rgb', rgb: '0070C0' },
    },
  })),
  { fill: { pattern: 'darkUp', patternColor: { kind: 'theme', theme: 5 } } },
];
const LINES = [
  'hair',
  'dotted',
  'dashed',
  'dashDot',
  'dashDotDot',
  'thin',
  'medium',
  'mediumDashed',
  'mediumDashDot',
  'mediumDashDotDot',
  'slantDashDot',
  'thick',
  'double',
];
const BORDER = [
  ...LINES.map((style) => ({ border: { bottom: { style } } })),
  { border: { top: { style: 'thin', color: { kind: 'rgb', rgb: 'FF0000' } } } },
  { border: { left: { style: 'medium', color: { kind: 'theme', theme: 4 } } } },
  { border: { right: { style: 'thick', color: { kind: 'theme', theme: 5, tint: 0.4 } } } },
  { border: { diagonal: { style: 'thin' }, diagonalDown: true } },
  { border: { diagonal: { style: 'dashed' }, diagonalUp: true } },
  { border: { diagonal: { style: 'medium' }, diagonalUp: true, diagonalDown: true } },
];
const ALIGNMENT = [
  ...['left', 'center', 'right', 'fill', 'justify', 'centerContinuous', 'distributed'].map(
    (horizontal) => ({ horizontal }),
  ),
  ...['top', 'center', 'justify', 'distributed'].map((vertical) => ({ vertical })),
  { wrap: true },
  { shrink: true },
  { horizontal: 'left', indent: 3 },
  { horizontal: 'right', indent: 2 },
  { rotation: 45 },
  { rotation: 90 },
  { rotation: 135 },
  { rotation: 180 },
  { rotation: 255 },
  { readingOrder: 1 },
  { readingOrder: 2 },
];
const CODES = [
  ...DATE_CODES,
  ...TIME_CODES,
  ...FRACTION_CODES,
  ...SPECIAL_CODES,
  '0.000" kg"',
  '#,##0.00_);[Red]\\(#,##0.00\\)',
  '[Blue]0.0;[Red]-0.0;0',
  '"$"#,##0.00',
  '0.0%',
];
const NUMBER = [
  ...[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22].map(
    (numberFormatId) => ({ numberFormatId }),
  ),
  ...[37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49].map((numberFormatId) => ({
    numberFormatId,
  })),
  ...CODES.map((numberFormat) => ({ numberFormat })),
];

const FAMILIES = { font: FONT, fill: FILL, border: BORDER, alignment: ALIGNMENT, number: NUMBER };

/** The commands, on a blank workbook of a few sheets, and which cells and lines they should reach. */
function plan() {
  const commands = [
    { kind: 'insertSheet', position: 1, name: 'Lines' },
    { kind: 'insertSheet', position: 2, name: 'Whole' },
    { kind: 'insertSheet', position: 3, name: 'Top row' },
    { kind: 'insertSheet', position: 4, name: 'First column' },
    { kind: 'insertSheet', position: 5, name: 'Both' },
  ];
  const cells = [];
  const rows = [];
  const cols = [];
  let col = 1;
  for (const [family, list] of Object.entries(FAMILIES)) {
    list.forEach((change, index) => {
      // Every other row, so a cell's neighbour says nothing about its edges.
      const row = index * 2 + 2;
      const text = family === 'number' ? '1234.5' : 'Text ' + family;
      commands.push({ kind: 'input', sheet: 0, row, col, text });
      commands.push({ kind: 'format', sheet: 0, areas: [area(row, col)], change });
      cells.push({ sheet: 0, row, col, family, change });
    });
    col += 2;
  }
  // Wide enough that a number shows as itself, rather than as hashes.
  commands.push({ kind: 'lines', sheet: 0, axis: 'col', first: 9, last: 9, size: 320 });
  // Selections: where each cell sits decides its lines.
  commands.push({
    kind: 'format',
    sheet: 0,
    areas: [area(2, 12, 4, 14)],
    change: {
      border: {
        top: { style: 'thick' },
        bottom: { style: 'double' },
        left: { style: 'medium' },
        right: { style: 'dashed' },
        insideHorizontal: { style: 'thin' },
        insideVertical: { style: 'dotted' },
      },
    },
  });
  for (let row = 2; row <= 4; row++) {
    for (let column = 12; column <= 14; column++) {
      cells.push({ sheet: 0, row, col: column, family: 'selection', change: {} });
    }
  }
  // Merges.
  const merged = [
    [{ sheet: 0, areas: [area(7, 12, 8, 14)], center: true }, area(7, 12, 8, 14)],
    [{ sheet: 0, areas: [area(10, 12, 11, 14)], across: true }, area(10, 12)],
    [{ sheet: 0, areas: [area(13, 12, 14, 13)] }, area(13, 12, 14, 13)],
  ];
  commands.push({ kind: 'input', sheet: 0, row: 7, col: 12, text: 'kept' });
  commands.push({ kind: 'input', sheet: 0, row: 7, col: 13, text: 'lost' });
  commands.push({ kind: 'input', sheet: 0, row: 13, col: 13, text: 'moves up' });
  for (const [command, at] of merged) {
    commands.push({ kind: 'merge', ...command });
    cells.push({ sheet: 0, row: at.top, col: at.left, family: 'merge', change: {} });
  }
  commands.push({ kind: 'merge', sheet: 0, areas: [area(16, 12, 16, 13)] });
  commands.push({ kind: 'unmerge', sheet: 0, areas: [area(16, 12)] });
  cells.push({ sheet: 0, row: 16, col: 12, family: 'merge', change: {} });
  // Sizes, on the second sheet. Nothing here goes in cells, except what fits by itself.
  const line = (axis, first, last, extra) =>
    commands.push({ kind: 'lines', sheet: 1, axis, first, last, ...extra });
  line('row', 2, 2, { size: 40 });
  line('row', 4, 6, { size: 15 });
  line('row', 8, 8, { hidden: true });
  line('row', 10, 12, { hidden: true });
  line('row', 11, 11, { hidden: false });
  line('row', 14, 14, { size: 0 });
  line('col', 2, 2, { size: 100 });
  line('col', 4, 6, { size: 30 });
  line('col', 8, 8, { hidden: true });
  line('col', 10, 12, { hidden: true });
  line('col', 11, 11, { hidden: false });
  line('col', 14, 14, { size: 0 });
  commands.push({ kind: 'input', sheet: 1, row: 16, col: 1, text: 'big' });
  commands.push({
    kind: 'format',
    sheet: 1,
    areas: [area(16, 1)],
    change: { fontSize: 24 },
  });
  for (let row = 1; row <= 17; row++) rows.push({ sheet: 1, line: row });
  for (let column = 1; column <= 15; column++) cols.push({ sheet: 1, line: column });
  // Whole rows and columns, and the whole sheet's look through them.
  const whole = (areaOf, change) =>
    commands.push({ kind: 'format', sheet: 2, areas: [areaOf], change });
  commands.push({ kind: 'input', sheet: 2, row: 2, col: 2, text: 'holds a value' });
  whole(area(1, 2, 1048576, 2), {
    fill: { pattern: 'solid', color: { kind: 'rgb', rgb: 'DDEBF7' } },
  });
  whole(area(3, 1, 3, 16384), { bold: true });
  whole(area(1, 4, 1048576, 4), { fontSize: 20 });
  cells.push({ sheet: 2, row: 2, col: 2, family: 'whole', change: {} });
  cells.push({ sheet: 2, row: 9, col: 2, family: 'whole', change: {} });
  cells.push({ sheet: 2, row: 3, col: 2, family: 'whole', change: {} });
  cells.push({ sheet: 2, row: 3, col: 7, family: 'whole', change: {} });
  cells.push({ sheet: 2, row: 9, col: 4, family: 'whole', change: {} });
  rows.push({ sheet: 2, line: 9 });
  rows.push({ sheet: 2, line: 2 });
  // Panes and what a sheet shows, on the sheets of their own.
  commands.push({ kind: 'freeze', sheet: 3, rows: 1, cols: 0 });
  commands.push({ kind: 'freeze', sheet: 4, rows: 0, cols: 1 });
  commands.push({ kind: 'freeze', sheet: 5, rows: 2, cols: 3 });
  commands.push({ kind: 'viewFlags', sheet: 5, grid: false, headings: false });
  commands.push({ kind: 'viewFlags', sheet: 4, grid: false });
  const sheets = [
    { name: 'Sheet1', frozen: false },
    { name: 'Lines', frozen: false },
    { name: 'Whole', frozen: false },
    { name: 'Top row', frozen: true, splitRow: 1, splitColumn: 0 },
    { name: 'First column', frozen: true, splitRow: 0, splitColumn: 1, gridlines: false },
    {
      name: 'Both',
      frozen: true,
      splitRow: 2,
      splitColumn: 3,
      gridlines: false,
      headings: false,
    },
  ];
  return { commands, cells, rows, cols, sheets };
}

function savecheck(args) {
  const run = spawnSync(tool, args, { encoding: 'utf8', windowsHide: true, maxBuffer: 1 << 28 });
  if (run.status !== 0) throw new Error('savecheck failed: ' + run.stderr.slice(0, 400));
  return run.stdout;
}

function excelReads(path, wanted) {
  const cellsFile = join(scratch, 'cells.json');
  const result = join(scratch, 'excel.json');
  const copy = join(scratch, `excel-copy-${Math.random().toString(36).slice(2)}.xlsx`);
  writeFileSync(cellsFile, JSON.stringify(wanted));
  rmSync(result, { force: true });
  const parameters = { Workbook: resolve(path), Cells: cellsFile, Copy: copy, Out: result };
  const args = Object.entries(parameters)
    .map(([key, value]) => `-${key} ${quoted(value)}`)
    .join(' ');
  const run = spawnSync(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-Command', `& {${script}} ${args}`],
    { encoding: 'utf8', timeout: 900000, windowsHide: true },
  );
  if (run.status !== 0) throw new Error('Excel script failed: ' + run.stderr.slice(0, 400));
  return { read: JSON.parse(readFileSync(result, 'utf8').replace(/^\uFEFF/, '')), copy };
}

function ourRead(path, wanted) {
  const cellsFile = join(scratch, 'ours.json');
  writeFileSync(cellsFile, JSON.stringify(wanted));
  return JSON.parse(savecheck(['--formats', path, cellsFile]));
}

const UNDERLINE = {
  none: -4142,
  single: 2,
  double: -4119,
  singleAccounting: 4,
  doubleAccounting: 5,
};
const HORIZONTAL = {
  general: 1,
  left: -4131,
  center: -4108,
  right: -4152,
  fill: 5,
  justify: -4130,
  centerContinuous: 7,
  distributed: -4117,
};
const VERTICAL = { top: -4160, center: -4108, bottom: -4107, justify: -4130, distributed: -4117 };
const PATTERN = {
  none: -4142,
  solid: 1,
  darkGray: -4126,
  mediumGray: -4125,
  lightGray: -4124,
  gray125: 17,
  gray0625: 18,
  darkHorizontal: -4128,
  darkVertical: -4166,
  darkDown: -4121,
  darkUp: -4162,
  darkGrid: 9,
  darkTrellis: 10,
  lightHorizontal: 11,
  lightVertical: 12,
  lightDown: 13,
  lightUp: 14,
  lightGrid: 15,
  lightTrellis: 16,
};
const LINE = {
  none: [-4142],
  hair: [1, 1],
  dotted: [-4118, 2],
  dashed: [-4115, 2],
  dashDot: [4, 2],
  dashDotDot: [5, 2],
  thin: [1, 2],
  medium: [1, -4138],
  mediumDashed: [-4115, -4138],
  mediumDashDot: [4, -4138],
  mediumDashDotDot: [5, -4138],
  slantDashDot: [13, -4138],
  thick: [1, 4],
  double: [-4119, 4],
};
const bgr = (hex) => {
  const value = parseInt(hex.slice(1), 16);
  return ((value & 0xff) << 16) | (value & 0xff00) | ((value >> 16) & 0xff);
};
const near = (a, b, within = 2) => {
  for (const shift of [0, 8, 16]) {
    if (Math.abs(((a >> shift) & 255) - ((b >> shift) & 255)) > within) return false;
  }
  return true;
};
const ROTATION = (stored) => {
  if (stored === 255) return -4166;
  if (stored === 90) return -4171;
  if (stored === 180) return -4170;
  return stored > 90 ? 90 - stored : stored || -4128;
};

/** Where our reading of a saved file and Excel's differ, for one cell. */
function compareCell(ours, excel) {
  const problems = [];
  const same = (name, a, b) => {
    if (a !== b) problems.push({ what: name, ours: a, excel: b });
  };
  same('font name', ours.fontName, excel.fontName);
  same('font size', ours.fontSize, excel.fontSize);
  same('bold', ours.bold, excel.bold);
  same('italic', ours.italic, excel.italic);
  same('strike', ours.strike, excel.strike);
  same('underline', UNDERLINE[ours.underline], excel.underline);
  if (ours.fontColor.kind !== 'auto') {
    if (!near(bgr(ours.fontColor.color), excel.fontColor)) {
      problems.push({ what: 'font color', ours: ours.fontColor, excel: excel.fontColor });
    }
  } else if (excel.fontColor !== 0) {
    problems.push({ what: 'automatic font color', ours: 'automatic', excel: excel.fontColor });
  }
  same('pattern', PATTERN[ours.fill.pattern], excel.pattern);
  if (ours.fill.pattern !== 'none' && ours.fill.color.kind !== 'auto') {
    if (!near(bgr(ours.fill.color.color), excel.fillColor)) {
      problems.push({ what: 'fill color', ours: ours.fill.color, excel: excel.fillColor });
    }
  }
  if (!['none', 'solid'].includes(ours.fill.pattern) && ours.fill.patternColor.kind !== 'auto') {
    if (!near(bgr(ours.fill.patternColor.color), excel.patternColor)) {
      problems.push({
        what: 'pattern color',
        ours: ours.fill.patternColor,
        excel: excel.patternColor,
      });
    }
  }
  same('horizontal', HORIZONTAL[ours.horizontal], excel.horizontal);
  same('vertical', VERTICAL[ours.vertical], excel.vertical);
  same('wrap', ours.wrap, excel.wrap);
  same('shrink', ours.shrink, excel.shrink);
  same('indent', ours.indent, excel.indent);
  same('rotation', ROTATION(ours.rotation), excel.orientation);
  same('reading order', [-5002, -5003, -5004][ours.readingOrder], excel.readingOrder);
  same('merged', ours.merged, excel.merged);
  // A line on one cell's edge shows on its neighbour too, so only what ours says is on is compared.
  for (const side of ['top', 'bottom', 'left', 'right']) {
    const line = ours.border[side];
    if (line.style === 'none') continue;
    const [style, weight] = LINE[line.style];
    same(side + ' line', style, excel[side].s);
    same(side + ' weight', weight, excel[side].w);
    if (line.color.kind !== 'auto' && !near(bgr(line.color.color), excel[side].c)) {
      problems.push({ what: side + ' color', ours: line.color, excel: excel[side].c });
    }
  }
  if (ours.border.diagonal.style !== 'none') {
    if (ours.border.diagonalDown)
      same('diagonal down', LINE[ours.border.diagonal.style][0], excel.diagonalDown.s);
    if (ours.border.diagonalUp)
      same('diagonal up', LINE[ours.border.diagonal.style][0], excel.diagonalUp.s);
  }
  return problems;
}

/** Comparing our reading of our save with our reading of Excel's copy. */
function compareOurs(a, b) {
  const problems = [];
  // Excel keeps a tint to its own precision, and writes plain characters in a code as escapes.
  const plain = (value) =>
    JSON.stringify(value, (name, item) => {
      if (name === 'tint') return Math.round(item * 1000) / 1000;
      if (name === 'color' && typeof item === 'string') return item.slice(0, 5);
      return item;
    });
  const code = (text) => text.replace(/\\([^A-Za-z0-9])/g, '$1');
  for (const key of Object.keys(a)) {
    if (key === 'numberFormat') {
      if (code(a[key]) !== code(b[key])) {
        problems.push({ what: key, saved: a[key], excelCopy: b[key] });
      }
    } else if (key === 'numberFormatId') {
      continue;
    } else if (plain(a[key]) !== plain(b[key])) {
      problems.push({ what: key, saved: a[key], excelCopy: b[key] });
    }
  }
  return problems;
}

/** The entries of one list in `styles.xml`, in order. */
function entries(prefixed, list, entry) {
  const styles = prefixed.replace(/<(\/?)[A-Za-z0-9]+:/g, '<$1');
  const start = styles.indexOf('<' + list);
  if (start < 0) return [];
  const open = styles.indexOf('>', start);
  if (styles[open - 1] === '/') return [];
  const end = styles.indexOf('</' + list + '>', open);
  const body = styles.slice(open + 1, end);
  const out = [];
  let at = 0;
  const pattern = new RegExp(`<${entry}(\\s[^>]*)?(/>|>)`, 'g');
  for (;;) {
    pattern.lastIndex = at;
    const found = pattern.exec(body);
    if (!found) break;
    let stop = found.index + found[0].length;
    if (!found[0].endsWith('/>')) {
      const close = body.indexOf(`</${entry}>`, stop);
      if (close < 0) break;
      stop = close + entry.length + 3;
    }
    out.push(body.slice(found.index, stop));
    at = stop;
  }
  return out;
}

const STYLE_LISTS = [
  ['numFmts', 'numFmt'],
  ['fonts', 'font'],
  ['fills', 'fill'],
  ['borders', 'border'],
  ['cellStyleXfs', 'xf'],
  ['cellXfs', 'xf'],
  ['cellStyles', 'cellStyle'],
  ['dxfs', 'dxf'],
];

/** Which of a file's existing style entries moved or changed. Nothing should have. */
function stylesKept(before, after) {
  const changed = [];
  let compared = 0;
  for (const [list, entry] of STYLE_LISTS) {
    const old = entries(before, list, entry);
    const next = entries(after, list, entry);
    compared += old.length;
    old.forEach((item, index) => {
      if (next[index] !== item) changed.push(list + '[' + index + ']');
    });
  }
  return { compared, changed };
}

/** A zip's entries by its central directory, as Excel's zips leave sizes out of local headers. */
function readZip(bytes) {
  let end = bytes.length - 22;
  while (end >= 0 && bytes.readUInt32LE(end) !== 0x06054b50) end--;
  const count = bytes.readUInt16LE(end + 10);
  let at = bytes.readUInt32LE(end + 16);
  const out = [];
  for (let index = 0; index < count; index++) {
    const method = bytes.readUInt16LE(at + 10);
    const packed = bytes.readUInt32LE(at + 20);
    const nameLength = bytes.readUInt16LE(at + 28);
    const extra = bytes.readUInt16LE(at + 30);
    const comment = bytes.readUInt16LE(at + 32);
    const local = bytes.readUInt32LE(at + 42);
    const name = bytes.toString('utf8', at + 46, at + 46 + nameLength);
    const start = local + 30 + bytes.readUInt16LE(local + 26) + bytes.readUInt16LE(local + 28);
    const body = bytes.subarray(start, start + packed);
    out.push([name, method === 8 ? inflateRawSync(body) : body]);
    at += 46 + nameLength + extra + comment;
  }
  return out;
}

const part = (bytes, name) => {
  const found = readZip(bytes).find(([entry]) => entry === name);
  return found ? found[1].toString('utf8') : '';
};

/** The part of a workbook's first sheet. */
function firstSheetPart(bytes) {
  const workbook = part(bytes, 'xl/workbook.xml');
  const rels = part(bytes, 'xl/_rels/workbook.xml.rels');
  const tag = /<(?:[a-z0-9]+:)?sheet\s[^>]*>/.exec(workbook)?.[0] ?? '';
  const id = /r:id="([^"]+)"/.exec(tag)?.[1];
  for (const found of rels.matchAll(/<Relationship\s[^>]*>/g)) {
    if (found[0].includes('Id="' + id + '"')) {
      const target = /Target="([^"]+)"/.exec(found[0])?.[1] ?? '';
      return target.startsWith('/') ? target.slice(1) : 'xl/' + target;
    }
  }
  return 'xl/worksheets/sheet1.xml';
}

/** Cells' style indices in a sheet's XML. */
function cellStyles(xml) {
  const out = new Map();
  for (const found of xml.matchAll(/<c\s[^>]*?r="([A-Z]+\d+)"[^>]*?>/g)) {
    const style = /\ss="(\d+)"/.exec(found[0]);
    out.set(found[1], style ? Number(style[1]) : 0);
  }
  return out;
}

/** Formats a few cells of a file's first sheet, and sees what that leaves of its styles. */
const TOUCHED = (sheet) => [
  { kind: 'format', sheet, areas: [area(1, 1, 3, 3)], change: { bold: true, fontSize: 13 } },
  {
    kind: 'format',
    sheet,
    areas: [area(1, 1, 3, 3)],
    change: { fill: { pattern: 'solid', color: { kind: 'rgb', rgb: 'FFF2CC' } } },
  },
  { kind: 'format', sheet, areas: [area(2, 2)], change: { numberFormat: '0.000" kg"' } },
  {
    kind: 'format',
    sheet,
    areas: [area(1, 1, 3, 3)],
    change: { border: { bottom: { style: 'thin' }, top: { style: 'medium' } } },
  },
  {
    kind: 'format',
    sheet,
    areas: [area(1, 1, 3, 3)],
    change: { horizontal: 'center', wrap: true },
  },
  { kind: 'lines', sheet, axis: 'col', first: 2, last: 2, size: 90 },
  { kind: 'lines', sheet, axis: 'row', first: 2, last: 2, size: 33 },
];

const WANT_TOUCHED = {
  cells: [
    { sheet: 0, row: 1, col: 1 },
    { sheet: 0, row: 2, col: 2 },
    { sheet: 0, row: 3, col: 3 },
  ],
  rows: [{ sheet: 0, line: 2 }],
  cols: [{ sheet: 0, line: 2 }],
};

const results = {
  generated: {
    cells: 0,
    cellsAgree: 0,
    roundTripCells: 0,
    roundTripAgree: 0,
    displayedTexts: 0,
    displayedTextsAgree: 0,
    problems: [],
    lines: { compared: 0, agree: 0, problems: [] },
    sheets: { compared: 0, agree: 0, problems: [] },
  },
  indices: { files: 0, compared: 0, changed: [], cellsCompared: 0, cellsChanged: [] },
  private: null,
};

try {
  const { commands, cells, rows, cols, sheets } = plan();
  const saved = join(scratch, 'formats.xlsx');
  const args = ['--edit', 'blank', saved];
  const listFile = join(scratch, 'commands.json');
  writeFileSync(listFile, JSON.stringify(commands));
  const report = savecheck([...args, '--command-file=' + listFile]);
  results.generated.report = JSON.parse(report).changedParts;
  const wanted = {
    cells: cells.map(({ sheet, row, col }) => ({ sheet, row, col })),
    rows,
    cols,
  };
  const { read, copy } = excelReads(saved, wanted);
  if (!read.opened) throw new Error('Excel refused the saved file: ' + read.error);
  const ours = ourRead(saved, wanted);
  const again = ourRead(copy, wanted);
  cells.forEach((cell, index) => {
    const excel = read.cells[index];
    const mine = ours.cells[index];
    results.generated.cells++;
    const problems = compareCell(mine.format, excel);
    if (!problems.length) results.generated.cellsAgree++;
    else results.generated.problems.push({ cell, problems });
    results.generated.displayedTexts++;
    // Excel's text pads a repeated character out to the column, and ours leaves that to drawing.
    const flat = (text) => text.replace(/\s+/g, '');
    if (flat(mine.display) === flat(excel.text)) results.generated.displayedTextsAgree++;
    else
      results.generated.problems.push({
        cell,
        problems: [{ what: 'shows', ours: mine.display, excel: excel.text }],
      });
    results.generated.roundTripCells++;
    const lost = compareOurs(mine.format, again.cells[index].format);
    if (!lost.length) results.generated.roundTripAgree++;
    else results.generated.problems.push({ cell, roundTrip: lost });
  });
  // Sizes come to whole pixels on both sides.
  rows.forEach((row, index) => {
    const excel = read.rows[index];
    const mine = ours.rows[index];
    results.generated.lines.compared++;
    const px = Math.round((excel.height * 96) / 72);
    if (mine.hidden === excel.hidden && (excel.hidden || mine.px === px)) {
      results.generated.lines.agree++;
    } else results.generated.lines.problems.push({ row, ours: mine, excel });
  });
  cols.forEach((col, index) => {
    const excel = read.cols[index];
    const mine = ours.cols[index];
    results.generated.lines.compared++;
    const px = Math.round((excel.width * 96) / 72);
    if (mine.hidden === excel.hidden && (excel.hidden || mine.px === px)) {
      results.generated.lines.agree++;
    } else results.generated.lines.problems.push({ col, ours: mine, excel });
  });
  sheets.forEach((expected, index) => {
    const excel = read.sheets[index];
    results.generated.sheets.compared++;
    const fine =
      excel.frozen === expected.frozen &&
      (!expected.frozen ||
        (excel.splitRow === expected.splitRow && excel.splitColumn === expected.splitColumn)) &&
      excel.gridlines === (expected.gridlines ?? true) &&
      excel.headings === (expected.headings ?? true);
    if (fine) results.generated.sheets.agree++;
    else results.generated.sheets.problems.push({ expected, excel });
  });
  rmSync(copy, { force: true });

  // A file's existing styles stay as they were, and so does every cell a format didn't reach.
  for (const name of readdirSync(fixtures).filter((file) => /\.xls[xm]$/.test(file))) {
    const source = join(fixtures, name);
    const target = join(scratch, 'touched-' + name);
    savecheck([
      '--edit',
      source,
      target,
      ...TOUCHED(0).map((c) => '--command=' + JSON.stringify(c)),
    ]);
    const before = readFileSync(source);
    const after = readFileSync(target);
    results.indices.files++;
    const kept = stylesKept(part(before, 'xl/styles.xml'), part(after, 'xl/styles.xml'));
    results.indices.compared += kept.compared;
    results.indices.changed.push(...kept.changed.map((entry) => name + ':' + entry));
    // Every sheet's cells outside what was formatted.
    const oldSheets = readZip(before).filter(([entry]) =>
      /^xl\/worksheets\/[^/]+\.xml$/.test(entry),
    );
    for (const [entry, body] of oldSheets) {
      const was = cellStyles(body.toString('utf8'));
      const now = cellStyles(part(after, entry));
      const first = entry === firstSheetPart(before);
      for (const [at, style] of was) {
        const match = /([A-Z]+)(\d+)/.exec(at);
        const reached =
          first && match[1].length === 1 && 'ABC'.includes(match[1]) && Number(match[2]) <= 4;
        if (reached) continue;
        results.indices.cellsCompared++;
        if (now.get(at) !== style) results.indices.cellsChanged.push(name + ':' + entry + ':' + at);
      }
    }
    const seen = excelReads(target, WANT_TOUCHED);
    if (!seen.read.opened) results.indices.changed.push(name + ': Excel refused it');
    rmSync(seen.copy, { force: true });
    rmSync(target, { force: true });
  }

  if (privateFolder) {
    // Counts only, from here on. Nothing names a file or says what is in one.
    const summary = {
      files: 0,
      opened: 0,
      failed: 0,
      stylesCompared: 0,
      stylesChanged: 0,
      cellsCompared: 0,
      cellsChanged: 0,
      cellsRead: 0,
      cellsAgree: 0,
      roundTripCells: 0,
      roundTripAgree: 0,
    };
    const folder = join(scratch, 'private');
    mkdirSync(folder);
    for (const name of readdirSync(privateFolder).filter((file) => /\.xls[xm]$/i.test(file))) {
      summary.files++;
      const source = join(privateFolder, name);
      const extension = name.split('.').pop();
      const target = join(folder, `file-${summary.files}.${extension}`);
      try {
        savecheck([
          '--edit',
          source,
          target,
          ...TOUCHED(0).map((c) => '--command=' + JSON.stringify(c)),
        ]);
        const before = readFileSync(source);
        const after = readFileSync(target);
        const kept = stylesKept(part(before, 'xl/styles.xml'), part(after, 'xl/styles.xml'));
        summary.stylesCompared += kept.compared;
        summary.stylesChanged += kept.changed.length;
        const oldSheets = readZip(before).filter(([entry]) =>
          /^xl\/worksheets\/[^/]+\.xml$/.test(entry),
        );
        for (const [entry, body] of oldSheets) {
          const was = cellStyles(body.toString('utf8'));
          const now = cellStyles(part(after, entry));
          const first = entry === firstSheetPart(before);
          for (const [at, style] of was) {
            const match = /([A-Z]+)(\d+)/.exec(at);
            const reached =
              first && match[1].length === 1 && 'ABC'.includes(match[1]) && Number(match[2]) <= 4;
            if (reached) continue;
            summary.cellsCompared++;
            if (now.get(at) !== style) summary.cellsChanged++;
          }
        }
        const { read: seen, copy: excelCopy } = excelReads(target, WANT_TOUCHED);
        if (seen.opened) summary.opened++;
        const mine = ourRead(target, WANT_TOUCHED);
        mine.cells.forEach((cell, index) => {
          summary.cellsRead++;
          if (!compareCell(cell.format, seen.cells[index]).length) summary.cellsAgree++;
        });
        const again2 = ourRead(excelCopy, WANT_TOUCHED);
        mine.cells.forEach((cell, index) => {
          summary.roundTripCells++;
          if (!compareOurs(cell.format, again2.cells[index].format).length)
            summary.roundTripAgree++;
        });
        rmSync(excelCopy, { force: true });
      } catch (error) {
        summary.failed++;
        // The kind of failure only: nothing from inside a file.
        const kind = /^[^:]*/.exec(String(error?.message))?.[0] ?? 'unknown';
        summary.failures = summary.failures ?? {};
        summary.failures[kind] = (summary.failures[kind] ?? 0) + 1;
      }
      rmSync(target, { force: true });
    }
    results.private = summary;
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
  writeFileSync(out, JSON.stringify(results, null, 1) + '\n');
}
const g = results.generated;
console.log(
  `Excel read ${g.cellsAgree} of ${g.cells} cells as ours do. They show the same in ${g.displayedTextsAgree} of ${g.displayedTexts}. ` +
    `Back from Excel: ${g.roundTripAgree} of ${g.roundTripCells}. Lines: ${g.lines.agree} of ${g.lines.compared}. Sheets: ${g.sheets.agree} of ${g.sheets.compared}.`,
);
console.log(
  `Existing styles: ${results.indices.compared} compared in ${results.indices.files} files, ${results.indices.changed.length} changed. ` +
    `Cells a format didn't reach: ${results.indices.cellsCompared} compared, ${results.indices.cellsChanged.length} changed.`,
);
if (results.private) console.log('Private: ' + JSON.stringify(results.private));
