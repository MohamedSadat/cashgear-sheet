//! Rewrites references in formula text: shared formula expansion now, and inserts,
//! deletes and renames from Phase 4. Used by the xlsx reader, the bench and the engine.

pub const MAX_ROW: i64 = 1_048_576;
pub const MAX_COL: i64 = 16_384;

pub fn col_from_letters(letters: &str) -> Option<i64> {
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut col = 0i64;
    for byte in letters.bytes() {
        if !byte.is_ascii_alphabetic() {
            return None;
        }
        col = col * 26 + i64::from(byte.to_ascii_uppercase() - b'A') + 1;
    }
    (1..=MAX_COL).contains(&col).then_some(col)
}

pub fn col_to_letters(col: i64) -> String {
    let mut out = String::with_capacity(3);
    push_letters(&mut out, col);
    out
}

fn push_letters(out: &mut String, col: i64) {
    let mut letters = [0u8; 4];
    let mut at = letters.len();
    let mut rest = col;
    while rest > 0 && at > 0 {
        at -= 1;
        letters[at] = b'A' + ((rest - 1) % 26) as u8;
        rest = (rest - 1) / 26;
    }
    for &letter in &letters[at..] {
        out.push(char::from(letter));
    }
}

fn push_number(out: &mut String, number: i64) {
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    let mut rest = number;
    loop {
        at -= 1;
        digits[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    for &digit in &digits[at..] {
        out.push(char::from(digit));
    }
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

/// Parses `A1:C3` or `A1`, returning ((top, left), (bottom, right)). `$` signs are fine.
pub fn parse_range(text: &str) -> Option<((u32, u32), (u32, u32))> {
    let cell = |part: &str| {
        if part.contains('$') {
            parse_a1(&part.replace('$', ""))
        } else {
            parse_a1(part)
        }
    };
    match text.split_once(':') {
        Some((from, to)) => Some((cell(from)?, cell(to)?)),
        None => {
            let at = cell(text)?;
            Some((at, at))
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Piece {
    /// Text copied as it is, as a byte range of the source.
    Literal(u32, u32),
    Col(bool, i64),
    Row(bool, i64),
}

/// A master formula split around its references, so that its many followers don't each
/// scan the text again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFormula {
    text: String,
    pieces: Vec<Piece>,
}

impl SharedFormula {
    pub fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut pieces = Vec::new();
        let mut literal = 0;
        let mut at = 0;
        let flush = |pieces: &mut Vec<Piece>, from: usize, to: usize| {
            if to > from {
                pieces.push(Piece::Literal(from as u32, to as u32));
            }
        };
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
                    let word = &text[start..at];
                    // A function's name, or a sheet's: neither is a reference.
                    if matches!(bytes.get(at), Some(b'(' | b'!')) {
                        continue;
                    }
                    if let Some((abs_col, col, abs_row, row)) = split_cell(word) {
                        flush(&mut pieces, literal, start);
                        pieces.push(Piece::Col(abs_col, col));
                        pieces.push(Piece::Row(abs_row, row));
                        literal = at;
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
                        let pair = match (split_col(word), split_col(second)) {
                            (Some(a), Some(b)) => {
                                Some((Piece::Col(a.0, a.1), Piece::Col(b.0, b.1)))
                            }
                            _ => match (split_row(word), split_row(second)) {
                                (Some(a), Some(b)) => {
                                    Some((Piece::Row(a.0, a.1), Piece::Row(b.0, b.1)))
                                }
                                _ => None,
                            },
                        };
                        if let Some((first, last)) = pair {
                            flush(&mut pieces, literal, start);
                            pieces.push(first);
                            pieces.push(Piece::Literal(at as u32, (at + 1) as u32));
                            pieces.push(last);
                            at = second_end;
                            literal = at;
                        }
                    }
                }
                _ => at += text[at..].chars().next().map_or(1, char::len_utf8),
            }
        }
        flush(&mut pieces, literal, bytes.len());
        Self {
            text: text.to_string(),
            pieces,
        }
    }

    /// Appends the formula as it reads `rows` below and `cols` right of where it is stored.
    pub fn render(&self, rows: i64, cols: i64, out: &mut String) {
        for piece in &self.pieces {
            match *piece {
                Piece::Literal(from, to) => out.push_str(&self.text[from as usize..to as usize]),
                Piece::Col(abs, col) => {
                    if abs {
                        out.push('$');
                        push_letters(out, col);
                    } else {
                        push_letters(out, wrap(col + cols, MAX_COL));
                    }
                }
                Piece::Row(abs, row) => {
                    if abs {
                        out.push('$');
                        push_number(out, row);
                    } else {
                        push_number(out, wrap(row + rows, MAX_ROW));
                    }
                }
            }
        }
    }

    pub fn source(&self) -> &str {
        &self.text
    }
}

/// Returns the formula as it reads `rows` below and `cols` right of where it is stored.
pub fn shift(text: &str, rows: i64, cols: i64) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    SharedFormula::new(text).render(rows, cols, &mut out);
    out
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
    fn leaves_a_sheet_named_like_a_cell_alone() {
        assert_eq!(shift("AB1!A1+1", 1, 1), "AB1!B2+1");
    }

    #[test]
    fn keeps_text_outside_ascii() {
        assert_eq!(
            shift("'ورقة 1'!A1&\"نص\"&مبلغ", 1, 0),
            "'ورقة 1'!A2&\"نص\"&مبلغ"
        );
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
    fn parses_a1_and_ranges() {
        assert_eq!(parse_a1("B3"), Some((3, 2)));
        assert_eq!(parse_range("A1:C3"), Some(((1, 1), (3, 3))));
        assert_eq!(parse_range("$B$2"), Some(((2, 2), (2, 2))));
        assert_eq!(col_to_letters(28), "AB");
        assert_eq!(col_from_letters("XFD"), Some(16_384));
        assert_eq!(col_from_letters("XFE"), None);
    }
}
