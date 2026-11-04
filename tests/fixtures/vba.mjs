// Builds a `vbaProject.bin` from source text, so a macro fixture needs nothing binary checked in
// and every byte of its macro comes from here. The format is MS-CFB around MS-OVBA. A project
// version Excel doesn't recognise makes it ignore compiled code and compile the source itself,
// so no compiled code has to be made up.

const SECTOR = 512;
const MINI_SECTOR = 64;
const MINI_CUTOFF = 4096;
const FREE = 0xffffffff;
const END_OF_CHAIN = 0xfffffffe;
const FAT_SECTOR = 0xfffffffd;

/** MS-OVBA's compressed container, with every token a literal, which it allows. */
export function compressVba(bytes) {
  const chunks = [Buffer.from([0x01])];
  for (let at = 0; at < bytes.length || at === 0; at += 3640) {
    const piece = bytes.subarray(at, at + 3640);
    const tokens = [];
    for (let from = 0; from < piece.length; from += 8) {
      tokens.push(Buffer.from([0x00]), piece.subarray(from, from + 8));
    }
    const body = Buffer.concat(tokens);
    const header = Buffer.alloc(2);
    header.writeUInt16LE(0xb000 | (body.length - 1));
    chunks.push(header, body);
  }
  return Buffer.concat(chunks);
}

const record = (id, data = Buffer.alloc(0)) => {
  const head = Buffer.alloc(6);
  head.writeUInt16LE(id, 0);
  head.writeUInt32LE(data.length, 2);
  return Buffer.concat([head, data]);
};

const u16 = (value) => {
  const out = Buffer.alloc(2);
  out.writeUInt16LE(value);
  return out;
};

const u32 = (value) => {
  const out = Buffer.alloc(4);
  out.writeUInt32LE(value);
  return out;
};

const ascii = (text) => Buffer.from(text, 'latin1');
const utf16 = (text) => Buffer.from(text, 'utf16le');

const STDOLE =
  '*\\G{00020430-0000-0000-C000-000000000046}#2.0#0#C:\\Windows\\System32\\stdole2.tlb#OLE Automation';

/** A reference to a registered type library. */
function referenceRecords(name, libid) {
  const id = ascii(libid);
  const body = Buffer.concat([u32(id.length), id, u32(0), u16(0)]);
  return [record(0x0016, ascii(name)), record(0x003e, utf16(name)), record(0x000d, body)];
}

/** The `dir` stream's records: MS-OVBA 2.3.4.2. */
function dirStream(modules) {
  const parts = [
    record(0x0001, u32(1)), // Win32.
    record(0x0002, u32(0x0409)),
    record(0x0014, u32(0x0409)),
    record(0x0003, u16(1252)),
    record(0x0004, ascii('VBAProject')),
    record(0x0005),
    record(0x0040),
    record(0x0006),
    record(0x003d),
    record(0x0007, u32(0)),
    record(0x0008, u32(0)),
    // A version record has its own shape: a reserved size, then major and minor.
    Buffer.concat([u16(0x0009), u32(4), u32(1), u16(0)]),
    record(0x000c),
    record(0x003c),
    ...referenceRecords('stdole', STDOLE),
    record(0x000f, u16(modules.length)),
    record(0x0013, u16(0xffff)),
  ];
  for (const module of modules) {
    parts.push(
      record(0x0019, ascii(module.name)),
      record(0x0047, utf16(module.name)),
      record(0x001a, ascii(module.name)),
      record(0x0032, utf16(module.name)),
      record(0x001c),
      record(0x0048),
      record(0x0031, u32(0)),
      record(0x001e, u32(0)),
      record(0x002c, u16(0xffff)),
      record(module.document ? 0x0022 : 0x0021),
      record(0x002b),
    );
  }
  parts.push(record(0x0010));
  return Buffer.concat(parts);
}

const PROJECT_ID = '{5B1E2F10-3C4D-4A5B-8C6D-7E8F90A1B2C3}';

/** MS-OVBA 2.4.3's obfuscation, which Excel checks the shape of. `seed` is any byte. */
function obfuscate(data, seed) {
  const projectKey = [...ascii(PROJECT_ID)].reduce((sum, byte) => (sum + byte) & 0xff, 0);
  const out = [seed, seed ^ 2, seed ^ projectKey];
  let plainBefore = projectKey;
  let cipherBefore = seed ^ projectKey;
  let cipherTwoBack = seed ^ 2;
  const push = (byte) => {
    const cipher = byte ^ ((cipherTwoBack + plainBefore) & 0xff);
    out.push(cipher);
    cipherTwoBack = cipherBefore;
    cipherBefore = cipher;
    plainBefore = byte;
  };
  for (let at = 0; at < (seed & 6) >> 1; at++) push(0x07);
  for (const byte of u32(data.length)) push(byte);
  for (const byte of data) push(byte);
  return Buffer.from(out).toString('hex').toUpperCase();
}

function projectStream(modules) {
  const lines = [`ID="${PROJECT_ID}"`];
  for (const module of modules) {
    lines.push(module.document ? `Document=${module.name}/&H00000000` : `Module=${module.name}`);
  }
  lines.push(
    'Name="VBAProject"',
    'HelpContextID="0"',
    'VersionCompatible32="393222000"',
    // Not protected, no password, and visible.
    `CMG="${obfuscate(Buffer.alloc(4), 0x07)}"`,
    `DPB="${obfuscate(Buffer.alloc(1), 0x0e)}"`,
    `GC="${obfuscate(Buffer.from([0xff]), 0x15)}"`,
    '',
    '[Host Extender Info]',
    '&H00000001={3832D640-CF90-11CF-8E43-00A0C911005A};VBE;&H00000000',
    '',
    '[Workspace]',
  );
  for (const module of modules) lines.push(`${module.name}=0, 0, 0, 0, C`);
  return ascii(lines.join('\r\n') + '\r\n');
}

/** The `PROJECTwm` stream: each module's name, in both encodings. */
function projectNames(modules) {
  const parts = [];
  for (const module of modules) {
    parts.push(ascii(module.name), Buffer.from([0]), utf16(module.name), Buffer.from([0, 0]));
  }
  parts.push(Buffer.from([0, 0]));
  return Buffer.concat(parts);
}

/** CFB orders a storage's children by name length, then by name in capitals. */
const compareNames = (a, b) => {
  const [first, second] = [a.name.toUpperCase(), b.name.toUpperCase()];
  // By code unit, not by locale, which would put `_` ahead of the letters and hide the stream.
  return a.name.length - b.name.length || (first < second ? -1 : first > second ? 1 : 0);
};

/** Writes a compound file holding a tree of `{ name, children }` and `{ name, data }`. */
function compoundFile(root) {
  const entries = [];
  const add = (node, type) => {
    const entry = { ...node, type, left: FREE, right: FREE, child: FREE };
    entries.push(entry);
    if (node.children) {
      const sorted = [...node.children].sort(compareNames);
      // A balanced tree of black nodes is a valid red-black tree.
      const place = (low, high) => {
        if (low > high) return FREE;
        const middle = (low + high) >> 1;
        const item = sorted[middle];
        const id = add(item, item.children ? 1 : 2);
        const added = entries[id];
        added.left = place(low, middle - 1);
        added.right = place(middle + 1, high);
        return id;
      };
      entry.child = place(0, sorted.length - 1);
    }
    return entries.indexOf(entry);
  };
  add(root, 5);

  // Small streams live in the mini stream, one chain of 64-byte sectors each.
  const miniParts = [];
  const miniFat = [];
  const bigStreams = [];
  for (const entry of entries) {
    if (entry.type !== 2) continue;
    if (entry.data.length < MINI_CUTOFF) {
      const count = Math.ceil(entry.data.length / MINI_SECTOR);
      entry.start = count ? miniFat.length : END_OF_CHAIN;
      for (let at = 0; at < count; at++) {
        miniFat.push(at === count - 1 ? END_OF_CHAIN : miniFat.length + 1);
      }
      const padded = Buffer.alloc(count * MINI_SECTOR);
      entry.data.copy(padded);
      miniParts.push(padded);
    } else {
      bigStreams.push(entry);
    }
  }
  const miniStream = Buffer.concat(miniParts);

  const sectors = [];
  const fat = [];
  const place = (data) => {
    const count = Math.ceil(data.length / SECTOR);
    if (count === 0) return END_OF_CHAIN;
    const first = sectors.length;
    for (let at = 0; at < count; at++) {
      const sector = Buffer.alloc(SECTOR);
      data.copy(sector, 0, at * SECTOR, (at + 1) * SECTOR);
      sectors.push(sector);
      fat.push(at === count - 1 ? END_OF_CHAIN : first + at + 1);
    }
    return first;
  };
  for (const entry of bigStreams) entry.start = place(entry.data);
  entries[0].start = place(miniStream);
  entries[0].size = miniStream.length;

  const miniFatBytes = Buffer.alloc(Math.ceil((miniFat.length * 4) / SECTOR) * SECTOR, 0xff);
  miniFat.forEach((value, at) => miniFatBytes.writeUInt32LE(value, at * 4));
  const miniFatStart = place(miniFatBytes);
  const miniFatSectors = miniFatBytes.length / SECTOR;

  const directory = Buffer.alloc(Math.ceil((entries.length * 128) / SECTOR) * SECTOR);
  entries.forEach((entry, at) => {
    const base = at * 128;
    const name = utf16(entry.name);
    name.copy(directory, base);
    directory.writeUInt16LE(name.length + 2, base + 64);
    directory[base + 66] = entry.type;
    directory[base + 67] = 1;
    directory.writeUInt32LE(entry.left, base + 68);
    directory.writeUInt32LE(entry.right, base + 72);
    directory.writeUInt32LE(entry.child, base + 76);
    directory.writeUInt32LE(entry.start ?? 0, base + 116);
    directory.writeUInt32LE(entry.type === 5 ? entry.size : (entry.data?.length ?? 0), base + 120);
  });
  // The unused slots of a directory sector are marked empty.
  for (let at = entries.length; at * 128 < directory.length; at++) {
    const base = at * 128;
    directory.writeUInt32LE(FREE, base + 68);
    directory.writeUInt32LE(FREE, base + 72);
    directory.writeUInt32LE(FREE, base + 76);
  }
  const directoryStart = place(directory);

  // The FAT describes itself, which takes some working out.
  let fatSectors = 1;
  while (Math.ceil(((sectors.length + fatSectors) * 4) / SECTOR) > fatSectors) fatSectors++;
  const fatStart = sectors.length;
  for (let at = 0; at < fatSectors; at++) fat.push(FAT_SECTOR);
  const fatBytes = Buffer.alloc(fatSectors * SECTOR, 0xff);
  fat.forEach((value, at) => fatBytes.writeUInt32LE(value, at * 4));

  const header = Buffer.alloc(SECTOR, 0);
  Buffer.from([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]).copy(header);
  header.writeUInt16LE(0x003e, 24);
  header.writeUInt16LE(3, 26);
  header.writeUInt16LE(0xfffe, 28);
  header.writeUInt16LE(9, 30);
  header.writeUInt16LE(6, 32);
  header.writeUInt32LE(fatSectors, 44);
  header.writeUInt32LE(directoryStart, 48);
  header.writeUInt32LE(MINI_CUTOFF, 56);
  header.writeUInt32LE(miniFatStart, 60);
  header.writeUInt32LE(miniFatSectors, 64);
  header.writeUInt32LE(END_OF_CHAIN, 68);
  header.writeUInt32LE(0, 72);
  for (let at = 0; at < 109; at++) {
    header.writeUInt32LE(at < fatSectors ? fatStart + at : FREE, 76 + at * 4);
  }
  return Buffer.concat([header, ...sectors, fatBytes]);
}

const DOCUMENT_ATTRIBUTES = (name, base) =>
  [
    `Attribute VB_Name = "${name}"`,
    `Attribute VB_Base = "${base}"`,
    'Attribute VB_GlobalNameSpace = False',
    'Attribute VB_Creatable = False',
    'Attribute VB_PredeclaredId = True',
    'Attribute VB_Exposed = True',
    'Attribute VB_TemplateDerived = False',
    'Attribute VB_Customizable = True',
    '',
  ].join('\r\n');

/** A project with `ThisWorkbook`, a document module for one sheet, and `Module1` with this code. */
export function vbaProject(moduleCode, sheetCodeName = 'Sheet1') {
  const modules = [
    {
      name: 'ThisWorkbook',
      document: true,
      source: DOCUMENT_ATTRIBUTES('ThisWorkbook', '0{00020819-0000-0000-C000-000000000046}'),
    },
    {
      name: sheetCodeName,
      document: true,
      source: DOCUMENT_ATTRIBUTES(sheetCodeName, '0{00020820-0000-0000-C000-000000000046}'),
    },
    {
      name: 'Module1',
      document: false,
      source: `Attribute VB_Name = "Module1"\r\n${moduleCode.replace(/\r?\n/g, '\r\n')}\r\n`,
    },
  ];
  // 0xFFFF as the version: "this cache isn't for you", so Excel compiles the source.
  const versionMark = Buffer.from([0xcc, 0x61, 0xff, 0xff, 0x00, 0x00, 0x00]);
  return compoundFile({
    name: 'Root Entry',
    children: [
      { name: 'PROJECT', data: projectStream(modules) },
      { name: 'PROJECTwm', data: projectNames(modules) },
      {
        name: 'VBA',
        children: [
          { name: '_VBA_PROJECT', data: versionMark },
          { name: 'dir', data: compressVba(dirStream(modules)) },
          ...modules.map((module) => ({
            name: module.name,
            data: compressVba(ascii(module.source)),
          })),
        ],
      },
    ],
  });
}
