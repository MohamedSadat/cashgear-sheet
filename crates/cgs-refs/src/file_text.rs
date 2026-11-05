//! Formula text as a file stores it, which isn't quite as people write it. Functions newer than
//! Excel 2007 carry a prefix, without which Excel shows `#NAME?`. `LET` and `LAMBDA` parameters do
//! too, and `@` has a function's name inside an array or a call.

use super::{is_word, skip_brackets, skip_quoted};

/// Functions a file stores as `_xlfn.NAME`. From the Open XML specification's list and what Excel
/// writes today. A name Excel has no prefix for would come back from it as a name it doesn't know.
const FUTURE: &[&str] = &[
    "ACOT",
    "ACOTH",
    "AGGREGATE",
    "ARABIC",
    "ARRAYTOTEXT",
    "BASE",
    "BETA.DIST",
    "BETA.INV",
    "BINOM.DIST",
    "BINOM.DIST.RANGE",
    "BINOM.INV",
    "BITAND",
    "BITLSHIFT",
    "BITOR",
    "BITRSHIFT",
    "BITXOR",
    "BYCOL",
    "BYROW",
    "CEILING.MATH",
    "CEILING.PRECISE",
    "CHISQ.DIST",
    "CHISQ.DIST.RT",
    "CHISQ.INV",
    "CHISQ.INV.RT",
    "CHISQ.TEST",
    "CHOOSECOLS",
    "CHOOSEROWS",
    "COMBINA",
    "CONCAT",
    "CONFIDENCE.NORM",
    "CONFIDENCE.T",
    "COT",
    "COTH",
    "COVARIANCE.P",
    "COVARIANCE.S",
    "CSC",
    "CSCH",
    "DAYS",
    "DECIMAL",
    "DROP",
    "ERF.PRECISE",
    "ERFC.PRECISE",
    "EXPAND",
    "EXPON.DIST",
    "F.DIST",
    "F.DIST.RT",
    "F.INV",
    "F.INV.RT",
    "F.TEST",
    "FIELDVALUE",
    "FILTERXML",
    "FLOOR.MATH",
    "FLOOR.PRECISE",
    "FORECAST.ETS",
    "FORECAST.ETS.CONFINT",
    "FORECAST.ETS.SEASONALITY",
    "FORECAST.ETS.STAT",
    "FORECAST.LINEAR",
    "FORMULATEXT",
    "GAMMA",
    "GAMMA.DIST",
    "GAMMA.INV",
    "GAMMALN.PRECISE",
    "GAUSS",
    "GROUPBY",
    "HSTACK",
    "HYPGEOM.DIST",
    "IFNA",
    "IFS",
    "IMAGE",
    "IMCOSH",
    "IMCOT",
    "IMCSC",
    "IMCSCH",
    "IMSEC",
    "IMSECH",
    "IMSINH",
    "IMTAN",
    "ISFORMULA",
    "ISOMITTED",
    "ISOWEEKNUM",
    "LAMBDA",
    "LET",
    "LOGNORM.DIST",
    "LOGNORM.INV",
    "MAKEARRAY",
    "MAP",
    "MAXIFS",
    "MINIFS",
    "MODE.MULT",
    "MODE.SNGL",
    "MUNIT",
    "NEGBINOM.DIST",
    "NORM.DIST",
    "NORM.INV",
    "NORM.S.DIST",
    "NORM.S.INV",
    "NUMBERVALUE",
    "PDURATION",
    "PERCENTILE.EXC",
    "PERCENTILE.INC",
    "PERCENTOF",
    "PERCENTRANK.EXC",
    "PERCENTRANK.INC",
    "PERMUTATIONA",
    "PHI",
    "PIVOTBY",
    "POISSON.DIST",
    "QUARTILE.EXC",
    "QUARTILE.INC",
    "RANDARRAY",
    "RANK.AVG",
    "RANK.EQ",
    "REDUCE",
    "REGEXEXTRACT",
    "REGEXREPLACE",
    "REGEXTEST",
    "RRI",
    "SCAN",
    "SEC",
    "SECH",
    "SEQUENCE",
    "SHEET",
    "SHEETS",
    "SKEW.P",
    "SORTBY",
    "STDEV.P",
    "STDEV.S",
    "STOCKHISTORY",
    "SWITCH",
    "T.DIST",
    "T.DIST.2T",
    "T.DIST.RT",
    "T.INV",
    "T.INV.2T",
    "T.TEST",
    "TAKE",
    "TEXTAFTER",
    "TEXTBEFORE",
    "TEXTJOIN",
    "TEXTSPLIT",
    "TOCOL",
    "TOROW",
    "TRIMRANGE",
    "UNICHAR",
    "UNICODE",
    "UNIQUE",
    "VALUETOTEXT",
    "VAR.P",
    "VAR.S",
    "VSTACK",
    "WEBSERVICE",
    "WEIBULL.DIST",
    "WRAPCOLS",
    "WRAPROWS",
    "XLOOKUP",
    "XMATCH",
    "XOR",
    "Z.TEST",
];

/// Functions a file stores as `_xlfn._xlws.NAME`.
const WORKSHEET_FUTURE: &[&str] = &["FILTER", "SORT"];

fn file_prefix(upper: &str) -> Option<&'static str> {
    if WORKSHEET_FUTURE.contains(&upper) {
        Some("_xlfn._xlws.")
    } else if FUTURE.contains(&upper) {
        Some("_xlfn.")
    } else {
        None
    }
}

fn word_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && is_word(bytes[at]) {
        at += 1;
    }
    at
}

/// Where the argument starting at `at` ends: at a comma or closing bracket at its own depth.
fn argument_end(bytes: &[u8], mut at: usize) -> usize {
    let mut depth = 0i32;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                at = skip_quoted(bytes, at, b'"');
                continue;
            }
            b'\'' => {
                at = skip_quoted(bytes, at, b'\'');
                continue;
            }
            b'[' => {
                at = skip_brackets(bytes, at);
                continue;
            }
            b'(' | b'{' => depth += 1,
            b')' | b'}' => {
                if depth == 0 {
                    return at;
                }
                depth -= 1;
            }
            b',' if depth == 0 => return at,
            _ => {}
        }
        at += 1;
    }
    at
}

/// The names a `LET` or `LAMBDA` starting at `open` (its bracket) introduces.
fn parameters(text: &str, open: usize, lambda: bool) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut args: Vec<(usize, usize)> = Vec::new();
    let mut at = open + 1;
    loop {
        let end = argument_end(bytes, at);
        args.push((at, end));
        if bytes.get(end) != Some(&b',') {
            break;
        }
        at = end + 1;
    }
    let is_name = |arg: &(usize, usize)| {
        let name = text[arg.0..arg.1].trim();
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    };
    let mut out = Vec::new();
    if lambda {
        // All but the last, which is the calculation.
        for arg in &args[..args.len().saturating_sub(1)] {
            if is_name(arg) {
                out.push(text[arg.0..arg.1].trim().to_string());
            }
        }
    } else {
        // Name, value, name, value and then the calculation.
        let mut index = 0;
        while index + 1 < args.len() {
            if is_name(&args[index]) {
                out.push(text[args[index].0..args[index].1].trim().to_string());
            }
            index += 2;
        }
    }
    out
}

/// Where the operand after an `@` ends: a call, or a reference with its range.
fn operand_end(bytes: &[u8], mut at: usize) -> usize {
    if bytes.get(at) == Some(&b'\'') {
        at = skip_quoted(bytes, at, b'\'');
        if bytes.get(at) == Some(&b'!') {
            at += 1;
        }
    }
    loop {
        at = word_end(bytes, at);
        match bytes.get(at) {
            Some(b'!') | Some(b':') => at += 1,
            Some(b'(') => {
                let mut depth = 0;
                while at < bytes.len() {
                    match bytes[at] {
                        b'"' => {
                            at = skip_quoted(bytes, at, b'"');
                            continue;
                        }
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                return at + 1;
                            }
                        }
                        _ => {}
                    }
                    at += 1;
                }
                return at;
            }
            Some(b'[') => at = skip_brackets(bytes, at),
            _ => return at,
        }
    }
}

/// `text` as a file stores it. `array` says the formula is stored as an array, where nothing is
/// implicitly cut down to one value, so `@` must always be spelled out. Text already in that form
/// comes back as it is.
pub fn formula_for_file(text: &str, array: bool) -> String {
    // Names a `LET` or `LAMBDA` brings in, anywhere: a formula doesn't reuse one for a function.
    let mut names: Vec<String> = Vec::new();
    {
        let bytes = text.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            match bytes[at] {
                b'"' => at = skip_quoted(bytes, at, b'"'),
                b'\'' => at = skip_quoted(bytes, at, b'\''),
                byte if is_word(byte) => {
                    let end = word_end(bytes, at);
                    if bytes.get(end) == Some(&b'(') {
                        let word = text[at..end]
                            .trim_start_matches("_xlfn.")
                            .to_ascii_uppercase();
                        if word == "LET" || word == "LAMBDA" {
                            names.extend(parameters(text, end, word == "LAMBDA"));
                        }
                    }
                    at = end;
                }
                _ => at += 1,
            }
        }
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 8);
    let mut at = 0;
    let mut depth = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' | b'\'' => {
                let end = skip_quoted(bytes, at, bytes[at]);
                out.push_str(&text[at..end]);
                at = end;
            }
            b'[' => {
                let end = skip_brackets(bytes, at);
                // A column whose name has spaces at its edge needs brackets of its own.
                let inner = &text[at + 1..end.saturating_sub(1).max(at + 1)];
                if !inner.contains('[')
                    && !inner.starts_with('@')
                    && (inner.starts_with(' ') || inner.ends_with(' '))
                {
                    out.push_str("[[");
                    out.push_str(inner);
                    out.push_str("]]");
                } else {
                    out.push_str(&text[at..end]);
                }
                at = end;
            }
            b'(' => {
                depth += 1;
                out.push('(');
                at += 1;
            }
            b')' => {
                depth -= 1;
                out.push(')');
                at += 1;
            }
            b'@' => {
                let end = operand_end(bytes, at + 1);
                let inner = formula_for_file(&text[at + 1..end], array);
                if depth == 0 && !array {
                    // At the top of a plain formula, anything is cut down to one value anyway.
                    out.push_str(&inner);
                } else {
                    out.push_str("_xlfn.SINGLE(");
                    out.push_str(&inner);
                    out.push(')');
                }
                at = end;
            }
            byte if is_word(byte) => {
                let end = word_end(bytes, at);
                let word = &text[at..end];
                let upper = word.to_ascii_uppercase();
                if bytes.get(end) == Some(&b'(') {
                    if let Some(prefix) = file_prefix(&upper) {
                        out.push_str(prefix);
                        out.push_str(&upper);
                    } else {
                        out.push_str(word);
                    }
                } else if !word.starts_with("_xlpm.")
                    && names.iter().any(|name| name.eq_ignore_ascii_case(word))
                {
                    out.push_str("_xlpm.");
                    out.push_str(word);
                } else if bytes.get(end) == Some(&b'#') && !word.is_empty() {
                    // `A1#`: the area an array spills over.
                    out.push_str("_xlfn.ANCHORARRAY(");
                    out.push_str(word);
                    out.push(')');
                    at = end + 1;
                    continue;
                } else {
                    out.push_str(word);
                }
                at = end;
            }
            _ => {
                let width = text[at..].chars().next().map_or(1, char::len_utf8);
                out.push_str(&text[at..at + width]);
                at += width;
            }
        }
    }
    out
}

/// Formula text as Excel shows it: without the prefixes a file stores newer functions with, and
/// without the spaces the engine prints once it has parsed a formula. A space between two operands
/// stays, because that is the intersection operator.
pub fn formula_for_people(text: &str) -> String {
    let mut plain = text.to_string();
    for prefix in ["_xlfn.", "_xlws.", "_xlpm."] {
        if plain.contains(prefix) {
            plain = plain.replace(prefix, "");
        }
    }
    let operand = |ch: char| {
        ch.is_alphanumeric() || matches!(ch, '_' | '$' | '\'' | '"' | ')' | ']' | '.' | '#' | '!')
    };
    let chars: Vec<char> = plain.chars().collect();
    let mut out = String::with_capacity(plain.len());
    let mut quote = None;
    for (at, &ch) in chars.iter().enumerate() {
        match quote {
            Some(open) => {
                if ch == open {
                    quote = None;
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch == ' ' {
                    let before = out.chars().last().is_some_and(operand);
                    let after = chars[at + 1..]
                        .iter()
                        .find(|&&next| next != ' ')
                        .is_some_and(|&next| operand(next) || next == '(');
                    if !(before && after) {
                        continue;
                    }
                }
            }
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_functions_get_their_prefix() {
        assert_eq!(
            formula_for_file("XLOOKUP(A1,B:B,C:C)", false),
            "_xlfn.XLOOKUP(A1,B:B,C:C)"
        );
        assert_eq!(
            formula_for_file("xlookup(A1,B:B,C:C)", false),
            "_xlfn.XLOOKUP(A1,B:B,C:C)"
        );
        assert_eq!(
            formula_for_file("SUM(FILTER(A:A,A:A>1))", true),
            "SUM(_xlfn._xlws.FILTER(A:A,A:A>1))"
        );
        assert_eq!(
            formula_for_file("STDEV.S(A:A)+IFNA(A1,0)", false),
            "_xlfn.STDEV.S(A:A)+_xlfn.IFNA(A1,0)"
        );
        assert_eq!(formula_for_file("SUM(A1)", false), "SUM(A1)");
    }

    #[test]
    fn prefixes_already_there_stay_as_they_are() {
        for text in [
            "_xlfn.XLOOKUP(A1,B:B,C:C)",
            "_xlfn._xlws.SORT(A:A)",
            "_xlfn.LET(_xlpm.x,1,_xlpm.x+1)",
        ] {
            assert_eq!(formula_for_file(text, false), text);
        }
    }

    #[test]
    fn let_and_lambda_names_get_their_prefix() {
        assert_eq!(
            formula_for_file("LET(x,1,x+A1)", false),
            "_xlfn.LET(_xlpm.x,1,_xlpm.x+A1)"
        );
        assert_eq!(
            formula_for_file("LAMBDA(a,b,a+b)(1,2)", false),
            "_xlfn.LAMBDA(_xlpm.a,_xlpm.b,_xlpm.a+_xlpm.b)(1,2)"
        );
    }

    #[test]
    fn at_goes_at_the_top_and_is_spelled_out_inside() {
        assert_eq!(formula_for_file("@A1:A5+1", false), "A1:A5+1");
        assert_eq!(
            formula_for_file("SUM(@A1:A5)", false),
            "SUM(_xlfn.SINGLE(A1:A5))"
        );
        assert_eq!(formula_for_file("@A1:A5", true), "_xlfn.SINGLE(A1:A5)");
        assert_eq!(
            formula_for_file("@INDEX(A:A,1)", true),
            "_xlfn.SINGLE(INDEX(A:A,1))"
        );
    }

    #[test]
    fn a_spilled_area_gets_its_function() {
        assert_eq!(
            formula_for_file("SUM(A1#)", false),
            "SUM(_xlfn.ANCHORARRAY(A1))"
        );
    }

    #[test]
    fn strings_and_sheet_names_are_not_read() {
        assert_eq!(
            formula_for_file("\"XLOOKUP(\"&'LET(x'!A1", false),
            "\"XLOOKUP(\"&'LET(x'!A1"
        );
    }

    #[test]
    fn a_table_column_with_spaces_at_its_edge_gets_more_brackets() {
        assert_eq!(formula_for_file("SUM(T[  a  ])", false), "SUM(T[[  a  ]])");
        assert_eq!(formula_for_file("SUM(T[a b])", false), "SUM(T[a b])");
        assert_eq!(
            formula_for_file("SUM(T[[  a  ]])", false),
            "SUM(T[[  a  ]])"
        );
    }

    #[test]
    fn people_see_no_prefixes() {
        assert_eq!(
            formula_for_people("_xlfn.LET(_xlpm.x, 1, _xlpm.x + 1)"),
            "LET(x,1,x+1)"
        );
        assert_eq!(formula_for_people("A1:B2 B1:C3"), "A1:B2 B1:C3");
    }
}
