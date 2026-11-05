//! Find and replace. Cells are looked at as a person sees them: by what they show, or by what the
//! formula bar shows. `*` and `?` are wildcards, and `~` makes the next one plain, as in Excel.

use cgs_calc::{CalcEngine, CalcValue, CellRef, CellSink, FormulaShape, Rect, SheetRect};
use serde::Deserialize;

use crate::command::{Area, Op};
use crate::edit::{EditCell, EditInput};
use crate::input::parse_input;
use crate::tile::{self, TileInput};
use crate::{json, Engine, Session};

/// A search, as the dialog sets it up.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindQuery {
    pub text: String,
    /// The sheet showing, where a search starts.
    pub sheet: u32,
    /// All the workbook's sheets, in tab order, rather than this one.
    #[serde(default)]
    pub workbook: bool,
    /// Down each column first, rather than along each row.
    #[serde(default)]
    pub by_columns: bool,
    /// What the formula bar shows, rather than what the cell shows.
    #[serde(default)]
    pub formulas: bool,
    #[serde(default)]
    pub match_case: bool,
    /// All of a cell's text has to match.
    #[serde(default)]
    pub whole_cell: bool,
    /// Only these, as when several cells are selected.
    #[serde(default)]
    pub within: Option<Area>,
}

/// A pattern's pieces.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Char(char),
    Any,
    Many,
}

fn fold(ch: char, keep_case: bool) -> char {
    if keep_case {
        ch
    } else {
        ch.to_lowercase().next().unwrap_or(ch)
    }
}

fn compile(text: &str, match_case: bool) -> Vec<Piece> {
    let mut out = Vec::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '*' => out.push(Piece::Many),
            '?' => out.push(Piece::Any),
            '~' => match chars.next() {
                Some(next @ ('*' | '?' | '~')) => out.push(Piece::Char(next)),
                Some(next) => {
                    out.push(Piece::Char('~'));
                    out.push(Piece::Char(fold(next, match_case)));
                }
                None => out.push(Piece::Char('~')),
            },
            other => out.push(Piece::Char(fold(other, match_case))),
        }
    }
    out
}

/// How far a pattern matches from the start of `chars`: the longest it can, as `*` is greedy.
fn match_from(pattern: &[Piece], chars: &[char]) -> Option<usize> {
    // Set of pattern positions reachable at each text position, kept small by being a bit list.
    let mut states = vec![false; pattern.len() + 1];
    states[0] = true;
    let close = |states: &mut Vec<bool>| {
        for at in 0..pattern.len() {
            if states[at] && pattern[at] == Piece::Many {
                states[at + 1] = true;
            }
        }
    };
    close(&mut states);
    let mut best = states[pattern.len()].then_some(0);
    for (index, ch) in chars.iter().enumerate() {
        let mut next = vec![false; pattern.len() + 1];
        let mut alive = false;
        for at in 0..pattern.len() {
            if !states[at] {
                continue;
            }
            match &pattern[at] {
                Piece::Many => {
                    next[at] = true;
                    alive = true;
                }
                Piece::Any => {
                    next[at + 1] = true;
                    alive = true;
                }
                Piece::Char(want) => {
                    if want == ch {
                        next[at + 1] = true;
                        alive = true;
                    }
                }
            }
        }
        if !alive {
            break;
        }
        close(&mut next);
        states = next;
        if states[pattern.len()] {
            best = Some(index + 1);
        }
    }
    best
}

/// Where a pattern matches in text, as character positions, first to last.
fn matches_in(pattern: &[Piece], chars: &[char], whole: bool) -> Vec<(usize, usize)> {
    if whole {
        return match match_from(pattern, chars) {
            Some(end) if end == chars.len() => vec![(0, end)],
            _ => Vec::new(),
        };
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at <= chars.len() {
        match match_from(pattern, &chars[at..]) {
            Some(length) if length > 0 => {
                out.push((at, at + length));
                at += length;
            }
            _ => at += 1,
        }
    }
    out
}

fn folded(text: &str, match_case: bool) -> Vec<char> {
    text.chars().map(|ch| fold(ch, match_case)).collect()
}

/// Reads a block in one go, keeping values.
#[derive(Default)]
struct Filled(Vec<(u32, u32, CalcValue)>);

impl CellSink for Filled {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        if *value != CalcValue::Empty {
            self.0.push((row, col, value.clone()));
        }
    }
}

/// A match, for the dialog's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub sheet: u32,
    pub row: u32,
    pub col: u32,
    /// What it shows, or what the formula bar shows.
    pub text: String,
}

/// Past this, a list of matches stops, as nobody reads further.
const MAX_LISTED: usize = 10_000;

impl Session {
    /// The sheets a search goes through, starting with the one showing.
    fn search_sheets(&self, query: &FindQuery) -> Vec<u32> {
        if !query.workbook {
            return vec![query.sheet];
        }
        let order = self.book.tab_order();
        let start = order
            .iter()
            .position(|&sheet| sheet == query.sheet)
            .unwrap_or(0);
        let mut out = Vec::with_capacity(order.len());
        out.extend_from_slice(&order[start..]);
        out.extend_from_slice(&order[..start]);
        out
    }

    fn cells_of(&self, sheet: u32, query: &FindQuery) -> Vec<(u32, u32, CalcValue)> {
        let Some(info) = self.book.sheets.get(sheet as usize) else {
            return Vec::new();
        };
        if !info.loaded || info.used.0 == 0 || info.used.1 == 0 {
            return Vec::new();
        }
        let mut rect = Rect {
            top: 1,
            left: 1,
            bottom: info.used.0,
            right: info.used.1,
        };
        if let Some(area) = query.within.filter(|_| sheet == query.sheet) {
            rect.top = rect.top.max(area.top);
            rect.left = rect.left.max(area.left);
            rect.bottom = rect.bottom.min(area.bottom);
            rect.right = rect.right.min(area.right);
            if rect.top > rect.bottom || rect.left > rect.right {
                return Vec::new();
            }
        }
        let mut found = Filled::default();
        self.calc.read_block(SheetRect { sheet, rect }, &mut found);
        if query.by_columns {
            found.0.sort_unstable_by_key(|&(row, col, _)| (col, row));
        } else {
            found.0.sort_unstable_by_key(|&(row, col, _)| (row, col));
        }
        found.0
    }

    /// The text a search looks at in a cell.
    fn search_text(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        formulas: bool,
        options: &cgs_numfmt::Options<'_>,
    ) -> String {
        let input = TileInput {
            book: &self.book,
            calc: &self.calc,
            options,
        };
        if formulas {
            tile::edit_text(&input, &mut self.cache, sheet, row, col)
        } else {
            tile::display_text(&input, &mut self.cache, sheet, row, col)
        }
    }

    fn cell_matches(
        &mut self,
        pattern: &[Piece],
        query: &FindQuery,
        at: (u32, u32, u32),
        value: &CalcValue,
        options: &cgs_numfmt::Options<'_>,
    ) -> Option<String> {
        // Plain text showing as itself needn't go through a format.
        let (sheet, row, col) = at;
        let text = match value {
            CalcValue::Text(text)
                if !query.formulas
                    && self
                        .book
                        .styles
                        .xf(self.book.sheets[sheet as usize].style_at(row, col))
                        .num_fmt
                        == 0 =>
            {
                text.clone()
            }
            _ => self.search_text(sheet, row, col, query.formulas, options),
        };
        let chars = folded(&text, query.match_case);
        (!matches_in(pattern, &chars, query.whole_cell).is_empty()).then_some(text)
    }

    pub(crate) fn find_next(
        &mut self,
        query: &FindQuery,
        from: (u32, u32),
        back: bool,
        options: &cgs_numfmt::Options<'_>,
    ) -> Option<Found> {
        if query.text.is_empty() {
            return None;
        }
        let pattern = compile(&query.text, query.match_case);
        let sheets = self.search_sheets(query);
        let key = |by_columns: bool, row: u32, col: u32| {
            if by_columns {
                (col, row)
            } else {
                (row, col)
            }
        };
        let here = key(query.by_columns, from.0, from.1);
        let mut order: Vec<u32> = sheets.clone();
        if back {
            // Backwards from the sheet showing, ending on its cells past the start.
            order[1..].reverse();
        }
        order.push(query.sheet);
        for (index, &sheet) in order.iter().enumerate() {
            let cells = self.cells_of(sheet, query);
            let first = index == 0;
            let last = index + 1 == order.len();
            let mut list: Vec<&(u32, u32, CalcValue)> = if back {
                cells.iter().rev().collect()
            } else {
                cells.iter().collect()
            };
            list.retain(|&&(row, col, _)| {
                let at = key(query.by_columns, row, col);
                if first {
                    if back {
                        at < here
                    } else {
                        at > here
                    }
                } else if last {
                    if back {
                        at >= here
                    } else {
                        at <= here
                    }
                } else {
                    true
                }
            });
            for (row, col, value) in list.iter().map(|cell| (cell.0, cell.1, &cell.2)) {
                if let Some(text) =
                    self.cell_matches(&pattern, query, (sheet, row, col), value, options)
                {
                    return Some(Found {
                        sheet,
                        row,
                        col,
                        text,
                    });
                }
            }
        }
        None
    }

    pub(crate) fn find_all(
        &mut self,
        query: &FindQuery,
        options: &cgs_numfmt::Options<'_>,
    ) -> Vec<Found> {
        let mut out = Vec::new();
        if query.text.is_empty() {
            return out;
        }
        let pattern = compile(&query.text, query.match_case);
        for sheet in self.search_sheets(query) {
            for (row, col, value) in self.cells_of(sheet, query) {
                if let Some(text) =
                    self.cell_matches(&pattern, query, (sheet, row, col), &value, options)
                {
                    out.push(Found {
                        sheet,
                        row,
                        col,
                        text,
                    });
                    if out.len() >= MAX_LISTED {
                        return out;
                    }
                }
            }
        }
        out
    }

    /// The edit that replaces what a search finds in one cell, if it can. Arrays and spills stay as
    /// they are, and so does a formula the replacement breaks, as in Excel.
    fn replaced_edit(
        &mut self,
        pattern: &[Piece],
        query: &FindQuery,
        with: &str,
        at: (u32, u32, u32),
        options: &cgs_numfmt::Options<'_>,
        locale: &cgs_numfmt::Locale,
    ) -> Option<EditCell> {
        let (sheet, row, col) = at;
        let cell = CellRef { sheet, row, col };
        if matches!(
            self.calc.formula_shape(cell),
            FormulaShape::Array(_) | FormulaShape::Dynamic(_)
        ) {
            return None;
        }
        if self.calc.spill_areas().iter().any(|spill| {
            spill.sheet == sheet
                && spill.rect.contains(row, col)
                && (spill.rect.top, spill.rect.left) != (row, col)
        }) {
            return None;
        }
        let text = self.search_text(sheet, row, col, true, options);
        let chars: Vec<char> = text.chars().collect();
        let keys = folded(&text, query.match_case);
        let found = matches_in(pattern, &keys, query.whole_cell);
        if found.is_empty() {
            return None;
        }
        let mut next = String::with_capacity(text.len() + with.len());
        let mut copied = 0;
        for (from, to) in found {
            next.extend(&chars[copied..from]);
            next.push_str(with);
            copied = to;
        }
        next.extend(&chars[copied..]);
        if next == text {
            return None;
        }
        let mut typed = parse_input(&next, locale, self.book.date1904, self.today);
        if let crate::input::Typed::Formula(formula) = &typed {
            let fixed = self.calc.check_formula(formula).ok()?;
            typed = crate::input::Typed::Formula(fixed);
        }
        Some(self.typed_edit_at(sheet, (row, col), &typed))
    }

    /// Replaces in one cell, and says what happened.
    pub(crate) fn replace_one(
        &mut self,
        query: &FindQuery,
        with: &str,
        at: (u32, u32, u32),
        options: &cgs_numfmt::Options<'_>,
        locale: &cgs_numfmt::Locale,
    ) -> Option<Op> {
        let pattern = compile(&query.text, query.match_case);
        let edit = self.replaced_edit(&pattern, query, with, at, options, locale)?;
        Some(Op::Cells(vec![edit]))
    }

    /// Replaces everywhere a search reaches, and says how many cells changed.
    pub(crate) fn replace_all(
        &mut self,
        query: &FindQuery,
        with: &str,
        options: &cgs_numfmt::Options<'_>,
        locale: &cgs_numfmt::Locale,
    ) -> Vec<EditCell> {
        let pattern = compile(&query.text, query.match_case);
        let mut edits = Vec::new();
        if query.text.is_empty() {
            return edits;
        }
        for sheet in self.search_sheets(query) {
            for (row, col, _) in self.cells_of(sheet, query) {
                if let Some(edit) =
                    self.replaced_edit(&pattern, query, with, (sheet, row, col), options, locale)
                {
                    if let Some(input) = &edit.input {
                        // A heading stays a heading: `perform` looks after the table's side.
                        if !matches!(input, EditInput::Clear) || edit.style.is_some() {
                            edits.push(edit);
                            continue;
                        }
                    }
                    edits.push(edit);
                }
            }
        }
        edits
    }
}

impl Engine {
    /// The next match after a cell, going round, as JSON: `null` if there's none. `query` is a
    /// `FindQuery` as JSON.
    pub fn find_next_json(
        &mut self,
        query: &str,
        row: u32,
        col: u32,
        back: bool,
    ) -> Result<String, String> {
        let query: FindQuery = serde_json::from_str(query).map_err(|error| error.to_string())?;
        Ok(self.find_next_of(&query, row, col, back))
    }

    fn find_next_of(&mut self, query: &FindQuery, row: u32, col: u32, back: bool) -> String {
        let locale = self.locale.clone();
        let Some(session) = self.session.as_mut() else {
            return "null".to_string();
        };
        let options = cgs_numfmt::Options {
            locale: &locale,
            date1904: session.book.date1904,
        };
        match session.find_next(query, (row, col), back, &options) {
            Some(found) => found_json(&found),
            None => "null".to_string(),
        }
    }

    /// Every match, as JSON.
    pub fn find_all_json(&mut self, query: &str) -> Result<String, String> {
        let query: FindQuery = serde_json::from_str(query).map_err(|error| error.to_string())?;
        Ok(self.find_all_of(&query))
    }

    fn find_all_of(&mut self, query: &FindQuery) -> String {
        let locale = self.locale.clone();
        let Some(session) = self.session.as_mut() else {
            return "[]".to_string();
        };
        let options = cgs_numfmt::Options {
            locale: &locale,
            date1904: session.book.date1904,
        };
        let list = session.find_all(query, &options);
        let mut out = String::from("[");
        for (index, found) in list.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&found_json(found));
        }
        out.push(']');
        out
    }
}

fn found_json(found: &Found) -> String {
    let mut out = format!(
        "{{\"sheet\":{},\"row\":{},\"col\":{},\"text\":",
        found.sheet, found.row, found.col
    );
    json::push_string(&mut out, &found.text);
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(pattern: &str, text: &str, keep_case: bool, whole: bool) -> Vec<(usize, usize)> {
        let compiled = compile(pattern, keep_case);
        matches_in(&compiled, &folded(text, keep_case), whole)
    }

    #[test]
    fn plain_text_matches_anywhere_and_in_any_case() {
        assert_eq!(hits("ab", "xABxab", false, false), vec![(1, 3), (4, 6)]);
        assert_eq!(hits("ab", "xABxab", true, false), vec![(4, 6)]);
        assert!(hits("ab", "b", false, false).is_empty());
    }

    #[test]
    fn wildcards_read_as_excel_reads_them() {
        assert_eq!(hits("a?c", "abc", false, true), vec![(0, 3)]);
        assert!(hits("a?c", "abbc", false, true).is_empty());
        assert_eq!(hits("a*c", "abbc", false, true), vec![(0, 4)]);
        assert_eq!(hits("a*", "xab", false, false), vec![(1, 3)]);
        assert_eq!(hits("~*", "a*b", false, false), vec![(1, 2)]);
        assert_eq!(hits("~?", "a?b", false, false), vec![(1, 2)]);
        assert_eq!(hits("~~", "a~b", false, false), vec![(1, 2)]);
        assert_eq!(hits("*", "abc", false, true), vec![(0, 3)]);
    }

    #[test]
    fn a_whole_cell_has_to_match_all_of_it() {
        assert!(hits("ab", "abc", false, true).is_empty());
        assert_eq!(hits("ab", "AB", false, true), vec![(0, 2)]);
    }
}
