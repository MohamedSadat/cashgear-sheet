//! Small fast helpers over raw XML bytes. Cells outnumber everything else in a workbook by
//! millions, so reading a `<c>` must not allocate.

use quick_xml::events::{BytesStart, Event};

/// Calls `visit` with each attribute's local name and raw value, unescaped text left as it is.
pub fn each_attr<'a>(element: &'a BytesStart<'_>, mut visit: impl FnMut(&'a [u8], &'a [u8])) {
    let bytes: &'a [u8] = element.attributes_raw();
    let mut at = 0;
    loop {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let start = at;
        while at < bytes.len() && bytes[at] != b'=' && !bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at >= bytes.len() {
            return;
        }
        let name = &bytes[start..at];
        while at < bytes.len() && bytes[at] != b'"' && bytes[at] != b'\'' {
            at += 1;
        }
        if at >= bytes.len() {
            return;
        }
        let quote = bytes[at];
        at += 1;
        let value_start = at;
        while at < bytes.len() && bytes[at] != quote {
            at += 1;
        }
        if at > bytes.len() {
            return;
        }
        let local = match name.iter().rposition(|&byte| byte == b':') {
            Some(colon) => &name[colon + 1..],
            None => name,
        };
        visit(local, &bytes[value_start..at]);
        at += 1;
    }
}

/// An attribute's text, with XML escapes resolved.
pub fn attr(element: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    let mut found = None;
    each_attr(element, |key, value| {
        if found.is_none() && key == name {
            found = Some(value);
        }
    });
    found.map(unescape_attr)
}

pub fn unescape_attr(value: &[u8]) -> String {
    let raw = String::from_utf8_lossy(value);
    quick_xml::escape::unescape(&raw).map_or_else(|_| raw.to_string(), std::borrow::Cow::into_owned)
}

pub fn attr_bool(element: &BytesStart<'_>, name: &[u8]) -> Option<bool> {
    let mut found = None;
    each_attr(element, |key, value| {
        if found.is_none() && key == name {
            found = Some(parse_bool(value));
        }
    });
    found
}

pub fn attr_f64(element: &BytesStart<'_>, name: &[u8]) -> Option<f64> {
    let mut found = None;
    each_attr(element, |key, value| {
        if found.is_none() && key == name {
            found = std::str::from_utf8(value)
                .ok()
                .and_then(|text| text.trim().parse().ok());
        }
    });
    found
}

pub fn attr_u32(element: &BytesStart<'_>, name: &[u8]) -> Option<u32> {
    let mut found = None;
    each_attr(element, |key, value| {
        if found.is_none() && key == name {
            found = parse_u32(value);
        }
    });
    found
}

pub fn parse_bool(value: &[u8]) -> bool {
    matches!(value, b"1" | b"true" | b"on")
}

pub fn parse_u32(value: &[u8]) -> Option<u32> {
    if value.is_empty() || value.len() > 10 {
        return None;
    }
    let mut out = 0u64;
    for &byte in value {
        if !byte.is_ascii_digit() {
            return None;
        }
        out = out * 10 + u64::from(byte - b'0');
    }
    u32::try_from(out).ok()
}

/// Parses `AB12` into (row, column), both 1-based.
pub fn parse_cell(value: &[u8]) -> Option<(u32, u32)> {
    let mut at = 0;
    let mut col = 0u32;
    while at < value.len() && value[at].is_ascii_alphabetic() {
        col = col
            .checked_mul(26)?
            .checked_add(u32::from(value[at].to_ascii_uppercase() - b'A') + 1)?;
        at += 1;
    }
    if at == 0 || col > cgs_model::MAX_COLS {
        return None;
    }
    let row = parse_u32(&value[at..])?;
    (1..=cgs_model::MAX_ROWS)
        .contains(&row)
        .then_some((row, col))
}

/// Parses `A1:C3` or `A1` into a rectangle, tolerating `$` signs.
pub fn parse_rect(text: &str) -> Option<cgs_model::Rect> {
    let ((top, left), (bottom, right)) = cgs_refs::parse_range(text)?;
    Some(cgs_model::Rect {
        top: top.min(bottom),
        left: left.min(right),
        bottom: top.max(bottom),
        right: left.max(right),
    })
}

/// XML parsers turn line endings into `\n` before anyone sees the text, and so does Excel.
/// A real carriage return is written as `_x000D_`, which survives this.
fn push_normalized(text: &str, out: &mut String) {
    if !text.contains('\r') {
        out.push_str(text);
        return;
    }
    out.push_str(&text.replace("\r\n", "\n").replace('\r', "\n"));
}

/// Appends an element's text, including entity references.
pub fn push_text(event: &Event<'_>, out: &mut String) {
    match event {
        Event::Text(text) => {
            if let Ok(decoded) = text.decode() {
                push_normalized(&decoded, out);
            }
        }
        Event::CData(text) => {
            if let Ok(decoded) = text.decode() {
                push_normalized(&decoded, out);
            }
        }
        Event::GeneralRef(reference) => {
            if let Ok(Some(ch)) = reference.resolve_char_ref() {
                out.push(ch);
            } else if let Ok(name) = reference.decode() {
                match name.as_ref() {
                    "amp" => out.push('&'),
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "quot" => out.push('"'),
                    "apos" => out.push('\''),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// Excel escapes control characters in text as `_x000D_`.
pub fn unescape_x(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("_x") {
        return std::borrow::Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'_'
            && at + 7 <= bytes.len()
            && bytes[at + 1] == b'x'
            && bytes[at + 6] == b'_'
            && bytes[at + 2..at + 6].iter().all(u8::is_ascii_hexdigit)
        {
            if let Some(ch) = text
                .get(at + 2..at + 6)
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .and_then(char::from_u32)
            {
                out.push(ch);
                at += 7;
                continue;
            }
        }
        let ch = text[at..].chars().next().unwrap_or(' ');
        out.push(ch);
        at += ch.len_utf8();
    }
    std::borrow::Cow::Owned(out)
}

/// Reads `0`/`1` from a child element's `val`, whose absence means true: `<b/>`.
pub fn val_flag(element: &BytesStart<'_>) -> bool {
    attr_bool(element, b"val").unwrap_or(true)
}

/// A relationship target relative to the part that names it, as a package path.
pub fn resolve_target(base: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// The `.rels` part that goes with a part.
pub fn rels_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::events::Event;
    use quick_xml::Reader;

    fn with_element(xml: &str, check: impl FnOnce(&BytesStart<'_>)) {
        let mut reader = Reader::from_str(xml);
        match reader.read_event() {
            Ok(Event::Start(element) | Event::Empty(element)) => check(&element),
            other => panic!("not an element: {other:?}"),
        }
    }

    #[test]
    fn reads_attributes_by_local_name() {
        with_element(
            r#"<sheet name="A &amp; B" sheetId='2' r:id = "rId7" state="hidden"/>"#,
            |element| {
                assert_eq!(attr(element, b"name").as_deref(), Some("A & B"));
                assert_eq!(attr_u32(element, b"sheetId"), Some(2));
                assert_eq!(attr(element, b"id").as_deref(), Some("rId7"));
                assert_eq!(attr(element, b"missing"), None);
            },
        );
    }

    #[test]
    fn parses_cell_references() {
        assert_eq!(parse_cell(b"A1"), Some((1, 1)));
        assert_eq!(parse_cell(b"ab12"), Some((12, 28)));
        assert_eq!(parse_cell(b"XFD1048576"), Some((1_048_576, 16_384)));
        assert_eq!(parse_cell(b"XFE1"), None);
        assert_eq!(parse_cell(b"A0"), None);
        assert_eq!(parse_cell(b"12"), None);
    }

    #[test]
    fn resolves_relationship_targets() {
        assert_eq!(
            resolve_target("xl/workbook.xml", "worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
        assert_eq!(
            resolve_target("xl/worksheets/sheet1.xml", "../drawings/drawing1.xml"),
            "xl/drawings/drawing1.xml"
        );
        assert_eq!(resolve_target("xl/workbook.xml", "/xl/a.xml"), "xl/a.xml");
        assert_eq!(
            rels_path("xl/worksheets/sheet1.xml"),
            "xl/worksheets/_rels/sheet1.xml.rels"
        );
    }

    #[test]
    fn unescapes_control_characters() {
        assert_eq!(unescape_x("a_x000D_b"), "a\rb");
        assert_eq!(unescape_x("a_xZZZZ_b"), "a_xZZZZ_b");
        assert_eq!(unescape_x("plain"), "plain");
    }
}
