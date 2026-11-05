// A workbook that shows what formatting can do, made by the engine's own commands from a blank one.
// The golden images draw it, and bench/run-visual-review.mjs has Excel draw the same file to compare.

const area = (top, left, bottom = top, right = left) => ({ top, left, bottom, right });
const sheet = 0;
const input = (row, col, text) => ({ kind: 'input', sheet, row, col, text });
const format = (areas, change) => ({ kind: 'format', sheet, areas, change });
const rgb = (hex) => ({ kind: 'rgb', rgb: hex });
const theme = (index, tint = 0) => ({ kind: 'theme', theme: index, tint });

const PATTERNS = [
  'darkGray',
  'mediumGray',
  'lightGray',
  'gray125',
  'gray0625',
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

export const SHOWCASE_RANGE = 'A1:N22';

export function showcaseCommands() {
  const list = [];
  const push = (...commands) => list.push(...commands);
  // A title across the top.
  push(input(1, 1, 'Formatting showcase'));
  push({ kind: 'merge', sheet, areas: [area(1, 1, 1, 6)], center: true });
  push(
    format([area(1, 1, 1, 6)], {
      bold: true,
      fontSize: 14,
      fill: { pattern: 'solid', color: theme(4) },
      fontColor: theme(0),
    }),
  );
  // Fonts.
  const fonts = [
    ['Calibri', {}],
    ['Arial', { fontName: 'Arial' }],
    ['Times', { fontName: 'Times New Roman' }],
    ['Courier', { fontName: 'Courier New' }],
    ['Bold', { bold: true }],
    ['Italic', { italic: true }],
    ['Under', { underline: 'single' }],
    ['Double', { underline: 'double' }],
    ['Strike', { strike: true }],
    ['Red', { fontColor: rgb('FF0000') }],
    ['Accent 2', { fontColor: theme(5, -0.25) }],
    ['Bold italic', { bold: true, italic: true }],
  ];
  fonts.forEach(([text, change], index) => {
    push(input(2, index + 1, text));
    if (Object.keys(change).length) push(format([area(2, index + 1)], change));
  });
  // Sizes: the row grows to the biggest.
  [8, 11, 14, 18, 24].forEach((size, index) => {
    push(input(3, index + 1, String(size)));
    push(format([area(3, index + 1)], { fontSize: size }));
  });
  // Solid fills, then every pattern.
  push(input(4, 1, 'Yellow'));
  push(format([area(4, 1)], { fill: { pattern: 'solid', color: rgb('FFFF00') } }));
  push(input(4, 2, 'Accent 1'));
  push(format([area(4, 2)], { fill: { pattern: 'solid', color: theme(4, 0.6) } }));
  PATTERNS.forEach((pattern, index) => {
    const row = index < 11 ? 4 : 5;
    const col = index < 11 ? index + 3 : index - 10;
    push(
      format([area(row, col)], {
        fill: { pattern, color: rgb('FFFFFF'), patternColor: rgb('1F4E79') },
      }),
    );
  });
  // Lines, one style a cell, all round.
  LINES.forEach((style, index) => {
    const at = area(7, index + 1);
    push(input(7, index + 1, style.slice(0, 7)));
    push(
      format([at], {
        border: {
          top: { style },
          bottom: { style },
          left: { style },
          right: { style },
        },
      }),
    );
  });
  // Alignment.
  push({ kind: 'lines', sheet, axis: 'row', first: 9, last: 9, size: 64 });
  const aligned = [
    ['Left', { horizontal: 'left' }],
    ['Centre', { horizontal: 'center' }],
    ['Right', { horizontal: 'right' }],
    ['Top', { vertical: 'top' }],
    ['Middle', { vertical: 'center' }],
    ['Wrapped text that goes on', { wrap: true }],
    ['Indent', { horizontal: 'left', indent: 2 }],
    ['Angle', { rotation: 45 }],
    ['Up', { rotation: 90 }],
    ['Down', { rotation: 180 }],
    ['Stack', { rotation: 255 }],
    ['Shrink this to fit', { shrink: true }],
    ['Right to left', { readingOrder: 2, horizontal: 'right' }],
  ];
  aligned.forEach(([text, change], index) => {
    push(input(9, index + 1, text));
    push(format([area(9, index + 1)], change));
  });
  // Merges.
  push(input(11, 1, 'Merged and centred'));
  push({ kind: 'merge', sheet, areas: [area(11, 1, 12, 3)], center: true });
  push(
    format([area(11, 1, 12, 3)], {
      vertical: 'center',
      border: {
        top: { style: 'thick' },
        bottom: { style: 'thick' },
        left: { style: 'thick' },
        right: { style: 'thick' },
      },
    }),
  );
  push(input(11, 5, 'Across'));
  push({ kind: 'merge', sheet, areas: [area(11, 5, 12, 7)], across: true });
  push(format([area(11, 5, 12, 7)], { fill: { pattern: 'solid', color: theme(9, 0.8) } }));
  push(input(11, 9, 'Centred across a selection'));
  push(format([area(11, 9, 11, 12)], { horizontal: 'centerContinuous' }));
  // Numbers.
  const numbers = [
    ['1234.5', { numberFormatId: 4 }],
    ['-1234.5', { numberFormatId: 8 }],
    ['0.125', { numberFormat: '0.0%' }],
    ['45678', { numberFormatId: 15 }],
    ['0.75', { numberFormatId: 18 }],
    ['1.75', { numberFormat: '# ?/?' }],
    ['123456789', { numberFormatId: 11 }],
    ['1234.5', { numberFormatId: 44 }],
    ['-12', { numberFormat: '[Blue]0.0;[Red]-0.0' }],
    ['007', { numberFormatId: 49 }],
    ['5', { numberFormat: '0.000" kg"' }],
  ];
  numbers.forEach(([text, change], index) => {
    push(format([area(13, index + 1)], change));
    push(input(13, index + 1, text));
  });
  // A block with lines by where each cell sits, a diagonal, and a dark fill with automatic text.
  push(input(15, 1, 'Q1'), input(15, 2, 'Q2'), input(16, 1, '10'), input(16, 2, '20'));
  push(input(17, 1, '30'), input(17, 2, '40'));
  push(
    format([area(15, 1, 17, 3)], {
      border: {
        top: { style: 'medium', color: rgb('C00000') },
        bottom: { style: 'double' },
        left: { style: 'medium', color: rgb('C00000') },
        right: { style: 'medium', color: rgb('C00000') },
        insideHorizontal: { style: 'thin' },
        insideVertical: { style: 'dotted' },
      },
    }),
  );
  push(
    format([area(15, 1, 15, 3)], { bold: true, fill: { pattern: 'solid', color: theme(3, 0.8) } }),
  );
  push(
    format([area(15, 5)], {
      border: { diagonal: { style: 'thin' }, diagonalDown: true, diagonalUp: true },
    }),
  );
  push(input(15, 7, 'On black'));
  push(
    format([area(15, 7)], {
      fill: { pattern: 'solid', color: rgb('000000') },
      fontColor: theme(0),
    }),
  );
  // Sizes and a hidden row and column.
  push({ kind: 'lines', sheet, axis: 'col', first: 6, last: 6, size: 110 });
  push({ kind: 'lines', sheet, axis: 'col', first: 14, last: 14, hidden: true });
  push({ kind: 'lines', sheet, axis: 'row', first: 18, last: 18, hidden: true });
  push(input(19, 1, 'After the hidden row'));
  // The numbers above are wide, so their column fits them.
  push({ kind: 'lines', sheet, axis: 'col', first: 8, last: 8, size: 96 });
  return list;
}
