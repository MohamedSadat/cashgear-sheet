// Writes large workbooks for the speed targets into tests/workbooks/generated/speed, which git
// ignores, without Excel: their saved values are worked out here. Streams to disk, because a
// 100 MB file holds far more XML than a Node string can.
// Usage: node bench/generate-speed-books.mjs [--only=name]
import { createWriteStream, existsSync } from 'node:fs';
import { mkdir, rename, stat } from 'node:fs/promises';
import { join } from 'node:path';
import { constants, crc32, createDeflateRaw } from 'node:zlib';
import { root } from '../scripts/lib.mjs';

const outRoot = join(root, 'tests/workbooks/generated/speed');
const only = process.argv.find((arg) => arg.startsWith('--only='))?.split('=')[1];

const HEADER = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';
const MAIN = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
const RELS = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';

/** Writes a zip one entry at a time, each from a generator of text chunks. */
class ZipStream {
  constructor(path) {
    this.file = createWriteStream(path);
    this.offset = 0;
    this.central = [];
    this.count = 0;
  }

  write(buffer) {
    this.offset += buffer.length;
    return new Promise((resolve, reject) => {
      this.file.write(buffer, (error) => (error ? reject(error) : resolve()));
    });
  }

  async entry(name, chunks) {
    const nameBytes = Buffer.from(name);
    const start = this.offset;
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    // Sizes follow the data, in a data descriptor.
    local.writeUInt16LE(0x0808, 6);
    local.writeUInt16LE(8, 8);
    local.writeUInt16LE(nameBytes.length, 26);
    await this.write(local);
    await this.write(nameBytes);

    let crc = 0;
    let size = 0;
    let packed = 0;
    const deflate = createDeflateRaw({ level: 6, strategy: constants.Z_DEFAULT_STRATEGY });
    const finished = (async () => {
      for await (const chunk of deflate) {
        packed += chunk.length;
        await this.write(chunk);
      }
    })();
    for (const text of chunks) {
      const body = Buffer.from(text, 'utf8');
      crc = crc32(body, crc);
      size += body.length;
      if (!deflate.write(body)) await new Promise((resolve) => deflate.once('drain', resolve));
    }
    deflate.end();
    await finished;

    const descriptor = Buffer.alloc(16);
    descriptor.writeUInt32LE(0x08074b50, 0);
    descriptor.writeUInt32LE(crc, 4);
    descriptor.writeUInt32LE(packed, 8);
    descriptor.writeUInt32LE(size, 12);
    await this.write(descriptor);

    const header = Buffer.alloc(46);
    header.writeUInt32LE(0x02014b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(20, 6);
    header.writeUInt16LE(0x0808, 8);
    header.writeUInt16LE(8, 10);
    header.writeUInt32LE(crc, 16);
    header.writeUInt32LE(packed, 20);
    header.writeUInt32LE(size, 24);
    header.writeUInt16LE(nameBytes.length, 28);
    header.writeUInt32LE(start, 42);
    this.central.push(header, nameBytes);
    this.count++;
    return { packed, size };
  }

  async finish() {
    const directory = Buffer.concat(this.central);
    const start = this.offset;
    await this.write(directory);
    const end = Buffer.alloc(22);
    end.writeUInt32LE(0x06054b50, 0);
    end.writeUInt16LE(this.count, 8);
    end.writeUInt16LE(this.count, 10);
    end.writeUInt32LE(directory.length, 12);
    end.writeUInt32LE(start, 16);
    await this.write(end);
    await new Promise((resolve) => this.file.end(resolve));
  }
}

const STRINGS = Array.from({ length: 4096 }, (_, index) => {
  const words = ['North', 'South', 'East', 'West', 'Central', 'Harbour', 'Valley', 'Ridge'];
  return words[index % 8] + ' ' + words[(index >> 3) % 8] + ' ' + index.toString(36);
});

function* sharedStrings() {
  yield `${HEADER}<sst xmlns="${MAIN}" count="${STRINGS.length}" uniqueCount="${STRINGS.length}">`;
  for (const text of STRINGS) yield `<si><t>${text}</t></si>`;
  yield '</sst>';
}

/** A cheap repeatable generator, so values don't compress unrealistically well. */
function* sequence(seed) {
  let state = seed >>> 0 || 1;
  for (;;) {
    state ^= state << 13;
    state >>>= 0;
    state ^= state >>> 17;
    state ^= state << 5;
    state >>>= 0;
    yield state;
  }
}

/** Mostly values, as a typical export is: text, numbers, dates and a few more numbers. */
function* valuesSheet(rows) {
  const random = sequence(0x2f6e2b1);
  yield `${HEADER}<worksheet xmlns="${MAIN}" xmlns:r="${RELS}"><dimension ref="A1:H${rows}"/><sheetData>`;
  let batch = [];
  for (let r = 1; r <= rows; r++) {
    const a = random.next().value;
    const b = random.next().value;
    const c = random.next().value;
    batch.push(
      `<row r="${r}"><c r="A${r}" t="s"><v>${a % STRINGS.length}</v></c><c r="B${r}"><v>${r}</v></c><c r="C${r}"><v>${b % 100000}</v></c><c r="D${r}"><v>${(c % 10000000) / 100}</v></c><c r="E${r}" t="s"><v>${c % STRINGS.length}</v></c><c r="F${r}"><v>${40000 + (a % 8000)}</v></c><c r="G${r}"><v>${(b % 1000) / 1000}</v></c><c r="H${r}"><v>${a % 1000}</v></c></row>`,
    );
    if (batch.length === 4096) {
      yield batch.join('');
      batch = [];
    }
  }
  yield batch.join('');
  yield '</sheetData></worksheet>';
}

/** Three formulas to a row with their saved values, shared as Excel saves them. */
function* formulasSheet(rows) {
  yield `${HEADER}<worksheet xmlns="${MAIN}" xmlns:r="${RELS}"><dimension ref="A1:E${rows}"/><sheetData>`;
  let batch = [];
  for (let r = 1; r <= rows; r++) {
    const a = r % 1000;
    const b = (r * 7) % 13;
    const c = a * b;
    const d = c + 1;
    const e = d > 100 ? d - 1 : d;
    const f = (si, text) =>
      r === 1
        ? `<f t="shared" ref="${'CDE'[si]}1:${'CDE'[si]}${rows}" si="${si}">${text}</f>`
        : `<f t="shared" si="${si}"/>`;
    batch.push(
      `<row r="${r}"><c r="A${r}"><v>${a}</v></c><c r="B${r}"><v>${b}</v></c><c r="C${r}">${f(0, 'A1*B1')}<v>${c}</v></c><c r="D${r}">${f(1, 'C1+1')}<v>${d}</v></c><c r="E${r}">${f(2, 'IF(D1&gt;100,D1-1,D1)')}<v>${e}</v></c></row>`,
    );
    if (batch.length === 4096) {
      yield batch.join('');
      batch = [];
    }
  }
  yield batch.join('');
  yield '</sheetData></worksheet>';
}

/** A million formulas that all read one cell, G1, so that changing it recalculates all of them. */
function* recalculationSheet(rows) {
  yield `${HEADER}<worksheet xmlns="${MAIN}" xmlns:r="${RELS}"><dimension ref="A1:G${rows}"/><sheetData>`;
  let batch = [];
  for (let r = 1; r <= rows; r++) {
    const a = r % 1000;
    const f =
      r === 1 ? `<f t="shared" ref="B1:B${rows}" si="0">A1*$G$1+1</f>` : '<f t="shared" si="0"/>';
    const input = r === 1 ? '<c r="G1"><v>1</v></c>' : '';
    batch.push(
      `<row r="${r}"><c r="A${r}"><v>${a}</v></c><c r="B${r}">${f}<v>${a + 1}</v></c>${input}</row>`,
    );
    if (batch.length === 4096) {
      yield batch.join('');
      batch = [];
    }
  }
  yield batch.join('');
  yield '</sheetData></worksheet>';
}

async function writeBook(name, sheets) {
  const final = join(outRoot, name + '.xlsx');
  if (existsSync(final)) {
    console.log(
      name + ': already there, ' + ((await stat(final)).size / 1048576).toFixed(1) + ' MB',
    );
    return;
  }
  const partial = final + '.partial';
  const zip = new ZipStream(partial);
  await zip.entry('_rels/.rels', [
    `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
  ]);
  await zip.entry('xl/workbook.xml', [
    `${HEADER}<workbook xmlns="${MAIN}" xmlns:r="${RELS}"><sheets>` +
      sheets
        .map(
          (_, index) =>
            `<sheet name="Data ${index + 1}" sheetId="${index + 1}" r:id="rId${index + 1}"/>`,
        )
        .join('') +
      '</sheets></workbook>',
  ]);
  await zip.entry('xl/_rels/workbook.xml.rels', [
    `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
      sheets
        .map(
          (_, index) =>
            `<Relationship Id="rId${index + 1}" Type="${RELS}/worksheet" Target="worksheets/sheet${index + 1}.xml"/>`,
        )
        .join('') +
      `<Relationship Id="rStrings" Type="${RELS}/sharedStrings" Target="sharedStrings.xml"/></Relationships>`,
  ]);
  await zip.entry('xl/sharedStrings.xml', sharedStrings());
  let size = 0;
  for (const [index, sheet] of sheets.entries()) {
    size += (await zip.entry(`xl/worksheets/sheet${index + 1}.xml`, sheet)).size;
  }
  await zip.finish();
  await rename(partial, final);
  const bytes = (await stat(final)).size;
  console.log(
    name +
      ': ' +
      (bytes / 1048576).toFixed(1) +
      ' MB, ' +
      (size / 1048576).toFixed(0) +
      ' MB of sheet XML',
  );
}

await mkdir(outRoot, { recursive: true });
const books = {
  // The size target: 25 MB, about 3M cells, of each kind.
  'values-25mb': () => [valuesSheet(490_000)],
  'formulas-25mb': () => [formulasSheet(860_000)],
  // The upper target: 100 MB of mostly values.
  'values-100mb': () => [valuesSheet(1_048_576), valuesSheet(950_000)],
  // Past WASM's 4 GB once its graph is built, for checking the app says so.
  'formulas-past-memory': () => Array.from({ length: 5 }, () => formulasSheet(1_000_000)),
  // Scrolling.
  'rows-1m': () => [valuesSheet(1_048_576)],
  // Typing during a long recalculation.
  'recalc-1m': () => [recalculationSheet(1_000_000)],
};
for (const [name, sheet] of Object.entries(books)) {
  if (!only || only === name) await writeBook(name, sheet());
}
