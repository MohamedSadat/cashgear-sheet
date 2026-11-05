//! What a formula read from a file means that the engine has no form for. Each of these changes
//! the text the engine gets, so the cell's formula then reads the way it is rewritten here.
//!
//! - A defined name that holds relative references, such as `Sheet1!$B1`, is written as seen from
//!   A1, and reads a different cell for each formula. The engine keeps one value per name, so such
//!   a name goes into the formula, moved to where the formula stands.
//! - `[Column]` in a table's own cell means that table's column.
//! - `SUM(Sheet1:Sheet3!A1)` reads that cell on each sheet from the first to the last, which is the
//!   same as naming them all.

use std::collections::{HashMap, HashSet};

use cgs_refs::{col_to_letters, quote_sheet};
use formualizer_parse::parser::{
    parse, ASTNode, ASTNodeType, ReferenceType, TableRowSpecifier, TableSpecifier,
};

use crate::{NameInfo, TableInfo};

const MAX_ROW: i64 = 1_048_576;
const MAX_COL: i64 = 16_384;
/// A name can read a name, and a file can make them read each other.
const MAX_DEPTH: usize = 8;

/// A name's definition can start a reference with `!`, for the sheet the formula is on.
fn without_bare_sheet_marks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut quote = None;
    let mut previous = None;
    for ch in text.chars() {
        match quote {
            Some(open) => {
                if ch == open {
                    quote = None;
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch == '!'
                    && !previous.is_some_and(|before: char| {
                        before.is_alphanumeric()
                            || matches!(before, '_' | '.' | '\'' | '$' | ']' | '#')
                    })
                {
                    previous = Some(ch);
                    continue;
                }
            }
        }
        out.push(ch);
        previous = Some(ch);
    }
    out
}

/// Every reference in a formula, in the order they are read, with what it stands in.
fn references<'a>(node: &'a ASTNode, out: &mut Vec<&'a ASTNode>) {
    match &node.node_type {
        ASTNodeType::Reference { .. } => out.push(node),
        ASTNodeType::UnaryOp { expr, .. } => references(expr, out),
        ASTNodeType::BinaryOp { left, right, .. } => {
            references(left, out);
            references(right, out);
        }
        ASTNodeType::Function { args, .. } => {
            for arg in args {
                references(arg, out);
            }
        }
        ASTNodeType::Call { callee, args } => {
            references(callee, out);
            for arg in args {
                references(arg, out);
            }
        }
        ASTNodeType::Array(rows) => {
            for row in rows {
                for cell in row {
                    references(cell, out);
                }
            }
        }
        ASTNodeType::Literal(_) | ASTNodeType::Omitted => {}
    }
}

/// Whether a reference reads something different from one formula to the next.
fn moves_with_formula(reference: &ReferenceType) -> bool {
    match reference {
        ReferenceType::Cell {
            row_abs, col_abs, ..
        } => !row_abs || !col_abs,
        ReferenceType::Range {
            start_row,
            start_col,
            end_row,
            end_col,
            start_row_abs,
            start_col_abs,
            end_row_abs,
            end_col_abs,
            ..
        } => {
            (start_row.is_some() && !start_row_abs)
                || (start_col.is_some() && !start_col_abs)
                || (end_row.is_some() && !end_row_abs)
                || (end_col.is_some() && !end_col_abs)
        }
        ReferenceType::Table(table) => table_this_row(table.specifier.as_ref()),
        _ => false,
    }
}

fn table_this_row(specifier: Option<&TableSpecifier>) -> bool {
    match specifier {
        Some(TableSpecifier::Row(TableRowSpecifier::Current)) => true,
        Some(TableSpecifier::Combination(parts)) => {
            parts.iter().any(|part| table_this_row(Some(part.as_ref())))
        }
        _ => false,
    }
}

/// Keeps a position on the sheet when a relative reference moves past its edge, as Excel does.
fn wrapped(place: u32, by: u32, size: i64) -> u32 {
    let moved = (i64::from(place) - 1 + i64::from(by) - 1).rem_euclid(size);
    (moved + 1) as u32
}

fn cell_text(col: Option<(u32, bool)>, row: Option<(u32, bool)>) -> String {
    let mut out = String::new();
    if let Some((col, absolute)) = col {
        if absolute {
            out.push('$');
        }
        out.push_str(&col_to_letters(i64::from(col)));
    }
    if let Some((row, absolute)) = row {
        if absolute {
            out.push('$');
        }
        out.push_str(&row.to_string());
    }
    out
}

/// The reference's text as seen from `(row, col)`, keeping how it names its sheet.
fn moved_reference(token: &str, reference: &ReferenceType, row: u32, col: u32) -> Option<String> {
    let prefix = token.rfind('!').map_or("", |bang| &token[..=bang]);
    let move_row = |place: u32, absolute: bool| {
        if absolute {
            place
        } else {
            wrapped(place, row, MAX_ROW)
        }
    };
    let move_col = |place: u32, absolute: bool| {
        if absolute {
            place
        } else {
            wrapped(place, col, MAX_COL)
        }
    };
    let text = match reference {
        ReferenceType::Cell {
            row: at_row,
            col: at_col,
            row_abs,
            col_abs,
            ..
        } => cell_text(
            Some((move_col(*at_col, *col_abs), *col_abs)),
            Some((move_row(*at_row, *row_abs), *row_abs)),
        ),
        ReferenceType::Range {
            start_row,
            start_col,
            end_row,
            end_col,
            start_row_abs,
            start_col_abs,
            end_row_abs,
            end_col_abs,
            ..
        } => {
            let start = cell_text(
                start_col.map(|at| (move_col(at, *start_col_abs), *start_col_abs)),
                start_row.map(|at| (move_row(at, *start_row_abs), *start_row_abs)),
            );
            let end = cell_text(
                end_col.map(|at| (move_col(at, *end_col_abs), *end_col_abs)),
                end_row.map(|at| (move_row(at, *end_row_abs), *end_row_abs)),
            );
            format!("{start}:{end}")
        }
        _ => return None,
    };
    Some(format!("{prefix}{text}"))
}

/// Defined names that mean something different from one formula to the next.
#[derive(Default)]
pub(crate) struct MovingNames {
    /// Lower-case name to its definitions, at workbook scope or a sheet's.
    definitions: HashMap<String, Vec<(Option<u32>, String)>>,
}

impl MovingNames {
    pub(crate) fn new(names: &[NameInfo]) -> Self {
        struct Entry {
            name: String,
            sheet: Option<u32>,
            text: String,
            reads: Vec<String>,
            moves: bool,
        }
        let mut all = Vec::new();
        for name in names {
            let text = without_bare_sheet_marks(name.formula.trim().trim_start_matches('='));
            let Ok(ast) = parse(format!("={text}")) else {
                continue;
            };
            let mut found = Vec::new();
            references(&ast, &mut found);
            let moves = found.iter().any(|node| {
                matches!(&node.node_type,
                    ASTNodeType::Reference { reference, original } if moves_with_formula(reference)
                        || original.contains("#This Row")
                        || original.contains("[@"))
            });
            let reads = found
                .iter()
                .filter_map(|node| match &node.node_type {
                    ASTNodeType::Reference {
                        reference: ReferenceType::NamedRange(other),
                        ..
                    } => Some(other.to_ascii_lowercase()),
                    _ => None,
                })
                .collect();
            all.push(Entry {
                name: name.name.to_ascii_lowercase(),
                sheet: name.local_sheet,
                text,
                reads,
                moves,
            });
        }
        // A name that reads such a name is one too.
        let mut moving: HashSet<String> = all
            .iter()
            .filter(|entry| entry.moves)
            .map(|entry| entry.name.clone())
            .collect();
        loop {
            let before = moving.len();
            for entry in &all {
                if entry.reads.iter().any(|other| moving.contains(other)) {
                    moving.insert(entry.name.clone());
                }
            }
            if moving.len() == before {
                break;
            }
        }
        let mut definitions: HashMap<String, Vec<(Option<u32>, String)>> = HashMap::new();
        for entry in all {
            if moving.contains(&entry.name) {
                definitions
                    .entry(entry.name)
                    .or_default()
                    .push((entry.sheet, entry.text));
            }
        }
        Self { definitions }
    }

    fn definition(&self, name: &str, sheet: u32) -> Option<&str> {
        let entries = self.definitions.get(&name.to_ascii_lowercase())?;
        entries
            .iter()
            .find(|(local, _)| *local == Some(sheet))
            .or_else(|| entries.iter().find(|(local, _)| local.is_none()))
            .map(|(_, text)| text.as_str())
    }

    /// `text` with each such name replaced by what it reads from `(row, col)`.
    pub(crate) fn expand(&self, text: &str, sheet: u32, row: u32, col: u32) -> Option<String> {
        if self.definitions.is_empty()
            || !crate::intersect::mentions_name(text, &|word| {
                self.definitions.contains_key(&word.to_ascii_lowercase())
            })
        {
            return None;
        }
        self.rewrite(text, sheet, row, col, false, 0)
    }

    /// With `moved`, this is a name's definition, whose own references move as well.
    fn rewrite(
        &self,
        text: &str,
        sheet: u32,
        row: u32,
        col: u32,
        moved: bool,
        depth: usize,
    ) -> Option<String> {
        if depth > MAX_DEPTH {
            return None;
        }
        let source = format!("={text}");
        let ast = parse(&source).ok()?;
        let mut found = Vec::new();
        references(&ast, &mut found);
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        for node in found {
            let ASTNodeType::Reference { reference, .. } = &node.node_type else {
                continue;
            };
            let Some(token) = &node.source_token else {
                continue;
            };
            let spelled = source.get(token.start..token.end)?;
            if let ReferenceType::NamedRange(name) = reference {
                if let Some(definition) = self.definition(name, sheet) {
                    let inner = self.rewrite(definition, sheet, row, col, true, depth + 1)?;
                    let inner = inner.trim_start_matches('=').to_string();
                    // A lone reference stays a reference, which brackets would hide from a
                    // function that wants one.
                    let lone = parse(format!("={inner}"))
                        .ok()
                        .is_some_and(|ast| matches!(ast.node_type, ASTNodeType::Reference { .. }));
                    let inner = if lone { inner } else { format!("({inner})") };
                    edits.push((token.start, token.end, inner));
                }
            } else if moved && moves_with_formula(reference) {
                if let Some(next) = moved_reference(spelled, reference, row, col) {
                    edits.push((token.start, token.end, next));
                }
            }
        }
        if edits.is_empty() && !moved {
            return None;
        }
        splice(&source, edits)
    }
}

/// `source` with spans replaced. The spans don't overlap.
fn splice(source: &str, mut edits: Vec<(usize, usize, String)>) -> Option<String> {
    edits.sort_by_key(|edit| edit.0);
    let mut out = String::with_capacity(source.len());
    let mut from = 0;
    for (start, end, text) in edits {
        out.push_str(source.get(from..start)?);
        out.push_str(&text);
        from = end;
    }
    out.push_str(source.get(from..)?);
    Some(out)
}

/// A column's name holds a line end as one character, and a file can write two inside `[...]`.
pub(crate) fn column_line_ends(text: &str) -> Option<String> {
    if !text.contains("\r\n") {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut quote = None;
    let mut depth = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match quote {
            Some(open) => {
                if ch == open {
                    quote = None;
                }
            }
            None => match ch {
                '"' if depth == 0 => quote = Some(ch),
                '\'' if depth == 0 => quote = Some(ch),
                '[' => depth += 1,
                ']' => depth -= 1,
                '\r' if depth > 0 && chars.peek() == Some(&'\n') => continue,
                _ => {}
            },
        }
        out.push(ch);
    }
    (out.len() != text.len()).then_some(out)
}

/// `text` with each `[Column]` that names no table given the table the formula stands in.
pub(crate) fn qualify_bare_columns(
    text: &str,
    tables: &[TableInfo],
    sheet: u32,
    row: u32,
    col: u32,
) -> Option<String> {
    if !text.contains('[') {
        return None;
    }
    let table = tables
        .iter()
        .find(|table| table.sheet == sheet && table.range.contains(row, col))?;
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + table.name.len());
    let mut from = 0;
    let mut at = 0;
    let mut changed = false;
    while at < bytes.len() {
        match bytes[at] {
            quote @ (b'"' | b'\'') => {
                at += 1;
                while at < bytes.len() && bytes[at] != quote {
                    at += 1;
                }
                at += 1;
            }
            b'[' => {
                let named = at > 0
                    && (bytes[at - 1].is_ascii_alphanumeric()
                        || matches!(bytes[at - 1], b'_' | b'.' | b'\\' | b']' | b'\'')
                        || bytes[at - 1] >= 0x80);
                if !named {
                    out.push_str(&text[from..at]);
                    out.push_str(&table.name);
                    from = at;
                    changed = true;
                }
                // To the matching bracket. An apostrophe in here escapes what follows it.
                let mut depth = 0;
                while at < bytes.len() {
                    match bytes[at] {
                        b'\'' => at += 1,
                        b'[' => depth += 1,
                        b']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    at += 1;
                }
                at += 1;
            }
            _ => at += 1,
        }
    }
    changed.then(|| {
        out.push_str(&text[from..]);
        out
    })
}

fn three_d_args<'a>(node: &'a ASTNode, out: &mut Vec<&'a ASTNode>) {
    match &node.node_type {
        ASTNodeType::UnaryOp { expr, .. } => three_d_args(expr, out),
        ASTNodeType::BinaryOp { left, right, .. } => {
            three_d_args(left, out);
            three_d_args(right, out);
        }
        ASTNodeType::Function { args, .. } => {
            for arg in args {
                if matches!(
                    &arg.node_type,
                    ASTNodeType::Reference {
                        reference: ReferenceType::Cell3D { .. } | ReferenceType::Range3D { .. },
                        ..
                    }
                ) {
                    out.push(arg);
                } else {
                    three_d_args(arg, out);
                }
            }
        }
        _ => {}
    }
}

/// `text` with each reference across sheets, given to a function, written out sheet by sheet.
pub(crate) fn expand_three_d(text: &str, sheets: &[String]) -> Option<String> {
    if !text.contains(':') || !text.contains('!') {
        return None;
    }
    let source = format!("={text}");
    let ast = parse(&source).ok()?;
    let mut found = Vec::new();
    three_d_args(&ast, &mut found);
    if found.is_empty() {
        return None;
    }
    let position = |name: &str| sheets.iter().position(|s| s.eq_ignore_ascii_case(name));
    let mut edits = Vec::new();
    for node in found {
        let ASTNodeType::Reference { reference, .. } = &node.node_type else {
            continue;
        };
        let token = node.source_token.as_ref()?;
        let spelled = source.get(token.start..token.end)?;
        let (first, last) = match reference {
            ReferenceType::Cell3D {
                sheet_first,
                sheet_last,
                ..
            }
            | ReferenceType::Range3D {
                sheet_first,
                sheet_last,
                ..
            } => (position(sheet_first)?, position(sheet_last)?),
            _ => continue,
        };
        let address = &spelled[spelled.rfind('!')? + 1..];
        let (first, last) = (first.min(last), first.max(last));
        let parts: Vec<String> = sheets[first..=last]
            .iter()
            .map(|sheet| format!("{}!{address}", quote_sheet(sheet)))
            .collect();
        edits.push((token.start, token.end, parts.join(",")));
    }
    splice(&source, edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rect;

    fn names(list: &[(&str, &str)]) -> MovingNames {
        let infos: Vec<NameInfo> = list
            .iter()
            .map(|(name, formula)| NameInfo {
                name: (*name).to_string(),
                formula: (*formula).to_string(),
                local_sheet: None,
            })
            .collect();
        MovingNames::new(&infos)
    }

    #[test]
    fn a_relative_name_reads_from_where_the_formula_stands() {
        let moving = names(&[
            ("Fixed", "Sheet1!$B$1"),
            ("RowOf", "Sheet1!$B1"),
            ("Twice", "RowOf*Sheet1!$B$1"),
            ("Span", "Sheet1!A$4:A$8"),
            ("Back", "Sheet1!XFD1048576"),
            ("Here", "!A1"),
        ]);
        assert_eq!(moving.expand("Fixed*2", 0, 5, 3), None);
        let expanded = |text| moving.expand(text, 0, 5, 3).unwrap();
        assert_eq!(expanded("RowOf*2"), "=Sheet1!$B5*2");
        assert_eq!(expanded("Twice"), "=(Sheet1!$B5*Sheet1!$B$1)");
        assert_eq!(expanded("SUM(Span)"), "=SUM(Sheet1!C$4:C$8)");
        assert_eq!(expanded("Back"), "=Sheet1!B4");
        assert_eq!(expanded("Here"), "=C5");
    }

    #[test]
    fn a_bare_column_means_the_table_it_stands_in() {
        let tables = [TableInfo {
            name: "T1".to_string(),
            sheet: 0,
            range: Rect {
                top: 1,
                left: 1,
                bottom: 5,
                right: 3,
            },
            header_row: true,
            totals_row: true,
            columns: Vec::new(),
        }];
        let qualified = |text| qualify_bare_columns(text, &tables, 0, 5, 2);
        assert_eq!(
            qualified("SUBTOTAL(109,[Col 1])").as_deref(),
            Some("SUBTOTAL(109,T1[Col 1])")
        );
        assert_eq!(qualified("SUM(T2[Col])"), None);
        assert_eq!(qualified("\"[x]\"&A1"), None);
        assert_eq!(qualified("[Column'#]").as_deref(), Some("T1[Column'#]"));
        assert_eq!(qualify_bare_columns("[Col]", &tables, 0, 9, 9), None);
    }

    #[test]
    fn a_line_end_is_one_character_in_a_column_name_only() {
        assert_eq!(
            column_line_ends("SUM(T[a\r\nb])&\"c\r\nd\"").as_deref(),
            Some("SUM(T[a\nb])&\"c\r\nd\"")
        );
        assert_eq!(column_line_ends("\"c\r\nd\""), None);
    }

    #[test]
    fn a_reference_across_sheets_names_each_sheet() {
        let sheets: Vec<String> = ["A", "B c", "D", "E"].map(String::from).to_vec();
        assert_eq!(
            expand_three_d("SUM(A:D!B$1:C2)+1", &sheets).as_deref(),
            Some("=SUM(A!B$1:C2,'B c'!B$1:C2,D!B$1:C2)+1")
        );
        assert_eq!(expand_three_d("SUM(A!A1:B2)", &sheets), None);
    }
}
