//! The fill handle: dragging a selection out over more cells. Copies formulas and formats, and
//! carries on what the cells look like a series of, as Excel's AutoFill does. The rules here were
//! read off Excel: a source splits into runs of the same kind of cell, and each run counts on by
//! itself, once for each time the source repeats.

use cgs_calc::{CalcEngine, CalcValue, CellInput, CellRef};

use crate::command::{Area, PasteMode};
use crate::edit::{EditCell, EditInput};
use crate::Session;

const DAYS: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];
const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    Lower,
    Upper,
    Title,
}

/// What kind of series a source cell can be part of.
#[derive(Debug, Clone, PartialEq)]
enum Kind {
    /// Copied as it is: a formula shifts, and anything else stays.
    Fixed,
    Number(f64),
    /// `Item 7`: text and then a number that counts on.
    Counted {
        prefix: String,
        number: i64,
        width: usize,
    },
    /// `1st`, `2nd`.
    Ordinal(i64),
    /// A day or month name, in a list that wraps.
    Named {
        list: &'static [&'static str],
        short: bool,
        case: Case,
        index: i64,
    },
}

impl Kind {
    /// Whether two cells side by side belong to one run.
    fn runs_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(_), Self::Number(_)) | (Self::Ordinal(_), Self::Ordinal(_)) => true,
            (Self::Counted { prefix: a, .. }, Self::Counted { prefix: b, .. }) => a == b,
            (
                Self::Named {
                    list: a, short: c, ..
                },
                Self::Named {
                    list: b, short: d, ..
                },
            ) => std::ptr::eq(*a, *b) && c == d,
            _ => false,
        }
    }
}

fn case_of(text: &str) -> Case {
    if text.chars().all(char::is_uppercase) {
        Case::Upper
    } else if text.chars().next().is_some_and(char::is_uppercase) {
        Case::Title
    } else {
        Case::Lower
    }
}

fn named(text: &str) -> Option<Kind> {
    let lower = text.to_lowercase();
    for list in [&DAYS[..], &MONTHS[..]] {
        for (index, name) in list.iter().enumerate() {
            if *name == lower {
                return Some(Kind::Named {
                    list,
                    short: false,
                    case: case_of(text),
                    index: index as i64,
                });
            }
            if name.get(..3) == Some(lower.as_str()) {
                return Some(Kind::Named {
                    list,
                    short: true,
                    case: case_of(text),
                    index: index as i64,
                });
            }
        }
    }
    None
}

fn name_text(list: &[&str], short: bool, case: Case, index: i64) -> String {
    let name = list[index.rem_euclid(list.len() as i64) as usize];
    let name = if short { &name[..3] } else { name };
    match case {
        Case::Lower => name.to_string(),
        Case::Upper => name.to_uppercase(),
        Case::Title => {
            let mut chars = name.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    }
}

fn ordinal_text(number: i64) -> String {
    let suffix = match (number % 10, number % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{number}{suffix}")
}

/// Reads a cell's text for what it can count on from.
fn text_kind(text: &str) -> Kind {
    if let Some(kind) = named(text) {
        return kind;
    }
    let lower = text.to_ascii_lowercase();
    for suffix in ["st", "nd", "rd", "th"] {
        if let Some(digits) = lower.strip_suffix(suffix) {
            if let Ok(number) = digits.parse::<i64>() {
                if ordinal_text(number) == lower {
                    return Kind::Ordinal(number);
                }
            }
        }
    }
    // A number at the very end. `5x` has none, and copies.
    let bytes = text.as_bytes();
    let digits = bytes
        .iter()
        .rev()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digits == 0 || digits == text.len() {
        return Kind::Fixed;
    }
    let split = text.len() - digits;
    let Ok(number) = text[split..].parse() else {
        return Kind::Fixed;
    };
    Kind::Counted {
        prefix: text[..split].to_string(),
        number,
        width: digits,
    }
}

fn kind_of(input: &CellInput) -> Kind {
    match input {
        CellInput::Value(CalcValue::Number(number)) => Kind::Number(*number),
        CellInput::Value(CalcValue::Text(text)) => text_kind(text),
        _ => Kind::Fixed,
    }
}

/// A run's value `steps` places on from its first cell.
fn run_value(
    run: &[Kind],
    steps: i64,
    lone_number_counts: bool,
    lone_date: bool,
) -> Option<EditInput> {
    let count = run.len() as i64;
    // Whole-number kinds: where it starts and by how much it goes up.
    let int_step = |values: Vec<i64>| -> (i64, i64) {
        let step = if values.len() > 1 {
            values[1] - values[0]
        } else {
            1
        };
        (values[0], step)
    };
    Some(EditInput::Value(match &run[0] {
        Kind::Fixed => return None,
        Kind::Number(first) => {
            if count == 1 {
                if !(lone_number_counts || lone_date) {
                    return None;
                }
                CalcValue::Number(first + steps as f64)
            } else {
                let numbers: Vec<f64> = run
                    .iter()
                    .map(|kind| match kind {
                        Kind::Number(number) => *number,
                        _ => 0.0,
                    })
                    .collect();
                // The straight line that fits them best, which is the series when they make one.
                let total = numbers.len() as f64;
                let mean_x = (total - 1.0) / 2.0;
                let mean_y = numbers.iter().sum::<f64>() / total;
                let mut above = 0.0;
                let mut below = 0.0;
                for (index, number) in numbers.iter().enumerate() {
                    let x = index as f64 - mean_x;
                    above += x * (number - mean_y);
                    below += x * x;
                }
                let slope = above / below;
                CalcValue::Number(mean_y + slope * (steps as f64 - mean_x))
            }
        }
        Kind::Counted { prefix, width, .. } => {
            let values: Vec<i64> = run
                .iter()
                .map(|kind| match kind {
                    Kind::Counted { number, .. } => *number,
                    _ => 0,
                })
                .collect();
            let (start, step) = int_step(values);
            let number = start + step * steps;
            let digits = if number < 0 {
                number.to_string()
            } else {
                format!("{number:0width$}", width = *width)
            };
            CalcValue::Text(format!("{prefix}{digits}"))
        }
        Kind::Ordinal(_) => {
            let values: Vec<i64> = run
                .iter()
                .map(|kind| match kind {
                    Kind::Ordinal(number) => *number,
                    _ => 0,
                })
                .collect();
            let (start, step) = int_step(values);
            CalcValue::Text(ordinal_text(start + step * steps))
        }
        Kind::Named {
            list, short, case, ..
        } => {
            let values: Vec<i64> = run
                .iter()
                .map(|kind| match kind {
                    Kind::Named { index, .. } => *index,
                    _ => 0,
                })
                .collect();
            let (start, step) = int_step(values);
            CalcValue::Text(name_text(list, *short, *case, start + step * steps))
        }
    }))
}

/// A source line split into runs of the same kind of cell, as (start, end).
fn runs_of(kinds: &[Kind]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = 0;
    for at in 1..=kinds.len() {
        let joined = at < kinds.len() && kinds[at - 1].runs_with(&kinds[at]);
        if !joined {
            runs.push((start, at));
            start = at;
        }
    }
    runs
}

/// What the cell `offset` places from the source's start holds, if it counts on from the source.
/// `None` copies from the matching source cell. `lone_date` says a single cell is a date.
fn series_value(kinds: &[Kind], offset: i64, lone_date: bool) -> Option<EditInput> {
    let length = kinds.len() as i64;
    let position = offset.rem_euclid(length) as usize;
    let cycle = offset.div_euclid(length);
    let (start, end) = runs_of(kinds)
        .into_iter()
        .find(|&(start, end)| position >= start && position < end)?;
    let run = &kinds[start..end];
    let steps = (position - start) as i64 + cycle * run.len() as i64;
    // A lone number copies if it is all there is, and counts if it sits among other things.
    run_value(run, steps, length > 1, lone_date)
}

impl Session {
    /// The edits that drag `from` out over `to`. Dragging back inside it empties what it leaves.
    pub(crate) fn fill_edits(
        &self,
        sheet: u32,
        from: Area,
        to: Area,
        copy: bool,
    ) -> Result<Vec<EditCell>, String> {
        let covers = to.top <= from.top
            && to.left <= from.left
            && to.bottom >= from.bottom
            && to.right >= from.right;
        let mut edits = Vec::new();
        if !covers {
            // Dragged back: what falls outside `to` goes.
            for row in from.top..=from.bottom {
                for col in from.left..=from.right {
                    let inside =
                        row >= to.top && row <= to.bottom && col >= to.left && col <= to.right;
                    if !inside {
                        edits.push(EditCell {
                            sheet,
                            row,
                            col,
                            input: Some(EditInput::Clear),
                            style: None,
                        });
                    }
                }
            }
            return Ok(edits);
        }
        let down = to.rows() != from.rows();
        let across = to.cols() != from.cols();
        if down && across {
            return Err("A fill goes in one direction at a time.".to_string());
        }
        let info = &self.book.sheets[sheet as usize];
        // Each column going down, or each row going across, is a series of its own.
        let (lines, length, extent, line_count) = if down {
            (
                from.left..=from.right,
                from.rows(),
                (to.top, to.bottom),
                from.cols(),
            )
        } else {
            (
                from.top..=from.bottom,
                from.cols(),
                (to.left, to.right),
                from.rows(),
            )
        };
        let first = if down { from.top } else { from.left };
        // Lone numbers across several lines copy if the source is all numbers, and count on if
        // anything else is in it, a formula or an empty cell too. Read off Excel 2016.
        let all_numbers = (from.top..=from.bottom).all(|row| {
            (from.left..=from.right).all(|col| {
                matches!(
                    self.calc.cell_input(CellRef { sheet, row, col }),
                    CellInput::Value(CalcValue::Number(_))
                )
            })
        });
        for line in lines {
            let at = |offset: u32| {
                if down {
                    (first + offset, line)
                } else {
                    (line, first + offset)
                }
            };
            let kinds: Vec<Kind> = (0..length)
                .map(|offset| {
                    let (row, col) = at(offset);
                    kind_of(&self.calc.cell_input(CellRef { sheet, row, col }))
                })
                .collect();
            let (row, col) = at(0);
            let num_fmt = self.book.styles.xf(info.style_at(row, col)).num_fmt;
            let lone_date = self.is_date_format(num_fmt);
            let lone = length == 1 && line_count > 1;
            let copies = copy || (lone && all_numbers);
            for position in extent.0..=extent.1 {
                let offset = i64::from(position) - i64::from(first);
                if offset >= 0 && offset < i64::from(length) {
                    continue;
                }
                let (row, col) = if down {
                    (position, line)
                } else {
                    (line, position)
                };
                let source = offset.rem_euclid(i64::from(length)) as u32;
                let (source_row, source_col) = at(source);
                let counted = if copies {
                    None
                } else if lone {
                    match kinds[0] {
                        Kind::Number(number) => {
                            Some(EditInput::Value(CalcValue::Number(number + offset as f64)))
                        }
                        _ => None,
                    }
                } else {
                    series_value(&kinds, offset, lone_date)
                };
                let input = match self.copied_array_cell(
                    sheet,
                    from,
                    (source_row, source_col),
                    (row, col),
                    to,
                ) {
                    Some(input) => input,
                    None => counted.or_else(|| {
                        let cols = i64::from(col) - i64::from(source_col);
                        let input = self.copied_input(
                            sheet,
                            source_row,
                            source_col,
                            i64::from(row) - i64::from(source_row),
                            cols,
                            PasteMode::All,
                        );
                        // Across, a table's columns move along as well, as Excel's fill does.
                        match input {
                            Some(EditInput::Formula(text)) if across => Some(EditInput::Formula(
                                self.fill_table_columns(sheet, source_row, source_col, &text, cols),
                            )),
                            other => other,
                        }
                    }),
                };
                edits.push(EditCell {
                    sheet,
                    row,
                    col,
                    input,
                    style: Some(info.style_at(source_row, source_col)),
                });
            }
        }
        Ok(edits)
    }

    /// `text` with its references to one column of a table moved `cols` along that table.
    fn fill_table_columns(&self, sheet: u32, row: u32, col: u32, text: &str, cols: i64) -> String {
        let tables = self.calc.tables();
        let home = tables
            .iter()
            .find(|table| table.sheet == sheet && table.range.contains(row, col))
            .map(|table| table.name.as_str());
        let list: Vec<cgs_refs::TableColumns<'_>> = tables
            .iter()
            .map(|table| cgs_refs::TableColumns {
                name: &table.name,
                columns: &table.columns,
            })
            .collect();
        cgs_refs::fill_table_columns(text, cols, &list, home).into_owned()
    }

    fn is_date_format(&self, num_fmt: u32) -> bool {
        match self.book.styles.num_fmts.get(&num_fmt) {
            Some(code) => cgs_numfmt::NumFmt::parse(code).is_date(),
            None => cgs_numfmt::NumFmt::builtin(num_fmt).is_some_and(|format| format.is_date()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(text: &str) -> Kind {
        match text.parse::<f64>() {
            Ok(number) => Kind::Number(number),
            Err(_) => text_kind(text),
        }
    }

    /// What dragging these down gives. Each line was read off Excel 2016 over COM.
    fn filled(source: &[&str], total: i64) -> Vec<String> {
        let kinds: Vec<Kind> = source.iter().map(|text| cell(text)).collect();
        (source.len() as i64..total)
            .map(|offset| {
                let position = offset.rem_euclid(source.len() as i64) as usize;
                match series_value(&kinds, offset, false) {
                    Some(EditInput::Value(CalcValue::Text(text))) => text,
                    Some(EditInput::Value(CalcValue::Number(number))) => number.to_string(),
                    None => source[position].to_string(),
                    other => panic!("{other:?}"),
                }
            })
            .collect()
    }

    #[test]
    fn counts_on_as_excel_does() {
        assert_eq!(filled(&["1"], 3), ["1", "1"]);
        assert_eq!(filled(&["1", "3"], 5), ["5", "7", "9"]);
        assert_eq!(filled(&["-1", "-3"], 4), ["-5", "-7"]);
        assert_eq!(filled(&["x1"], 3), ["x2", "x3"]);
        assert_eq!(filled(&["t3", "t5"], 5), ["t7", "t9", "t11"]);
        assert_eq!(filled(&["Jan"], 3), ["Feb", "Mar"]);
        assert_eq!(filled(&["Dec"], 2), ["Jan"]);
        assert_eq!(filled(&["Mon", "Wed"], 4), ["Fri", "Sun"]);
        assert_eq!(filled(&["MONDAY"], 2), ["TUESDAY"]);
        assert_eq!(filled(&["a", "b"], 5), ["a", "b", "a"]);
        assert_eq!(filled(&["1", "a"], 5), ["2", "a", "3"]);
        assert_eq!(filled(&["1", "2", "a"], 7), ["3", "4", "a", "5"]);
        assert_eq!(filled(&["1st"], 3), ["2nd", "3rd"]);
        assert_eq!(filled(&["x1", "y2"], 5), ["x2", "y3", "x3"]);
        assert_eq!(filled(&["a1b2"], 3), ["a1b3", "a1b4"]);
        assert_eq!(filled(&["1", "x1"], 6), ["2", "x2", "3", "x3"]);
        assert_eq!(filled(&["x1", "1"], 6), ["x2", "2", "x3", "3"]);
        assert_eq!(filled(&["x-1"], 3), ["x-2", "x-3"]);
        assert_eq!(filled(&["5x"], 3), ["5x", "5x"]);
        assert_eq!(filled(&["x 09"], 3), ["x 10", "x 11"]);
        assert_eq!(filled(&["2.5"], 2), ["2.5"]);
        let line = filled(&["1", "2", "4"], 5);
        assert!(line[0].starts_with("5.3333"), "{line:?}");
        assert!(line[1].starts_with("6.8333"), "{line:?}");
    }

    #[test]
    fn a_date_counts_by_days() {
        let kinds = [Kind::Number(100.0)];
        assert_eq!(
            series_value(&kinds, 1, true),
            Some(EditInput::Value(CalcValue::Number(101.0)))
        );
    }

    #[test]
    fn going_back_counts_down() {
        let kinds = [cell("3"), cell("4")];
        assert_eq!(
            series_value(&kinds, -1, false),
            Some(EditInput::Value(CalcValue::Number(2.0)))
        );
    }
}
