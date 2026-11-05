//! Structured references to tables, as Excel moves and shows them. A reference's brackets are read
//! into what they select, so that one can be written back in Excel's own spelling.

use std::borrow::Cow;

use super::{is_word, skip_brackets, skip_quoted};

/// A table as a formula sees it: its name and its columns' names, in order.
#[derive(Debug, Clone, Copy)]
pub struct TableColumns<'a> {
    pub name: &'a str,
    pub columns: &'a [String],
}

/// What a table reference's brackets select.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Selection {
    /// `#All`, `#Data`, `#Headers` and `#Totals`, as written.
    items: Vec<String>,
    this_row: bool,
    /// One column, or the first and last of a span of them, unescaped.
    columns: Option<(String, Option<String>)>,
}

fn unescape(text: &str) -> String {
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

fn escape(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if matches!(ch, '[' | ']' | '#' | '\'') {
            out.push('\'');
        }
        out.push(ch);
    }
    out
}

/// A column's name can go straight after `@` or inside a single pair of brackets.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.')
}

/// The pieces between the outer brackets, split at top-level commas: `[a],[b]:[c]`.
fn pieces(inner: &str) -> Option<Vec<&str>> {
    let bytes = inner.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'[' => at = skip_brackets(bytes, at),
            b',' => {
                out.push(inner[start..at].trim());
                at += 1;
                start = at;
            }
            b'\'' => at += 2,
            _ => at += 1,
        }
    }
    out.push(inner[start..].trim());
    Some(out)
}

/// `[x]` without its brackets, if that is all `piece` is.
fn bracketed(piece: &str) -> Option<&str> {
    let bytes = piece.as_bytes();
    if bytes.first() != Some(&b'[') || skip_brackets(bytes, 0) != bytes.len() {
        return None;
    }
    Some(&piece[1..piece.len() - 1])
}

fn read_columns(piece: &str) -> Option<(String, Option<String>)> {
    let bytes = piece.as_bytes();
    let first_end = skip_brackets(bytes, 0);
    let first = bracketed(&piece[..first_end])?;
    if first_end == bytes.len() {
        return Some((unescape(first), None));
    }
    let rest = piece[first_end..]
        .trim_start()
        .strip_prefix(':')?
        .trim_start();
    let last = bracketed(rest)?;
    Some((unescape(first), Some(unescape(last))))
}

/// Reads `[...]`, a whole table reference's brackets.
fn read(brackets: &str) -> Option<Selection> {
    let inner = bracketed(brackets)?;
    let mut selection = Selection {
        items: Vec::new(),
        this_row: false,
        columns: None,
    };
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        return Some(selection);
    }
    if !trimmed.starts_with('[') {
        // `[Col]`, `[@Col]`, `[@]`, `[#All]` or `[@[Col]]`.
        if let Some(rest) = trimmed.strip_prefix('@') {
            selection.this_row = true;
            let rest = rest.trim();
            if rest.is_empty() {
                return Some(selection);
            }
            if rest.starts_with('[') {
                selection.columns = Some(read_columns(rest)?);
            } else {
                selection.columns = Some((unescape(rest), None));
            }
            return Some(selection);
        }
        if trimmed.starts_with('#') {
            if trimmed.eq_ignore_ascii_case("#This Row") {
                selection.this_row = true;
            } else {
                selection.items.push(trimmed.to_string());
            }
            return Some(selection);
        }
        selection.columns = Some((unescape(inner), None));
        return Some(selection);
    }
    for piece in pieces(inner)? {
        let first = bracketed(piece).map(str::trim);
        match first {
            Some("@") => selection.this_row = true,
            Some(item) if item.eq_ignore_ascii_case("#This Row") => selection.this_row = true,
            Some(item) if item.starts_with('#') => selection.items.push(item.to_string()),
            _ => {
                if selection.columns.is_some() {
                    return None;
                }
                selection.columns = Some(read_columns(piece)?);
            }
        }
    }
    Some(selection)
}

/// The brackets Excel writes for a selection.
fn write(selection: &Selection) -> String {
    let column_part = |columns: &(String, Option<String>)| match columns {
        (first, None) => format!("[{}]", escape(first)),
        (first, Some(last)) => format!("[{}]:[{}]", escape(first), escape(last)),
    };
    match (
        &selection.columns,
        selection.this_row,
        selection.items.is_empty(),
    ) {
        (None, false, true) => "[]".to_string(),
        (None, true, true) => "[@]".to_string(),
        (Some((first, None)), false, true) => format!("[{}]", escape(first)),
        (Some((first, None)), true, true) if plain(first) => format!("[@{first}]"),
        (Some(columns), true, true) => format!("[@{}]", column_part(columns)),
        (Some(columns), false, true) => format!("[{}]", column_part(columns)),
        (columns, this_row, _) => {
            let mut parts: Vec<String> = selection
                .items
                .iter()
                .map(|item| format!("[{item}]"))
                .collect();
            if this_row {
                parts.push("[#This Row]".to_string());
            }
            if let Some(columns) = columns {
                parts.push(column_part(columns));
            }
            if parts.len() == 1 && columns.is_none() {
                return parts.remove(0);
            }
            format!("[{}]", parts.join(","))
        }
    }
}

/// Calls `found` with each table reference: where it starts, its table's name as written if it
/// names one, and where its brackets are. Another workbook's `[1]Sheet!A1` isn't one.
fn each_reference(text: &str, mut found: impl FnMut(usize, Option<&str>, usize, usize)) {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' | b'\'' => at = skip_quoted(bytes, at, bytes[at]),
            b'[' => {
                let end = skip_brackets(bytes, at);
                let mut next = end;
                while next < bytes.len() && is_word(bytes[next]) {
                    next += 1;
                }
                if bytes.get(next) != Some(&b'!') {
                    found(at, None, at, end);
                }
                at = end;
            }
            byte if is_word(byte) => {
                let start = at;
                while at < bytes.len() && is_word(bytes[at]) {
                    at += 1;
                }
                if bytes.get(at) == Some(&b'[') {
                    let end = skip_brackets(bytes, at);
                    found(start, Some(&text[start..at]), at, end);
                    at = end;
                } else if !matches!(bytes.get(at), Some(b'(' | b'!' | b':'))
                    && !(start > 0 && matches!(bytes[start - 1], b':' | b'!'))
                {
                    // A table's name on its own means its data.
                    found(start, Some(&text[start..at]), at, at);
                }
            }
            _ => at += text[at..].chars().next().map_or(1, char::len_utf8),
        }
    }
}

/// Replaces some table references in `text`. `change` gets the table's name as written (`None`
/// for one with no name) and what the brackets select, and returns the reference's new text.
fn replace_references<'a>(
    text: &'a str,
    mut change: impl FnMut(Option<&str>, &Selection) -> Option<String>,
) -> Cow<'a, str> {
    let mut out: Option<String> = None;
    let mut copied = 0;
    each_reference(text, |start, name, open, end| {
        let selection = if open == end {
            Selection {
                items: Vec::new(),
                this_row: false,
                columns: None,
            }
        } else {
            match read(&text[open..end]) {
                Some(selection) => selection,
                None => return,
            }
        };
        if let Some(next) = change(name, &selection) {
            let buffer = out.get_or_insert_with(|| String::with_capacity(text.len() + 8));
            buffer.push_str(&text[copied..start]);
            buffer.push_str(&next);
            copied = end;
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

/// `text` filled `cols` columns across, as Excel's fill handle does: a reference to one column of a
/// table moves that many columns along it, wrapping round from its last column to its first. Spans
/// of columns and whole tables stay. `home` is the table the formula started in, which references
/// with no table's name mean; they get its name, as a file stores them.
pub fn fill_table_columns<'a>(
    text: &'a str,
    cols: i64,
    tables: &[TableColumns<'_>],
    home: Option<&str>,
) -> Cow<'a, str> {
    if cols == 0 || !text.contains('[') {
        return Cow::Borrowed(text);
    }
    replace_references(text, |name, selection| {
        let table_name = name.or(home)?;
        let table = tables
            .iter()
            .find(|table| table.name.eq_ignore_ascii_case(table_name))?;
        let (column, None) = selection.columns.as_ref()? else {
            return None;
        };
        let count = table.columns.len() as i64;
        let index = table
            .columns
            .iter()
            .position(|other| other.eq_ignore_ascii_case(column))? as i64;
        let next = (index + cols).rem_euclid(count) as usize;
        let mut moved = selection.clone();
        moved.columns = Some((table.columns[next].clone(), None));
        Some(format!("{}{}", table.name, write(&moved)))
    })
}

/// `text` as Excel shows it in a cell of `table`: a reference to one of its columns, or to one of
/// them in this row, loses the table's name.
pub fn own_table_for_people<'a>(text: &'a str, table: &str) -> Cow<'a, str> {
    if !text.contains('[') {
        return Cow::Borrowed(text);
    }
    replace_references(text, |name, selection| {
        if !name?.eq_ignore_ascii_case(table) {
            return None;
        }
        match &selection.columns {
            Some((_, None)) if selection.items.is_empty() => Some(write(selection)),
            _ => None,
        }
    })
}

/// The brackets a file holds for a selection: this row as `[#This Row]`, as Excel stores it.
fn write_for_file(selection: &Selection) -> String {
    if !selection.this_row {
        return write(selection);
    }
    let mut parts: Vec<String> = selection
        .items
        .iter()
        .map(|item| format!("[{item}]"))
        .collect();
    parts.push("[#This Row]".to_string());
    match &selection.columns {
        Some((first, None)) => parts.push(format!("[{}]", escape(first))),
        Some((first, Some(last))) => parts.push(format!("[{}]:[{}]", escape(first), escape(last))),
        None => {}
    }
    if parts.len() == 1 {
        return parts.remove(0);
    }
    format!("[{}]", parts.join(","))
}

/// `text` as a file holds it: references with no table's name get `home`'s, and this row is
/// spelled out.
pub fn table_refs_for_file<'a>(text: &'a str, home: Option<&str>) -> Cow<'a, str> {
    if !text.contains('[') {
        return Cow::Borrowed(text);
    }
    replace_references(text, |name, selection| {
        let bare = name.is_some()
            && selection.items.is_empty()
            && !selection.this_row
            && selection.columns.is_none();
        if bare {
            return None;
        }
        match name {
            Some(name) if selection.this_row => {
                Some(format!("{name}{}", write_for_file(selection)))
            }
            Some(_) => None,
            None => home.map(|home| format!("{home}{}", write_for_file(selection))),
        }
    })
}

/// `text` as Excel shows it: this row as `@`.
pub fn table_refs_for_people(text: &str) -> Cow<'_, str> {
    if !text.contains('[') {
        return Cow::Borrowed(text);
    }
    replace_references(text, |name, selection| {
        let spelled = selection.this_row && write(selection) != write_for_file(selection);
        spelled.then(|| format!("{}{}", name.unwrap_or(""), write(selection)))
    })
}

/// `text` with a table's new name, wherever it is named.
pub fn rename_table<'a>(text: &'a str, old: &str, new: &str) -> Cow<'a, str> {
    replace_references(text, |name, selection| {
        let name = name.filter(|name| name.eq_ignore_ascii_case(old))?;
        let bare = selection.items.is_empty()
            && !selection.this_row
            && selection.columns.is_none()
            && !name.is_empty();
        Some(if bare {
            new.to_string()
        } else {
            format!("{new}{}", write(selection))
        })
    })
}

/// Where a table is, for turning its references into cells.
#[derive(Debug, Clone, Copy)]
pub struct TablePlace<'a> {
    pub name: &'a str,
    pub columns: &'a [String],
    /// 1-based and inclusive.
    pub top: u32,
    pub left: u32,
    pub bottom: u32,
    pub right: u32,
    pub header_row: bool,
    pub totals_row: bool,
    /// The sheet's name as a formula writes it, quoted if need be.
    pub sheet: &'a str,
}

/// `text`, in a cell at `row` (in `home` if that's a table), with references to `table` turned into
/// the cells they mean, as Excel's Convert to Range writes them: absolute, with the sheet's name. A
/// reference to this row keeps its row relative.
pub fn table_to_cells<'a>(
    text: &'a str,
    table: &TablePlace<'_>,
    row: u32,
    home: Option<&str>,
) -> Cow<'a, str> {
    replace_references(text, |name, selection| {
        let named = name.or(home)?;
        if !named.eq_ignore_ascii_case(table.name) {
            return None;
        }
        let find = |column: &str| {
            table
                .columns
                .iter()
                .position(|other| other.eq_ignore_ascii_case(column))
                .map(|index| table.left + index as u32)
        };
        let (left, right) = match &selection.columns {
            None => (table.left, table.right),
            Some((first, None)) => {
                let col = find(first)?;
                (col, col)
            }
            Some((first, Some(last))) => {
                let (a, b) = (find(first)?, find(last)?);
                (a.min(b), a.max(b))
            }
        };
        let data_top = table.top + u32::from(table.header_row);
        let data_bottom = table.bottom - u32::from(table.totals_row);
        let has = |item: &str| {
            selection
                .items
                .iter()
                .any(|other| other.eq_ignore_ascii_case(item))
        };
        let rows = if selection.this_row {
            None
        } else if has("#All") {
            Some((table.top, table.bottom))
        } else {
            let mut span: Option<(u32, u32)> = None;
            let mut take = |top: u32, bottom: u32| {
                span = Some(match span {
                    Some((a, b)) => (a.min(top), b.max(bottom)),
                    None => (top, bottom),
                });
            };
            if has("#Headers") && table.header_row {
                take(table.top, table.top);
            }
            if has("#Totals") && table.totals_row {
                take(table.bottom, table.bottom);
            }
            if has("#Data") || selection.items.is_empty() {
                take(data_top, data_bottom);
            }
            Some(span?)
        };
        let letters = |col: u32| super::col_to_letters(i64::from(col));
        let mut out = format!("{}!", table.sheet);
        match rows {
            None => {
                out.push_str(&format!("${}{row}", letters(left)));
                if right != left {
                    out.push_str(&format!(":${}{row}", letters(right)));
                }
            }
            Some((top, bottom)) => {
                out.push_str(&format!("${}${top}", letters(left)));
                if (top, left) != (bottom, right) {
                    out.push_str(&format!(":${}${bottom}", letters(right)));
                }
            }
        }
        Some(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sales() -> Vec<String> {
        ["Item", "Cost", "Qty"]
            .iter()
            .map(|name| name.to_string())
            .collect()
    }

    #[test]
    fn reads_and_writes_excels_spellings() {
        for (text, back) in [
            ("[Cost]", "[Cost]"),
            ("[@Cost]", "[@Cost]"),
            ("[@[Cost]]", "[@Cost]"),
            ("[[#This Row],[Cost]]", "[@Cost]"),
            ("[[@],[Cost]]", "[@Cost]"),
            ("[@[a b]]", "[@[a b]]"),
            ("[[#All],[Cost]]", "[[#All],[Cost]]"),
            ("[[Item]:[Cost]]", "[[Item]:[Cost]]"),
            ("[@[Item]:[Cost]]", "[@[Item]:[Cost]]"),
            ("[[#Headers],[Item]:[Cost]]", "[[#Headers],[Item]:[Cost]]"),
            ("[#All]", "[#All]"),
            ("[@]", "[@]"),
            ("[]", "[]"),
            ("[x'[y]", "[x'[y]"),
        ] {
            let selection = read(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(write(&selection), back, "{text}");
        }
    }

    #[test]
    fn a_fill_across_moves_one_column_round_the_table() {
        let columns = sales();
        let tables = [TableColumns {
            name: "Sales",
            columns: &columns,
        }];
        let fill = |text, cols| fill_table_columns(text, cols, &tables, None).into_owned();
        assert_eq!(fill("SUM(Sales[Cost])", 1), "SUM(Sales[Qty])");
        assert_eq!(fill("SUM(Sales[Cost])", 2), "SUM(Sales[Item])");
        assert_eq!(fill("SUM(Sales[Cost])", -1), "SUM(Sales[Item])");
        assert_eq!(fill("SUM(sales[Cost])", 5), "SUM(Sales[Item])");
        assert_eq!(fill("Sales[@Qty]", 1), "Sales[@Item]");
        assert_eq!(
            fill("SUM(Sales[[#All],[Qty]])", 2),
            "SUM(Sales[[#All],[Cost]])"
        );
        assert_eq!(
            fill("SUM(Sales[[Item]:[Cost]])", 1),
            "SUM(Sales[[Item]:[Cost]])"
        );
        assert_eq!(
            fill("COUNTA(Sales)+COUNTA(Sales[#Headers])", 1),
            "COUNTA(Sales)+COUNTA(Sales[#Headers])"
        );
        assert_eq!(
            fill("Other[Cost]+\"Sales[Cost]\"", 1),
            "Other[Cost]+\"Sales[Cost]\""
        );
        assert_eq!(
            fill_table_columns("[@Cost]*2", 1, &tables, Some("Sales")).into_owned(),
            "Sales[@Qty]*2"
        );
    }

    #[test]
    fn its_own_table_loses_its_name_for_one_column() {
        let run = |text| own_table_for_people(text, "Sales").into_owned();
        assert_eq!(run("SUM(Sales[Cost])*Rate"), "SUM([Cost])*Rate");
        assert_eq!(run("Sales[[#This Row],[Cost]]"), "[@Cost]");
        assert_eq!(run("Sales[@[a b]]"), "[@[a b]]");
        assert_eq!(
            run(
                "SUM(Sales[[Item]:[Cost]])+Sales[[#Data],[Cost]]+Sales[@]+Sales[[#Headers],[Item]]"
            ),
            "SUM(Sales[[Item]:[Cost]])+Sales[[#Data],[Cost]]+Sales[@]+Sales[[#Headers],[Item]]"
        );
        assert_eq!(run("Other[Cost]"), "Other[Cost]");
        assert_eq!(run("[1]Sheet1!A1"), "[1]Sheet1!A1");
    }

    #[test]
    fn a_renamed_table_follows() {
        assert_eq!(
            rename_table(
                "SUM(Table1[Qty])+table1[@x]+Table12[a]+COUNTA(Table1)",
                "Table1",
                "Sales"
            )
            .into_owned(),
            "SUM(Sales[Qty])+Sales[@x]+Table12[a]+COUNTA(Sales)"
        );
    }

    #[test]
    fn converting_to_a_range_writes_absolute_cells_with_the_sheet() {
        let columns: Vec<String> = ["Item", "Cost", "Qty"]
            .iter()
            .map(|x| x.to_string())
            .collect();
        let place = TablePlace {
            name: "Table1",
            columns: &columns,
            top: 1,
            left: 8,
            bottom: 7,
            right: 10,
            header_row: true,
            totals_row: true,
            sheet: "Sheet1",
        };
        let run = |text, row, home| table_to_cells(text, &place, row, home).into_owned();
        assert_eq!(run("SUM(Table1[Cost])", 2, None), "SUM(Sheet1!$I$2:$I$6)");
        assert_eq!(run("Table1[[#Headers],[Qty]]", 3, None), "Sheet1!$J$1");
        assert_eq!(run("Table1[@Cost]", 3, None), "Sheet1!$I3");
        assert_eq!(run("[@Cost]*2", 3, Some("Table1")), "Sheet1!$I3*2");
        assert_eq!(run("COUNTA(Table1)", 4, None), "COUNTA(Sheet1!$H$2:$J$6)");
        assert_eq!(
            run("SUM(Table1[[#All],[Qty]])", 5, None),
            "SUM(Sheet1!$J$1:$J$7)"
        );
        assert_eq!(
            run("SUM(Table1[[Item]:[Cost]])", 6, None),
            "SUM(Sheet1!$H$2:$I$6)"
        );
        assert_eq!(
            run("SUBTOTAL(109,[Qty])", 7, Some("Table1")),
            "SUBTOTAL(109,Sheet1!$J$2:$J$6)"
        );
        assert_eq!(run("Other[Qty]", 7, None), "Other[Qty]");
    }

    #[test]
    fn people_see_this_row_as_an_at() {
        assert_eq!(
            table_refs_for_people("T[[#This Row],[Cost]]*2+[[#This Row],[a b]]").into_owned(),
            "T[@Cost]*2+[@[a b]]"
        );
    }

    #[test]
    fn a_file_spells_this_row_out_and_names_the_table() {
        let run = |text, home| table_refs_for_file(text, home).into_owned();
        assert_eq!(run("[@Cost]*2", Some("T")), "T[[#This Row],[Cost]]*2");
        assert_eq!(run("T[@[a b]]+T[x]", None), "T[[#This Row],[a b]]+T[x]");
        assert_eq!(run("SUM([Cost])", Some("T")), "SUM(T[Cost])");
        assert_eq!(run("T[@]", None), "T[#This Row]");
        assert_eq!(run("SUM(A1:B2)", Some("T")), "SUM(A1:B2)");
    }
}
