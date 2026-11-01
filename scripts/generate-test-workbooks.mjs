// Writes scale workbooks into tests/workbooks/generated, which git ignores, and has Excel
// calculate and resave them over COM, so their stored values are Excel's. These measure
// time and memory; the accuracy target is pooled over non-generated files only.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { crc32, deflateRawSync } from 'node:zlib';
import { root } from './lib.mjs';

const outRoot = join(root, 'tests/workbooks/generated');

function zip(entries) {
  const chunks = [];
  const central = [];
  let offset = 0;
  for (const [name, text] of entries) {
    const body = Buffer.from(text, 'utf8');
    const packed = deflateRawSync(body, { level: 6 });
    const nameBytes = Buffer.from(name);
    const crc = crc32(body);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(8, 8);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(packed.length, 18);
    local.writeUInt32LE(body.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    const header = Buffer.alloc(46);
    header.writeUInt32LE(0x02014b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(20, 6);
    header.writeUInt16LE(8, 10);
    header.writeUInt32LE(crc, 16);
    header.writeUInt32LE(packed.length, 20);
    header.writeUInt32LE(body.length, 24);
    header.writeUInt16LE(nameBytes.length, 28);
    header.writeUInt32LE(offset, 42);
    central.push(header, nameBytes);
    chunks.push(local, nameBytes, packed);
    offset += local.length + nameBytes.length + packed.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...chunks, directory, end]);
}

const column = (index) => {
  let text = '';
  for (let rest = index; rest > 0; rest = Math.floor((rest - 1) / 26)) {
    text = String.fromCharCode(65 + ((rest - 1) % 26)) + text;
  }
  return text;
};

const escapeXml = (text) => text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/** `rows` yields arrays: a number, a `=formula` string, a plain string, or null. */
function sheetXml(rows) {
  const parts = [
    '<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>',
  ];
  let rowNumber = 0;
  for (const row of rows) {
    rowNumber++;
    parts.push(`<row r="${rowNumber}">`);
    row.forEach((cell, index) => {
      if (cell === null) return;
      const at = column(index + 1) + rowNumber;
      if (typeof cell === 'number') parts.push(`<c r="${at}"><v>${cell}</v></c>`);
      else if (cell.startsWith('='))
        parts.push(`<c r="${at}"><f>${escapeXml(cell.slice(1))}</f></c>`);
      else parts.push(`<c r="${at}" t="inlineStr"><is><t>${escapeXml(cell)}</t></is></c>`);
    });
    parts.push('</row>');
  }
  parts.push('</sheetData></worksheet>');
  return parts.join('');
}

function workbookZip(sheets, names = '') {
  const entries = [
    [
      '[Content_Types].xml',
      '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>' +
        sheets
          .map(
            (_, index) =>
              `<Override PartName="/xl/worksheets/sheet${index + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`,
          )
          .join('') +
        '</Types>',
    ],
    [
      '_rels/.rels',
      '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>',
    ],
    [
      'xl/workbook.xml',
      '<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>' +
        sheets
          .map(
            (sheet, index) =>
              `<sheet name="${sheet.name}" sheetId="${index + 1}" r:id="rId${index + 1}"/>`,
          )
          .join('') +
        '</sheets>' +
        names +
        '</workbook>',
    ],
    [
      'xl/_rels/workbook.xml.rels',
      '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
        sheets
          .map(
            (_, index) =>
              `<Relationship Id="rId${index + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet${index + 1}.xml"/>`,
          )
          .join('') +
        '</Relationships>',
    ],
  ];
  sheets.forEach((sheet, index) => {
    entries.push([`xl/worksheets/sheet${index + 1}.xml`, sheetXml(sheet.rows())]);
  });
  return zip(entries);
}

function* rowModel(count, sheet = '') {
  for (let r = 1; r <= count; r++) {
    yield [
      r % 1000,
      (r * 7) % 13,
      `=A${r}*B${r}`,
      `=C${r}+1${sheet}`,
      `=IF(D${r}>100,D${r}-1,D${r})`,
    ];
  }
}

function* chain(count) {
  yield [1];
  for (let r = 2; r <= count; r++) yield [`=A${r - 1}+1`];
}

function* lookups(table, formulas) {
  for (let r = 1; r <= Math.max(table, formulas); r++) {
    yield [
      r <= table ? r : null,
      r <= table ? (r * 3) % 1009 : null,
      null,
      r <= formulas ? (r * 37) % table : null,
      r <= formulas ? `=VLOOKUP(D${r},$A$1:$B$${table},2,FALSE)` : null,
      r <= formulas ? `=INDEX($B$1:$B$${table},MATCH(D${r},$A$1:$A$${table},0))` : null,
    ];
  }
}

function* aggregates(data, formulas) {
  for (let r = 1; r <= Math.max(data, formulas); r++) {
    yield [
      r <= data ? r % 100 : null,
      r <= data ? r % 7 : null,
      null,
      r <= formulas ? r % 7 : null,
      r <= formulas ? `=SUMIF($B$1:$B$${data},D${r},$A$1:$A$${data})` : null,
      r <= formulas ? `=COUNTIF($B$1:$B$${data},D${r})` : null,
      r <= formulas ? `=SUM($A$1:A${Math.min(r, data)})` : null,
    ];
  }
}

function* textModel(count) {
  for (let r = 1; r <= count; r++) {
    yield [
      `item ${r % 500}`,
      `=UPPER(A${r})`,
      `=LEFT(B${r},3)&"-"&LEN(A${r})`,
      `=IF(ISNUMBER(SEARCH("1",A${r})),"has one","none")`,
    ];
  }
}

function crossSheets(sheets, rows) {
  return Array.from({ length: sheets }, (_, index) => ({
    name: 'S' + (index + 1),
    rows: function* () {
      for (let r = 1; r <= rows; r++) {
        const previous = index === 0 ? String(r % 97) : `S${index}!C${r}`;
        yield [r % 1000, `=A${r}*2`, `=B${r}+${previous}`];
      }
    },
  }));
}

/** Every kind of reference that moves differently under inserts and deletes. */
function structuralSeed() {
  const formula = (row, col) => {
    const left = column(Math.max(1, col - 1)) + row;
    // The first row reads a constant, so that no formula reads itself.
    const up = row > 1 ? column(col) + (row - 1) : 'A2';
    switch ((row * 3 + col * 5) % 16) {
      case 0:
        return `=${left}+1`;
      case 1:
        return `=$A${row}*2`;
      case 2:
        return `=$A${row}+B$1`;
      case 3:
        return `=SUM(A${row}:${left})`;
      case 4:
        return `=SUM($A$1:$A$20)`;
      case 5:
        return `=Data!B${row}+1`;
      case 6:
        return `=SUM(Data!$A$1:C${row})`;
      case 7:
        return `=SUM(A:A)`;
      case 8:
        return `=SUM(62:62)+COUNT(62:63)`;
      case 9:
        return `=Rate*${up}`;
      case 10:
        return `=SUM(Block)`;
      case 11:
        return `=ROW()+COLUMN()`;
      case 12:
        return `=ROWS($A$1:A${row})+COLUMNS($A$1:${left})`;
      case 13:
        return `=IF(${left}>10,"big",${up})`;
      case 14:
        return `=INDEX($A$1:$B$40,${(row % 40) + 1},${(col % 2) + 1})`;
      default:
        return `=VLOOKUP(MOD(${row},30)+1,Data!$A$1:$C$40,2,FALSE)`;
    }
  };
  return [
    {
      name: 'Main',
      rows: function* () {
        for (let row = 1; row <= 63; row++) {
          // Rows 61 to 63 hold constants for the whole-row references.
          if (row > 60) {
            yield Array.from({ length: 14 }, (_, index) => row * (index + 1));
            continue;
          }
          yield Array.from({ length: 14 }, (_, index) => {
            if (index === 0) return row;
            if (index === 1) return (row * 7) % 11;
            return row % 4 === 0 && index % 3 === 0 ? null : formula(row, index + 1);
          });
        }
      },
    },
    {
      name: 'Data',
      rows: function* () {
        for (let row = 1; row <= 60; row++) {
          yield [
            row,
            (row * 13) % 17,
            row % 5 === 0 ? 'text ' + row : row * 2,
            `=A${row}+B${row}`,
            `=SUM($A$1:A${row})`,
            `=Main!A${row}*2`,
            `=SUM(Main!$B$1:B${row})`,
            // Every sixth row divides by zero, so errors move with the edits too.
            row % 3 === 0 ? `=D${row}/(A${row}-${row % 6 === 0 ? row : row - 1})` : null,
          ];
        }
      },
    },
  ];
}
const structuralNames =
  '<definedNames><definedName name="Block">Data!$A$2:$B$9</definedName><definedName name="Rate">Data!$B$3</definedName></definedNames>';

const one = (rows) => [{ name: 'Sheet1', rows }];
const workbooks = {
  'row-model-10k': one(() => rowModel(10_000)),
  'row-model-100k': one(() => rowModel(100_000)),
  'row-model-333k': one(() => rowModel(333_334)),
  'chain-10k': one(() => chain(10_000)),
  'chain-100k': one(() => chain(100_000)),
  'lookups-5k': one(() => lookups(20_000, 5_000)),
  'aggregates-2k': one(() => aggregates(100_000, 2_000)),
  'text-50k': one(() => textModel(50_000)),
  'cross-sheet-8x20k': crossSheets(8, 20_000),
  'structural-seed': structuralSeed(),
};
const definedNames = { 'structural-seed': structuralNames };

/** Has Excel calculate a generated file and save it. No macros and no links are involved. */
function excelRecalc(source, target) {
  const script = `
$ErrorActionPreference = 'Stop'
$x = New-Object -ComObject Excel.Application
try {
  $x.Visible = $false
  $x.DisplayAlerts = $false
  $x.AutomationSecurity = 3
  $w = $x.Workbooks.Open('${source}', 0, $false)
  $x.CalculateFullRebuild()
  $w.SaveAs('${target}', 51)
  $w.Close($false)
} finally {
  $x.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($x)
}`;
  const result = spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', script], {
    encoding: 'utf8',
    timeout: 1_800_000,
  });
  if (result.status !== 0) throw new Error('Excel failed on ' + source + ': ' + result.stderr);
}

await mkdir(outRoot, { recursive: true });
const manifest = [];
const only = process.argv.slice(2).filter((arg) => !arg.startsWith('--'));
for (const [name, sheets] of Object.entries(workbooks)) {
  if (only.length && !only.includes(name)) continue;
  const final = join(outRoot, name + '.xlsx');
  if (!existsSync(final)) {
    const raw = join(outRoot, name + '.raw.xlsx');
    await mkdir(dirname(raw), { recursive: true });
    await writeFile(raw, workbookZip(sheets, definedNames[name]));
    console.log(name + ': calculating in Excel');
    excelRecalc(raw.replace(/\//g, '\\'), final.replace(/\//g, '\\'));
    await rm(raw);
  }
  const bytes = await readFile(final);
  manifest.push({
    source: 'generated',
    engineFixture: false,
    path: name + '.xlsx',
    bytes: bytes.length,
    sha256: createHash('sha256').update(bytes).digest('hex'),
  });
  console.log(name + ': ' + (bytes.length / 1048576).toFixed(1) + ' MB');
}
if (!only.length) {
  await writeFile(
    join(outRoot, 'manifest.json'),
    JSON.stringify({ files: manifest }, null, 1) + '\n',
  );
}
