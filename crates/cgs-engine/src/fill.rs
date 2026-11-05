//! The fill handle: dragging a selection out over more cells. Copies formulas and formats, and
//! carries on what the cells look like a series of, as Excel's AutoFill does.

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

/// How a run of source cells carries on.
#[derive(Debug, Clone, PartialEq)]
enum Series {
    /// Repeat them: formulas shift, and anything else copies.
    Copy,
    /// `start + step * n`, counted from the first source cell.
    Numbers { start: f64, step: f64 },
    /// `Item 7`: text, then a number that counts on, then text.
    Counted {
        prefix: String,
        start: i64,
        step: i64,
        width: usize,
        suffix: String,
    },
    /// A day or month name, in a list that wraps.
    Named {
        list: &'static [&'static str],
        short: bool,
        case: Case,
        start: i64,
        step: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    Lower,
    Upper,
    Title,
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

fn named(text: &str) -> Option<(&'static [&'static str], bool, Case, i64)> {
    let lower = text.to_lowercase();
    for list in [&DAYS[..], &MONTHS[..]] {
        for (index, name) in list.iter().enumerate() {
            if *name == lower {
                return Some((list, false, case_of(text), index as i64));
            }
            if name.get(..3) == Some(lower.as_str()) {
                return Some((list, true, case_of(text), index as i64));
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

/// Splits `Item 7b` around its last number.
fn counted(text: &str) -> Option<(String, i64, usize, String)> {
    let bytes = text.as_bytes();
    let end = bytes.iter().rposition(u8::is_ascii_digit)? + 1;
    let start = bytes[..end]
        .iter()
        .rposition(|byte| !byte.is_ascii_digit())
        .map_or(0, |at| at + 1);
    let digits = &text[start..end];
    let number = digits.parse().ok()?;
    Some((
        text[..start].to_string(),
        number,
        digits.len(),
        text[end..].to_string(),
    ))
}

fn series_of(values: &[CellInput], is_date: bool, copy: bool) -> Series {
    if copy || values.is_empty() {
        return Series::Copy;
    }
    let numbers: Option<Vec<f64>> = values
        .iter()
        .map(|value| match value {
            CellInput::Value(CalcValue::Number(number)) => Some(*number),
            _ => None,
        })
        .collect();
    if let Some(numbers) = numbers {
        if numbers.len() == 1 {
            // One number copies, and one date counts by a day.
            return if is_date {
                Series::Numbers {
                    start: numbers[0],
                    step: 1.0,
                }
            } else {
                Series::Copy
            };
        }
        // The straight line that fits them best, which is the series itself when they make one.
        let count = numbers.len() as f64;
        let mean_x = (count - 1.0) / 2.0;
        let mean_y = numbers.iter().sum::<f64>() / count;
        let mut above = 0.0;
        let mut below = 0.0;
        for (index, number) in numbers.iter().enumerate() {
            let x = index as f64 - mean_x;
            above += x * (number - mean_y);
            below += x * x;
        }
        let step = above / below;
        return Series::Numbers {
            start: mean_y - step * mean_x,
            step,
        };
    }
    let texts: Option<Vec<&str>> = values
        .iter()
        .map(|value| match value {
            CellInput::Value(CalcValue::Text(text)) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let Some(texts) = texts else {
        return Series::Copy;
    };
    if let Some((list, short, case, start)) = named(texts[0]) {
        let mut step = 1;
        let mut matches = true;
        if texts.len() > 1 {
            match named(texts[1]) {
                Some((other, _, _, next)) if std::ptr::eq(other, list) => step = next - start,
                _ => matches = false,
            }
            for (index, text) in texts.iter().enumerate() {
                match named(text) {
                    Some((other, _, _, at))
                        if std::ptr::eq(other, list)
                            && (at - (start + step * index as i64))
                                .rem_euclid(list.len() as i64)
                                == 0 => {}
                    _ => matches = false,
                }
            }
        }
        if matches {
            return Series::Named {
                list,
                short,
                case,
                start,
                step,
            };
        }
    }
    let parts: Option<Vec<(String, i64, usize, String)>> =
        texts.iter().map(|text| counted(text)).collect();
    if let Some(parts) = parts {
        let (prefix, start, width, suffix) = parts[0].clone();
        let same = parts
            .iter()
            .all(|part| part.0 == prefix && part.3 == suffix);
        let step = if parts.len() > 1 {
            parts[1].1 - start
        } else {
            1
        };
        let regular = parts
            .iter()
            .enumerate()
            .all(|(index, part)| part.1 == start + step * index as i64);
        if same && regular {
            return Series::Counted {
                prefix,
                start,
                step,
                width,
                suffix,
            };
        }
    }
    Series::Copy
}

fn series_value(series: &Series, index: i64) -> Option<EditInput> {
    Some(EditInput::Value(match series {
        Series::Copy => return None,
        Series::Numbers { start, step } => CalcValue::Number(start + step * index as f64),
        Series::Counted {
            prefix,
            start,
            step,
            width,
            suffix,
        } => {
            let number = start + step * index;
            let digits = if number < 0 {
                number.to_string()
            } else {
                format!("{number:0width$}", width = *width)
            };
            CalcValue::Text(format!("{prefix}{digits}{suffix}"))
        }
        Series::Named {
            list,
            short,
            case,
            start,
            step,
        } => CalcValue::Text(name_text(list, *short, *case, start + step * index)),
    }))
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
        let (lines, length, extent) = if down {
            (from.left..=from.right, from.rows(), (to.top, to.bottom))
        } else {
            (from.top..=from.bottom, from.cols(), (to.left, to.right))
        };
        let first = if down { from.top } else { from.left };
        for line in lines {
            let at = |offset: u32| {
                if down {
                    (first + offset, line)
                } else {
                    (line, first + offset)
                }
            };
            let values: Vec<CellInput> = (0..length)
                .map(|offset| {
                    let (row, col) = at(offset);
                    self.calc.cell_input(CellRef { sheet, row, col })
                })
                .collect();
            let (row, col) = at(0);
            let format = info.style_at(row, col);
            let num_fmt = self.book.styles.xf(format).num_fmt;
            let is_date = self.is_date_format(num_fmt);
            let series = series_of(&values, is_date, copy);
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
                let input = series_value(&series, offset).or_else(|| {
                    self.copied_input(
                        sheet,
                        source_row,
                        source_col,
                        i64::from(row) - i64::from(source_row),
                        i64::from(col) - i64::from(source_col),
                        PasteMode::All,
                    )
                });
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

    fn num(value: f64) -> CellInput {
        CellInput::Value(CalcValue::Number(value))
    }

    fn text(value: &str) -> CellInput {
        CellInput::Value(CalcValue::Text(value.to_string()))
    }

    fn values(series: &Series, from: i64, to: i64) -> Vec<CalcValue> {
        (from..to)
            .map(|index| match series_value(series, index) {
                Some(EditInput::Value(value)) => value,
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn numbers_carry_on_as_a_line_and_one_number_copies() {
        let series = series_of(&[num(1.0), num(3.0)], false, false);
        assert_eq!(
            values(&series, 2, 4),
            vec![CalcValue::Number(5.0), CalcValue::Number(7.0)]
        );
        assert_eq!(series_of(&[num(1.0)], false, false), Series::Copy);
        assert_eq!(series_of(&[num(1.0), num(3.0)], false, true), Series::Copy);
        let series = series_of(&[num(1.0), num(2.0), num(4.0)], false, false);
        let next = values(&series, 3, 4);
        let CalcValue::Number(next) = next[0] else {
            panic!();
        };
        assert!((next - 5.333_333_333_333_333).abs() < 1e-9);
    }

    #[test]
    fn a_date_counts_by_days() {
        let series = series_of(&[num(100.0)], true, false);
        assert_eq!(values(&series, 1, 2), vec![CalcValue::Number(101.0)]);
    }

    #[test]
    fn text_with_a_number_counts() {
        let series = series_of(&[text("Item 1")], false, false);
        assert_eq!(
            values(&series, 1, 3),
            vec![
                CalcValue::Text("Item 2".to_string()),
                CalcValue::Text("Item 3".to_string())
            ]
        );
        let series = series_of(&[text("a09b")], false, false);
        assert_eq!(
            values(&series, 1, 2),
            vec![CalcValue::Text("a10b".to_string())]
        );
        let series = series_of(&[text("x2"), text("x4")], false, false);
        assert_eq!(
            values(&series, 2, 3),
            vec![CalcValue::Text("x6".to_string())]
        );
    }

    #[test]
    fn days_and_months_wrap_and_keep_their_look() {
        let series = series_of(&[text("Dec")], false, false);
        assert_eq!(
            values(&series, 1, 2),
            vec![CalcValue::Text("Jan".to_string())]
        );
        let series = series_of(&[text("MONDAY")], false, false);
        assert_eq!(
            values(&series, 1, 2),
            vec![CalcValue::Text("TUESDAY".to_string())]
        );
        let series = series_of(&[text("Mon"), text("Wed")], false, false);
        assert_eq!(
            values(&series, 2, 3),
            vec![CalcValue::Text("Fri".to_string())]
        );
    }

    #[test]
    fn plain_text_and_mixtures_copy() {
        assert_eq!(series_of(&[text("abc")], false, false), Series::Copy);
        assert_eq!(
            series_of(&[num(1.0), text("a")], false, false),
            Series::Copy
        );
        assert_eq!(
            series_of(&[text("x1"), text("y2")], false, false),
            Series::Copy
        );
    }
}
