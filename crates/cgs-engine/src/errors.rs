//! Excel 2016's background error checking: the rules behind the green triangle in a cell's corner,
//! as `Range.Errors` reported them over COM. Each rule is a bit; `ignoredErrors` in the sheet
//! turns bits off for its cells, and Options turns rules off everywhere.

use cgs_calc::{CalcEngine, CalcValue, CellInput, CellRef};
use cgs_model::{Book, Rect};

use crate::input::{parse_input, Typed};

pub use cgs_model::checks::*;

/// What a cell holds, for the rules.
enum Held {
    Nothing,
    Value(CalcValue),
    Formula(String),
}

fn held(calc: &dyn CalcEngine, at: CellRef) -> Held {
    match calc.cell_input(at) {
        CellInput::Formula { text, .. } => Held::Formula(text),
        CellInput::Value(value) => Held::Value(value),
        CellInput::Spill { .. } | CellInput::Clear => Held::Nothing,
    }
}

fn value(calc: &dyn CalcEngine, at: CellRef) -> Option<CalcValue> {
    struct One(Option<CalcValue>);
    impl cgs_calc::CellSink for One {
        fn cell(&mut self, _row: u32, _col: u32, value: &CalcValue) {
            self.0 = Some(value.clone());
        }
    }
    let mut one = One(None);
    calc.read_block(
        cgs_calc::SheetRect {
            sheet: at.sheet,
            rect: cgs_calc::Rect {
                top: at.row,
                left: at.col,
                bottom: at.row,
                right: at.col,
            },
        },
        &mut one,
    );
    one.0
}

/// A date written with a two-digit year, such as `1/1/25` or `1-Jan-25`.
fn two_digit_year(text: &str) -> bool {
    let parts: Vec<&str> = text
        .trim()
        .split(['/', '-', ' '])
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() != 3 {
        return false;
    }
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let month_name = |part: &str| part.len() >= 3 && part.bytes().all(|b| b.is_ascii_alphabetic());
    let year = parts[2];
    year.len() == 2
        && digits(year)
        && digits(parts[0])
        && (digits(parts[1]) || month_name(parts[1]))
}

/// The text inside a formula's string literals.
fn literals(formula: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = formula.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '"' {
            continue;
        }
        let mut text = String::new();
        while let Some(next) = chars.next() {
            if next == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    text.push('"');
                    continue;
                }
                break;
            }
            text.push(next);
        }
        out.push(text);
    }
    out
}

/// A formula's references to cells and ranges on its own sheet: `(top, left, bottom, right)`, and
/// whether each is a range.
fn references(formula: &str) -> Vec<((u32, u32, u32, u32), bool)> {
    let bytes = formula.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'$' || b == b'_' || b == b'.';
    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                at += 1;
                while at < bytes.len() && bytes[at] != b'"' {
                    at += 1;
                }
                at += 1;
            }
            b'\'' => {
                // A quoted sheet name: what follows is another sheet's.
                at += 1;
                while at < bytes.len() && bytes[at] != b'\'' {
                    at += 1;
                }
                at += 1;
                while at < bytes.len() && (word(bytes[at]) || matches!(bytes[at], b'!' | b':')) {
                    at += 1;
                }
            }
            b if word(b) => {
                let start = at;
                while at < bytes.len() && (word(bytes[at]) || bytes[at] == b':') {
                    at += 1;
                }
                let token = &formula[start..at];
                if bytes.get(at) == Some(&b'(') || bytes.get(at) == Some(&b'!') {
                    if bytes.get(at) == Some(&b'!') {
                        // Another sheet's reference.
                        at += 1;
                        while at < bytes.len() && (word(bytes[at]) || bytes[at] == b':') {
                            at += 1;
                        }
                    }
                    continue;
                }
                let clean = token.replace('$', "");
                if !clean.contains(':') {
                    if let Some((row, col)) = cgs_refs::parse_a1(&clean) {
                        out.push(((row, col, row, col), false));
                    }
                } else if let Some(((top, left), (bottom, right))) = cgs_refs::parse_range(&clean) {
                    out.push(((top, left, bottom, right), true));
                } else if let Some((row, col)) = cgs_refs::parse_a1(&clean) {
                    out.push(((row, col, row, col), false));
                }
            }
            _ => at += 1,
        }
    }
    out
}

fn r1c1(formula: &str, row: u32, col: u32) -> String {
    cgs_refs::SharedFormula::new(formula).r1c1_key(i64::from(row), i64::from(col))
}

/// Every rule a cell breaks, whether or not it is on or ignored.
pub fn cell_errors(
    book: &Book,
    calc: &dyn CalcEngine,
    options: &cgs_numfmt::Options<'_>,
    today: f64,
    at: (u32, u32, u32),
) -> u16 {
    rules_broken(book, calc, options, today, at, u16::MAX)
}

/// The rules among `wanted` a cell breaks, and perhaps others: a rule not wanted may go
/// unchecked, as reading a text as a number does, which costs the most.
fn rules_broken(
    book: &Book,
    calc: &dyn CalcEngine,
    options: &cgs_numfmt::Options<'_>,
    today: f64,
    (sheet, row, col): (u32, u32, u32),
    wanted: u16,
) -> u16 {
    let at = CellRef { sheet, row, col };
    let mut found = 0;
    match held(calc, at) {
        Held::Nothing => {}
        Held::Value(CalcValue::Text(text)) => {
            if two_digit_year(&text) {
                found |= TEXT_DATE;
            } else if wanted & NUMBER_AS_TEXT != 0 {
                let typed = parse_input(text.trim(), options.locale, options.date1904, today);
                let date = match &typed {
                    // Built-in date and time formats.
                    Typed::Formatted(_, crate::input::Format::Builtin(id)) => {
                        (14..=22).contains(id) || (45..=47).contains(id)
                    }
                    _ => false,
                };
                let number = matches!(
                    typed,
                    Typed::Value(CalcValue::Number(_)) | Typed::Formatted(..)
                );
                if number && !date && !text.trim().is_empty() {
                    found |= NUMBER_AS_TEXT;
                }
            }
        }
        Held::Value(_) => {}
        Held::Formula(formula) => {
            if matches!(value(calc, at), Some(CalcValue::Error(_))) {
                found |= EVALUATE_TO_ERROR;
            }
            if literals(&formula).iter().any(|text| two_digit_year(text)) {
                found |= TEXT_DATE;
            }
            let info = &book.sheets[sheet as usize];
            let locked = book
                .styles
                .xfs
                .get(info.style_at(row, col) as usize)
                .is_none_or(|xf| xf.locked);
            if !locked {
                found |= UNLOCKED_FORMULA;
            }
            // Unlike both neighbours along a row or a column, which agree.
            let key = r1c1(&formula, row, col);
            let neighbour = |r: u32, c: u32| match held(
                calc,
                CellRef {
                    sheet,
                    row: r,
                    col: c,
                },
            ) {
                Held::Formula(text) => Some(r1c1(&text, r, c)),
                _ => None,
            };
            let differs = |a: Option<String>, b: Option<String>| match (a, b) {
                (Some(a), Some(b)) => a == b && a != key,
                _ => false,
            };
            if (row > 1 && differs(neighbour(row - 1, col), neighbour(row + 1, col)))
                || (col > 1 && differs(neighbour(row, col - 1), neighbour(row, col + 1)))
            {
                found |= INCONSISTENT_FORMULA;
            }
            let number = |r: u32, c: u32| {
                (r, c) != (row, col)
                    && r >= 1
                    && c >= 1
                    && matches!(
                        held(
                            calc,
                            CellRef {
                                sheet,
                                row: r,
                                col: c
                            }
                        ),
                        Held::Value(CalcValue::Number(_))
                    )
            };
            for ((top, left, bottom, right), range) in references(&formula) {
                if range && left == right && bottom > top {
                    // A column of cells with more numbers just above or below it.
                    if (top > 1 && number(top - 1, left)) || number(bottom + 1, left) {
                        found |= OMITTED_CELLS;
                    }
                } else if range && top == bottom && right > left {
                    if (left > 1 && number(top, left - 1)) || number(top, right + 1) {
                        found |= OMITTED_CELLS;
                    }
                } else if !range
                    && matches!(
                        held(
                            calc,
                            CellRef {
                                sheet,
                                row: top,
                                col: left
                            }
                        ),
                        Held::Nothing
                    )
                {
                    found |= EMPTY_REFERENCE;
                }
            }
        }
    }
    found
}

/// The rules `ignoredErrors` turns off for a cell.
pub fn ignored(book: &Book, sheet: u32, row: u32, col: u32) -> u16 {
    book.sheets[sheet as usize]
        .ignored_errors
        .iter()
        .filter(|(areas, _)| areas.iter().any(|area: &Rect| area.contains(row, col)))
        .fold(0, |bits, (_, kinds)| bits | kinds)
}

/// What the green triangle shows for: the rules a cell breaks that are on and not ignored.
pub fn shown_errors(
    book: &Book,
    calc: &dyn CalcEngine,
    options: &cgs_numfmt::Options<'_>,
    today: f64,
    rules: u16,
    (sheet, row, col): (u32, u32, u32),
) -> u16 {
    if rules == 0 {
        return 0;
    }
    let wanted = rules & !ignored(book, sheet, row, col);
    if wanted == 0 {
        return 0;
    }
    rules_broken(book, calc, options, today, (sheet, row, col), wanted) & wanted
}

/// `<ignoredErrors>` for a sheet's list, or nothing when it has none.
pub fn ignored_xml(list: &[(Vec<Rect>, u16)]) -> String {
    if list.is_empty() {
        return String::new();
    }
    let mut out = String::from("<ignoredErrors>");
    for (areas, kinds) in list {
        let sqref: Vec<String> = areas
            .iter()
            .map(|area| {
                let first = cgs_xlsx::patch_sheet::cell_name(area.top, area.left);
                if area.top == area.bottom && area.left == area.right {
                    first
                } else {
                    format!(
                        "{first}:{}",
                        cgs_xlsx::patch_sheet::cell_name(area.bottom, area.right)
                    )
                }
            })
            .collect();
        out.push_str(&format!("<ignoredError sqref=\"{}\"", sqref.join(" ")));
        for (bit, name) in RULES {
            if kinds & bit != 0 {
                out.push_str(&format!(" {name}=\"1\""));
            }
        }
        out.push_str("/>");
    }
    out.push_str("</ignoredErrors>");
    out
}

/// A formula's ranges grown over the numbers just past their ends, for Update Formula to Include
/// Cells; nothing when no range has any.
pub fn include_adjacent(
    calc: &dyn CalcEngine,
    formula: &str,
    (sheet, row, col): (u32, u32, u32),
) -> Option<String> {
    let number = |r: u32, c: u32| {
        (r, c) != (row, col)
            && r >= 1
            && c >= 1
            && matches!(
                held(
                    calc,
                    CellRef {
                        sheet,
                        row: r,
                        col: c
                    }
                ),
                Held::Value(CalcValue::Number(_))
            )
    };
    let mut out = formula.to_string();
    let mut changed = false;
    for ((top, left, bottom, right), range) in references(formula) {
        if !range {
            continue;
        }
        let (mut t, mut l, mut b, mut r) = (top, left, bottom, right);
        if left == right && bottom > top {
            while t > 1 && number(t - 1, l) {
                t -= 1;
            }
            while number(b + 1, l) {
                b += 1;
            }
        } else if top == bottom && right > left {
            while l > 1 && number(t, l - 1) {
                l -= 1;
            }
            while number(t, r + 1) {
                r += 1;
            }
        }
        if (t, l, b, r) == (top, left, bottom, right) {
            continue;
        }
        let name = |row: u32, col: u32| cgs_xlsx::patch_sheet::cell_name(row, col);
        let old = format!("{}:{}", name(top, left), name(bottom, right));
        let new = format!("{}:{}", name(t, l), name(b, r));
        if out.contains(&old) {
            out = out.replacen(&old, &new, 1);
            changed = true;
        }
    }
    changed.then_some(out)
}

impl crate::Session {
    fn check_options<'a>(&self, locale: &'a cgs_numfmt::Locale) -> cgs_numfmt::Options<'a> {
        cgs_numfmt::Options {
            locale,
            date1904: self.book.date1904,
        }
    }

    fn shown_here(&self, sheet: u32, row: u32, col: u32, locale: &cgs_numfmt::Locale) -> u16 {
        if sheet as usize >= self.book.sheets.len() {
            return 0;
        }
        let options = self.check_options(locale);
        shown_errors(
            &self.book,
            &self.calc,
            &options,
            self.today,
            self.error_rules,
            (sheet, row, col),
        )
    }

    /// The errors a cell shows, as `[{ rule, value?, from? }]`.
    pub(crate) fn cell_errors_json(
        &self,
        sheet: u32,
        row: u32,
        col: u32,
        locale: &cgs_numfmt::Locale,
    ) -> serde_json::Value {
        let found = self.shown_here(sheet, row, col, locale);
        let names = [
            (EVALUATE_TO_ERROR, "evaluateToError"),
            (TEXT_DATE, "textDate"),
            (NUMBER_AS_TEXT, "numberAsText"),
            (INCONSISTENT_FORMULA, "inconsistentFormula"),
            (OMITTED_CELLS, "omittedCells"),
            (UNLOCKED_FORMULA, "unlockedFormula"),
            (EMPTY_REFERENCE, "emptyReference"),
        ];
        let at = CellRef { sheet, row, col };
        let formula = |r: u32| {
            matches!(
                held(&self.calc, CellRef { sheet, row: r, col }),
                Held::Formula(_)
            )
        };
        serde_json::Value::Array(
            names
                .iter()
                .filter(|(bit, _)| found & bit != 0)
                .map(|&(bit, name)| {
                    let mut entry = serde_json::json!({ "rule": name });
                    if bit == EVALUATE_TO_ERROR {
                        if let Some(CalcValue::Error(error)) = value(&self.calc, at) {
                            entry["value"] = serde_json::json!(error.to_string());
                        }
                    }
                    if bit == INCONSISTENT_FORMULA {
                        // From above when the column agrees around it, or else from the left.
                        let above = row > 1 && formula(row - 1) && formula(row + 1);
                        entry["from"] = serde_json::json!(if above { "above" } else { "left" });
                    }
                    entry
                })
                .collect(),
        )
    }

    /// See `Question::NextError`.
    pub(crate) fn next_error(
        &self,
        sheet: u32,
        row: u32,
        col: u32,
        back: bool,
        locale: &cgs_numfmt::Locale,
    ) -> Option<(u32, u32)> {
        let info = self.book.sheets.get(sheet as usize)?;
        let (rows, cols) = info.used;
        if rows == 0 || cols == 0 || self.error_rules == 0 {
            return None;
        }
        let total = u64::from(rows) * u64::from(cols);
        let index = |r: u32, c: u32| u64::from(r - 1) * u64::from(cols) + u64::from(c - 1);
        let start = if row >= 1 && col >= 1 && row <= rows && col <= cols {
            index(row, col)
        } else if back {
            0
        } else {
            total - 1
        };
        for step in 1..=total.min(4_000_000) {
            let at = if back {
                (start + total - step % total) % total
            } else {
                (start + step) % total
            };
            let (r, c) = (
                (at / u64::from(cols)) as u32 + 1,
                (at % u64::from(cols)) as u32 + 1,
            );
            if self.shown_here(sheet, r, c, locale) != 0 {
                return Some((r, c));
            }
        }
        None
    }

    /// Ignore Error, for the rules the cell shows now.
    pub(crate) fn ignore_error(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        locale: &cgs_numfmt::Locale,
    ) -> Result<crate::command::Applied, String> {
        let found = self.shown_here(sheet, row, col, locale);
        if found == 0 {
            return Ok(crate::command::Applied::default());
        }
        let mut list = self.book.sheets[sheet as usize].ignored_errors.clone();
        let cell = Rect {
            top: row,
            left: col,
            bottom: row,
            right: col,
        };
        list.push((vec![cell], found));
        self.perform(
            "ignoring an error",
            crate::command::Op::IgnoredErrors { sheet, list },
            Some((sheet, crate::command::Area::cell(row, col))),
        )
    }

    /// Convert to Number.
    pub(crate) fn convert_to_number(
        &mut self,
        sheet: u32,
        areas: &[crate::command::Area],
        locale: &cgs_numfmt::Locale,
    ) -> Result<crate::command::Applied, String> {
        self.sheet_for_edit(sheet)?;
        let options = self.check_options(locale);
        let used = self.book.sheets[sheet as usize].used;
        let mut edits = Vec::new();
        for area in areas {
            for r in area.top..=area.bottom.min(used.0) {
                for c in area.left..=area.right.min(used.1) {
                    let at = CellRef {
                        sheet,
                        row: r,
                        col: c,
                    };
                    let Held::Value(CalcValue::Text(text)) = held(&self.calc, at) else {
                        continue;
                    };
                    // ERP text fields hold numbers as text on purpose.
                    if self.bound_text_cell(sheet, r, c) {
                        continue;
                    }
                    let typed =
                        parse_input(text.trim(), options.locale, options.date1904, self.today);
                    let number = match typed {
                        Typed::Value(CalcValue::Number(n)) | Typed::Formatted(n, _) => n,
                        _ => continue,
                    };
                    edits.push(crate::edit::EditCell {
                        sheet,
                        row: r,
                        col: c,
                        input: Some(crate::edit::EditInput::Value(CalcValue::Number(number))),
                        style: None,
                    });
                }
            }
        }
        if edits.is_empty() {
            return Ok(crate::command::Applied::default());
        }
        let first = areas.first().map(|&area| (sheet, area));
        self.perform(
            "converting to numbers",
            crate::command::Op::Cells(edits),
            first,
        )
    }

    /// Update Formula to Include Cells.
    pub(crate) fn include_cells(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        locale: &cgs_numfmt::Locale,
    ) -> Result<crate::command::Applied, String> {
        self.sheet_for_edit(sheet)?;
        let Held::Formula(formula) = held(&self.calc, CellRef { sheet, row, col }) else {
            return Ok(crate::command::Applied::default());
        };
        let Some(next) = include_adjacent(&self.calc, &formula, (sheet, row, col)) else {
            return Ok(crate::command::Applied::default());
        };
        self.command(
            crate::command::Command::Input {
                sheet,
                row,
                col,
                text: format!("={next}"),
                areas: Vec::new(),
                force: true,
            },
            locale,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_two_digit_years_and_references() {
        assert!(two_digit_year("1/1/25"));
        assert!(two_digit_year("1-Jan-25"));
        assert!(!two_digit_year("1/1/2025"));
        assert!(!two_digit_year("abc"));
        assert_eq!(
            literals(r#"DATEVALUE("1/1/25")&"a""b""#),
            ["1/1/25", "a\"b"]
        );
        assert_eq!(
            references("SUM(J2:J3)+A1+Sheet2!B5+'My sheet'!C3"),
            [((2, 10, 3, 10), true), ((1, 1, 1, 1), false)]
        );
    }
}
