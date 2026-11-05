// The number formats the ribbon and the Format Cells dialog offer. Codes are as a file stores them,
// and the engine turns ones with a built-in id into that id.

export type NumberCategory =
  | 'General'
  | 'Number'
  | 'Currency'
  | 'Accounting'
  | 'Date'
  | 'Time'
  | 'Percentage'
  | 'Fraction'
  | 'Scientific'
  | 'Text'
  | 'Special'
  | 'Custom';

export const NUMBER_CATEGORIES: NumberCategory[] = [
  'General',
  'Number',
  'Currency',
  'Accounting',
  'Date',
  'Time',
  'Percentage',
  'Fraction',
  'Scientific',
  'Text',
  'Special',
  'Custom',
];

/** Symbols the Currency and Accounting lists offer, with the currency each is the mark of. */
export const CURRENCIES: { label: string; symbol: string }[] = [
  { label: '$ (US dollar)', symbol: '$' },
  { label: '€ (euro)', symbol: '€' },
  { label: '£ (pound)', symbol: '£' },
  { label: '¥ (yen)', symbol: '¥' },
  { label: 'ر.س (Saudi riyal)', symbol: '[$ر.س.-401]' },
  { label: 'د.إ (UAE dirham)', symbol: '[$د.إ.-3801]' },
  { label: 'ج.م (Egyptian pound)', symbol: '[$ج.م.-C01]' },
  { label: 'د.ك (Kuwaiti dinar)', symbol: '[$د.ك.-3401]' },
  { label: 'None', symbol: '' },
];

const zeros = (places: number) => (places > 0 ? '.' + '0'.repeat(places) : '');

function quoted(symbol: string) {
  if (!symbol) return '';
  return symbol.startsWith('[') ? symbol : '"' + symbol + '"';
}

/** A number code from the dialog's choices. */
export function numberCode(places: number, grouping: boolean, negative: number): string {
  const body = (grouping ? '#,##0' : '0') + zeros(places);
  switch (negative) {
    case 1:
      return body + ';[Red]' + body;
    case 2:
      return body + '_);\\(' + body + '\\)';
    case 3:
      return body + '_);[Red]\\(' + body + '\\)';
    default:
      return body;
  }
}

export function currencyCode(places: number, symbol: string, negative: number): string {
  const mark = quoted(symbol);
  const body = mark + '#,##0' + zeros(places);
  switch (negative) {
    case 1:
      return body + ';[Red]' + body;
    case 2:
      return body + '_);\\(' + body + '\\)';
    case 3:
      return body + '_);[Red]\\(' + body + '\\)';
    default:
      return body + ';-' + body;
  }
}

export function accountingCode(places: number, symbol: string): string {
  const mark = quoted(symbol);
  const star = mark ? '_(' + mark + '* ' : '_(* ';
  const digits = '#,##0' + zeros(places);
  const dash = places > 0 ? '"-"' + '?'.repeat(places) : '"-"';
  return `${star}${digits}_);${star}\\(${digits}\\);${star}${dash}_);_(@_)`;
}

export const DATE_CODES = [
  'm/d/yyyy',
  '[$-F800]dddd\\, mmmm dd\\, yyyy',
  'm/d;@',
  'm/d/yy;@',
  'mm/dd/yy;@',
  'd-mmm',
  'd-mmm-yy',
  'dd-mmm-yy;@',
  'mmm-yy',
  'mmmm-yy;@',
  'mmmm d\\, yyyy;@',
  'yyyy-mm-dd;@',
  '[$-1060000]d/m/yyyy;@',
  '[$-1060401]dddd\\, d mmmm yyyy;@',
];

export const TIME_CODES = [
  '[$-F400]h:mm:ss AM/PM',
  'h:mm;@',
  'h:mm AM/PM',
  'h:mm:ss',
  'h:mm:ss AM/PM',
  'mm:ss',
  'mm:ss.0',
  '[h]:mm:ss',
  'm/d/yyyy h:mm',
];

export const FRACTION_CODES = [
  '# ?/?',
  '# ??/??',
  '# ???/???',
  '# ?/2',
  '# ?/4',
  '# ?/8',
  '# ??/16',
  '# ?/10',
  '# ??/100',
];

export const SPECIAL_CODES = [
  '00000',
  '00000-0000',
  '[<=9999999]###-####;\\(###\\) ###-####',
  '000-00-0000',
];

/** What the Custom list always offers, as Excel's does. */
export const BUILT_IN_CODES = [
  'General',
  '0',
  '0.00',
  '#,##0',
  '#,##0.00',
  '#,##0_);\\(#,##0\\)',
  '#,##0_);[Red]\\(#,##0\\)',
  '#,##0.00_);\\(#,##0.00\\)',
  '#,##0.00_);[Red]\\(#,##0.00\\)',
  '"$"#,##0_);\\("$"#,##0\\)',
  '"$"#,##0_);[Red]\\("$"#,##0\\)',
  '"$"#,##0.00_);\\("$"#,##0.00\\)',
  '"$"#,##0.00_);[Red]\\("$"#,##0.00\\)',
  '0%',
  '0.00%',
  '0.00E+00',
  '##0.0E+0',
  '# ?/?',
  '# ??/??',
  'm/d/yyyy',
  'd-mmm-yy',
  'd-mmm',
  'mmm-yy',
  'h:mm AM/PM',
  'h:mm:ss AM/PM',
  'h:mm',
  'h:mm:ss',
  'm/d/yyyy h:mm',
  'mm:ss',
  'mm:ss.0',
  '@',
  '[h]:mm:ss',
  '_("$"* #,##0_);_("$"* \\(#,##0\\);_("$"* "-"_);_(@_)',
  '_(* #,##0_);_(* \\(#,##0\\);_(* "-"_);_(@_)',
  '_("$"* #,##0.00_);_("$"* \\(#,##0.00\\);_("$"* "-"??_);_(@_)',
  '_(* #,##0.00_);_(* \\(#,##0.00\\);_(* "-"??_);_(@_)',
];

/** What the ribbon's list offers, with each code's name. */
export const RIBBON_FORMATS: { name: string; code: string }[] = [
  { name: 'General', code: 'General' },
  { name: 'Number', code: '0.00' },
  { name: 'Currency', code: '"$"#,##0.00_);[Red]\\("$"#,##0.00\\)' },
  { name: 'Accounting', code: '_("$"* #,##0.00_);_("$"* \\(#,##0.00\\);_("$"* "-"??_);_(@_)' },
  { name: 'Short Date', code: 'm/d/yyyy' },
  { name: 'Long Date', code: '[$-F800]dddd\\, mmmm dd\\, yyyy' },
  { name: 'Time', code: '[$-F400]h:mm:ss AM/PM' },
  { name: 'Percentage', code: '0.00%' },
  { name: 'Fraction', code: '# ?/?' },
  { name: 'Scientific', code: '0.00E+00' },
  { name: 'Text', code: '@' },
];

/** Which list of the dialog a code belongs to, and so which it opens on. */
export function categoryOf(code: string): NumberCategory {
  if (code === 'General') return 'General';
  if (code === '@') return 'Text';
  if (DATE_CODES.includes(code) || code === 'd-mmm-yy' || code === 'mmm-yy') return 'Date';
  if (TIME_CODES.includes(code)) return 'Time';
  if (FRACTION_CODES.includes(code)) return 'Fraction';
  if (SPECIAL_CODES.includes(code)) return 'Special';
  if (/^0(\.0+)?E\+0+$/.test(code)) return 'Scientific';
  if (/^0(\.0+)?%$/.test(code)) return 'Percentage';
  if (code.startsWith('_(')) return 'Accounting';
  if (/^(#,##)?0(\.0+)?(;\[Red\]|_\);(\[Red\])?\\\(|$)/.test(code)) return 'Number';
  if (/^("[^"]+"|\[\$[^\]]+\])#,##0(\.0+)?/.test(code)) return 'Currency';
  return 'Custom';
}

/** How many places a code shows. */
export function placesOf(code: string): number {
  const first = code.split(';')[0] ?? '';
  const match = /\.(0+)/.exec(first);
  return match ? match[1]!.length : 0;
}

/** What the ribbon's list shows for a code. */
export function ribbonFormatName(code: string): string {
  const known = RIBBON_FORMATS.find((format) => format.code === code);
  if (known) return known.name;
  const category = categoryOf(code);
  return category === 'Custom' ? 'Custom' : category;
}
