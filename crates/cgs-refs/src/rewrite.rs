//! What inserting or deleting rows and columns, or renaming or deleting a sheet, does to the
//! references in a piece of formula text. It follows Excel: a reference moves with what it points
//! at, a range grows when something is inserted inside it, and a range loses what is deleted from
//! it. A reference with nothing left to point at becomes `#REF!`.

use std::borrow::Cow;

use super::{
    is_word, push_letters, push_number, skip_brackets, skip_quoted, split_cell, split_col,
    split_row, MAX_COL, MAX_ROW,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    Row,
    Col,
}

impl Axis {
    pub fn max(self) -> u32 {
        match self {
            Self::Row => MAX_ROW as u32,
            Self::Col => MAX_COL as u32,
        }
    }
}

/// `count` rows or columns inserted before `at`, or deleted starting from it. 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AxisEdit {
    pub axis: Axis,
    pub at: u32,
    pub count: u32,
    pub delete: bool,
    /// Only cells in this stretch of the other axis move, as when cells shift down or across. A
    /// reference has to lie inside it to follow, and anything else stays as it is.
    pub band: Option<(u32, u32)>,
}

/// An inclusive stretch of rows or columns.
pub type Span = (u32, u32);

impl AxisEdit {
    pub fn insert(axis: Axis, at: u32, count: u32) -> Self {
        Self {
            axis,
            at,
            count,
            delete: false,
            band: None,
        }
    }

    pub fn delete(axis: Axis, at: u32, count: u32) -> Self {
        Self {
            axis,
            at,
            count,
            delete: true,
            band: None,
        }
    }

    /// Whether something across this stretch of the other axis moves with the edit.
    pub fn covers(&self, (first, last): Span) -> bool {
        self.band
            .is_none_or(|(from, to)| first >= from && last <= to)
    }

    /// The edit that puts positions back. What a delete removed has to come back some other way.
    pub fn inverse(self) -> Self {
        Self {
            delete: !self.delete,
            ..self
        }
    }

    /// Where a single row or column goes, if it survives.
    pub fn point(&self, at: u32) -> Option<u32> {
        if self.delete {
            if at < self.at {
                Some(at)
            } else if at - self.at < self.count {
                None
            } else {
                Some(at - self.count)
            }
        } else if at < self.at {
            Some(at)
        } else {
            let moved = u64::from(at) + u64::from(self.count);
            (moved <= u64::from(self.axis.max())).then_some(moved as u32)
        }
    }

    /// Where a stretch goes, if any of it survives.
    pub fn span(&self, (first, last): Span) -> Option<Span> {
        if self.delete {
            let end = self.at + self.count - 1;
            if first >= self.at && last <= end {
                return None;
            }
            let top = if first < self.at {
                first
            } else if first <= end {
                self.at
            } else {
                first - self.count
            };
            let bottom = if last < self.at {
                last
            } else if last <= end {
                self.at - 1
            } else {
                last - self.count
            };
            Some((top, bottom))
        } else {
            let max = self.axis.max();
            let top = self.point(first)?;
            let bottom = if last >= self.at {
                // A stretch that runs into the sheet's edge stops there.
                (u64::from(last) + u64::from(self.count)).min(u64::from(max)) as u32
            } else {
                last
            };
            Some((top, bottom))
        }
    }

    /// Where a position goes, falling back to the band's start for things that must stay on the
    /// sheet, such as the selected cell.
    pub fn clamp(&self, at: u32) -> u32 {
        self.point(at)
            .unwrap_or_else(|| self.at.min(self.axis.max()))
    }
}

/// One change to the workbook's shape, as formula text sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookEdit {
    /// Rows or columns on the sheet that has this name at the time.
    Axis {
        sheet: String,
        edit: AxisEdit,
    },
    Rename {
        old: String,
        new: String,
    },
    DeleteSheet {
        name: String,
    },
    /// Tables that no longer exist.
    DeleteTables {
        names: Vec<String>,
    },
    /// Columns gone from a table that stays.
    DeleteTableColumns {
        table: String,
        columns: Vec<String>,
    },
}

/// How sheet names compare: Excel ignores case, for all of Unicode.
pub fn sheet_key(name: &str) -> String {
    name.to_lowercase()
}

fn same_sheet(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || sheet_key(a) == sheet_key(b)
}

/// A sheet's name as a formula writes it.
pub fn quote_sheet(name: &str) -> String {
    if needs_quotes(name) {
        format!("'{}'", name.replace('\'', "''"))
    } else {
        name.to_string()
    }
}

fn needs_quotes(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes[0].is_ascii_digit() || bytes[0] == b'.' {
        return true;
    }
    let plain = name.chars().all(|c| {
        c == '_' || c == '.' || c.is_alphanumeric() || (!c.is_ascii() && !c.is_whitespace())
    });
    if !plain {
        return true;
    }
    // Would read as a reference, in either style.
    if split_cell(name).is_some()
        || name.eq_ignore_ascii_case("R")
        || name.eq_ignore_ascii_case("C")
    {
        return true;
    }
    let upper = name.to_ascii_uppercase();
    if let Some(rest) = upper.strip_prefix('R') {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let after = &rest[digits..];
        if after.is_empty() || after.starts_with('C') {
            let tail = after.strip_prefix('C').unwrap_or("");
            if tail.bytes().all(|b| b.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Cell {
        abs_col: bool,
        col: u32,
        abs_row: bool,
        row: u32,
    },
    Cells {
        abs: [bool; 4],
        top: u32,
        left: u32,
        bottom: u32,
        right: u32,
    },
    Cols {
        abs: [bool; 2],
        first: u32,
        last: u32,
    },
    Rows {
        abs: [bool; 2],
        first: u32,
        last: u32,
    },
}

/// A reference as it appears in text.
#[derive(Debug, Clone)]
struct Found {
    /// Where the whole reference starts and ends, sheet name included.
    start: usize,
    end: usize,
    /// The name, unquoted, if it has one of its own.
    sheet: Option<String>,
    /// The text of the name as written, with its `!`, for putting back.
    prefix: Option<(usize, usize)>,
    /// Another workbook's, or several sheets'. Edits here don't touch these.
    foreign: bool,
    /// Where `target` starts, after the sheet's name.
    target_start: usize,
    target: Target,
}

fn unquote(raw: &str) -> String {
    let inner = raw
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .unwrap_or(raw);
    inner.replace("''", "'")
}

fn parse_target(first: &str, second: Option<&str>) -> Option<Target> {
    let Some(second) = second else {
        let (abs_col, col, abs_row, row) = split_cell(first)?;
        return Some(Target::Cell {
            abs_col,
            col: col as u32,
            abs_row,
            row: row as u32,
        });
    };
    if let (Some(a), Some(b)) = (split_cell(first), split_cell(second)) {
        // A file holds these in order. One that isn't gets put in order when it changes.
        let (a_abs_col, a_col, a_abs_row, a_row) = a;
        let (b_abs_col, b_col, b_abs_row, b_row) = b;
        let (left, right, abs_left, abs_right) = if a_col <= b_col {
            (a_col, b_col, a_abs_col, b_abs_col)
        } else {
            (b_col, a_col, b_abs_col, a_abs_col)
        };
        let (top, bottom, abs_top, abs_bottom) = if a_row <= b_row {
            (a_row, b_row, a_abs_row, b_abs_row)
        } else {
            (b_row, a_row, b_abs_row, a_abs_row)
        };
        return Some(Target::Cells {
            abs: [abs_left, abs_top, abs_right, abs_bottom],
            top: top as u32,
            left: left as u32,
            bottom: bottom as u32,
            right: right as u32,
        });
    }
    if let (Some(a), Some(b)) = (split_col(first), split_col(second)) {
        let (a, b) = if a.1 <= b.1 { (a, b) } else { (b, a) };
        return Some(Target::Cols {
            abs: [a.0, b.0],
            first: a.1 as u32,
            last: b.1 as u32,
        });
    }
    if let (Some(a), Some(b)) = (split_row(first), split_row(second)) {
        let (a, b) = if a.1 <= b.1 { (a, b) } else { (b, a) };
        return Some(Target::Rows {
            abs: [a.0, b.0],
            first: a.1 as u32,
            last: b.1 as u32,
        });
    }
    None
}

fn word_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && is_word(bytes[at]) {
        at += 1;
    }
    at
}

/// Calls `found` for every reference in `text`.
fn scan(text: &str, mut found: impl FnMut(Found)) {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => at = skip_quoted(bytes, at, b'"'),
            b'[' => {
                // `[1]Sheet1!A1`: another workbook's. Anything else is a table's own syntax.
                let end = skip_brackets(bytes, at);
                let next = word_end(bytes, end);
                if bytes.get(next) == Some(&b'!') {
                    at = target_after(text, next + 1, at, None, true, &mut found);
                } else {
                    at = end;
                }
            }
            b'#' => {
                // An error's spelling: `#REF!` isn't a sheet.
                at += 1;
                while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'/') {
                    at += 1;
                }
                if matches!(bytes.get(at), Some(b'!' | b'?')) {
                    at += 1;
                }
            }
            b'\'' => {
                let end = skip_quoted(bytes, at, b'\'');
                if bytes.get(end) == Some(&b'!') {
                    let name = unquote(&text[at..end]);
                    let foreign = (name.contains('[') && name.contains(']')) || name.contains(':');
                    at = target_after(
                        text,
                        end + 1,
                        at,
                        Some((name, (at, end + 1))),
                        foreign,
                        &mut found,
                    );
                } else {
                    at = end;
                }
            }
            byte if is_word(byte) => {
                let start = at;
                at = word_end(bytes, at);
                match bytes.get(at) {
                    Some(b'(') => {}
                    // A table's name: its brackets are its own.
                    Some(b'[') => at = skip_brackets(bytes, at),
                    Some(b'!') => {
                        let word = text[start..at].to_string();
                        at = target_after(
                            text,
                            at + 1,
                            start,
                            Some((word, (start, at + 1))),
                            false,
                            &mut found,
                        );
                    }
                    Some(b':') => {
                        // `Sheet1:Sheet3!A1` covers several sheets.
                        let second_end = word_end(bytes, at + 1);
                        if bytes.get(second_end) == Some(&b'!') {
                            at = target_after(text, second_end + 1, start, None, true, &mut found);
                            continue;
                        }
                        at = range_or_cell(text, start, start, at, None, false, &mut found);
                    }
                    _ => at = range_or_cell(text, start, start, at, None, false, &mut found),
                }
            }
            _ => at += text[at..].chars().next().map_or(1, char::len_utf8),
        }
    }
}

/// Reads the reference after a sheet's name and its `!`, and returns where to carry on.
fn target_after(
    text: &str,
    at: usize,
    start: usize,
    sheet: Option<(String, (usize, usize))>,
    foreign: bool,
    found: &mut impl FnMut(Found),
) -> usize {
    let bytes = text.as_bytes();
    // `Sheet1!#REF!` points at nothing, whatever happens to the sheet.
    if bytes.get(at) == Some(&b'#') {
        return at;
    }
    let end = word_end(bytes, at);
    if end == at || bytes.get(end) == Some(&b'(') {
        return end;
    }
    range_or_cell(text, start, at, end, sheet, foreign, found)
}

/// Reads `first` or `first:second` starting at `target_start`, whose first word ends at `end`.
fn range_or_cell(
    text: &str,
    start: usize,
    target_start: usize,
    end: usize,
    sheet: Option<(String, (usize, usize))>,
    foreign: bool,
    found: &mut impl FnMut(Found),
) -> usize {
    let bytes = text.as_bytes();
    let first = &text[target_start..end];
    let mut second = None;
    let mut stop = end;
    if bytes.get(end) == Some(&b':') {
        let mut from = end + 1;
        // `Sheet1!A1:Sheet1!B2` names the sheet twice.
        if bytes.get(from) == Some(&b'\'') {
            let quoted = skip_quoted(bytes, from, b'\'');
            if bytes.get(quoted) == Some(&b'!') {
                from = quoted + 1;
            }
        } else {
            let word = word_end(bytes, from);
            if bytes.get(word) == Some(&b'!') {
                from = word + 1;
            }
        }
        let until = word_end(bytes, from);
        if until > from && bytes.get(until) != Some(&b'(') {
            second = Some(&text[from..until]);
            stop = until;
        }
    }
    let mut target = parse_target(first, second);
    if target.is_none() && second.is_some() {
        // `A1:OFFSET(...)` or `A1:Name`: the first part still counts by itself.
        stop = end;
        target = parse_target(first, None);
    }
    let Some(target) = target else {
        return stop;
    };
    if bytes.get(stop) == Some(&b'[') {
        return stop;
    }
    let (name, prefix) = match sheet {
        Some((name, prefix)) => (Some(name), Some(prefix)),
        None => (None, None),
    };
    found(Found {
        start,
        end: stop,
        sheet: name,
        prefix,
        foreign,
        target_start,
        target,
    });
    stop
}

fn push_col(out: &mut String, abs: bool, col: u32) {
    if abs {
        out.push('$');
    }
    push_letters(out, i64::from(col));
}

fn push_row(out: &mut String, abs: bool, row: u32) {
    if abs {
        out.push('$');
    }
    push_number(out, i64::from(row));
}

/// The target after the edit, or `None` if nothing is left of it.
fn edited(target: &Target, edit: &AxisEdit) -> Option<Target> {
    if edit.band.is_some() {
        let across = match target {
            Target::Cell { col, row, .. } => match edit.axis {
                Axis::Row => Some((*col, *col)),
                Axis::Col => Some((*row, *row)),
            },
            Target::Cells {
                top,
                left,
                bottom,
                right,
                ..
            } => match edit.axis {
                Axis::Row => Some((*left, *right)),
                Axis::Col => Some((*top, *bottom)),
            },
            // A whole line has no part of itself that moves.
            Target::Cols { .. } | Target::Rows { .. } => None,
        };
        if !across.is_some_and(|span| edit.covers(span)) {
            return Some(target.clone());
        }
    }
    match target.clone() {
        Target::Cell {
            abs_col,
            col,
            abs_row,
            row,
        } => {
            let (col, row) = match edit.axis {
                Axis::Row => (col, edit.point(row)?),
                Axis::Col => (edit.point(col)?, row),
            };
            Some(Target::Cell {
                abs_col,
                col,
                abs_row,
                row,
            })
        }
        Target::Cells {
            abs,
            top,
            left,
            bottom,
            right,
        } => {
            let (top, left, bottom, right) = edit_rect((top, left, bottom, right), edit)?;
            Some(Target::Cells {
                abs,
                top,
                left,
                bottom,
                right,
            })
        }
        Target::Cols { abs, first, last } => {
            let (first, last) = match edit.axis {
                Axis::Row => (first, last),
                Axis::Col => edit.span((first, last))?,
            };
            Some(Target::Cols { abs, first, last })
        }
        Target::Rows { abs, first, last } => {
            let (first, last) = match edit.axis {
                Axis::Row => edit.span((first, last))?,
                Axis::Col => (first, last),
            };
            Some(Target::Rows { abs, first, last })
        }
    }
}

fn push_target(out: &mut String, target: &Target) {
    match target {
        Target::Cell {
            abs_col,
            col,
            abs_row,
            row,
        } => {
            push_col(out, *abs_col, *col);
            push_row(out, *abs_row, *row);
        }
        Target::Cells {
            abs,
            top,
            left,
            bottom,
            right,
        } => {
            push_col(out, abs[0], *left);
            push_row(out, abs[1], *top);
            out.push(':');
            push_col(out, abs[2], *right);
            push_row(out, abs[3], *bottom);
        }
        Target::Cols { abs, first, last } => {
            push_col(out, abs[0], *first);
            out.push(':');
            push_col(out, abs[1], *last);
        }
        Target::Rows { abs, first, last } => {
            push_row(out, abs[0], *first);
            out.push(':');
            push_row(out, abs[1], *last);
        }
    }
}

/// Applies `change` to every reference, keeping all the rest of the text as it stands.
fn rewritten<'a>(
    text: &'a str,
    mut change: impl FnMut(&Found, &str) -> Option<String>,
) -> Cow<'a, str> {
    let mut out: Option<String> = None;
    let mut copied = 0;
    scan(text, |found| {
        if let Some(new) = change(&found, text) {
            let buffer = out.get_or_insert_with(|| String::with_capacity(text.len() + 8));
            buffer.push_str(&text[copied..found.start]);
            buffer.push_str(&new);
            copied = found.end;
        }
    });
    match out {
        Some(mut buffer) => {
            buffer.push_str(&text[copied..]);
            Cow::Owned(buffer)
        }
        None => Cow::Borrowed(text),
    }
}

/// `text` after an edit on sheet `sheet`, read from a cell on sheet `home`.
pub fn edit_formula<'a>(text: &'a str, home: &str, sheet: &str, edit: &AxisEdit) -> Cow<'a, str> {
    if !same_sheet(home, sheet) && !text.contains('!') {
        return Cow::Borrowed(text);
    }
    rewritten(text, |found, text| {
        if found.foreign || !same_sheet(found.sheet.as_deref().unwrap_or(home), sheet) {
            return None;
        }
        let prefix = found.prefix.map_or("", |(from, to)| &text[from..to]);
        let mut out = String::with_capacity(found.end - found.start + 4);
        out.push_str(prefix);
        match edited(&found.target, edit) {
            Some(target) => {
                if target == found.target {
                    return None;
                }
                push_target(&mut out, &target);
            }
            None => out.push_str("#REF!"),
        }
        Some(out)
    })
}

/// `text` with a sheet's new name.
pub fn rename_in_formula<'a>(text: &'a str, old: &str, new: &str) -> Cow<'a, str> {
    if !text.contains('!') {
        return Cow::Borrowed(text);
    }
    rewritten(text, |found, text| {
        if found.foreign || !same_sheet(found.sheet.as_deref()?, old) {
            return None;
        }
        Some(format!(
            "{}!{}",
            quote_sheet(new),
            &text[found.target_start..found.end]
        ))
    })
}

/// `text` with references to a deleted sheet gone.
pub fn delete_sheet_in_formula<'a>(text: &'a str, name: &str) -> Cow<'a, str> {
    if !text.contains('!') {
        return Cow::Borrowed(text);
    }
    rewritten(text, |found, _| {
        if found.foreign || !same_sheet(found.sheet.as_deref()?, name) {
            return None;
        }
        Some("#REF!".to_string())
    })
}

/// `text` with references to tables that have gone turned to `#REF!`.
pub fn delete_tables_in_formula<'a>(text: &'a str, names: &[String]) -> Cow<'a, str> {
    if names.is_empty() || !text.contains('[') {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' | b'\'' => at = skip_quoted(bytes, at, bytes[at]),
            b'[' => at = skip_brackets(bytes, at),
            byte if is_word(byte) => {
                let start = at;
                at = word_end(bytes, at);
                if bytes.get(at) == Some(&b'[') {
                    let end = skip_brackets(bytes, at);
                    let word = &text[start..at];
                    if names.iter().any(|name| same_sheet(name, word)) {
                        let buffer = out.get_or_insert_with(|| String::with_capacity(text.len()));
                        buffer.push_str(&text[copied..start]);
                        buffer.push_str("#REF!");
                        copied = end;
                    }
                    at = end;
                }
            }
            _ => at += 1,
        }
    }
    match out {
        Some(mut buffer) => {
            buffer.push_str(&text[copied..]);
            Cow::Owned(buffer)
        }
        None => Cow::Borrowed(text),
    }
}

/// Whether `text` uses any of these tables.
pub fn reads_tables(text: &str, names: &[String]) -> bool {
    matches!(delete_tables_in_formula(text, names), Cow::Owned(_))
}

/// `text`, read from a cell on sheet `home`, through a list of edits in order. `home` follows
/// renames, as the list names sheets as they were called at the time.
pub fn apply_edits<'a>(text: &'a str, home: &str, edits: &[BookEdit]) -> Cow<'a, str> {
    let mut current = Cow::Borrowed(text);
    let mut home = Cow::Borrowed(home);
    for edit in edits {
        let next = match edit {
            BookEdit::Axis { sheet, edit } => match edit_formula(&current, &home, sheet, edit) {
                Cow::Owned(owned) => Some(owned),
                Cow::Borrowed(_) => None,
            },
            BookEdit::Rename { old, new } => {
                let changed = match rename_in_formula(&current, old, new) {
                    Cow::Owned(owned) => Some(owned),
                    Cow::Borrowed(_) => None,
                };
                if same_sheet(&home, old) {
                    home = Cow::Owned(new.clone());
                }
                changed
            }
            BookEdit::DeleteSheet { name } => match delete_sheet_in_formula(&current, name) {
                Cow::Owned(owned) => Some(owned),
                Cow::Borrowed(_) => None,
            },
            BookEdit::DeleteTables { names } => match delete_tables_in_formula(&current, names) {
                Cow::Owned(owned) => Some(owned),
                Cow::Borrowed(_) => None,
            },
            BookEdit::DeleteTableColumns { table, columns } => {
                match delete_table_columns(&current, table, columns) {
                    Cow::Owned(owned) => Some(owned),
                    Cow::Borrowed(_) => None,
                }
            }
        };
        if let Some(next) = next {
            current = Cow::Owned(next);
        }
    }
    current
}

/// `text` as it reads copied `rows` below and `cols` right of where it is. Relative references move
/// with it, and one that would land off the sheet becomes `#REF!`, as in Excel.
pub fn moved_formula(text: &str, rows: i64, cols: i64) -> String {
    if rows == 0 && cols == 0 {
        return text.to_string();
    }
    let in_range = |value: i64, max: i64| (1..=max).contains(&value);
    let shift_col = |abs: bool, col: u32| {
        let next = if abs {
            i64::from(col)
        } else {
            i64::from(col) + cols
        };
        in_range(next, MAX_COL).then_some(next as u32)
    };
    let shift_row = |abs: bool, row: u32| {
        let next = if abs {
            i64::from(row)
        } else {
            i64::from(row) + rows
        };
        in_range(next, MAX_ROW).then_some(next as u32)
    };
    let moved = rewritten(text, |found, text| {
        let next = match found.target {
            Target::Cell {
                abs_col,
                col,
                abs_row,
                row,
            } => match (shift_col(abs_col, col), shift_row(abs_row, row)) {
                (Some(col), Some(row)) => Some(Target::Cell {
                    abs_col,
                    col,
                    abs_row,
                    row,
                }),
                _ => None,
            },
            Target::Cells {
                abs,
                top,
                left,
                bottom,
                right,
            } => match (
                shift_col(abs[0], left),
                shift_row(abs[1], top),
                shift_col(abs[2], right),
                shift_row(abs[3], bottom),
            ) {
                (Some(left), Some(top), Some(right), Some(bottom)) => Some(Target::Cells {
                    abs,
                    top,
                    left,
                    bottom,
                    right,
                }),
                _ => None,
            },
            Target::Cols { abs, first, last } => {
                match (shift_col(abs[0], first), shift_col(abs[1], last)) {
                    (Some(first), Some(last)) => Some(Target::Cols { abs, first, last }),
                    _ => None,
                }
            }
            Target::Rows { abs, first, last } => {
                match (shift_row(abs[0], first), shift_row(abs[1], last)) {
                    (Some(first), Some(last)) => Some(Target::Rows { abs, first, last }),
                    _ => None,
                }
            }
        };
        let mut out = String::new();
        if let Some((from, to)) = found.prefix {
            out.push_str(&text[from..to]);
        }
        match next {
            Some(target) => push_target(&mut out, &target),
            None => out.push_str("#REF!"),
        }
        Some(out)
    });
    moved.into_owned()
}

/// A block of cells moved by cut and paste. References to it follow.
#[derive(Debug, Clone, Copy)]
pub struct MovedCells<'a> {
    pub from_sheet: &'a str,
    /// (top, left, bottom, right), where it was.
    pub area: (u32, u32, u32, u32),
    pub to_sheet: &'a str,
    pub rows: i64,
    pub cols: i64,
}

/// `text`, read from sheet `home`, with references to moved cells following them. A reference to
/// where they landed, not one of those cells itself, has lost what it pointed at.
pub fn follow_moved_cells<'a>(text: &'a str, home: &str, moved: &MovedCells<'_>) -> Cow<'a, str> {
    let (top, left, bottom, right) = moved.area;
    let new_area = (
        (i64::from(top) + moved.rows) as u32,
        (i64::from(left) + moved.cols) as u32,
        (i64::from(bottom) + moved.rows) as u32,
        (i64::from(right) + moved.cols) as u32,
    );
    let inside = |area: (u32, u32, u32, u32), row: u32, col: u32| {
        row >= area.0 && row <= area.2 && col >= area.1 && col <= area.3
    };
    let same_place = same_sheet(moved.from_sheet, moved.to_sheet);
    rewritten(text, |found, text| {
        if found.foreign {
            return None;
        }
        let name = found.sheet.as_deref().unwrap_or(home);
        let from_here = same_sheet(name, moved.from_sheet);
        let to_here = same_sheet(name, moved.to_sheet);
        let corners = match found.target {
            Target::Cell { row, col, .. } => (row, col, row, col),
            Target::Cells {
                top,
                left,
                bottom,
                right,
                ..
            } => (top, left, bottom, right),
            _ => return None,
        };
        let held = from_here
            && inside(moved.area, corners.0, corners.1)
            && inside(moved.area, corners.2, corners.3);
        // A block that takes one whole edge of a range moves that edge, and a block landing on one
        // takes it away, on the same sheet.
        if !held && from_here && same_place {
            if let Target::Cells {
                abs,
                top: range_top,
                left: range_left,
                bottom: range_bottom,
                right: range_right,
            } = found.target
            {
                let (new_top, new_left, new_bottom, new_right) = new_area;
                let mut next = (range_top, range_left, range_bottom, range_right);
                let mut changed = false;
                let spans_width = left <= range_left && right >= range_right;
                let spans_height = top <= range_top && bottom >= range_bottom;
                let lands_across = new_left <= range_left && new_right >= range_right;
                let lands_along = new_top <= range_top && new_bottom >= range_bottom;
                if spans_width && moved.cols == 0 {
                    if top <= range_top && bottom >= range_top {
                        next.0 = if new_top < range_top {
                            new_top
                        } else {
                            bottom + 1
                        };
                        changed = true;
                    } else if top <= range_bottom && bottom >= range_bottom {
                        next.2 = if new_bottom > range_bottom {
                            new_bottom
                        } else {
                            top.saturating_sub(1)
                        };
                        changed = true;
                    }
                    // Landing over the far edge carries that edge out too.
                    if changed && new_top <= range_bottom && new_bottom > range_bottom {
                        next.2 = new_bottom;
                    }
                } else if spans_height && moved.rows == 0 {
                    if left <= range_left && right >= range_left {
                        next.1 = if new_left < range_left {
                            new_left
                        } else {
                            right + 1
                        };
                        changed = true;
                    } else if left <= range_right && right >= range_right {
                        next.3 = if new_right > range_right {
                            new_right
                        } else {
                            left.saturating_sub(1)
                        };
                        changed = true;
                    }
                    if changed && new_left <= range_right && new_right > range_right {
                        next.3 = new_right;
                    }
                }
                // Landing across an edge takes it away. Ending exactly on it leaves it.
                if !changed && lands_across {
                    if new_top <= range_top && new_bottom >= range_bottom {
                        return Some("#REF!".to_string());
                    }
                    if new_top < range_top && new_bottom >= range_top {
                        next.0 = new_bottom + 1;
                        changed = true;
                    } else if new_top <= range_bottom && new_bottom > range_bottom {
                        next.2 = new_top.saturating_sub(1);
                        changed = true;
                    }
                }
                if !changed && lands_along {
                    if new_left <= range_left && new_right >= range_right {
                        return Some("#REF!".to_string());
                    }
                    if new_left < range_left && new_right >= range_left {
                        next.1 = new_right + 1;
                        changed = true;
                    } else if new_left <= range_right && new_right > range_right {
                        next.3 = new_left.saturating_sub(1);
                        changed = true;
                    }
                }
                if changed && next.0 <= next.2 && next.1 <= next.3 {
                    let mut out = String::new();
                    if let Some((from, to)) = found.prefix {
                        out.push_str(&text[from..to]);
                    }
                    push_target(
                        &mut out,
                        &Target::Cells {
                            abs,
                            top: next.0,
                            left: next.1,
                            bottom: next.2,
                            right: next.3,
                        },
                    );
                    return Some(out);
                }
            }
        }
        if !held {
            let overwritten = to_here
                && inside(new_area, corners.0, corners.1)
                && inside(new_area, corners.2, corners.3)
                && matches!(found.target, Target::Cell { .. });
            return overwritten.then(|| "#REF!".to_string());
        }
        let next = match found.target.clone() {
            Target::Cell {
                abs_col,
                col,
                abs_row,
                row,
            } => Target::Cell {
                abs_col,
                col: (i64::from(col) + moved.cols) as u32,
                abs_row,
                row: (i64::from(row) + moved.rows) as u32,
            },
            Target::Cells {
                abs,
                top,
                left,
                bottom,
                right,
            } => Target::Cells {
                abs,
                top: (i64::from(top) + moved.rows) as u32,
                left: (i64::from(left) + moved.cols) as u32,
                bottom: (i64::from(bottom) + moved.rows) as u32,
                right: (i64::from(right) + moved.cols) as u32,
            },
            other => other,
        };
        let mut out = String::new();
        if same_place {
            if let Some((from, to)) = found.prefix {
                out.push_str(&text[from..to]);
            }
        } else if found.sheet.is_some() || !same_sheet(home, moved.to_sheet) {
            out.push_str(&quote_sheet(moved.to_sheet));
            out.push('!');
        }
        push_target(&mut out, &next);
        Some(out)
    })
}

/// A table column's name as a reference writes it, with Excel's escapes.
fn escape_column(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if matches!(ch, '[' | ']' | '#' | '\'') {
            out.push('\'');
        }
        out.push(ch);
    }
    out
}

fn unescape_column(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Renames a column inside one table reference, whose brackets span `start..end`.
fn rename_in_brackets(text: &str, old: &str, new: &str, out: &mut String) {
    // The innermost brackets hold a column's name or a keyword: `[x]`, `[@x]`, `[[#All],[x]]`.
    let bytes = text.as_bytes();
    let mut at = 0;
    let mut copied = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\'' => at += 2,
            b'[' => {
                // Find its end, and whether anything is nested inside.
                let mut end = at + 1;
                let mut nested = false;
                while end < bytes.len() && bytes[end] != b']' {
                    match bytes[end] {
                        b'\'' => end += 1,
                        b'[' => nested = true,
                        _ => {}
                    }
                    end += 1;
                }
                if nested || end >= bytes.len() {
                    at += 1;
                    continue;
                }
                let inner = &text[at + 1..end];
                let (mark, name) = match inner.strip_prefix('@') {
                    Some(rest) => ("@", rest),
                    None => ("", inner),
                };
                if !name.starts_with('#') && same_sheet(&unescape_column(name), old) {
                    out.push_str(&text[copied..at + 1]);
                    out.push_str(mark);
                    let plain = new
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '.');
                    // A name with spaces after `@` has brackets of its own.
                    if !mark.is_empty() && !plain {
                        out.push('[');
                        out.push_str(&escape_column(new));
                        out.push(']');
                    } else {
                        out.push_str(&escape_column(new));
                    }
                    copied = end;
                }
                at = end + 1;
            }
            _ => at += 1,
        }
    }
    out.push_str(&text[copied..]);
}

/// `text` with references to a table's lost columns turned to `#REF!`.
pub fn delete_table_columns<'a>(text: &'a str, table: &str, columns: &[String]) -> Cow<'a, str> {
    if !text.contains('[') {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' | b'\'' => at = skip_quoted(bytes, at, bytes[at]),
            b'[' => at = skip_brackets(bytes, at),
            byte if is_word(byte) => {
                let start = at;
                at = word_end(bytes, at);
                if bytes.get(at) == Some(&b'[') {
                    let end = skip_brackets(bytes, at);
                    if same_sheet(&text[start..at], table) {
                        let inside = &text[at..end];
                        let reads = columns.iter().any(|column| {
                            let mut piece = String::new();
                            rename_in_brackets(inside, column, "\u{1}", &mut piece);
                            piece != inside
                        });
                        if reads {
                            let buffer =
                                out.get_or_insert_with(|| String::with_capacity(text.len()));
                            buffer.push_str(&text[copied..start]);
                            buffer.push_str("#REF!");
                            copied = end;
                        }
                    }
                    at = end;
                }
            }
            _ => at += 1,
        }
    }
    match out {
        Some(mut buffer) => {
            buffer.push_str(&text[copied..]);
            Cow::Owned(buffer)
        }
        None => Cow::Borrowed(text),
    }
}

/// `text` after a table's column has a new name. `inside` says the formula sits in that table, where
/// `[@x]` and `[x]` with no table's name mean this one.
pub fn rename_table_column<'a>(
    text: &'a str,
    table: &str,
    old: &str,
    new: &str,
    inside: bool,
) -> Cow<'a, str> {
    if !text.contains('[') {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 8);
    let mut copied = 0;
    let mut at = 0;
    let mut changed = false;
    while at < bytes.len() {
        match bytes[at] {
            b'"' | b'\'' => at = skip_quoted(bytes, at, bytes[at]),
            b'[' => {
                let end = skip_brackets(bytes, at);
                // A workbook's `[1]Sheet!A1` isn't a column.
                let next = word_end(bytes, end);
                let foreign = bytes.get(next) == Some(&b'!');
                if inside && !foreign {
                    let mut piece = String::new();
                    rename_in_brackets(&text[at..end], old, new, &mut piece);
                    if piece != text[at..end] {
                        out.push_str(&text[copied..at]);
                        out.push_str(&piece);
                        copied = end;
                        changed = true;
                    }
                }
                at = end;
            }
            byte if is_word(byte) => {
                let start = at;
                at = word_end(bytes, at);
                if bytes.get(at) == Some(&b'[') {
                    let end = skip_brackets(bytes, at);
                    if same_sheet(&text[start..at], table) {
                        let mut piece = String::new();
                        rename_in_brackets(&text[at..end], old, new, &mut piece);
                        if piece != text[at..end] {
                            out.push_str(&text[copied..at]);
                            out.push_str(&piece);
                            copied = end;
                            changed = true;
                        }
                    }
                    at = end;
                }
            }
            _ => at += 1,
        }
    }
    if !changed {
        return Cow::Borrowed(text);
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
}

/// Whether any reference in `text`, read from sheet `home`, is on a sheet with this name.
pub fn reads_sheet(text: &str, home: &str, sheet: &str) -> bool {
    let mut hit = false;
    scan(text, |found| {
        if !found.foreign && same_sheet(found.sheet.as_deref().unwrap_or(home), sheet) {
            hit = true;
        }
    });
    hit
}

/// Whether a delete on sheet `sheet` changes `text` by more than position, so that putting the
/// rows back wouldn't give back this text: a cell in the band, or a range with an end in it.
pub fn touches_band(text: &str, home: &str, sheet: &str, edit: &AxisEdit) -> bool {
    let mut hit = false;
    let (from, to) = (edit.at, edit.at + edit.count - 1);
    let inside = |at: u32| at >= from && at <= to;
    scan(text, |found| {
        if found.foreign || !same_sheet(found.sheet.as_deref().unwrap_or(home), sheet) {
            return;
        }
        hit |= match (&found.target, edit.axis) {
            (Target::Cell { row, .. }, Axis::Row) => inside(*row),
            (Target::Cell { col, .. }, Axis::Col) => inside(*col),
            (Target::Cells { top, bottom, .. }, Axis::Row) => inside(*top) || inside(*bottom),
            (Target::Cells { left, right, .. }, Axis::Col) => inside(*left) || inside(*right),
            (Target::Rows { first, last, .. }, Axis::Row)
            | (Target::Cols { first, last, .. }, Axis::Col) => inside(*first) || inside(*last),
            _ => false,
        };
    });
    hit
}

/// Where a rectangle goes, for a merge or a table. Rows and columns are 1-based, inclusive.
pub fn edit_rect(
    (top, left, bottom, right): (u32, u32, u32, u32),
    edit: &AxisEdit,
) -> Option<(u32, u32, u32, u32)> {
    let across = match edit.axis {
        Axis::Row => (left, right),
        Axis::Col => (top, bottom),
    };
    if !edit.covers(across) {
        return Some((top, left, bottom, right));
    }
    match edit.axis {
        Axis::Row => {
            let (top, bottom) = edit.span((top, bottom))?;
            Some((top, left, bottom, right))
        }
        Axis::Col => {
            let (left, right) = edit.span((left, right))?;
            Some((top, left, bottom, right))
        }
    }
}

fn parse_area(text: &str) -> Option<(u32, u32, u32, u32)> {
    let ((top, left), (bottom, right)) = super::parse_range(text)?;
    Some((
        top.min(bottom),
        left.min(right),
        top.max(bottom),
        left.max(right),
    ))
}

fn push_area(out: &mut String, (top, left, bottom, right): (u32, u32, u32, u32)) {
    push_letters(out, i64::from(left));
    push_number(out, i64::from(top));
    if (top, left) != (bottom, right) {
        out.push(':');
        push_letters(out, i64::from(right));
        push_number(out, i64::from(bottom));
    }
}

/// A file's `ref` or `sqref`: areas separated by spaces. Empty when nothing survives. An area this
/// can't read, such as `A:A`, stays as it stands.
pub fn edit_sqref(text: &str, edit: &AxisEdit) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    for area in text.split_ascii_whitespace() {
        match parse_area(area) {
            Some(parsed) => {
                if let Some(moved) = edit_rect(parsed, edit) {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    push_area(&mut out, moved);
                }
            }
            None => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(area);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ins(axis: Axis, at: u32, count: u32) -> AxisEdit {
        AxisEdit::insert(axis, at, count)
    }

    fn del(axis: Axis, at: u32, count: u32) -> AxisEdit {
        AxisEdit::delete(axis, at, count)
    }

    fn row(text: &str, edit: AxisEdit) -> String {
        edit_formula(text, "S", "S", &edit).into_owned()
    }

    #[test]
    fn inserting_rows_moves_what_is_below() {
        let edit = ins(Axis::Row, 5, 2);
        assert_eq!(row("A4+A5+$B$6", edit), "A4+A7+$B$8");
        assert_eq!(row("SUM(A1:A10)", edit), "SUM(A1:A12)");
        assert_eq!(row("SUM(A5:A10)", edit), "SUM(A7:A12)");
        assert_eq!(row("SUM(A1:A4)", edit), "SUM(A1:A4)");
        assert_eq!(row("SUM(A:A)", edit), "SUM(A:A)");
        assert_eq!(row("SUM(5:6)", edit), "SUM(7:8)");
        assert_eq!(row("SUM(1:9)", edit), "SUM(1:11)");
    }

    #[test]
    fn inserting_past_the_edge_gives_ref_or_stops_at_the_edge() {
        let edit = ins(Axis::Row, 5, 10);
        assert_eq!(row("A1048576", edit), "#REF!");
        assert_eq!(row("SUM(A1:A1048576)", edit), "SUM(A1:A1048576)");
    }

    #[test]
    fn deleting_rows_gives_ref_or_a_smaller_range() {
        let edit = del(Axis::Row, 5, 2);
        assert_eq!(row("A4+A5+A6+A7", edit), "A4+#REF!+#REF!+A5");
        assert_eq!(row("SUM(A1:A10)", edit), "SUM(A1:A8)");
        assert_eq!(row("SUM(A5:A6)", edit), "SUM(#REF!)");
        assert_eq!(row("SUM(A6:A10)", edit), "SUM(A5:A8)");
        assert_eq!(row("SUM(A1:A5)", edit), "SUM(A1:A4)");
        assert_eq!(row("SUM(5:6)", edit), "SUM(#REF!)");
    }

    #[test]
    fn columns_work_the_same_way() {
        assert_eq!(
            edit_formula("B1+$C$2+D:E", "S", "S", &ins(Axis::Col, 3, 1)),
            "B1+$D$2+E:F"
        );
        assert_eq!(
            edit_formula("B1+C2+SUM(B:D)", "S", "S", &del(Axis::Col, 3, 1)),
            "B1+#REF!+SUM(B:C)"
        );
    }

    #[test]
    fn only_the_edited_sheet_changes() {
        let edit = ins(Axis::Row, 1, 1);
        assert_eq!(edit_formula("A1+S!A1", "T", "S", &edit), "A1+S!A2");
        assert_eq!(edit_formula("A1+T!A1", "T", "S", &edit), "A1+T!A1");
        assert_eq!(edit_formula("A1+s!A1", "T", "S", &edit), "A1+s!A2");
        assert_eq!(
            edit_formula("'My Sheet'!A1:B2+A1", "T", "My Sheet", &edit),
            "'My Sheet'!A2:B3+A1"
        );
        assert_eq!(edit_formula("'it''s'!A1", "T", "it's", &edit), "'it''s'!A2");
        assert_eq!(edit_formula("S!A1:S!B2", "T", "S", &edit), "S!A2:B3");
        assert_eq!(
            edit_formula("'ورقة 1'!A1&\"A1\"", "T", "ورقة 1", &edit),
            "'ورقة 1'!A2&\"A1\""
        );
    }

    #[test]
    fn leaves_other_workbooks_tables_names_and_text_alone() {
        let edit = ins(Axis::Row, 1, 1);
        for text in [
            "[1]S!A1",
            "'[1]S'!A1",
            "S:T!A1",
            "Table1[Col]+Rate1",
            "\"A1\"&LOG10(2)",
            "#REF!+#N/A",
            "S!#REF!",
            "1.5E+3",
            "TRUE",
            "T1[A1]",
            "Table1[@[A1]]",
        ] {
            assert_eq!(edit_formula(text, "S", "S", &edit), text, "{text}");
        }
    }

    #[test]
    fn a_range_ending_in_a_function_keeps_its_first_cell_moving() {
        assert_eq!(
            row("A1:INDEX(A:A,3)", ins(Axis::Row, 1, 1)),
            "A2:INDEX(A:A,3)"
        );
    }

    #[test]
    fn renames_keep_each_reference_in_place() {
        assert_eq!(rename_in_formula("S!A1+A1", "S", "New"), "New!A1+A1");
        assert_eq!(rename_in_formula("s!A1:B2", "S", "My S"), "'My S'!A1:B2");
        assert_eq!(rename_in_formula("T!A1", "S", "New"), "T!A1");
        assert_eq!(rename_in_formula("'S'!$A$1", "S", "A1"), "'A1'!$A$1");
        assert_eq!(rename_in_formula("[1]S!A1", "S", "New"), "[1]S!A1");
    }

    #[test]
    fn a_table_that_has_gone_leaves_ref() {
        let names = ["Sales".to_string()];
        assert_eq!(
            delete_tables_in_formula("SUM(sales[Cost])+Other[a]+\"Sales[x]\"", &names),
            "SUM(#REF!)+Other[a]+\"Sales[x]\""
        );
        assert!(reads_tables("Sales[[#All],[Cost]]", &names));
        assert!(!reads_tables("SalesTax[x]", &names));
    }

    #[test]
    fn deleting_a_sheet_leaves_ref() {
        assert_eq!(delete_sheet_in_formula("S!A1+T!A1", "S"), "#REF!+T!A1");
        assert_eq!(delete_sheet_in_formula("SUM('S'!A1:B2)", "S"), "SUM(#REF!)");
    }

    #[test]
    fn a_list_of_edits_follows_renames() {
        let edits = [
            BookEdit::Rename {
                old: "S".to_string(),
                new: "T".to_string(),
            },
            BookEdit::Axis {
                sheet: "T".to_string(),
                edit: ins(Axis::Row, 1, 1),
            },
        ];
        assert_eq!(apply_edits("A1+S!B2", "S", &edits), "A2+T!B3");
    }

    #[test]
    fn sqrefs_move_and_lose_what_is_gone() {
        assert_eq!(edit_sqref("A1:B2 C5", &ins(Axis::Row, 2, 1)), "A1:B3 C6");
        assert_eq!(edit_sqref("A1:B2 C5", &del(Axis::Row, 5, 1)), "A1:B2");
        assert_eq!(edit_sqref("C5", &del(Axis::Row, 5, 1)), "");
        assert_eq!(edit_sqref("A1:A3", &del(Axis::Row, 2, 2)), "A1");
    }

    #[test]
    fn tells_which_formulas_a_delete_changes_beyond_position() {
        let edit = del(Axis::Row, 5, 2);
        assert!(touches_band("A5", "S", "S", &edit));
        assert!(touches_band("SUM(A1:A6)", "S", "S", &edit));
        assert!(!touches_band("SUM(A1:A9)", "S", "S", &edit));
        assert!(!touches_band("A7", "S", "S", &edit));
        assert!(!touches_band("T!A5", "S", "S", &edit));
        assert!(touches_band("S!A5", "T", "S", &edit));
        assert!(touches_band("SUM(5:5)", "S", "S", &edit));
    }

    #[test]
    fn copied_formulas_move_their_relative_parts() {
        assert_eq!(moved_formula("A1+$B$2+$C3+D$4", 1, 1), "B2+$B$2+$C4+E$4");
        assert_eq!(moved_formula("SUM(A1:B2)", 2, 0), "SUM(A3:B4)");
        assert_eq!(moved_formula("S!A1", 1, 0), "S!A2");
        assert_eq!(moved_formula("A1", -1, 0), "#REF!");
        assert_eq!(moved_formula("A1+B2", 0, -1), "#REF!+A2");
        assert_eq!(moved_formula("SUM(A:B)", 5, 1), "SUM(B:C)");
    }

    #[test]
    fn references_follow_moved_cells() {
        let moved = MovedCells {
            from_sheet: "S",
            area: (1, 1, 2, 1),
            to_sheet: "S",
            rows: 0,
            cols: 3,
        };
        assert_eq!(follow_moved_cells("A1+A2+A3", "S", &moved), "D1+D2+A3");
        assert_eq!(follow_moved_cells("SUM(A1:A2)", "S", &moved), "SUM(D1:D2)");
        assert_eq!(follow_moved_cells("SUM(A1:A3)", "S", &moved), "SUM(A1:A3)");
        assert_eq!(follow_moved_cells("S!A1", "T", &moved), "S!D1");
        assert_eq!(follow_moved_cells("A1", "T", &moved), "A1");
        assert_eq!(follow_moved_cells("D1", "S", &moved), "#REF!");
        let across = MovedCells {
            from_sheet: "S",
            area: (1, 1, 1, 1),
            to_sheet: "T",
            rows: 1,
            cols: 0,
        };
        assert_eq!(follow_moved_cells("A1", "S", &across), "T!A2");
        assert_eq!(follow_moved_cells("A1", "T", &across), "A1");
        assert_eq!(follow_moved_cells("S!A1", "T", &across), "T!A2");
    }

    #[test]
    fn moving_a_ranges_edge_moves_the_edge() {
        let moved = |area, rows, cols| MovedCells {
            from_sheet: "S",
            area,
            to_sheet: "S",
            rows,
            cols,
        };
        // Inward, what is left of the range stays.
        assert_eq!(
            follow_moved_cells("SUM(E2:E13)", "S", &moved((2, 5, 3, 5), 6, 0)),
            "SUM(E4:E13)"
        );
        // Outward, the edge goes with it.
        assert_eq!(
            follow_moved_cells("SUM(A1:A5)", "S", &moved((5, 1, 5, 1), 5, 0)),
            "SUM(A1:A10)"
        );
        assert_eq!(
            follow_moved_cells("SUM(A1:D1)", "S", &moved((1, 4, 1, 4), 0, 2)),
            "SUM(A1:F1)"
        );
        // A block that only takes part of an edge leaves it.
        assert_eq!(
            follow_moved_cells("SUM(A1:B5)", "S", &moved((5, 1, 5, 1), 5, 0)),
            "SUM(A1:B5)"
        );
        // Landing on a range's edge takes it away.
        assert_eq!(
            follow_moved_cells("SUM($C$2:C5)", "S", &moved((10, 6, 11, 6), -5, -3)),
            "SUM($C$2:C4)"
        );
        // Ending exactly on the edge leaves the range as it was.
        assert_eq!(
            follow_moved_cells("SUM(C2:C6)", "S", &moved((10, 6, 11, 6), -5, -3)),
            "SUM(C2:C6)"
        );
        assert_eq!(
            follow_moved_cells("A2:A4*2", "S", &moved((1, 1, 2, 1), 3, 0)),
            "A3:A5*2"
        );
    }

    #[test]
    fn a_lost_table_column_leaves_ref() {
        let columns = ["b".to_string()];
        assert_eq!(
            delete_table_columns("SUM(T[b])+T[a]", "T", &columns),
            "SUM(#REF!)+T[a]"
        );
        assert_eq!(
            delete_table_columns("T[[#All],[B]]", "T", &columns),
            "#REF!"
        );
        assert_eq!(delete_table_columns("U[b]", "T", &columns), "U[b]");
    }

    #[test]
    fn a_renamed_table_column_follows() {
        let run = |text, inside| rename_table_column(text, "T", "b", "new", inside).into_owned();
        assert_eq!(run("SUM(T[b])", false), "SUM(T[new])");
        assert_eq!(run("SUM(t[B])+U[b]", false), "SUM(t[new])+U[b]");
        assert_eq!(run("T[[#All],[b]]", false), "T[[#All],[new]]");
        assert_eq!(run("T[@b]", false), "T[@new]");
        assert_eq!(run("T[@[b]]", false), "T[@[new]]");
        assert_eq!(run("[@b]*2", false), "[@b]*2");
        assert_eq!(run("[@b]*2", true), "[@new]*2");
        assert_eq!(run("\"T[b]\"", false), "\"T[b]\"");
        assert_eq!(
            rename_table_column("T[@b]+T[b]", "T", "b", "a b", false),
            "T[@[a b]]+T[a b]"
        );
        assert_eq!(
            rename_table_column("T[b]", "T", "b", "x[y", false),
            "T[x'[y]"
        );
        assert_eq!(
            rename_table_column("T[x'[y]", "T", "x[y", "b", false),
            "T[b]"
        );
    }

    #[test]
    fn quotes_names_that_need_it() {
        assert_eq!(quote_sheet("Sheet1"), "Sheet1");
        assert_eq!(quote_sheet("My Sheet"), "'My Sheet'");
        assert_eq!(quote_sheet("A1"), "'A1'");
        assert_eq!(quote_sheet("1st"), "'1st'");
        assert_eq!(quote_sheet("R1C1"), "'R1C1'");
        assert_eq!(quote_sheet("it's"), "'it''s'");
        assert_eq!(quote_sheet("ورقة"), "ورقة");
        assert_eq!(quote_sheet("a-b"), "'a-b'");
    }
}
