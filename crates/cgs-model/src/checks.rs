//! Excel 2016's error checking rules, as bits, with `ignoredError`'s attribute for each.

/// A formula whose result is an error.
pub const EVALUATE_TO_ERROR: u16 = 1;
/// Text, or a formula's text, holding a date with a two-digit year.
pub const TEXT_DATE: u16 = 1 << 1;
/// A number kept as text, typed with an apostrophe or into a cell formatted as text.
pub const NUMBER_AS_TEXT: u16 = 1 << 2;
/// A formula unlike those on both sides of it, which agree with each other.
pub const INCONSISTENT_FORMULA: u16 = 1 << 3;
/// A formula whose range stops just short of more numbers.
pub const OMITTED_CELLS: u16 = 1 << 4;
/// A formula in an unlocked cell, which protection wouldn't keep.
pub const UNLOCKED_FORMULA: u16 = 1 << 5;
/// A formula reading an empty cell. Off unless chosen, as in Excel.
pub const EMPTY_REFERENCE: u16 = 1 << 6;

/// The rules Excel 2016 checks for unless told not to: all but empty references.
pub const DEFAULT_RULES: u16 = EVALUATE_TO_ERROR
    | TEXT_DATE
    | NUMBER_AS_TEXT
    | INCONSISTENT_FORMULA
    | OMITTED_CELLS
    | UNLOCKED_FORMULA;

/// Every rule, with `ignoredError`'s attribute for it, in the order Excel writes them.
pub const RULES: [(u16, &str); 7] = [
    (EVALUATE_TO_ERROR, "evalError"),
    (TEXT_DATE, "twoDigitTextYear"),
    (NUMBER_AS_TEXT, "numberStoredAsText"),
    (INCONSISTENT_FORMULA, "formula"),
    (OMITTED_CELLS, "formulaRange"),
    (UNLOCKED_FORMULA, "unlockedFormula"),
    (EMPTY_REFERENCE, "emptyCellReference"),
];
