// Generated workbooks that cover page setup: paper, orientation, scaling and fitting, margins,
// print titles, manual breaks, hidden and resized rows and columns, headings, centring, page
// order, print areas, and where the printed range starts and ends. Each holds one sheet of
// numbers, so Excel and the app break the same cells.
import { column, zip } from '../tests/fixtures/workbooks.mjs';

const MAIN = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
const RELS = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const HEADER = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';

const styles = (font = 'Calibri', size = 11) =>
  `<styleSheet xmlns="${MAIN}"><fonts count="1"><font><sz val="${size}"/><name val="${font}"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="0" fillId="2" borderId="0" xfId="0" applyFill="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>`.replace(
    '<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>',
    '<fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill></fills>',
  );

/**
 * One case. `rows` and `cols` are how far the numbers go, from `start`; `heights` and `widths`
 * size some lines; `hiddenRows` and `hiddenCols` hide some; `xml` adds page setup elements after
 * sheetData, `pr` goes in sheetPr, and `names` are defined names for the sheet.
 */
function makeBook({
  rows = 150,
  cols = 20,
  start = [1, 1],
  heights = {},
  widths = [],
  hiddenRows = [],
  hiddenCols = [],
  pr = '',
  format = '',
  xml = '',
  names = '',
  font,
  extra = '',
  rightToLeft = false,
}) {
  const body = [];
  const last = start[0] + rows - 1;
  for (let r = start[0]; r <= last; r++) {
    const attrs = [];
    if (heights[r]) attrs.push(`ht="${heights[r]}" customHeight="1"`);
    if (hiddenRows.includes(r)) attrs.push('hidden="1"');
    const cells = [];
    for (let c = start[1]; c < start[1] + cols; c++) {
      cells.push(`<c r="${column(c)}${r}"><v>${r * 100 + c}</v></c>`);
    }
    body.push(`<row r="${r}"${attrs.length ? ' ' + attrs.join(' ') : ''}>${cells.join('')}</row>`);
  }
  if (extra) body.push(extra);
  const colsXml = [
    ...widths.map(([c, w]) => `<col min="${c}" max="${c}" width="${w}" customWidth="1"/>`),
    ...hiddenCols.map((c) => `<col min="${c}" max="${c}" width="8.43" hidden="1"/>`),
  ].sort((a, b) => Number(/min="(\d+)"/.exec(a)[1]) - Number(/min="(\d+)"/.exec(b)[1]));
  const sheet =
    `${HEADER}<worksheet xmlns="${MAIN}" xmlns:r="${RELS}">` +
    (pr ? `<sheetPr>${pr}</sheetPr>` : '') +
    `<sheetViews><sheetView${rightToLeft ? ' rightToLeft="1"' : ''} workbookViewId="0"/></sheetViews><sheetFormatPr${format.includes('defaultRowHeight') ? '' : ' defaultRowHeight="15"'}${format}/>` +
    (colsXml.length ? `<cols>${colsXml.join('')}</cols>` : '') +
    `<sheetData>${body.join('')}</sheetData>` +
    (xml ||
      '<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>') +
    '</worksheet>';
  return zip([
    [
      '[Content_Types].xml',
      `${HEADER}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>`,
    ],
    [
      '_rels/.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
    ],
    [
      'xl/workbook.xml',
      `${HEADER}<workbook xmlns="${MAIN}" xmlns:r="${RELS}"><sheets><sheet name="Pages" sheetId="1" r:id="rId1"/></sheets>${names ? `<definedNames>${names}</definedNames>` : ''}<calcPr calcId="191029"/></workbook>`,
    ],
    [
      'xl/_rels/workbook.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="${RELS}/styles" Target="styles.xml"/></Relationships>`,
    ],
    ['xl/styles.xml', HEADER + styles(font?.[0], font?.[1])],
    ['xl/worksheets/sheet1.xml', sheet],
  ]);
}

const margins = (l, r, t, b, h = 0.3, f = 0.3) =>
  `<pageMargins left="${l}" right="${r}" top="${t}" bottom="${b}" header="${h}" footer="${f}"/>`;
const NORMAL = margins(0.7, 0.7, 0.75, 0.75);
const setup = (attrs, before = '', after = '') => `${before}${NORMAL}<pageSetup ${attrs}/>${after}`;
const name = (kind, text) =>
  `<definedName name="_xlnm.${kind}" localSheetId="0">${text}</definedName>`;

/** Every case, by file name. */
/** With `rightToLeft`, every sheet is right to left, for Phase 11. */
export function printBooks({ rightToLeft = false } = {}) {
  const book = (options) => makeBook({ ...options, rightToLeft });
  return {
    'letter-portrait.xlsx': book({}),
    'a4-portrait.xlsx': book({ xml: setup('paperSize="9" orientation="portrait"') }),
    'legal-landscape.xlsx': book({ xml: setup('paperSize="5" orientation="landscape"') }),
    'a3-landscape.xlsx': book({
      rows: 220,
      cols: 30,
      xml: setup('paperSize="8" orientation="landscape"'),
    }),
    'scale-50.xlsx': book({ rows: 300, cols: 40, xml: setup('scale="50"') }),
    'scale-75.xlsx': book({ xml: setup('scale="75"') }),
    'scale-130.xlsx': book({ xml: setup('scale="130"') }),
    'fit-1x1.xlsx': book({
      pr: '<pageSetUpPr fitToPage="1"/>',
      xml: setup('fitToWidth="1" fitToHeight="1"'),
    }),
    'fit-1-wide.xlsx': book({ pr: '<pageSetUpPr fitToPage="1"/>', xml: setup('fitToHeight="0"') }),
    'fit-1-tall.xlsx': book({ pr: '<pageSetUpPr fitToPage="1"/>', xml: setup('fitToWidth="0"') }),
    'fit-2x3.xlsx': book({
      rows: 300,
      cols: 40,
      pr: '<pageSetUpPr fitToPage="1"/>',
      xml: setup('fitToWidth="2" fitToHeight="3"'),
    }),
    'fit-ignores-breaks.xlsx': book({
      pr: '<pageSetUpPr fitToPage="1"/>',
      xml: setup(
        'fitToHeight="0"',
        '',
        '<rowBreaks count="1" manualBreakCount="1"><brk id="20" max="16383" man="1"/></rowBreaks>',
      ),
    }),
    'margins-wide.xlsx': book({ xml: margins(1, 1, 1, 1, 0.5, 0.5) }),
    'margins-narrow.xlsx': book({ xml: margins(0.25, 0.25, 0.75, 0.75) }),
    'margins-odd.xlsx': book({ xml: margins(0.33, 1.17, 0.61, 1.43, 0.2, 0.9) }),
    'titles-rows.xlsx': book({ names: name('Print_Titles', 'Pages!$1:$2') }),
    'titles-cols.xlsx': book({ names: name('Print_Titles', 'Pages!$A:$B') }),
    'titles-both.xlsx': book({ names: name('Print_Titles', 'Pages!$A:$A,Pages!$1:$3') }),
    'titles-inside.xlsx': book({ names: name('Print_Titles', 'Pages!$10:$11') }),
    'breaks-manual.xlsx': book({
      xml: `${NORMAL}<rowBreaks count="2" manualBreakCount="2"><brk id="20" max="16383" man="1"/><brk id="55" max="16383" man="1"/></rowBreaks><colBreaks count="1" manualBreakCount="1"><brk id="5" max="1048575" man="1"/></colBreaks>`,
    }),
    'hidden-lines.xlsx': book({ hiddenRows: [5, 6, 7, 30, 31, 90], hiddenCols: [3, 4, 12] }),
    'sizes.xlsx': book({
      heights: { 3: 25.5, 4: 9.75, 10: 40, 11: 40, 50: 13.2, 51: 30.6, 70: 100 },
      widths: [
        [1, 3],
        [2, 25],
        [5, 12.71],
        [9, 40.29],
      ],
    }),
    'tall-row.xlsx': book({ rows: 40, heights: { 12: 409.5, 13: 300 } }),
    'headings.xlsx': book({ xml: `<printOptions headings="1"/>${NORMAL}` }),
    'headings-many-rows.xlsx': book({
      rows: 1200,
      cols: 6,
      xml: `<printOptions headings="1"/>${NORMAL}`,
    }),
    'gridlines-centred.xlsx': book({
      xml: `<printOptions horizontalCentered="1" verticalCentered="1" gridLines="1"/>${NORMAL}`,
    }),
    'over-then-down.xlsx': book({ xml: setup('pageOrder="overThenDown"') }),
    'print-area.xlsx': book({ names: name('Print_Area', 'Pages!$B$3:$H$100') }),
    'print-areas.xlsx': book({ names: name('Print_Area', 'Pages!$A$1:$C$10,Pages!$E$20:$P$140') }),
    'print-area-rows.xlsx': book({ names: name('Print_Area', 'Pages!$5:$80') }),
    'starts-late.xlsx': book({ rows: 60, cols: 8, start: [12, 4] }),
    'formatted-tail.xlsx': book({
      rows: 40,
      cols: 6,
      extra: '<row r="95"><c r="J95" s="1"/></row>',
    }),
    'big-font.xlsx': book({ font: ['Calibri', 16], format: ' defaultRowHeight="21"' }),
    'arial-10.xlsx': book({ font: ['Arial', 10], format: ' defaultRowHeight="12.75"' }),
    'default-width.xlsx': book({ format: ' defaultColWidth="14.5"' }),
    'everything.xlsx': book({
      rows: 260,
      cols: 30,
      hiddenRows: [8, 9],
      hiddenCols: [7],
      heights: { 20: 30, 21: 12 },
      widths: [[3, 18]],
      names:
        name('Print_Titles', 'Pages!$A:$A,Pages!$1:$1') + name('Print_Area', 'Pages!$A$1:$Z$240'),
      xml:
        `<printOptions headings="1" horizontalCentered="1"/>` +
        margins(0.5, 0.4, 0.9, 0.6, 0.4, 0.3) +
        '<pageSetup paperSize="9" scale="85" orientation="landscape" pageOrder="overThenDown"/>' +
        '<rowBreaks count="1" manualBreakCount="1"><brk id="100" max="16383" man="1"/></rowBreaks>',
    }),
  };
}
