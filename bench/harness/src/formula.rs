//! A small scanner over stored formula text: finds function names and external
//! references, and shifts relative references when a shared formula is expanded.

/// Functions whose stored value can't be reproduced: their cells are seeded with the
/// stored value and not scored. OFFSET and INDIRECT are volatile in Excel, but their
/// value is fixed by their inputs, so they are scored.
pub const NON_DETERMINISTIC: &[&str] = &[
    "NOW",
    "TODAY",
    "RAND",
    "RANDBETWEEN",
    "RANDARRAY",
    "INFO",
    "CELL",
];

/// Functions whose value comes from outside the workbook's cells: an external data
/// connection, the web or a pivot cache. These count as external, like `[1]Sheet!A1`.
pub const OUTSIDE_DATA: &[&str] = &[
    "CUBEKPIMEMBER",
    "CUBEMEMBER",
    "CUBEMEMBERPROPERTY",
    "CUBERANKEDMEMBER",
    "CUBESET",
    "CUBESETCOUNT",
    "CUBEVALUE",
    "RTD",
    "WEBSERVICE",
    "GETPIVOTDATA",
    "STOCKHISTORY",
    "IMPORTDATA",
];

#[derive(Debug, Default, PartialEq)]
pub struct Scan {
    /// Upper case, without `_xlfn.`-style prefixes, one entry per distinct function.
    pub functions: Vec<String>,
    /// References another workbook through `[n]`.
    pub external: bool,
}

impl Scan {
    pub fn non_deterministic(&self) -> bool {
        self.functions
            .iter()
            .any(|name| NON_DETERMINISTIC.contains(&name.as_str()))
    }

    pub fn outside_data(&self) -> bool {
        self.functions
            .iter()
            .any(|name| OUTSIDE_DATA.contains(&name.as_str()))
    }
}

/// `[` that opens an external workbook index rather than a structured reference.
fn external_bracket(bytes: &[u8], at: usize) -> bool {
    let mut end = at + 1;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    end > at + 1 && end < bytes.len() && bytes[end] == b']'
}

/// Skips a bracketed structured reference, which can nest and uses `'` as an escape.
fn skip_brackets(bytes: &[u8], mut at: usize) -> usize {
    let mut depth = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\'' => at += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return at + 1;
                }
            }
            _ => {}
        }
        at += 1;
    }
    at
}

fn skip_quoted(bytes: &[u8], mut at: usize, quote: u8) -> usize {
    at += 1;
    while at < bytes.len() {
        if bytes[at] == quote {
            if bytes.get(at + 1) == Some(&quote) {
                at += 2;
                continue;
            }
            return at + 1;
        }
        at += 1;
    }
    at
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'$' | b'\\') || byte >= 0x80
}

pub fn strip_prefixes(name: &str) -> &str {
    let mut name = name;
    for prefix in ["_xlfn.", "_xlws.", "_xlpm.", "_xlfn."] {
        if name.len() >= prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix) {
            name = &name[prefix.len()..];
        }
    }
    name
}

pub fn scan(text: &str) -> Scan {
    let bytes = text.as_bytes();
    let mut out = Scan::default();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => at = skip_quoted(bytes, at, b'"'),
            b'\'' => {
                let end = skip_quoted(bytes, at, b'\'');
                if text[at..end].starts_with("'[") {
                    out.external = true;
                }
                at = end;
            }
            b'[' => {
                if external_bracket(bytes, at) {
                    out.external = true;
                }
                at = skip_brackets(bytes, at);
            }
            byte if is_word(byte) => {
                let start = at;
                while at < bytes.len() && is_word(bytes[at]) {
                    at += 1;
                }
                if bytes.get(at) == Some(&b'(') {
                    let name = strip_prefixes(&text[start..at]).to_ascii_uppercase();
                    if !out.functions.contains(&name) {
                        out.functions.push(name);
                    }
                }
            }
            _ => at += 1,
        }
    }
    out
}

/// Same-sheet cells a formula names directly, for picking edits that have an effect.
/// A range counts as its two corners.
pub fn referenced_cells(text: &str) -> Vec<(u32, u32)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => at = skip_quoted(bytes, at, b'"'),
            b'\'' => at = skip_quoted(bytes, at, b'\''),
            b'[' => at = skip_brackets(bytes, at),
            byte if is_word(byte) => {
                let start = at;
                while at < bytes.len() && is_word(bytes[at]) {
                    at += 1;
                }
                let other_sheet =
                    bytes.get(at) == Some(&b'!') || (start > 0 && bytes[start - 1] == b'!');
                if !other_sheet && bytes.get(at) != Some(&b'(') {
                    if let Some((_, col, _, row)) = split_cell(&text[start..at]) {
                        out.push((row as u32, col as u32));
                    }
                }
            }
            _ => at += 1,
        }
    }
    out
}

const MAX_ROW: i64 = 1_048_576;
const MAX_COL: i64 = 16_384;

pub fn col_from_letters(letters: &str) -> Option<i64> {
    let mut col = 0i64;
    for byte in letters.bytes() {
        if !byte.is_ascii_alphabetic() {
            return None;
        }
        col = col * 26 + i64::from(byte.to_ascii_uppercase() - b'A') + 1;
    }
    (1..=MAX_COL).contains(&col).then_some(col)
}

pub fn col_to_letters(mut col: i64) -> String {
    let mut out = Vec::new();
    while col > 0 {
        let rem = (col - 1) % 26;
        out.push(b'A' + rem as u8);
        col = (col - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Parses `A1` style text into (row, column), both 1-based.
pub fn parse_a1(text: &str) -> Option<(u32, u32)> {
    let split = text.find(|c: char| c.is_ascii_digit())?;
    let col = col_from_letters(&text[..split])?;
    let row: i64 = text[split..].parse().ok()?;
    (1..=MAX_ROW)
        .contains(&row)
        .then_some((row as u32, col as u32))
}

/// Parses `A1:C3` or `A1`, returning ((top, left), (bottom, right)).
pub fn parse_range(text: &str) -> Option<((u32, u32), (u32, u32))> {
    let text = text.replace('$', "");
    match text.split_once(':') {
        Some((from, to)) => Some((parse_a1(from)?, parse_a1(to)?)),
        None => {
            let at = parse_a1(&text)?;
            Some((at, at))
        }
    }
}

fn wrap(value: i64, max: i64) -> i64 {
    (value - 1).rem_euclid(max) + 1
}

/// `$A$1`-style reference split into its parts.
fn split_cell(word: &str) -> Option<(bool, i64, bool, i64)> {
    let bytes = word.as_bytes();
    let mut at = 0;
    let abs_col = bytes.first() == Some(&b'$');
    if abs_col {
        at += 1;
    }
    let col_start = at;
    while at < bytes.len() && bytes[at].is_ascii_alphabetic() {
        at += 1;
    }
    let col = col_from_letters(word.get(col_start..at)?)?;
    let abs_row = bytes.get(at) == Some(&b'$');
    if abs_row {
        at += 1;
    }
    let digits = word.get(at..)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let row: i64 = digits.parse().ok()?;
    (1..=MAX_ROW)
        .contains(&row)
        .then_some((abs_col, col, abs_row, row))
}

fn split_col(word: &str) -> Option<(bool, i64)> {
    let (abs, rest) = match word.strip_prefix('$') {
        Some(rest) => (true, rest),
        None => (false, word),
    };
    Some((abs, col_from_letters(rest)?))
}

fn split_row(word: &str) -> Option<(bool, i64)> {
    let (abs, rest) = match word.strip_prefix('$') {
        Some(rest) => (true, rest),
        None => (false, word),
    };
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let row: i64 = rest.parse().ok()?;
    (1..=MAX_ROW).contains(&row).then_some((abs, row))
}

/// Returns the formula as it reads `rows` below and `cols` right of where it is stored.
pub fn shift(text: &str, rows: i64, cols: i64) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 8);
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        match bytes[at] {
            b'"' => at = skip_quoted(bytes, at, b'"'),
            b'\'' => at = skip_quoted(bytes, at, b'\''),
            b'[' => at = skip_brackets(bytes, at),
            byte if is_word(byte) => {
                while at < bytes.len() && is_word(bytes[at]) {
                    at += 1;
                }
                let word = &text[start..at];
                if bytes.get(at) == Some(&b'(') {
                    out.push_str(word);
                    continue;
                }
                if let Some((abs_col, col, abs_row, row)) = split_cell(word) {
                    let col = if abs_col {
                        col
                    } else {
                        wrap(col + cols, MAX_COL)
                    };
                    let row = if abs_row {
                        row
                    } else {
                        wrap(row + rows, MAX_ROW)
                    };
                    if abs_col {
                        out.push('$');
                    }
                    out.push_str(&col_to_letters(col));
                    if abs_row {
                        out.push('$');
                    }
                    out.push_str(&row.to_string());
                    continue;
                }
                // Whole columns and whole rows only count as a pair around a colon.
                if bytes.get(at) == Some(&b':') {
                    let second_start = at + 1;
                    let mut second_end = second_start;
                    while second_end < bytes.len() && is_word(bytes[second_end]) {
                        second_end += 1;
                    }
                    let second = &text[second_start..second_end];
                    if let (Some(a), Some(b)) = (split_col(word), split_col(second)) {
                        push_col(&mut out, a, cols);
                        out.push(':');
                        push_col(&mut out, b, cols);
                        at = second_end;
                        continue;
                    }
                    if let (Some(a), Some(b)) = (split_row(word), split_row(second)) {
                        push_row(&mut out, a, rows);
                        out.push(':');
                        push_row(&mut out, b, rows);
                        at = second_end;
                        continue;
                    }
                }
                out.push_str(word);
                continue;
            }
            _ => at += text[at..].chars().next().map_or(1, char::len_utf8),
        }
        out.push_str(&text[start..at]);
    }
    out
}

fn push_col(out: &mut String, (abs, col): (bool, i64), cols: i64) {
    if abs {
        out.push('$');
        out.push_str(&col_to_letters(col));
    } else {
        out.push_str(&col_to_letters(wrap(col + cols, MAX_COL)));
    }
}

fn push_row(out: &mut String, (abs, row): (bool, i64), rows: i64) {
    if abs {
        out.push('$');
        out.push_str(&row.to_string());
    } else {
        out.push_str(&wrap(row + rows, MAX_ROW).to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifts_relative_parts_only() {
        assert_eq!(shift("A1+$B2+C$3+$D$4", 1, 2), "C2+$B3+E$3+$D$4");
        assert_eq!(shift("SUM(A1:B2)", 2, 0), "SUM(A3:B4)");
        assert_eq!(
            shift("Sheet2!A1*'My Sheet'!B1", 1, 1),
            "Sheet2!B2*'My Sheet'!C2"
        );
    }

    #[test]
    fn leaves_strings_functions_and_names_alone() {
        assert_eq!(shift("\"A1\"&A1", 1, 0), "\"A1\"&A2");
        assert_eq!(shift("LOG10(A1)", 1, 0), "LOG10(A2)");
        assert_eq!(shift("Rate1Year*A1", 1, 0), "Rate1Year*A2");
        assert_eq!(shift("Table1[Col A]+A1", 1, 0), "Table1[Col A]+A2");
        assert_eq!(shift("1.5E+3+A1", 1, 0), "1.5E+3+A2");
    }

    #[test]
    fn shifts_whole_rows_and_columns() {
        assert_eq!(shift("SUM(A:B)", 5, 1), "SUM(B:C)");
        assert_eq!(shift("SUM($A:B)", 5, 1), "SUM($A:C)");
        assert_eq!(shift("SUM(1:2)", 1, 5), "SUM(2:3)");
    }

    #[test]
    fn wraps_past_the_sheet_edge() {
        assert_eq!(shift("A1", -1, 0), "A1048576");
        assert_eq!(shift("XFD1", 0, 1), "A1");
    }

    #[test]
    fn scan_finds_functions_and_externals() {
        let found = scan("_xlfn.CONCAT(A1,SUM(B:B),\"NOW()\")+NOW()");
        assert_eq!(found.functions, ["CONCAT", "SUM", "NOW"]);
        assert!(found.non_deterministic());
        assert!(!found.external);
        assert!(scan("[1]Sheet1!A1").external);
        assert!(scan("'[Book.xlsx]Sheet 1'!A1+1").external);
        assert!(!scan("Table1[Col]").external);
    }

    #[test]
    fn parses_a1_and_ranges() {
        assert_eq!(parse_a1("B3"), Some((3, 2)));
        assert_eq!(parse_range("A1:C3"), Some(((1, 1), (3, 3))));
        assert_eq!(parse_range("$B$2"), Some(((2, 2), (2, 2))));
        assert_eq!(col_to_letters(28), "AB");
    }
}
