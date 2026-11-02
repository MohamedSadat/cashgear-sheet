//! A small scanner over stored formula text: finds function names and external references.

pub use cgs_refs::{col_to_letters, parse_a1, parse_range, shift};

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

/// `A1`-style text as (column, row), ignoring `$` signs.
fn split_cell(word: &str) -> Option<(i64, i64)> {
    let plain = word.replace('$', "");
    let (row, col) = parse_a1(&plain)?;
    Some((i64::from(col), i64::from(row)))
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
                    if let Some((col, row)) = split_cell(&text[start..at]) {
                        out.push((row as u32, col as u32));
                    }
                }
            }
            _ => at += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
