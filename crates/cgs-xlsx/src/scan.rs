//! Splits XML into tokens without changing a byte of it, for the save path. Reading uses
//! `quick-xml`, but a save copies what it doesn't change, so every token here is an exact span
//! of the input, including its quoting, spacing and anything we have never heard of.

use std::io::{self, Read};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `<a ...>`
    Start,
    /// `<a ... />`
    Empty,
    /// `</a>`
    End,
    /// Text between tags. Long text can arrive in several tokens.
    Text,
    /// A comment, CDATA, a processing instruction or a doctype.
    Other,
}

const FIRST_BUFFER: usize = 256 * 1024;

pub struct Scanner<R> {
    reader: R,
    buf: Vec<u8>,
    /// The current token is `buf[from..token_end]`, and unread bytes run up to `to`.
    from: usize,
    token_end: usize,
    to: usize,
    eof: bool,
    /// How many bytes before `buf[0]` have gone.
    dropped: u64,
}

impl<R: Read> Scanner<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buf: vec![0; FIRST_BUFFER],
            from: 0,
            token_end: 0,
            to: 0,
            eof: false,
            dropped: 0,
        }
    }

    /// The current token's bytes.
    pub fn token(&self) -> &[u8] {
        &self.buf[self.from..self.token_end]
    }

    /// Where the current token starts in the input.
    pub fn position(&self) -> u64 {
        self.dropped + self.from as u64
    }

    /// Brings in more of the input, keeping what is unread from `self.from`.
    fn fill(&mut self) -> io::Result<()> {
        if self.from > 0 {
            self.buf.copy_within(self.from..self.to, 0);
            self.dropped += self.from as u64;
            self.to -= self.from;
            self.token_end = self.token_end.saturating_sub(self.from);
            self.from = 0;
        }
        if self.to == self.buf.len() {
            self.buf.resize(self.buf.len() * 2, 0);
        }
        loop {
            match self.reader.read(&mut self.buf[self.to..]) {
                Ok(0) => {
                    self.eof = true;
                    return Ok(());
                }
                Ok(count) => {
                    self.to += count;
                    return Ok(());
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }

    /// Moves to the next token. `None` means the input has ended.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<Option<Kind>> {
        self.from = self.token_end;
        if self.from == self.to {
            if self.eof {
                return Ok(None);
            }
            self.fill()?;
            if self.from == self.to {
                return Ok(None);
            }
        }
        if self.buf[self.from] != b'<' {
            let unread = &self.buf[self.from..self.to];
            let length = unread
                .iter()
                .position(|&byte| byte == b'<')
                .unwrap_or(unread.len());
            self.token_end = self.from + length;
            return Ok(Some(Kind::Text));
        }
        loop {
            if let Some((length, kind)) = markup_end(&self.buf[self.from..self.to]) {
                self.token_end = self.from + length;
                return Ok(Some(kind));
            }
            if self.eof {
                // A tag with no end, in a damaged file: keep its bytes as they are.
                self.token_end = self.to;
                return Ok(Some(Kind::Text));
            }
            self.fill()?;
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// How long the markup at the start of `bytes` is, if all of it is there.
fn markup_end(bytes: &[u8]) -> Option<(usize, Kind)> {
    const CDATA: &[u8] = b"<![CDATA[";
    let second = *bytes.get(1)?;
    match second {
        b'?' => find(bytes, b"?>").map(|at| (at + 2, Kind::Other)),
        b'/' => find(bytes, b">").map(|at| (at + 1, Kind::End)),
        b'!' => {
            if bytes.starts_with(b"<!--") {
                return find(&bytes[4..], b"-->").map(|at| (at + 7, Kind::Other));
            }
            if bytes.starts_with(CDATA) {
                return find(&bytes[CDATA.len()..], b"]]>")
                    .map(|at| (CDATA.len() + at + 3, Kind::Other));
            }
            if bytes.len() < CDATA.len() {
                return None;
            }
            // A doctype, which can hold a bracketed subset.
            let mut depth = 0;
            for (at, &byte) in bytes.iter().enumerate() {
                match byte {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    b'>' if depth <= 0 => return Some((at + 1, Kind::Other)),
                    _ => {}
                }
            }
            None
        }
        _ => {
            let mut quote = 0;
            for (at, &byte) in bytes.iter().enumerate() {
                if quote != 0 {
                    if byte == quote {
                        quote = 0;
                    }
                } else if byte == b'"' || byte == b'\'' {
                    quote = byte;
                } else if byte == b'>' {
                    let kind = if bytes[at - 1] == b'/' {
                        Kind::Empty
                    } else {
                        Kind::Start
                    };
                    return Some((at + 1, kind));
                }
            }
            None
        }
    }
}

/// A start or end tag's full name, prefix included.
fn full_name(tag: &[u8]) -> &[u8] {
    let start = if tag.starts_with(b"</") { 2 } else { 1 };
    let rest = &tag[start.min(tag.len())..];
    let end = rest
        .iter()
        .position(|&byte| byte.is_ascii_whitespace() || byte == b'>' || byte == b'/')
        .unwrap_or(rest.len());
    &rest[..end]
}

/// A tag's name without its prefix.
pub fn local_name(tag: &[u8]) -> &[u8] {
    let name = full_name(tag);
    match name.iter().rposition(|&byte| byte == b':') {
        Some(colon) => &name[colon + 1..],
        None => name,
    }
}

/// A tag's namespace prefix with its colon, such as `x:`, or nothing.
pub fn prefix(tag: &[u8]) -> String {
    let name = full_name(tag);
    match name.iter().rposition(|&byte| byte == b':') {
        Some(colon) => String::from_utf8_lossy(&name[..=colon]).into_owned(),
        None => String::new(),
    }
}

/// One attribute of a tag, as ranges of the tag's bytes.
#[derive(Debug, Clone, Copy)]
struct AttrSpan {
    /// From the space before the name to the closing quote.
    whole: (usize, usize),
    name: (usize, usize),
    value: (usize, usize),
}

fn attr_spans(tag: &[u8]) -> Vec<AttrSpan> {
    let mut spans = Vec::new();
    let mut at = 1 + full_name(tag).len();
    loop {
        let before = at;
        while at < tag.len() && tag[at].is_ascii_whitespace() {
            at += 1;
        }
        let name_start = at;
        while at < tag.len() && tag[at] != b'=' && !tag[at].is_ascii_whitespace() && tag[at] != b'>'
        {
            at += 1;
        }
        let name_end = at;
        while at < tag.len() && tag[at] != b'"' && tag[at] != b'\'' && tag[at] != b'>' {
            at += 1;
        }
        if at >= tag.len() || tag[at] == b'>' || name_start == name_end {
            return spans;
        }
        let quote = tag[at];
        at += 1;
        let value_start = at;
        while at < tag.len() && tag[at] != quote {
            at += 1;
        }
        if at >= tag.len() {
            return spans;
        }
        spans.push(AttrSpan {
            whole: (before, at + 1),
            name: (name_start, name_end),
            value: (value_start, at),
        });
        at += 1;
    }
}

/// An attribute's raw value, found by its full name.
pub fn attr_value<'a>(tag: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    attr_spans(tag)
        .into_iter()
        .find(|span| &tag[span.name.0..span.name.1] == name)
        .map(|span| &tag[span.value.0..span.value.1])
}

/// Copies a tag, changing only what is named. `Some` sets an attribute, whose value must
/// already be escaped, and `None` removes one. Everything else keeps its bytes and its order.
pub fn edit_tag(tag: &[u8], changes: &[(&str, Option<&str>)]) -> Vec<u8> {
    let spans = attr_spans(tag);
    let mut out = Vec::with_capacity(tag.len() + 32);
    let mut copied = 0;
    for span in &spans {
        let name = &tag[span.name.0..span.name.1];
        let Some((_, change)) = changes.iter().find(|(key, _)| key.as_bytes() == name) else {
            continue;
        };
        out.extend_from_slice(&tag[copied..span.whole.0]);
        if let Some(value) = change {
            out.extend_from_slice(&tag[span.whole.0..span.value.0]);
            out.extend_from_slice(value.as_bytes());
            out.extend_from_slice(&tag[span.value.1..span.whole.1]);
        }
        copied = span.whole.1;
    }
    let closing = closing_length(tag);
    out.extend_from_slice(&tag[copied..tag.len() - closing]);
    for (key, change) in changes {
        let present = spans
            .iter()
            .any(|span| &tag[span.name.0..span.name.1] == key.as_bytes());
        if let (false, Some(value)) = (present, change) {
            out.push(b' ');
            out.extend_from_slice(key.as_bytes());
            out.extend_from_slice(b"=\"");
            out.extend_from_slice(value.as_bytes());
            out.push(b'"');
        }
    }
    out.extend_from_slice(&tag[tag.len() - closing..]);
    out
}

fn closing_length(tag: &[u8]) -> usize {
    if tag.ends_with(b"/>") {
        2
    } else {
        usize::from(tag.ends_with(b">"))
    }
}

/// `<a b="c"/>` as `<a b="c">`, for an element that is about to have children.
pub fn opened(tag: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    if out.ends_with(b"/>") {
        out.truncate(out.len() - 2);
        out.push(b'>');
    }
    out
}

/// `<a b="c">` as `<a b="c"/>`, for an element that has lost its children.
pub fn closed(tag: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    if out.ends_with(b">") && !out.ends_with(b"/>") {
        out.truncate(out.len() - 1);
        out.extend_from_slice(b"/>");
    }
    out
}

/// Text for an element, with XML's own characters escaped.
pub fn escape_text(text: &str, out: &mut Vec<u8>) {
    for byte in text.bytes() {
        match byte {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            other => out.push(other),
        }
    }
}

/// Text for an attribute, with quotes and line breaks kept from being lost.
pub fn escape_attr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\t' => out.push_str("&#9;"),
            '\r' => out.push_str("&#13;"),
            other => out.push(other),
        }
    }
    out
}

/// Writes characters XML can't hold as Excel's `_xHHHH_`, and escapes a literal one of those so
/// it reads back as itself. A file's text then round-trips through `unescape_x`.
pub fn escape_x(text: &str) -> String {
    let needs = |ch: char| matches!(ch, '\0'..='\x08' | '\x0B' | '\x0C' | '\x0E'..='\x1F' | '\r' | '\u{FFFE}' | '\u{FFFF}');
    let looks_escaped = |rest: &str| {
        let bytes = rest.as_bytes();
        bytes.len() >= 7
            && bytes[1] == b'x'
            && bytes[6] == b'_'
            && bytes[2..6].iter().all(u8::is_ascii_hexdigit)
    };
    let mut out = String::with_capacity(text.len());
    for (at, ch) in text.char_indices() {
        if needs(ch) {
            out.push_str(&format!("_x{:04X}_", ch as u32));
        } else if ch == '_' && looks_escaped(&text[at..]) {
            out.push_str("_x005F_");
        } else {
            out.push(ch);
        }
    }
    out
}

/// Replaces byte ranges of `source`. The ranges must be in order and must not overlap.
pub fn splice(source: &[u8], changes: &[(usize, usize, Vec<u8>)]) -> Vec<u8> {
    let added: usize = changes.iter().map(|(_, _, text)| text.len()).sum();
    let mut out = Vec::with_capacity(source.len() + added);
    let mut copied = 0;
    for (from, to, text) in changes {
        out.extend_from_slice(&source[copied..*from]);
        out.extend_from_slice(text);
        copied = *to;
    }
    out.extend_from_slice(&source[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(xml: &[u8]) -> Vec<(Kind, Vec<u8>)> {
        let mut scanner = Scanner::new(xml);
        let mut out = Vec::new();
        while let Some(kind) = scanner.next().unwrap() {
            out.push((kind, scanner.token().to_vec()));
        }
        out
    }

    #[test]
    fn tokens_join_up_to_the_exact_input() {
        let xml = br#"<?xml version="1.0"?><!-- a > b --><!DOCTYPE x [<!ENTITY y "z">]><a b='>' c="/">t<![CDATA[<d>]]><e/></a  >tail<broken"#;
        let joined: Vec<u8> = tokens(xml)
            .into_iter()
            .flat_map(|(_, bytes)| bytes)
            .collect();
        assert_eq!(joined, xml);
        let kinds: Vec<Kind> = tokens(xml).into_iter().map(|(kind, _)| kind).collect();
        assert_eq!(
            kinds,
            [
                Kind::Other,
                Kind::Other,
                Kind::Other,
                Kind::Start,
                Kind::Text,
                Kind::Other,
                Kind::Empty,
                Kind::End,
                Kind::Text,
                Kind::Text,
            ]
        );
    }

    #[test]
    fn a_token_survives_the_buffer_ending_inside_it() {
        // One byte at a time is the worst a reader can do.
        struct Slow<'a>(&'a [u8]);
        impl Read for Slow<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                if self.0.is_empty() {
                    return Ok(0);
                }
                out[0] = self.0[0];
                self.0 = &self.0[1..];
                Ok(1)
            }
        }
        let mut xml = b"<a>".to_vec();
        xml.extend(
            std::iter::repeat_n(b"<![CDATA[x]]><b c=\"d\"/>text<!--z-->".as_slice(), 20_000)
                .flatten(),
        );
        xml.extend_from_slice(b"</a>");
        let mut scanner = Scanner::new(Slow(&xml));
        let mut joined = Vec::new();
        let mut last_end = 0;
        while scanner.next().unwrap().is_some() {
            assert_eq!(scanner.position(), last_end);
            joined.extend_from_slice(scanner.token());
            last_end += scanner.token().len() as u64;
        }
        assert_eq!(joined, xml);
    }

    #[test]
    fn reads_tag_names_and_attributes() {
        let tag = br#"<x:c r="A1" x14ac:dyDescent='0.2' t = "s">"#;
        assert_eq!(local_name(tag), b"c");
        assert_eq!(prefix(tag), "x:");
        assert_eq!(attr_value(tag, b"r"), Some(&b"A1"[..]));
        assert_eq!(attr_value(tag, b"x14ac:dyDescent"), Some(&b"0.2"[..]));
        assert_eq!(attr_value(tag, b"t"), Some(&b"s"[..]));
        assert_eq!(attr_value(tag, b"dyDescent"), None);
        assert_eq!(local_name(b"</x:row>"), b"row");
    }

    #[test]
    fn editing_a_tag_touches_only_what_it_names() {
        let tag = br#"<c  r="A1"   s='3' t="s"  foo:bar="x" />"#;
        assert_eq!(
            edit_tag(tag, &[("t", None), ("s", Some("9")), ("cm", Some("1"))]),
            br#"<c  r="A1"   s='9'  foo:bar="x"  cm="1"/>"#.to_vec()
        );
        assert_eq!(
            edit_tag(br#"<c r="A1">"#, &[("t", Some("b"))]),
            br#"<c r="A1" t="b">"#
        );
        assert_eq!(edit_tag(br#"<c t="b">"#, &[("t", None)]), b"<c>");
        assert_eq!(opened(br#"<c r="A1"/>"#), br#"<c r="A1">"#);
        assert_eq!(closed(br#"<c r="A1">"#), br#"<c r="A1"/>"#);
    }

    #[test]
    fn text_that_looks_escaped_reads_back_as_itself() {
        for text in [
            "plain",
            "a_x0041_b",
            "line\r\nbreak",
            "\u{1}\u{1F}",
            "_x",
            "_x00_",
            "ع_x000D_",
        ] {
            let written = escape_x(text);
            assert!(!written.contains('\r'));
            assert_eq!(crate::xml::unescape_x(&written), text);
        }
    }
}
