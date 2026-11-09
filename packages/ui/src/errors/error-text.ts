// Excel 2016's words for each error checking rule: the title its smart tag and Error Checking
// dialog show, the description under it, and the fix it offers.

export type ErrorRule =
  | 'evaluateToError'
  | 'textDate'
  | 'numberAsText'
  | 'inconsistentFormula'
  | 'omittedCells'
  | 'unlockedFormula'
  | 'emptyReference';

/** What a cell's error says, from `cellErrors`. */
export interface CellError {
  rule: ErrorRule;
  /** For an error result: `#DIV/0!` and the rest. */
  value?: string;
  /** For an inconsistent formula: which neighbour it would copy, as the fix says. */
  from?: 'above' | 'left';
}

/** The rules as Options lists them, in Excel's order. */
export const RULE_LABELS: ReadonlyArray<[ErrorRule, string]> = [
  ['evaluateToError', 'Cells containing formulas that result in an error'],
  ['textDate', 'Cells containing years represented as 2 digits'],
  ['numberAsText', 'Numbers formatted as text or preceded by an apostrophe'],
  ['inconsistentFormula', 'Formulas inconsistent with other formulas in the region'],
  ['omittedCells', 'Formulas which omit cells in a region'],
  ['unlockedFormula', 'Unlocked cells containing formulas'],
  ['emptyReference', 'Formulas referring to empty cells'],
];

const VALUE_ERRORS: Record<string, [string, string]> = {
  '#DIV/0!': [
    'Divide by Zero Error',
    'The formula or function used is dividing by zero or empty cells.',
  ],
  '#N/A': ['Value Not Available Error', 'A value is not available to the formula or function.'],
  '#NAME?': ['Invalid Name Error', 'The formula contains unrecognized text.'],
  '#NULL!': ['Null Error', 'The ranges in the formula do not intersect.'],
  '#NUM!': ['Invalid Number Error', 'The formula or function contains invalid numeric values.'],
  '#REF!': [
    'Invalid Cell Reference Error',
    'The formula refers to cells that have been deleted or replaced.',
  ],
  '#VALUE!': ['Error in Value', 'A value used in the formula is of the wrong data type.'],
};

/** The title and description Excel shows for an error. */
export function errorText(error: CellError): { title: string; text: string } {
  switch (error.rule) {
    case 'evaluateToError': {
      const [title, text] = VALUE_ERRORS[error.value ?? ''] ?? [
        'Error in Value',
        'A value used in the formula is of the wrong data type.',
      ];
      return { title, text };
    }
    case 'textDate':
      return {
        title: 'Text Date with 2-Digit Year',
        text: 'The cell contains a text date with a 2-digit year.',
      };
    case 'numberAsText':
      return {
        title: 'Number Stored as Text',
        text: 'The number in this cell is formatted as text or preceded by an apostrophe.',
      };
    case 'inconsistentFormula':
      return {
        title: 'Inconsistent Formula',
        text: 'The formula in this cell differs from the formulas in this area of the spreadsheet.',
      };
    case 'omittedCells':
      return {
        title: 'Formula Omits Adjacent Cells',
        text: 'The formula in this cell refers to a range that has additional numbers adjacent to it.',
      };
    case 'unlockedFormula':
      return {
        title: 'Unprotected Formula',
        text: 'The formula in this cell is not locked to protect it from being changed inadvertently.',
      };
    case 'emptyReference':
      return {
        title: 'Formula Refers to Empty Cells',
        text: 'The formula in this cell refers to empty cells.',
      };
  }
}

/** The fix Excel offers first, if there is one. */
export function errorFix(error: CellError): string | null {
  switch (error.rule) {
    case 'numberAsText':
      return 'Convert to Number';
    case 'textDate':
      return 'Convert XX to 20XX';
    case 'inconsistentFormula':
      return error.from === 'left' ? 'Copy Formula from Left' : 'Copy Formula from Above';
    case 'omittedCells':
      return 'Update Formula to Include Cells';
    case 'unlockedFormula':
      return 'Lock Cell';
    default:
      return null;
  }
}
