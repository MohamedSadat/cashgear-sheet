// Builds test workbooks with their saved values already in them, so no Excel is needed and
// nothing binary is checked in. Used by the browser specs, the desktop smoke test and the
// speed checks. Run directly, it writes them to tests/workbooks/fixtures, which git ignores.
import { crc32, deflateRawSync, inflateRawSync } from 'node:zlib';
import { vbaProject } from './vba.mjs';

/** A zip of (name, text or bytes) entries. */
export function zip(entries) {
  const chunks = [];
  const central = [];
  let offset = 0;
  for (const [name, content] of entries) {
    const body = typeof content === 'string' ? Buffer.from(content, 'utf8') : content;
    const packed = deflateRawSync(body, { level: 6 });
    const nameBytes = Buffer.from(name);
    const crc = crc32(body);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0x0800, 6);
    local.writeUInt16LE(8, 8);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(packed.length, 18);
    local.writeUInt32LE(body.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    const header = Buffer.alloc(46);
    header.writeUInt32LE(0x02014b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(20, 6);
    header.writeUInt16LE(0x0800, 8);
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

export const escapeXml = (text) =>
  String(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');

export const column = (index) => {
  let text = '';
  for (let rest = index; rest > 0; rest = Math.floor((rest - 1) / 26)) {
    text = String.fromCharCode(65 + ((rest - 1) % 26)) + text;
  }
  return text;
};

const MAIN = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
const RELS = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const HEADER = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';

const ONE_PIXEL_PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
  'base64',
);

/** The smallest chart Excel accepts: columns over a sheet's numbers. */
function chartXml(sheet) {
  return `<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:plotArea><c:layout/><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:ser><c:idx val="0"/><c:order val="0"/><c:val><c:numRef><c:f>${sheet}!$B$4:$B$5</c:f></c:numRef></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:barChart><c:catAx><c:axId val="1"/><c:scaling/><c:delete val="0"/><c:axPos val="b"/><c:crossAx val="2"/></c:catAx><c:valAx><c:axId val="2"/><c:scaling/><c:delete val="0"/><c:axPos val="l"/><c:crossAx val="1"/></c:valAx></c:plotArea></c:chart></c:chartSpace>`;
}

/** One cell as XML: a number, `{ t: text }`, `{ f, v }` for a formula with a saved value. */
export function cellXml(at, value, style) {
  const s = style ? ' s="' + style + '"' : '';
  if (value === null || value === undefined) return style ? `<c r="${at}"${s}/>` : '';
  if (typeof value === 'number') return `<c r="${at}"${s}><v>${value}</v></c>`;
  if (typeof value === 'string') {
    return `<c r="${at}"${s} t="inlineStr"><is><t xml:space="preserve">${escapeXml(value)}</t></is></c>`;
  }
  if (typeof value === 'boolean') return `<c r="${at}"${s} t="b"><v>${value ? 1 : 0}</v></c>`;
  if ('e' in value) return `<c r="${at}"${s} t="e"><v>${escapeXml(value.e)}</v></c>`;
  const formula = `<f>${escapeXml(value.f)}</f>`;
  if (typeof value.v === 'string') {
    return `<c r="${at}"${s} t="str">${formula}<v>${escapeXml(value.v)}</v></c>`;
  }
  return `<c r="${at}"${s}>${formula}<v>${value.v}</v></c>`;
}

/**
 * Builds a workbook. `sheets` are `{ name, rows, xml, drawings, state }`, where `rows` is a
 * list of `[row number, cells, attributes]` and `cells` a list of `[column, value, style]`.
 * `xml` holds what goes around `sheetData`: `{ before, after }`.
 */
export function workbook(
  sheets,
  { styles = '<styleSheet xmlns="' + MAIN + '"/>', theme, active = 0 } = {},
) {
  const parts = [];
  const overrides = sheets
    .map((sheet, index) => {
      const number = index + 1;
      const drawing = sheet.drawings
        ? `<Override PartName="/xl/drawings/drawing${number}.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/><Override PartName="/xl/charts/chart${number}.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>`
        : '';
      return `<Override PartName="/xl/worksheets/sheet${number}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>${drawing}`;
    })
    .join('');
  parts.push([
    '[Content_Types].xml',
    `${HEADER}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>${overrides}</Types>`,
  ]);
  parts.push([
    '_rels/.rels',
    `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
  ]);
  parts.push([
    'xl/workbook.xml',
    `${HEADER}<workbook xmlns="${MAIN}" xmlns:r="${RELS}"><bookViews><workbookView activeTab="${active}"/></bookViews><sheets>` +
      sheets
        .map((sheet, index) => {
          const state = sheet.state ? ` state="${sheet.state}"` : '';
          return `<sheet name="${escapeXml(sheet.name)}" sheetId="${index + 1}"${state} r:id="rId${index + 1}"/>`;
        })
        .join('') +
      '</sheets></workbook>',
  ]);
  const extra = theme
    ? `<Relationship Id="rTheme" Type="${RELS}/theme" Target="theme/theme1.xml"/>`
    : '';
  parts.push([
    'xl/_rels/workbook.xml.rels',
    `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
      sheets
        .map(
          (_, index) =>
            `<Relationship Id="rId${index + 1}" Type="${RELS}/worksheet" Target="worksheets/sheet${index + 1}.xml"/>`,
        )
        .join('') +
      `<Relationship Id="rStyles" Type="${RELS}/styles" Target="styles.xml"/>${extra}</Relationships>`,
  ]);
  parts.push(['xl/styles.xml', HEADER + styles]);
  if (theme) parts.push(['xl/theme/theme1.xml', HEADER + theme]);
  sheets.forEach((sheet, index) => {
    const number = index + 1;
    const pieces = [
      `${HEADER}<worksheet xmlns="${MAIN}" xmlns:r="${RELS}">${sheet.xml?.before ?? ''}<sheetData>`,
    ];
    for (const [rowNumber, cells, attributes] of sheet.rows ?? []) {
      pieces.push(`<row r="${rowNumber}"${attributes ? ' ' + attributes : ''}>`);
      for (const [col, value, style] of cells) {
        pieces.push(cellXml(column(col) + rowNumber, value, style));
      }
      pieces.push('</row>');
    }
    pieces.push('</sheetData>');
    pieces.push(sheet.xml?.after ?? '');
    if (sheet.drawings) pieces.push('<drawing r:id="rDrawing"/>');
    pieces.push('</worksheet>');
    parts.push([`xl/worksheets/sheet${number}.xml`, pieces.join('')]);
    if (sheet.drawings) {
      parts.push([
        `xl/worksheets/_rels/sheet${number}.xml.rels`,
        `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rDrawing" Type="${RELS}/drawing" Target="../drawings/drawing${number}.xml"/></Relationships>`,
      ]);
      parts.push([`xl/drawings/drawing${number}.xml`, HEADER + sheet.drawings]);
      parts.push([
        `xl/drawings/_rels/drawing${number}.xml.rels`,
        `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/chart" Target="../charts/chart${number}.xml"/><Relationship Id="rId2" Type="${RELS}/image" Target="../media/image${number}.png"/></Relationships>`,
      ]);
      parts.push([`xl/charts/chart${number}.xml`, HEADER + chartXml(sheet.name)]);
      parts.push([`xl/media/image${number}.png`, ONE_PIXEL_PNG]);
    }
  });
  return zip(parts);
}

// A style table with one of everything the grid draws. The xf indices below are named so
// that a workbook reads like a description of itself.
export const XF = {
  plain: 0,
  bold: 1,
  italic: 2,
  underline: 3,
  strike: 4,
  big: 5,
  red: 6,
  themeDark: 7,
  fixedBlack: 8,
  fillYellow: 9,
  fillAccent: 10,
  fillWhiteTheme: 11,
  money: 12,
  percent: 13,
  date: 14,
  time: 15,
  redNegative: 16,
  accounting: 17,
  thinBox: 18,
  thickBottom: 19,
  doubleBottom: 20,
  dashedBox: 21,
  center: 22,
  right: 23,
  top: 24,
  wrap: 25,
  wrapCenter: 26,
  indent: 27,
  rotated: 28,
  stacked: 29,
  shrink: 30,
  fillCode: 31,
  arial: 32,
  times: 33,
  courier: 34,
  rtlOrder: 35,
  ltrOrder: 36,
  grayPattern: 37,
  centerAcross: 38,
  fixedDarkBlue: 39,
};

export const STYLES = `<styleSheet xmlns="${MAIN}">
<numFmts count="6">
  <numFmt numFmtId="164" formatCode="#,##0.00"/>
  <numFmt numFmtId="165" formatCode="yyyy-mm-dd"/>
  <numFmt numFmtId="166" formatCode="h:mm:ss"/>
  <numFmt numFmtId="167" formatCode="0.00;[Red]-0.00"/>
  <numFmt numFmtId="168" formatCode="_(&quot;$&quot;* #,##0.00_);_(&quot;$&quot;* \\(#,##0.00\\)"/>
  <numFmt numFmtId="169" formatCode="@*."/>
</numFmts>
<fonts count="14">
  <font><sz val="11"/><name val="Calibri"/></font>
  <font><b/><sz val="11"/><name val="Calibri"/></font>
  <font><i/><sz val="11"/><name val="Calibri"/></font>
  <font><u/><sz val="11"/><name val="Calibri"/></font>
  <font><strike/><sz val="11"/><name val="Calibri"/></font>
  <font><sz val="18"/><name val="Calibri"/></font>
  <font><sz val="11"/><color rgb="FFC00000"/><name val="Calibri"/></font>
  <font><sz val="11"/><color theme="1"/><name val="Calibri"/></font>
  <font><sz val="11"/><color rgb="FF000000"/><name val="Calibri"/></font>
  <font><sz val="10"/><name val="Arial"/></font>
  <font><sz val="11"/><name val="Times New Roman"/></font>
  <font><sz val="10"/><name val="Courier New"/></font>
  <font><sz val="11"/><color rgb="FF000080"/><name val="Calibri"/></font>
  <font><b/><sz val="11"/><color rgb="FFFFFFFF"/><name val="Calibri"/></font>
</fonts>
<fills count="6">
  <fill><patternFill patternType="none"/></fill>
  <fill><patternFill patternType="gray125"/></fill>
  <fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/><bgColor indexed="64"/></patternFill></fill>
  <fill><patternFill patternType="solid"><fgColor theme="4"/><bgColor indexed="64"/></patternFill></fill>
  <fill><patternFill patternType="solid"><fgColor theme="0"/><bgColor indexed="64"/></patternFill></fill>
  <fill><patternFill patternType="mediumGray"><fgColor rgb="FF808080"/><bgColor rgb="FFFFFFFF"/></patternFill></fill>
</fills>
<borders count="5">
  <border><left/><right/><top/><bottom/><diagonal/></border>
  <border><left style="thin"><color auto="1"/></left><right style="thin"><color auto="1"/></right><top style="thin"><color auto="1"/></top><bottom style="thin"><color auto="1"/></bottom><diagonal/></border>
  <border><left/><right/><top/><bottom style="thick"><color rgb="FF0000FF"/></bottom><diagonal/></border>
  <border><left/><right/><top/><bottom style="double"><color rgb="FF000000"/></bottom><diagonal/></border>
  <border><left style="dashed"><color rgb="FF00B050"/></left><right style="dashed"><color rgb="FF00B050"/></right><top style="dashed"><color rgb="FF00B050"/></top><bottom style="dashed"><color rgb="FF00B050"/></bottom><diagonal/></border>
</borders>
<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
<cellXfs count="40">
  <xf/>
  <xf fontId="1"/>
  <xf fontId="2"/>
  <xf fontId="3"/>
  <xf fontId="4"/>
  <xf fontId="5"/>
  <xf fontId="6"/>
  <xf fontId="7"/>
  <xf fontId="8"/>
  <xf fillId="2"/>
  <xf fontId="13" fillId="3"/>
  <xf fillId="4"/>
  <xf numFmtId="164"/>
  <xf numFmtId="10"/>
  <xf numFmtId="165"/>
  <xf numFmtId="166"/>
  <xf numFmtId="167"/>
  <xf numFmtId="168"/>
  <xf borderId="1"/>
  <xf borderId="2"/>
  <xf borderId="3"/>
  <xf borderId="4"/>
  <xf><alignment horizontal="center" vertical="center"/></xf>
  <xf><alignment horizontal="right"/></xf>
  <xf><alignment vertical="top"/></xf>
  <xf><alignment wrapText="1" vertical="top"/></xf>
  <xf><alignment horizontal="center" vertical="center" wrapText="1"/></xf>
  <xf><alignment horizontal="left" indent="2"/></xf>
  <xf><alignment horizontal="center" textRotation="45"/></xf>
  <xf><alignment horizontal="center" textRotation="255"/></xf>
  <xf><alignment shrinkToFit="1"/></xf>
  <xf numFmtId="169"/>
  <xf fontId="9"/>
  <xf fontId="10"/>
  <xf fontId="11"/>
  <xf><alignment readingOrder="2"/></xf>
  <xf><alignment readingOrder="1"/></xf>
  <xf fillId="5"/>
  <xf><alignment horizontal="centerContinuous"/></xf>
  <xf fontId="12"/>
</cellXfs>
</styleSheet>`;

/** Arabic and mixed text, in every position the direction logic cares about. */
export function arabicBook() {
  const rows = [
    [1, [[1, 'Arabic and mixed text', XF.bold]]],
    [
      2,
      [
        [1, 'مرحبا بالعالم'],
        [2, 'Hello'],
        [3, 'Hello مرحبا'],
        [4, 'مرحبا Hello'],
      ],
    ],
    [
      3,
      [
        [1, 'السعر 100 جنيه'],
        [2, '123'],
        [3, 'رقم 42'],
        [4, '42 رقم'],
      ],
    ],
    [
      4,
      [
        [1, 'نص طويل جدا يمتد إلى الخلايا المجاورة عندما تكون فارغة'],
        [5, 'after'],
      ],
    ],
    [
      5,
      [
        [2, 'long English text overflowing to the right of its cell, past a few columns'],
        [6, 'stop'],
      ],
    ],
    [
      6,
      [
        [1, 'يسار', XF.ltrOrder],
        [2, 'left', XF.rtlOrder],
        [3, 'يمين', XF.right],
        [4, 'وسط', XF.center],
      ],
    ],
    [
      7,
      [
        [1, 'عربي', XF.bold],
        [2, 'عربي', XF.italic],
        [3, 'عربي', XF.underline],
        [4, 'عربي', XF.big],
      ],
    ],
    [
      8,
      [
        [1, 'سطر أول\nسطر ثان', XF.wrap],
        [2, 'first\nثان', XF.wrap],
      ],
      'ht="45" customHeight="1"',
    ],
    [
      9,
      [
        [1, 1234.5],
        [2, 45306, XF.date],
        [3, 0.25, XF.percent],
        [4, 'النهاية'],
      ],
    ],
    [10, [[1, 'مدمج عبر خليتين', XF.center]]],
  ];
  return workbook(
    [
      {
        name: 'عربي',
        rows,
        xml: {
          before: '<cols><col min="1" max="4" width="20.7109375" customWidth="1"/></cols>',
          after: '<mergeCells count="1"><mergeCell ref="A10:B10"/></mergeCells>',
        },
      },
    ],
    { styles: STYLES },
  );
}

/** One of everything: fonts, fills, borders, alignments, formats, merges, panes, drawings. */
export function formattingBook() {
  const row = (number, cells, attributes) => [number, cells, attributes];
  const sheet = {
    name: 'Formats',
    rows: [
      row(1, [
        [1, 'Header one', XF.fillAccent],
        [2, 'Header two', XF.fillAccent],
        [3, 'Header three', XF.fillAccent],
        [4, 'Header four', XF.fillAccent],
        [5, 'Header five', XF.fillAccent],
      ]),
      row(2, [
        [1, 'bold', XF.bold],
        [2, 'italic', XF.italic],
        [3, 'underline', XF.underline],
        [4, 'strike', XF.strike],
        [5, 'big', XF.big],
      ]),
      row(3, [
        [1, 'red', XF.red],
        [2, 'theme black', XF.themeDark],
        [3, 'fixed black', XF.fixedBlack],
        [4, 'fixed navy', XF.fixedDarkBlue],
        [5, 'on yellow', XF.fillYellow],
      ]),
      row(4, [
        [1, 1234.5, XF.money],
        [2, 0.125, XF.percent],
        [3, 45306, XF.date],
        [4, 0.75, XF.time],
        [5, -42.5, XF.redNegative],
      ]),
      row(5, [
        [1, 1234.5, XF.accounting],
        [2, 3.14159265358979],
        [3, 123456789012],
        [4, 1234567.891, XF.money],
        [5, -1, XF.date],
      ]),
      row(6, [
        [1, true],
        [2, false],
        [3, { e: '#N/A' }],
        [4, { f: '1+1', v: 2 }],
        [5, { f: '"a"&"b"', v: 'ab' }],
      ]),
      row(7, [
        [1, 'boxed', XF.thinBox],
        [2, 'thick', XF.thickBottom],
        [3, 'double', XF.doubleBottom],
        [4, 'dashed', XF.dashedBox],
        [5, null, XF.fillWhiteTheme],
      ]),
      row(
        8,
        [
          [1, 'centre', XF.center],
          [2, 'right', XF.right],
          [3, 'top', XF.top],
          [4, 'wrapped text across some lines', XF.wrap],
          [5, 'centred and wrapped text', XF.wrapCenter],
        ],
        'ht="60" customHeight="1"',
      ),
      row(
        9,
        [
          [1, 'indented', XF.indent],
          [2, 'angled', XF.rotated],
          [3, 'up', XF.stacked],
          [4, 'shrink this text to fit', XF.shrink],
          [5, 'dots', XF.fillCode],
        ],
        'ht="48" customHeight="1"',
      ),
      row(10, [
        [1, 'Arial 10', XF.arial],
        [2, 'Times', XF.times],
        [3, 'Courier', XF.courier],
        [4, null, XF.grayPattern],
        [5, 'across', XF.centerAcross],
        [6, null, XF.centerAcross],
      ]),
      row(11, [[1, 'merged across three', XF.center]]),
      row(12, [[1, 'A very long piece of text that overflows into the empty cells on its right']]),
      row(13, [
        [1, 'cut off at the next value'],
        [3, 'here'],
      ]),
      row(14, [[3, 'right aligned text overflowing leftwards', XF.right]]),
      row(16, [[1, 'after the hidden row']], ''),
      row(15, [[1, 'hidden row']], 'hidden="1"'),
    ].sort((a, b) => a[0] - b[0]),
    xml: {
      before:
        '<sheetViews><sheetView workbookViewId="0"><pane xSplit="1" ySplit="1" topLeftCell="B2" activePane="bottomRight" state="frozen"/><selection pane="bottomRight" activeCell="B2" sqref="B2"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight="15"/><cols><col min="1" max="1" width="18.7109375" customWidth="1"/><col min="2" max="5" width="14.7109375" customWidth="1"/><col min="8" max="8" width="9" hidden="1"/></cols>',
      after: '<mergeCells count="1"><mergeCell ref="A11:C11"/></mergeCells>',
    },
    drawings: `<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:r="${RELS}">
      <xdr:twoCellAnchor><xdr:from><xdr:col>6</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>10</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>8</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="Sales chart"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor>
      <xdr:oneCellAnchor><xdr:from><xdr:col>6</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>10</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx="1905000" cy="952500"/>
        <xdr:pic><xdr:nvPicPr><xdr:cNvPr id="3" name="Logo"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic><xdr:clientData/></xdr:oneCellAnchor>
    </xdr:wsDr>`,
  };
  const second = {
    name: 'Second',
    rows: [
      [1, [[1, 'On the second sheet']]],
      [2, [[1, 2]]],
    ],
  };
  const hidden = { name: 'Hidden', state: 'hidden', rows: [[1, [[1, 'secret']]]] };
  return workbook([sheet, second, hidden], { styles: STYLES });
}

/** A workbook with only automatic and theme text, as Excel saves a new file. */
export function defaultStyledBook() {
  const rows = [];
  for (let r = 1; r <= 40; r++) {
    rows.push([
      r,
      [
        [1, 'Item ' + r, XF.themeDark],
        [2, r * 3.5, XF.money],
        [3, { f: `B${r}*2`, v: r * 7 }],
        [4, 45000 + r, XF.date],
      ],
    ]);
  }
  return workbook(
    [
      {
        name: 'Sheet1',
        rows,
        xml: { before: '<cols><col min="4" max="4" width="12.7109375" customWidth="1"/></cols>' },
      },
    ],
    { styles: STYLES },
  );
}

/** `count` rows of values, with no shared strings, for scrolling. */
export function manyRowsXml(count, columns = 6) {
  const parts = [];
  for (let r = 1; r <= count; r++) {
    parts.push(`<row r="${r}">`);
    parts.push(`<c r="A${r}" t="inlineStr"><is><t>Row ${r}</t></is></c>`);
    for (let c = 2; c <= columns; c++) {
      parts.push(`<c r="${column(c)}${r}"><v>${(r * c) % 9973}</v></c>`);
    }
    parts.push('</row>');
  }
  return parts.join('');
}

/** A sheet of `rows` rows of values, built in one piece, for the speed checks. */
export function manyRowsBook(rows, columns = 6) {
  const sheet = `${HEADER}<worksheet xmlns="${MAIN}" xmlns:r="${RELS}"><dimension ref="A1:${column(columns)}${rows}"/><sheetData>${manyRowsXml(rows, columns)}</sheetData></worksheet>`;
  return zip([
    [
      '_rels/.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
    ],
    [
      'xl/workbook.xml',
      `${HEADER}<workbook xmlns="${MAIN}" xmlns:r="${RELS}"><sheets><sheet name="Rows" sheetId="1" r:id="rId1"/></sheets></workbook>`,
    ],
    [
      'xl/_rels/workbook.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>`,
    ],
    ['xl/worksheets/sheet1.xml', sheet],
  ]);
}

const PLAIN_STYLES = `<styleSheet xmlns="${MAIN}"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>`;

const SALES = [
  ['North', 10],
  ['South', 20],
  ['North', 30],
  ['South', 40],
];

function rowsXml(rows) {
  return rows
    .map(
      ([number, cells]) =>
        `<row r="${number}">${cells.map(([col, value]) => cellXml(column(col) + number, value)).join('')}</row>`,
    )
    .join('');
}

/**
 * A pivot table over a sheet of sales, whose saved output is already right: North 40, South 60,
 * Grand Total 100. Excel can read and refresh it, so a save can be shown to leave it working.
 */
export function pivotBook() {
  const data = [
    [
      1,
      [
        [1, 'Region'],
        [2, 'Sales'],
      ],
    ],
    ...SALES.map(([region, sales], index) => [
      index + 2,
      [
        [1, region],
        [2, sales],
      ],
    ]),
  ];
  const output = [
    [
      3,
      [
        [1, 'Row Labels'],
        [2, 'Sum of Sales'],
      ],
    ],
    [
      4,
      [
        [1, 'North'],
        [2, 40],
      ],
    ],
    [
      5,
      [
        [1, 'South'],
        [2, 60],
      ],
    ],
    [
      6,
      [
        [1, 'Grand Total'],
        [2, 100],
      ],
    ],
  ];
  const records = SALES.map(
    ([region, sales]) => `<r><x v="${region === 'North' ? 0 : 1}"/><n v="${sales}"/></r>`,
  ).join('');
  return zip([
    [
      '[Content_Types].xml',
      `${HEADER}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/pivotTables/pivotTable1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml"/><Override PartName="/xl/pivotCache/pivotCacheDefinition1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml"/><Override PartName="/xl/pivotCache/pivotCacheRecords1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml"/></Types>`,
    ],
    [
      '_rels/.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
    ],
    [
      'xl/workbook.xml',
      `${HEADER}<workbook xmlns="${MAIN}" xmlns:r="${RELS}"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/><sheet name="Pivot" sheetId="2" r:id="rId2"/></sheets><pivotCaches><pivotCache cacheId="1" r:id="rId4"/></pivotCaches></workbook>`,
    ],
    [
      'xl/_rels/workbook.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="${RELS}/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="${RELS}/styles" Target="styles.xml"/><Relationship Id="rId4" Type="${RELS}/pivotCacheDefinition" Target="pivotCache/pivotCacheDefinition1.xml"/></Relationships>`,
    ],
    ['xl/styles.xml', HEADER + PLAIN_STYLES],
    [
      'xl/worksheets/sheet1.xml',
      `${HEADER}<worksheet xmlns="${MAIN}"><dimension ref="A1:B5"/><sheetData>${rowsXml(data)}</sheetData></worksheet>`,
    ],
    [
      'xl/worksheets/sheet2.xml',
      `${HEADER}<worksheet xmlns="${MAIN}"><dimension ref="A3:B6"/><sheetData>${rowsXml(output)}</sheetData></worksheet>`,
    ],
    [
      'xl/worksheets/_rels/sheet2.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/pivotTable" Target="../pivotTables/pivotTable1.xml"/></Relationships>`,
    ],
    [
      'xl/pivotTables/pivotTable1.xml',
      `${HEADER}<pivotTableDefinition xmlns="${MAIN}" name="Sales" cacheId="1" applyNumberFormats="0" applyBorderFormats="0" applyFontFormats="0" applyPatternFormats="0" applyAlignmentFormats="0" applyWidthHeightFormats="1" dataCaption="Values" updatedVersion="6" minRefreshableVersion="3" useAutoFormatting="1" itemPrintTitles="1" createdVersion="6" indent="0" outline="1" outlineData="1" multipleFieldFilters="0"><location ref="A3:B6" firstHeaderRow="1" firstDataRow="1" firstDataCol="1"/><pivotFields count="2"><pivotField axis="axisRow" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField><pivotField dataField="1" showAll="0"/></pivotFields><rowFields count="1"><field x="0"/></rowFields><rowItems count="3"><i><x/></i><i><x v="1"/></i><i t="grand"><x/></i></rowItems><colItems count="1"><i/></colItems><dataFields count="1"><dataField name="Sum of Sales" fld="1" baseField="0" baseItem="0"/></dataFields><pivotTableStyleInfo name="PivotStyleLight16" showRowHeaders="1" showColHeaders="1" showRowStripes="0" showColStripes="0" showLastColumn="1"/></pivotTableDefinition>`,
    ],
    [
      'xl/pivotTables/_rels/pivotTable1.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/pivotCacheDefinition" Target="../pivotCache/pivotCacheDefinition1.xml"/></Relationships>`,
    ],
    [
      'xl/pivotCache/pivotCacheDefinition1.xml',
      `${HEADER}<pivotCacheDefinition xmlns="${MAIN}" xmlns:r="${RELS}" r:id="rId1" refreshedBy="CashGear" refreshedDate="45000" createdVersion="6" refreshedVersion="6" minRefreshableVersion="3" recordCount="4"><cacheSource type="worksheet"><worksheetSource ref="A1:B5" sheet="Data"/></cacheSource><cacheFields count="2"><cacheField name="Region" numFmtId="0"><sharedItems count="2"><s v="North"/><s v="South"/></sharedItems></cacheField><cacheField name="Sales" numFmtId="0"><sharedItems containsSemiMixedTypes="0" containsString="0" containsNumber="1" containsInteger="1" minValue="10" maxValue="40"/></cacheField></cacheFields></pivotCacheDefinition>`,
    ],
    [
      'xl/pivotCache/_rels/pivotCacheDefinition1.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/pivotCacheRecords" Target="pivotCacheRecords1.xml"/></Relationships>`,
    ],
    [
      'xl/pivotCache/pivotCacheRecords1.xml',
      `${HEADER}<pivotCacheRecords xmlns="${MAIN}" xmlns:r="${RELS}" count="4">${records}</pivotCacheRecords>`,
    ],
  ]);
}

/** The macro the check runs: it reads a cell, so a save's edit shows in what it returns. */
export const MACRO_CODE = `Public Function Probe() As Double
    Probe = Worksheets("Data").Range("A1").Value * 2
End Function`;

/** A macro workbook whose whole VBA project is built from `MACRO_CODE` here. */
export function macroBook() {
  return zip([
    [
      '[Content_Types].xml',
      `${HEADER}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="bin" ContentType="application/vnd.ms-office.vbaProject"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.ms-excel.sheet.macroEnabled.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>`,
    ],
    [
      '_rels/.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
    ],
    [
      'xl/workbook.xml',
      `${HEADER}<workbook xmlns="${MAIN}" xmlns:r="${RELS}"><workbookPr codeName="ThisWorkbook"/><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>`,
    ],
    [
      'xl/_rels/workbook.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="${RELS}/styles" Target="styles.xml"/><Relationship Id="rId3" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="vbaProject.bin"/></Relationships>`,
    ],
    ['xl/styles.xml', HEADER + PLAIN_STYLES],
    [
      'xl/worksheets/sheet1.xml',
      `${HEADER}<worksheet xmlns="${MAIN}"><sheetPr codeName="Sheet1"/><dimension ref="A1"/><sheetData><row r="1"><c r="A1"><v>21</v></c></row></sheetData></worksheet>`,
    ],
    ['xl/vbaProject.bin', vbaProject(MACRO_CODE)],
  ]);
}

/** A table, a shared formula, an array, a merge, a calculation chain and shared strings. */
export function structuresBook() {
  const sheet = `${HEADER}<worksheet xmlns="${MAIN}" xmlns:r="${RELS}"><dimension ref="A1:E9"/><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetData><row r="1" spans="1:3"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c></row><row r="2" spans="1:3"><c r="A2"><v>1</v></c><c r="B2"><v>2</v></c><c r="C2"><f>A2+B2</f><v>3</v></c></row><row r="3" spans="1:3"><c r="A3"><v>3</v></c><c r="B3"><v>4</v></c><c r="C3"><f>A3+B3</f><v>7</v></c></row><row r="5"><c r="A5"><v>1</v></c><c r="B5"><f t="shared" ref="B5:B7" si="0">A5*2</f><v>2</v></c></row><row r="6"><c r="A6"><v>2</v></c><c r="B6"><f t="shared" si="0"/><v>4</v></c></row><row r="7"><c r="A7"><v>3</v></c><c r="B7"><f t="shared" si="0"/><v>6</v></c></row><row r="9"><c r="D9"><f t="array" ref="D9:E9">{1,2}*A5</f><v>1</v></c><c r="E9"><v>2</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="D2:E3"/></mergeCells><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><tableParts count="1"><tablePart r:id="rId1"/></tableParts></worksheet>`;
  return zip([
    [
      '[Content_Types].xml',
      `${HEADER}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/><Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/tables/table1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"/></Types>`,
    ],
    [
      '_rels/.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
    ],
    [
      'xl/workbook.xml',
      `${HEADER}<workbook xmlns="${MAIN}" xmlns:r="${RELS}"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets><calcPr calcId="191029"/></workbook>`,
    ],
    [
      'xl/_rels/workbook.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="${RELS}/styles" Target="styles.xml"/><Relationship Id="rId3" Type="${RELS}/sharedStrings" Target="sharedStrings.xml"/><Relationship Id="rId4" Type="${RELS}/calcChain" Target="calcChain.xml"/></Relationships>`,
    ],
    ['xl/styles.xml', HEADER + PLAIN_STYLES],
    [
      'xl/sharedStrings.xml',
      `${HEADER}<sst xmlns="${MAIN}" count="3" uniqueCount="3"><si><t>First</t></si><si><t>Second</t></si><si><t>Total</t></si></sst>`,
    ],
    [
      'xl/calcChain.xml',
      `${HEADER}<calcChain xmlns="${MAIN}"><c r="C2" i="1"/><c r="C3"/><c r="B5"/><c r="B6"/><c r="B7"/><c r="D9" a="1"/></calcChain>`,
    ],
    ['xl/worksheets/sheet1.xml', sheet],
    [
      'xl/worksheets/_rels/sheet1.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/table" Target="../tables/table1.xml"/></Relationships>`,
    ],
    [
      'xl/tables/table1.xml',
      `${HEADER}<table xmlns="${MAIN}" id="1" name="Table1" displayName="Table1" ref="A1:C3" totalsRowShown="0"><autoFilter ref="A1:C3"/><tableColumns count="3"><tableColumn id="1" name="First"/><tableColumn id="2" name="Second"/><tableColumn id="3" name="Total"><calculatedColumnFormula>A2+B2</calculatedColumnFormula></tableColumn></tableColumns><tableStyleInfo name="TableStyleMedium2" showFirstColumn="0" showLastColumn="0" showRowStripes="1" showColumnStripes="0"/></table>`,
    ],
  ]);
}

/**
 * One of each thing in a file that holds a cell address, for structural edits to get right: merges,
 * conditional formats old and new, validation, a link, a comment and its shape, a chart, a table,
 * names, arrays of both kinds, a sparkline, and another sheet that reads this one.
 */
export function movingPartsBook() {
  const X14 = 'http://schemas.microsoft.com/office/spreadsheetml/2009/9/main';
  const XM = 'http://schemas.microsoft.com/office/excel/2006/main';
  const cells = (row, list) =>
    `<row r="${row}">` +
    list
      .map(
        ([col, body]) =>
          `<c r="${col}${row}"${body.startsWith('t=') ? ' ' + body.split('|')[0] + '>' + body.split('|')[1] : '>' + body}</c>`,
      )
      .join('') +
    '</row>';
  const text = (value) => `t="inlineStr"|<is><t>${value}</t></is>`;
  const data =
    `${HEADER}<worksheet xmlns="${MAIN}" xmlns:r="${RELS}" xmlns:x14="${X14}" xmlns:xm="${XM}" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:xda="http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray">` +
    '<dimension ref="A1:H20"/><sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/><selection pane="bottomLeft" activeCell="B3" sqref="B3"/></sheetView></sheetViews>' +
    '<sheetFormatPr defaultRowHeight="15"/><cols><col min="2" max="3" width="14" customWidth="1"/></cols><sheetData>' +
    cells(1, [
      ['A', text('Name')],
      ['B', text('Amount')],
      ['C', text('Twice')],
    ]) +
    cells(2, [
      ['A', text('one')],
      ['B', '<v>10</v>'],
      ['C', '<f t="shared" ref="C2:C5" si="0">B2*2</f><v>20</v>'],
    ]) +
    cells(3, [
      ['A', text('two')],
      ['B', '<v>20</v>'],
      ['C', '<f t="shared" si="0"/><v>40</v>'],
    ]) +
    cells(4, [
      ['A', text('three')],
      ['B', '<v>30</v>'],
      ['C', '<f t="shared" si="0"/><v>60</v>'],
    ]) +
    cells(5, [
      ['A', text('four')],
      ['B', '<v>40</v>'],
      ['C', '<f t="shared" si="0"/><v>80</v>'],
    ]) +
    cells(7, [
      ['A', text('Total')],
      ['B', '<f>SUM(Sales[Amount])</f><v>100</v>'],
      ['C', '<f>SUM(C2:C5)</f><v>200</v>'],
    ]) +
    cells(9, [
      ['A', text('merged')],
      ['E', '<f t="array" ref="E9:F10">B2:C3</f><v>10</v>'],
      ['F', '<v>20</v>'],
    ]) +
    cells(10, [
      ['A', text('below')],
      ['E', '<v>20</v>'],
      ['F', '<v>40</v>'],
    ]) +
    `<row r="12"><c r="A12" cm="1"><f t="array" ref="A12:A13">B2:B3*2</f><v>20</v></c><c r="D12"><v>1</v></c><c r="E12"><v>2</v></c><c r="F12"><v>3</v></c></row>` +
    `<row r="13"><c r="A13"><v>40</v></c></row>` +
    cells(15, [
      ['A', 't="str"|<f>HYPERLINK("#B2","go")</f><v>go</v>'],
      ['B', '<v>5</v>'],
      ['C', '<f>OFFSET(B15,0,0)</f><v>5</v>'],
    ]) +
    '</sheetData>' +
    '<mergeCells count="2"><mergeCell ref="A9:C9"/><mergeCell ref="A18:B20"/></mergeCells>' +
    '<conditionalFormatting sqref="B2:B5 E12:F12"><cfRule type="expression" priority="1"><formula>$B2&gt;$B$15</formula></cfRule></conditionalFormatting>' +
    '<dataValidations count="1"><dataValidation type="list" allowBlank="1" showInputMessage="1" sqref="D2:D5"><formula1>$A$2:$A$5</formula1></dataValidation></dataValidations>' +
    `<hyperlinks><hyperlink ref="B15" location="Other!A1" display="there"/></hyperlinks>` +
    '<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>' +
    '<rowBreaks count="1" manualBreakCount="1"><brk id="10" max="16383" man="1"/></rowBreaks>' +
    '<drawing r:id="rDrawing"/><legacyDrawing r:id="rVml"/><tableParts count="1"><tablePart r:id="rTable"/></tableParts>' +
    '<extLst>' +
    `<ext xmlns:x14="${X14}" uri="{78C0D931-6437-407d-A8EE-F0AAD7539E65}"><x14:conditionalFormattings><x14:conditionalFormatting><x14:cfRule type="expression" priority="2" id="{00000000-0000-0000-0000-000000000001}"><xm:f>Other!$A$1&gt;0</xm:f><x14:dxf/></x14:cfRule><xm:sqref>C2:C5</xm:sqref></x14:conditionalFormatting></x14:conditionalFormattings></ext>` +
    `<ext xmlns:x14="${X14}" uri="{05C60535-1F16-4fd2-B633-F4F36F0B64E0}"><x14:sparklineGroups><x14:sparklineGroup><x14:colorSeries rgb="FF376092"/><x14:sparklines><x14:sparkline><xm:f>Data!B2:B5</xm:f><xm:sqref>G2</xm:sqref></x14:sparkline></x14:sparklines></x14:sparklineGroup></x14:sparklineGroups></ext>` +
    '</extLst></worksheet>';
  const other =
    `${HEADER}<worksheet xmlns="${MAIN}" xmlns:r="${RELS}"><sheetData>` +
    `<row r="1"><c r="A1"><f>Data!B3+1</f><v>21</v></c><c r="B1"><f>SUM(Data!B:B)</f><v>205</v></c><c r="C1"><f>SUM(Data!2:3)</f><v>90</v></c></row>` +
    `<row r="2"><c r="A2"><f>Total</f><v>100</v></c><c r="B2"><f>SUM(Sales[Twice])</f><v>200</v></c></row>` +
    '</sheetData></worksheet>';
  const table = `${HEADER}<table xmlns="${MAIN}" id="1" name="Sales" displayName="Sales" ref="A1:C5" totalsRowShown="0"><autoFilter ref="A1:C5"/><tableColumns count="3"><tableColumn id="1" name="Name"/><tableColumn id="2" name="Amount"/><tableColumn id="3" name="Twice"><calculatedColumnFormula>Sales[[#This Row],[Amount]]*2</calculatedColumnFormula></tableColumn></tableColumns><tableStyleInfo name="TableStyleMedium2" showRowStripes="1"/></table>`;
  const comments = `${HEADER}<comments xmlns="${MAIN}"><authors><author>Test</author></authors><commentList><comment ref="B3" authorId="0"><text><t>A note</t></text></comment></commentList></comments>`;
  const vml =
    '<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel"><o:shapelayout v:ext="edit"><o:idmap v:ext="edit" data="1"/></o:shapelayout><v:shapetype id="_x0000_t202" coordsize="21600,21600" o:spt="202" path="m,l,21600r21600,l21600,xe"><v:stroke joinstyle="miter"/><v:path gradientshapeok="t" o:connecttype="rect"/></v:shapetype><v:shape id="_x0000_s1025" type="#_x0000_t202" style="position:absolute;margin-left:100pt;margin-top:20pt;width:100pt;height:50pt;visibility:hidden" fillcolor="#ffffe1" o:insetmode="auto"><v:fill color2="#ffffe1"/><v:shadow on="t" color="black" obscured="t"/><v:path o:connecttype="none"/><v:textbox/><x:ClientData ObjectType="Note"><x:MoveWithCells/><x:SizeWithCells/><x:Anchor>3, 15, 1, 10, 5, 31, 5, 4</x:Anchor><x:AutoFill>False</x:AutoFill><x:Row>2</x:Row><x:Column>1</x:Column></x:ClientData></v:shape></xml>';
  const drawing =
    '<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><xdr:twoCellAnchor><xdr:from><xdr:col>8</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>12</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>12</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="Sales chart"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart r:id="rId1"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>';
  const metadata = `${HEADER}<metadata xmlns="${MAIN}" xmlns:xda="http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray"><metadataTypes count="1"><metadataType name="XLDAPR" minSupportedVersion="120000" copy="1" pasteAll="1" pasteValues="1" merge="1" splitFirst="1" rowColShift="1" clearFormats="1" clearComments="1" assign="1" coerce="1" cellMeta="1"/></metadataTypes><futureMetadata name="XLDAPR" count="1"><bk><extLst><ext uri="{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}"><xda:dynamicArrayProperties fDynamic="1" fCollapsed="0"/></ext></extLst></bk></futureMetadata><cellMetadata count="1"><bk><rc t="1" v="0"/></bk></cellMetadata></metadata>`;
  const types = 'application/vnd.openxmlformats-officedocument';
  return zip([
    [
      '[Content_Types].xml',
      `${HEADER}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="vml" ContentType="${types}.vmlDrawing"/><Override PartName="/xl/workbook.xml" ContentType="${types}.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="${types}.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="${types}.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="${types}.spreadsheetml.styles+xml"/><Override PartName="/xl/tables/table1.xml" ContentType="${types}.spreadsheetml.table+xml"/><Override PartName="/xl/comments1.xml" ContentType="${types}.spreadsheetml.comments+xml"/><Override PartName="/xl/drawings/drawing1.xml" ContentType="${types}.drawing+xml"/><Override PartName="/xl/charts/chart1.xml" ContentType="${types}.drawingml.chart+xml"/><Override PartName="/xl/metadata.xml" ContentType="${types}.spreadsheetml.sheetMetadata+xml"/></Types>`,
    ],
    [
      '_rels/.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
    ],
    [
      'xl/workbook.xml',
      `${HEADER}<workbook xmlns="${MAIN}" xmlns:r="${RELS}"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/><sheet name="Other" sheetId="2" r:id="rId2"/></sheets><definedNames><definedName name="_xlnm.Print_Area" localSheetId="0">Data!$A$1:$C$10</definedName><definedName name="_xlnm.Print_Titles" localSheetId="0">Data!$1:$1</definedName><definedName name="Total">Data!$B$7</definedName></definedNames><calcPr calcId="191029"/></workbook>`,
    ],
    [
      'xl/_rels/workbook.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="${RELS}/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="${RELS}/styles" Target="styles.xml"/><Relationship Id="rId4" Type="${RELS}/sheetMetadata" Target="metadata.xml"/></Relationships>`,
    ],
    ['xl/styles.xml', HEADER + PLAIN_STYLES],
    ['xl/metadata.xml', metadata],
    ['xl/worksheets/sheet1.xml', data],
    ['xl/worksheets/sheet2.xml', other],
    [
      'xl/worksheets/_rels/sheet1.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rDrawing" Type="${RELS}/drawing" Target="../drawings/drawing1.xml"/><Relationship Id="rVml" Type="${RELS}/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/><Relationship Id="rTable" Type="${RELS}/table" Target="../tables/table1.xml"/><Relationship Id="rComments" Type="${RELS}/comments" Target="../comments1.xml"/></Relationships>`,
    ],
    ['xl/tables/table1.xml', table],
    ['xl/comments1.xml', comments],
    ['xl/drawings/vmlDrawing1.vml', vml],
    ['xl/drawings/drawing1.xml', HEADER + drawing],
    [
      'xl/drawings/_rels/drawing1.xml.rels',
      `${HEADER}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/chart" Target="../charts/chart1.xml"/></Relationships>`,
    ],
    ['xl/charts/chart1.xml', HEADER + chartXml('Data')],
  ]);
}

/**
 * Files Excel must not open cleanly, one each way a save could go wrong. The open matrix runs
 * them first: a check that can't see these can't say a saved file had no repair prompt.
 */
export function brokenBooks() {
  const good = structuresBook();
  const parts = readZip(good);
  const change = (name, make) =>
    zip(parts.map(([part, body]) => [part, part === name ? make(body.toString('utf8')) : body]));
  const moving = readZip(movingPartsBook());
  const changeMoving = (make) =>
    zip(
      moving.map(([part, body]) => [
        part,
        part === 'xl/worksheets/sheet1.xml' ? make(body.toString('utf8')) : body,
      ]),
    );
  return {
    // A sparkline reads a range or `#REF!`, and Excel refuses a sheet name before the error.
    'sparkline-reads-sheet-and-error': changeMoving((xml) =>
      xml.replace('<xm:f>Data!B2:B5</xm:f>', '<xm:f>Data!#REF!</xm:f>'),
    ),
    // A frozen pane has to freeze something, which a save mustn't forget after a delete.
    'frozen-pane-split-zero': changeMoving((xml) => xml.replace('ySplit="1"', 'ySplit="0"')),
    'sparkline-group-with-none': changeMoving((xml) =>
      xml.replace(/<x14:sparkline>.*?<\/x14:sparkline>/, ''),
    ),
    'table-header-mismatch': change('xl/tables/table1.xml', (xml) =>
      xml.replace('name="First"', 'name="Not the header"'),
    ),
    'chain-names-a-constant': change('xl/calcChain.xml', (xml) =>
      xml.replace('<c r="C3"/>', '<c r="A2"/>'),
    ),
    'string-index-off-the-end': change('xl/worksheets/sheet1.xml', (xml) =>
      xml.replace('<c r="A1" t="s"><v>0</v></c>', '<c r="A1" t="s"><v>99</v></c>'),
    ),
    'rows-out-of-order': change('xl/worksheets/sheet1.xml', (xml) =>
      xml.replace('<row r="6">', '<row r="2">'),
    ),
    'elements-out-of-order': change('xl/worksheets/sheet1.xml', (xml) =>
      xml
        .replace('<mergeCells count="1"><mergeCell ref="D2:E3"/></mergeCells>', '')
        .replace(
          '</sheetData>',
          '</sheetData><tableParts count="0"/><mergeCells count="1"><mergeCell ref="D2:E3"/></mergeCells>',
        ),
    ),
    'style-index-off-the-end': change('xl/worksheets/sheet1.xml', (xml) =>
      xml.replace('<c r="A2"><v>1</v></c>', '<c r="A2" s="99"><v>1</v></c>'),
    ),
    'missing-content-type': change('[Content_Types].xml', (xml) =>
      xml.replace(
        '<Override PartName="/xl/tables/table1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"/>',
        '',
      ),
    ),
  };
}

/** Reads a zip this file wrote: (name, bytes) in order. */
export function readZip(bytes) {
  const parts = [];
  for (let at = 0; bytes.readUInt32LE(at) === 0x04034b50;) {
    const packed = bytes.readUInt32LE(at + 18);
    const nameLength = bytes.readUInt16LE(at + 26);
    const extra = bytes.readUInt16LE(at + 28);
    const name = bytes.toString('utf8', at + 30, at + 30 + nameLength);
    const start = at + 30 + nameLength + extra;
    parts.push([name, inflateRawSync(bytes.subarray(start, start + packed))]);
    at = start + packed;
  }
  return parts;
}
