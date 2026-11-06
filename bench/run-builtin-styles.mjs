// Writes Excel's built-in table, pivot and cell styles to crates/cgs-model/data, from Excel 2016 over
// COM. Built-in styles are only named in a file, never defined there, so this is the only source.
//
// Table styles come from COM, as a new workbook reports them. Excel's own copy of each one (`Duplicate`)
// is saved as a custom style with its formats spelled out, and every element has to agree with it.
// Cell styles come from the styles.xml Excel saves once each is in use, and COM has to agree with that.
// A probe style with one distinct fill per element checks which COM element type is which OOXML one.
//
// Colours keep their theme slot as styles.xml numbers it (0 lt1, 1 dk1, 2 lt2, 3 dk2, 4 to 9 accent1
// to 6, 10 hlink, 11 folHlink), which is COM's ThemeColor less one, with the tint, and also the RGB
// Excel resolved them to with the default Office theme.
//
// Excel saves cell style names in its UI language, so English names come from builtinId, as ECMA-376
// part 1, 18.8.7 lists them. Only a new workbook is used, and the one file Excel saves goes to a
// temporary folder that is always deleted.
// Usage: node bench/run-builtin-styles.mjs
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { inflateRawSync } from 'node:zlib';
import * as prettier from 'prettier';
import { root } from '../scripts/lib.mjs';

const script = readFileSync(join(root, 'bench/excel-builtin-styles.ps1'), 'utf8');
const dataFolder = join(root, 'crates/cgs-model/data');
const quoted = (text) => "'" + text.replace(/'/g, "''") + "'";

/** XlTableStyleElementType 0 to 12, by their tableStyleElement@type names. */
const ELEMENT_TYPES = [
  'wholeTable',
  'headerRow',
  'totalRow',
  'firstColumn',
  'lastColumn',
  'firstRowStripe',
  'secondRowStripe',
  'firstColumnStripe',
  'secondColumnStripe',
  'firstHeaderCell',
  'lastHeaderCell',
  'firstTotalCell',
  'lastTotalCell',
];
const STRIPES = new Set([5, 6, 7, 8]);

/** XlTableStyleElementType 13 to 27, the pivot styles' own, after the table ones. */
const PIVOT_TYPES = [
  ...ELEMENT_TYPES,
  'firstSubtotalColumn',
  'secondSubtotalColumn',
  'thirdSubtotalColumn',
  'firstSubtotalRow',
  'secondSubtotalRow',
  'thirdSubtotalRow',
  'blankRow',
  'firstColumnSubheading',
  'secondColumnSubheading',
  'thirdColumnSubheading',
  'firstRowSubheading',
  'secondRowSubheading',
  'thirdRowSubheading',
  'pageFieldLabels',
  'pageFieldValues',
];

/** cellStyle@builtinId to its English name. 1 and 2 are the outline levels, named per level. */
const BUILTIN_NAMES = {
  0: 'Normal',
  3: 'Comma',
  4: 'Currency',
  5: 'Percent',
  6: 'Comma [0]',
  7: 'Currency [0]',
  8: 'Hyperlink',
  9: 'Followed Hyperlink',
  10: 'Note',
  11: 'Warning Text',
  12: 'Emphasis 1',
  13: 'Emphasis 2',
  14: 'Emphasis 3',
  15: 'Title',
  16: 'Heading 1',
  17: 'Heading 2',
  18: 'Heading 3',
  19: 'Heading 4',
  20: 'Input',
  21: 'Output',
  22: 'Calculation',
  23: 'Check Cell',
  24: 'Linked Cell',
  25: 'Total',
  26: 'Good',
  27: 'Bad',
  28: 'Neutral',
  53: 'Explanatory Text',
};
for (let accent = 1; accent <= 6; accent++) {
  const base = 29 + (accent - 1) * 4;
  BUILTIN_NAMES[base] = 'Accent' + accent;
  BUILTIN_NAMES[base + 1] = '20% - Accent' + accent;
  BUILTIN_NAMES[base + 2] = '40% - Accent' + accent;
  BUILTIN_NAMES[base + 3] = '60% - Accent' + accent;
}

/** XlPattern to patternFill@patternType. */
const PATTERNS = {
  1: 'solid',
  [-4142]: 'none',
  [-4125]: 'mediumGray',
  [-4126]: 'darkGray',
  [-4124]: 'lightGray',
  [-4128]: 'darkHorizontal',
  [-4166]: 'darkVertical',
  [-4121]: 'darkDown',
  [-4162]: 'darkUp',
  9: 'darkGrid',
  10: 'darkTrellis',
  11: 'lightHorizontal',
  12: 'lightVertical',
  13: 'lightDown',
  14: 'lightUp',
  15: 'lightGrid',
  16: 'lightTrellis',
  17: 'gray125',
  18: 'gray0625',
};

/** XlLineStyle and XlBorderWeight to a border@style. */
function lineStyle(style, weight) {
  const medium = weight === -4138 || weight === 4;
  switch (style) {
    case 1:
      return { 1: 'hair', 2: 'thin', [-4138]: 'medium', 4: 'thick' }[weight];
    case -4115:
      return medium ? 'mediumDashed' : 'dashed';
    case 4:
      return medium ? 'mediumDashDot' : 'dashDot';
    case 5:
      return medium ? 'mediumDashDotDot' : 'dashDotDot';
    case -4118:
      return 'dotted';
    case -4119:
      return 'double';
    case 13:
      return 'slantDashDot';
  }
  throw new Error(`unknown line style ${style}/${weight}`);
}

/** COM's BGR colour as #RRGGBB. */
const rgb = (bgr) =>
  '#' +
  [bgr & 0xff, (bgr >> 8) & 0xff, (bgr >> 16) & 0xff]
    .map((part) => part.toString(16).padStart(2, '0'))
    .join('')
    .toUpperCase();

/** A COM colour, theme-relative when Excel holds it that way. */
function comColour(raw) {
  const out = {};
  if (Number.isInteger(raw.themeColor) && raw.themeColor >= 1 && raw.themeColor <= 12) {
    out.theme = raw.themeColor - 1;
    if (raw.tint) out.tint = raw.tint;
  }
  out.rgb = rgb(raw.color);
  return out;
}

// COM's border indices to dxf and cell border sides.
const BORDER_SIDES = {
  7: 'left',
  8: 'top',
  9: 'bottom',
  10: 'right',
  11: 'vertical',
  12: 'horizontal',
  5: 'diagonalDown',
  6: 'diagonalUp',
};

/**
 * A table style element as COM reports it. Excel leaves an unset property null: a font with a null
 * ColorIndex has no colour and a null Bold isn't bold or not bold. An interior with a null Pattern
 * has no fill, and a border whose LineStyle is xlNone (-4142) has no line.
 */
function comElement(raw) {
  const out = {};
  if (STRIPES.has(raw.type)) out.stripeSize = raw.stripeSize;
  const font = {};
  for (const key of ['bold', 'italic', 'strikethrough']) {
    if (raw.font[key] !== null) font[key] = raw.font[key];
  }
  if (raw.font.underline !== null) font.underline = raw.font.underline;
  if (raw.font.colorIndex !== null) font.color = comColour(raw.font);
  if (Object.keys(font).length) out.font = font;
  const inside = raw.interior;
  if (inside.pattern !== null) {
    const pattern = PATTERNS[inside.pattern];
    if (!pattern) throw new Error('unsupported table style pattern ' + inside.pattern);
    out.fill = { pattern };
    if (inside.colorIndex !== null && inside.colorIndex !== -4142) {
      out.fill.color = comColour(inside);
    }
    if (pattern !== 'solid' && inside.patternColor.colorIndex !== null) {
      out.fill.patternColor = comColour(inside.patternColor);
    }
  }
  const borders = {};
  for (const [index, side] of Object.entries(BORDER_SIDES)) {
    const line = raw.borders[index];
    if (!line || line.lineStyle === null || line.lineStyle === -4142) continue;
    borders[side] = { style: lineStyle(line.lineStyle, line.weight), color: comColour(line) };
  }
  if (Object.keys(borders).length) out.border = borders;
  return out;
}

// A small reader for styles.xml, which is flat enough for regular expressions.
const unescape = (text) =>
  text
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&');
function attributes(tag) {
  const out = {};
  for (const [, key, value] of tag.matchAll(/([\w:]+)="([^"]*)"/g)) out[key] = unescape(value);
  return out;
}
/** The children named `name` directly in `xml`, as [attributes, inner text]. */
function elements(xml, name) {
  const out = [];
  const pattern = new RegExp(`<${name}\\b([^>]*?)(/>|>([\\s\\S]*?)</${name}>)`, 'g');
  for (const match of xml.matchAll(pattern)) out.push([attributes(match[1]), match[3] ?? '']);
  return out;
}
const section = (xml, name) => elements(xml, name)[0]?.[1] ?? '';
const flag = (xml, name) => {
  const found = elements(xml, name)[0];
  if (!found) return undefined;
  return found[0].val === undefined ? true : found[0].val !== '0' && found[0].val !== 'false';
};

/** An XML colour, theme-relative when it is, with the RGB COM resolved it to. */
function xmlColour(attrs, resolved) {
  if (!attrs) return undefined;
  const out = {};
  if (attrs.theme !== undefined) {
    out.theme = Number(attrs.theme);
    if (attrs.tint !== undefined && Number(attrs.tint) !== 0) out.tint = Number(attrs.tint);
  }
  if (attrs.indexed !== undefined) out.indexed = Number(attrs.indexed);
  if (attrs.auto !== undefined) out.auto = true;
  if (attrs.rgb !== undefined) out.rgb = '#' + attrs.rgb.slice(-6).toUpperCase();
  else if (resolved !== undefined && out.indexed === undefined && !out.auto) out.rgb = resolved;
  return out;
}
const colourOf = (xml, name) => elements(xml, name)[0]?.[0];

function xmlFont(xml) {
  const out = {};
  for (const [key, tag] of [
    ['bold', 'b'],
    ['italic', 'i'],
    ['strikethrough', 'strike'],
  ]) {
    const value = flag(xml, tag);
    if (value !== undefined) out[key] = value;
  }
  const underline = elements(xml, 'u')[0];
  if (underline) out.underline = underline[0].val ?? 'single';
  return out;
}

function xmlBorder(xml) {
  const out = {};
  for (const side of ['left', 'right', 'top', 'bottom', 'diagonal', 'vertical', 'horizontal']) {
    const found = elements(xml, side)[0];
    if (!found || !found[0].style) continue;
    out[side] = { style: found[0].style, color: colourOf(found[1], 'color') };
  }
  return out;
}

/** Excel's own dxf for a copied element, in the shape comElement gives. */
function dxfElement(xml) {
  const out = {};
  const font = section(xml, 'font');
  if (font) {
    const parsed = xmlFont(font);
    const colour = colourOf(font, 'color');
    if (colour) parsed.color = xmlColour(colour);
    if (Object.keys(parsed).length) out.font = parsed;
  }
  const fill = elements(xml, 'patternFill')[0];
  if (fill) {
    // In a dxf, a solid fill's colour is its bgColor.
    out.fill = { pattern: fill[0].patternType ?? 'solid' };
    const colour = colourOf(fill[1], 'bgColor');
    if (colour) out.fill.color = xmlColour(colour);
  }
  const border = xmlBorder(section(xml, 'border'));
  if (Object.keys(border).length) out.border = border;
  return out;
}

/** Differences between a COM element and Excel's dxf for it, ignoring RGB the dxf leaves out. */
function differences(com, dxf, at, out) {
  const close = (a, b) => (a ?? 0) === (b ?? 0) || Math.abs((a ?? 0) - (b ?? 0)) < 1e-12;
  const sameColour = (a, b) =>
    a &&
    b &&
    a.theme === b.theme &&
    close(a.tint, b.tint) &&
    (a.theme !== undefined || a.rgb === b.rgb);
  for (const key of ['bold', 'italic', 'strikethrough']) {
    if (com.font?.[key] !== dxf.font?.[key]) out.push(`${at}: font ${key}`);
  }
  if (
    !!com.font?.color !== !!dxf.font?.color ||
    (com.font?.color && !sameColour(com.font.color, dxf.font.color))
  ) {
    out.push(`${at}: font colour`);
  }
  if (
    com.fill?.pattern !== dxf.fill?.pattern ||
    (com.fill && !sameColour(com.fill.color, dxf.fill.color))
  ) {
    out.push(`${at}: fill`);
  }
  for (const side of new Set([
    ...Object.keys(com.border ?? {}),
    ...Object.keys(dxf.border ?? {}),
  ])) {
    const a = com.border?.[side];
    const b = dxf.border?.[side];
    if (a?.style !== b?.style) out.push(`${at}: ${side} border`);
    else if (!sameColour(a.color, xmlColour(b.color))) out.push(`${at}: ${side} border colour`);
  }
}

/** A zip's entries by its central directory, as Excel's zips leave sizes out of local headers. */
function readZip(bytes) {
  let end = bytes.length - 22;
  while (end >= 0 && bytes.readUInt32LE(end) !== 0x06054b50) end--;
  const count = bytes.readUInt16LE(end + 10);
  let at = bytes.readUInt32LE(end + 16);
  const out = new Map();
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
    out.set(name, method === 8 ? inflateRawSync(body) : body);
    at += 46 + nameLength + extra + comment;
  }
  return out;
}

function runExcel(scratch) {
  const result = join(scratch, 'dump.json');
  const saved = join(scratch, 'saved');
  mkdirSync(saved);
  const args = `-Out ${quoted(result)} -Scratch ${quoted(saved)}`;
  const run = spawnSync(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-Command', `& {${script}} ${args}`],
    { encoding: 'utf8', timeout: 900000, windowsHide: true },
  );
  if (run.status !== 0) throw new Error('Excel script failed: ' + run.stderr.slice(0, 800));
  const dump = JSON.parse(readFileSync(result, 'utf8').trimStart());
  const styles = readZip(readFileSync(dump.saved)).get('xl/styles.xml').toString('utf8');
  return { dump, styles };
}

function tableStyles(dump, styles, problems, pivot = false) {
  const TYPES = pivot ? PIVOT_TYPES : ELEMENT_TYPES;
  const probeName = pivot ? 'cgs pivot probe' : 'cgs probe';
  const dxfs = elements(section(styles, 'dxfs'), 'dxf').map(([, inner]) => inner);
  const saved = new Map(
    elements(section(styles, 'tableStyles'), 'tableStyle').map(([attrs, inner]) => [
      attrs.name,
      elements(inner, 'tableStyleElement').map(([element]) => element),
    ]),
  );

  // The probe's element N is filled with RGB(N + 1, 0, 0), which says which type Excel saved it as.
  for (const element of saved.get(probeName) ?? []) {
    const fill = colourOf(dxfs[element.dxfId], 'bgColor');
    const comType = parseInt(fill.rgb.slice(2, 4), 16) - 1;
    if (TYPES[comType] !== element.type) {
      problems.push(`element type ${comType} saved as ${element.type}, not ${TYPES[comType]}`);
    }
  }
  if ((saved.get(probeName) ?? []).length !== TYPES.length) {
    problems.push(`the ${probeName} style lost elements`);
  }
  for (const name of saved.keys()) {
    if (!name.startsWith('cgs '))
      problems.push(`built-in table style ${name} is defined in the file`);
  }

  const out = [];
  for (const style of pivot ? dump.pivotStyles : dump.tableStyles) {
    const elementsOut = {};
    for (const raw of [...style.elements].sort((a, b) => a.type - b.type)) {
      elementsOut[TYPES[raw.type]] = comElement(raw);
    }
    const copy = saved.get('cgs copy ' + style.name);
    if (!copy) problems.push(`no saved copy of ${style.name}`);
    for (const element of copy ?? []) {
      const com = elementsOut[element.type];
      if (!com) {
        problems.push(`${style.name} ${element.type}: only in the saved copy`);
        continue;
      }
      differences(com, dxfElement(dxfs[element.dxfId]), `${style.name} ${element.type}`, problems);
    }
    for (const type of Object.keys(elementsOut)) {
      if (copy && !copy.some((element) => element.type === type)) {
        problems.push(`${style.name} ${type}: missing from the saved copy`);
      }
    }
    out.push(
      pivot
        ? { name: style.name, elements: elementsOut }
        : {
            name: style.name,
            showAsAvailableTableStyle: style.showAsAvailableTableStyle,
            showAsAvailablePivotTableStyle: style.showAsAvailablePivotTableStyle,
            elements: elementsOut,
          },
    );
  }
  return out;
}

/** XlHAlign and XlVAlign to alignment@horizontal and @vertical, for the cross-check. */
const HORIZONTAL = {
  1: 'general',
  [-4131]: 'left',
  [-4108]: 'center',
  [-4152]: 'right',
  5: 'fill',
  [-4130]: 'justify',
  7: 'centerContinuous',
  [-4117]: 'distributed',
};
const VERTICAL = {
  [-4160]: 'top',
  [-4108]: 'center',
  [-4107]: 'bottom',
  [-4130]: 'justify',
  [-4117]: 'distributed',
};

/** Built-in number formats a cell style can use without a numFmt in the file. */
const BUILTIN_FORMATS = { 0: 'General', 9: '0%' };

function cellStyles(dump, styles, problems) {
  const fonts = elements(section(styles, 'fonts'), 'font').map(([, inner]) => inner);
  const fills = elements(section(styles, 'fills'), 'fill').map(([, inner]) => inner);
  const borders = elements(section(styles, 'borders'), 'border');
  const xfs = elements(section(styles, 'cellStyleXfs'), 'xf');
  const formats = new Map(
    elements(section(styles, 'numFmts'), 'numFmt').map(([attrs]) => [
      Number(attrs.numFmtId),
      attrs.formatCode,
    ]),
  );
  const byLocalName = new Map(dump.cellStyles.map((style) => [style.nameLocal, style]));
  const byName = new Map(dump.cellStyles.map((style) => [style.name, style]));

  const out = [];
  for (const [attrs] of elements(section(styles, 'cellStyles'), 'cellStyle')) {
    const builtinId = Number(attrs.builtinId);
    const name = BUILTIN_NAMES[builtinId];
    const com = byName.get(attrs.name) ?? byLocalName.get(attrs.name);
    if (!name || !com) {
      problems.push(
        `cell style ${attrs.name} (${attrs.builtinId}) has no English name or COM entry`,
      );
      continue;
    }
    if (/^[\x20-\x7e]+$/.test(com.name) && com.name !== name) {
      problems.push(`builtinId ${builtinId} is ${com.name} over COM, not ${name}`);
    }
    const [xf, xfInner] = xfs[Number(attrs.xfId)];
    const include = {
      number: xf.applyNumberFormat !== '0',
      font: xf.applyFont !== '0',
      alignment: xf.applyAlignment !== '0',
      border: xf.applyBorder !== '0',
      fill: xf.applyFill !== '0',
      protection: xf.applyProtection !== '0',
    };
    const comInclude = { ...com.include, fill: com.include.patterns };
    delete comInclude.patterns;
    for (const key of Object.keys(include)) {
      if (include[key] !== comInclude[key]) problems.push(`${name}: include ${key}`);
    }

    const numFmtId = Number(xf.numFmtId ?? 0);
    const formatCode = formats.get(numFmtId) ?? BUILTIN_FORMATS[numFmtId];
    if (formatCode === undefined) problems.push(`${name}: unknown built-in format ${numFmtId}`);
    // The file escapes and quotes what COM leaves bare, so compare without backslashes and quotes.
    if (formatCode?.replace(/[\\"]/g, '') !== com.numberFormat.replace(/[\\"]/g, '')) {
      problems.push(`${name}: format ${formatCode} is ${com.numberFormat} over COM`);
    }

    const fontXml = fonts[Number(xf.fontId ?? 0)];
    const font = {
      name: elements(fontXml, 'name')[0]?.[0].val,
      size: Number(elements(fontXml, 'sz')[0]?.[0].val),
      ...xmlFont(fontXml),
    };
    const colour = xmlColour(colourOf(fontXml, 'color'), rgb(com.font.color));
    if (colour) font.color = colour;
    const family = elements(fontXml, 'family')[0];
    if (family) font.family = Number(family[0].val);
    const scheme = elements(fontXml, 'scheme')[0];
    if (scheme) font.scheme = scheme[0].val;
    if (font.name !== com.font.name || font.size !== com.font.size) problems.push(`${name}: font`);
    if (!!font.bold !== com.font.bold || !!font.italic !== com.font.italic)
      problems.push(`${name}: font weight`);
    if ((font.scheme ?? 'none') !== ['none', 'major', 'minor'][com.font.themeFont]) {
      problems.push(`${name}: theme font`);
    }
    if ((font.color?.theme ?? -1) !== (com.font.themeColor ?? 0) - 1)
      problems.push(`${name}: font theme`);

    const fillXml = fills[Number(xf.fillId ?? 0)];
    const pattern = elements(fillXml, 'patternFill')[0];
    const fill = { pattern: pattern?.[0].patternType ?? 'none' };
    if (pattern) {
      const fg = xmlColour(colourOf(pattern[1], 'fgColor'), rgb(com.interior.color));
      if (fg) fill.fgColor = fg;
      const bg = xmlColour(colourOf(pattern[1], 'bgColor'));
      if (bg) fill.bgColor = bg;
    }
    if (fill.pattern !== (PATTERNS[com.interior.pattern] ?? 'none'))
      problems.push(`${name}: fill pattern`);
    if (fill.fgColor && fill.fgColor.rgb !== rgb(com.interior.color))
      problems.push(`${name}: fill colour`);

    const [borderAttrs, borderXml] = borders[Number(xf.borderId ?? 0)];
    const border = xmlBorder(borderXml);
    const comSides = { left: -4131, top: -4160, bottom: -4107, right: -4152 };
    for (const [side, index] of Object.entries(comSides)) {
      const line = com.borders[index];
      const comStyle =
        line.lineStyle === null || line.lineStyle === -4142
          ? undefined
          : lineStyle(line.lineStyle, line.weight);
      if (comStyle !== border[side]?.style) problems.push(`${name}: ${side} border`);
      if (border[side]) border[side].color = xmlColour(border[side].color, rgb(line.color));
    }
    if (border.diagonal) {
      border.diagonal.color = xmlColour(border.diagonal.color);
      border.diagonalUp = borderAttrs.diagonalUp === '1';
      border.diagonalDown = borderAttrs.diagonalDown === '1';
    }

    const entry = { name, builtinId, include, numFmtId, formatCode, font, fill, border };
    const alignment = elements(xfInner, 'alignment')[0]?.[0];
    if (alignment) entry.alignment = alignment;
    if (
      (alignment?.horizontal ?? 'general') !== HORIZONTAL[com.alignment.horizontalAlignment] ||
      (alignment?.vertical ?? 'bottom') !== VERTICAL[com.alignment.verticalAlignment] ||
      (alignment?.wrapText === '1') !== com.alignment.wrapText
    ) {
      problems.push(`${name}: alignment`);
    }
    const protection = elements(xfInner, 'protection')[0]?.[0];
    if (protection) entry.protection = protection;
    if ((protection?.locked ?? '1') !== (com.locked ? '1' : '0'))
      problems.push(`${name}: protection`);
    out.push(entry);
  }
  if (out.length !== dump.cellStyles.length) {
    problems.push(`${dump.cellStyles.length} styles over COM, ${out.length} in the file`);
  }
  return out.sort((a, b) => a.builtinId - b.builtinId);
}

async function write(name, value) {
  const path = join(dataFolder, name);
  const options = (await prettier.resolveConfig(path)) ?? {};
  const text = await prettier.format(JSON.stringify(value, null, 2), {
    ...options,
    filepath: path,
  });
  writeFileSync(path, text.replace(/\r\n/g, '\n'));
  return path;
}

const scratch = mkdtempSync(join(tmpdir(), 'cgs-builtin-styles-'));
try {
  const { dump, styles } = runExcel(scratch);
  const problems = [];
  const tables = tableStyles(dump, styles, problems);
  const pivots = tableStyles(dump, styles, problems, true);
  if (pivots.length !== 85 && pivots.length !== 84) problems.push(`${pivots.length} pivot styles`);
  const cells = cellStyles(dump, styles, problems);
  const savedDefault = attributes(styles.match(/<tableStyles\b[^>]*>/)?.[0] ?? '');
  if (savedDefault.defaultTableStyle !== dump.defaultTableStyle)
    problems.push('default table style');
  if (tables.length !== 60) problems.push(`${tables.length} table styles, not 60`);
  if (problems.length) {
    console.error(problems.join('\n'));
    process.exitCode = 1;
  } else {
    const source = `Excel ${dump.version} over COM, by bench/run-builtin-styles.mjs. Colours keep styles.xml's theme index (0 lt1, 1 dk1, 2 lt2, 3 dk2, 4-9 accent1-6, 10 hlink, 11 folHlink) and tint, and rgb is what Excel resolved them to under the default Office theme.`;
    mkdirSync(dataFolder, { recursive: true });
    const written = [
      await write('table-styles.json', {
        source,
        defaultTableStyle: dump.defaultTableStyle,
        defaultPivotStyle: dump.defaultPivotTableStyle,
        styles: tables,
      }),
      await write('cell-styles.json', { source, styles: cells }),
      await write('pivot-styles.json', {
        source,
        defaultPivotStyle: dump.defaultPivotTableStyle,
        styles: pivots,
      }),
    ];
    console.log(
      `${tables.length} table styles, ${pivots.length} pivot styles and ${cells.length} cell styles agree with Excel's own file`,
    );
    console.log(written.join('\n'));
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
