//! Patches the package's smaller parts at byte spans: content types, relationships, the workbook
//! part, styles, shared strings, tables and pivot caches. Each adds or changes only what it must.

use std::io::{self, Read, Write};

use crate::patch_sheet::StringTable;
use crate::scan::{
    attr_value, closed, edit_tag, escape_attr, escape_text, escape_x, local_name, opened, prefix,
    splice, utf8_declaration, Kind, Scanner,
};
use crate::xml::{parse_u32, unescape_attr, unescape_x};

/// `CT_Workbook`'s children in the order the schema wants them.
const WORKBOOK_ORDER: &[&str] = &[
    "fileVersion",
    "fileSharing",
    "workbookPr",
    "workbookProtection",
    "bookViews",
    "sheets",
    "functionGroups",
    "externalReferences",
    "definedNames",
    "calcPr",
    "oleSize",
    "customWorkbookViews",
    "pivotCaches",
    "smartTagPr",
    "smartTagTypes",
    "webPublishing",
    "fileRecoveryPr",
    "webPublishObjects",
    "extLst",
];

/// `CT_Stylesheet`'s children in the order the schema wants them.
const STYLES_ORDER: &[&str] = &[
    "numFmts",
    "fonts",
    "fills",
    "borders",
    "cellStyleXfs",
    "cellXfs",
    "cellStyles",
    "dxfs",
    "tableStyles",
    "colors",
    "extLst",
];

fn order_in(table: &[&str], local: &[u8]) -> Option<usize> {
    table.iter().position(|name| name.as_bytes() == local)
}

/// One token with where it is, for the small parts.
struct Located {
    kind: Kind,
    start: usize,
    end: usize,
    depth: u32,
}

/// The tokens of a small part, each with the depth of the element it opens or sits in.
fn tokens(xml: &[u8]) -> Vec<Located> {
    let mut scanner = Scanner::new(xml);
    let mut out = Vec::new();
    let mut depth = 0;
    // Reading from memory has no way to fail.
    while let Ok(Some(kind)) = scanner.next() {
        let start = scanner.position() as usize;
        let end = start + scanner.token().len();
        if kind == Kind::End {
            depth = u32::max(depth, 1) - 1;
        }
        out.push(Located {
            kind,
            start,
            end,
            depth,
        });
        if kind == Kind::Start {
            depth += 1;
        }
    }
    out
}

fn root_prefix(xml: &[u8], list: &[Located]) -> String {
    list.iter()
        .find(|token| matches!(token.kind, Kind::Start | Kind::Empty))
        .map_or_else(String::new, |token| prefix(&xml[token.start..token.end]))
}

/// Puts `text` just before the root's end, opening a root that has no children yet.
fn before_root_end(xml: &[u8], text: &[u8]) -> Vec<u8> {
    let list = tokens(xml);
    if let Some(end) = list
        .iter()
        .rfind(|token| token.kind == Kind::End && token.depth == 0)
    {
        return splice(xml, &[(end.start, end.start, text.to_vec())]);
    }
    if let Some(root) = list
        .iter()
        .find(|token| token.kind == Kind::Empty && token.depth == 0)
    {
        let tag = &xml[root.start..root.end];
        let mut replacement = opened(tag);
        replacement.extend_from_slice(text);
        let name = String::from_utf8_lossy(&tag[1..])
            .split(|ch: char| ch.is_ascii_whitespace() || ch == '/')
            .next()
            .unwrap_or_default()
            .to_string();
        replacement.extend_from_slice(format!("</{name}>").as_bytes());
        return splice(xml, &[(root.start, root.end, replacement)]);
    }
    xml.to_vec()
}

fn same_part(a: &str, b: &str) -> bool {
    crate::zipdir::normalize(a) == crate::zipdir::normalize(b)
}

/// Takes the `Override`s for these parts out of `[Content_Types].xml`.
pub fn remove_overrides(xml: &[u8], parts: &[String]) -> Vec<u8> {
    let list = tokens(xml);
    let mut changes = Vec::new();
    for (at, token) in list.iter().enumerate() {
        if !matches!(token.kind, Kind::Start | Kind::Empty) {
            continue;
        }
        let tag = &xml[token.start..token.end];
        if local_name(tag) != b"Override" {
            continue;
        }
        let name = attr_value(tag, b"PartName")
            .map(unescape_attr)
            .unwrap_or_default();
        if !parts.iter().any(|part| same_part(part, &name)) {
            continue;
        }
        let end = if token.kind == Kind::Empty {
            token.end
        } else {
            list[at..]
                .iter()
                .find(|other| other.kind == Kind::End && other.depth == token.depth)
                .map_or(token.end, |other| other.end)
        };
        changes.push((token.start, end, Vec::new()));
    }
    splice(xml, &changes)
}

/// Adds an `Override` for a new part to `[Content_Types].xml`, in place of one naming a part the
/// file claims and doesn't have.
pub fn add_override(xml: &[u8], part: &str, content_type: &str) -> Vec<u8> {
    let xml = &remove_overrides(xml, &[part.to_string()]);
    let name = root_prefix(xml, &tokens(xml));
    let text = format!(
        "<{name}Override PartName=\"/{}\" ContentType=\"{}\"/>",
        escape_attr(part),
        escape_attr(content_type)
    );
    before_root_end(xml, text.as_bytes())
}

/// Takes the relationships of this type out of a `.rels` part.
pub fn remove_relationships(xml: &[u8], type_ends_with: &str) -> Vec<u8> {
    let list = tokens(xml);
    let mut changes = Vec::new();
    for (at, token) in list.iter().enumerate() {
        if !matches!(token.kind, Kind::Start | Kind::Empty) {
            continue;
        }
        let tag = &xml[token.start..token.end];
        if local_name(tag) != b"Relationship" {
            continue;
        }
        let kind = attr_value(tag, b"Type")
            .map(unescape_attr)
            .unwrap_or_default();
        if !kind.ends_with(type_ends_with) {
            continue;
        }
        let end = if token.kind == Kind::Empty {
            token.end
        } else {
            list[at..]
                .iter()
                .find(|other| other.kind == Kind::End && other.depth == token.depth)
                .map_or(token.end, |other| other.end)
        };
        changes.push((token.start, end, Vec::new()));
    }
    splice(xml, &changes)
}

/// Adds a relationship, with an id nothing in the part uses.
pub fn add_relationship(xml: &[u8], kind: &str, target: &str) -> Vec<u8> {
    add_relationship_with_id(xml, kind, target).0
}

/// Removes relationships by their ids.
pub fn remove_relationship_ids(xml: &[u8], ids: &[String]) -> Vec<u8> {
    let mut changes = Vec::new();
    for token in tokens(xml) {
        if !matches!(token.kind, Kind::Start | Kind::Empty) {
            continue;
        }
        let tag = &xml[token.start..token.end];
        if local_name(tag) != b"Relationship" {
            continue;
        }
        let Some(id) = attr_value(tag, b"Id").map(unescape_attr) else {
            continue;
        };
        if ids.contains(&id) {
            changes.push((token.start, token.end, Vec::new()));
        }
    }
    crate::scan::splice(xml, &changes)
}

/// Adds a relationship to something outside the package, such as a web address.
pub fn add_external_relationship(xml: &[u8], kind: &str, target: &str) -> (Vec<u8>, String) {
    add_relationship_of(xml, kind, target, true)
}

/// Adds a relationship and says its id.
pub fn add_relationship_with_id(xml: &[u8], kind: &str, target: &str) -> (Vec<u8>, String) {
    add_relationship_of(xml, kind, target, false)
}

fn add_relationship_of(xml: &[u8], kind: &str, target: &str, external: bool) -> (Vec<u8>, String) {
    let list = tokens(xml);
    let name = root_prefix(xml, &list);
    let mut used = Vec::new();
    for token in &list {
        if matches!(token.kind, Kind::Start | Kind::Empty) {
            if let Some(id) = attr_value(&xml[token.start..token.end], b"Id") {
                used.push(unescape_attr(id));
            }
        }
    }
    let mut number = used.len() + 1;
    let id = loop {
        let candidate = format!("rId{number}");
        if !used.contains(&candidate) {
            break candidate;
        }
        number += 1;
    };
    let mode = if external {
        " TargetMode=\"External\""
    } else {
        ""
    };
    let text = format!(
        "<{name}Relationship Id=\"{id}\" Type=\"{}\" Target=\"{}\"{mode}/>",
        escape_attr(kind),
        escape_attr(target)
    );
    (before_root_end(xml, text.as_bytes()), id)
}

/// Sets whether the workbook calculates only when asked.
pub fn set_calc_mode(xml: &[u8], manual: bool) -> Vec<u8> {
    let list = tokens(xml);
    let name = root_prefix(xml, &list);
    let wanted = order_in(WORKBOOK_ORDER, b"calcPr");
    let value = if manual { Some("manual") } else { None };
    for token in &list {
        if !matches!(token.kind, Kind::Start | Kind::Empty) {
            continue;
        }
        let tag = &xml[token.start..token.end];
        let local = local_name(tag);
        if local == b"calcPr" {
            let edited = edit_tag(tag, &[("calcMode", value)]);
            return crate::scan::splice(xml, &[(token.start, token.end, edited)]);
        }
        if token.depth == 1 && order_in(WORKBOOK_ORDER, local) > wanted {
            if !manual {
                return xml.to_vec();
            }
            let text = format!("<{name}calcPr calcMode=\"manual\"/>").into_bytes();
            return crate::scan::splice(xml, &[(token.start, token.start, text)]);
        }
    }
    if !manual {
        return xml.to_vec();
    }
    before_root_end(
        xml,
        format!("<{name}calcPr calcMode=\"manual\"/>").as_bytes(),
    )
}

/// Asks Excel to calculate the workbook when it opens, adding `calcPr` where the schema wants
/// it if the file has none.
pub fn set_full_calc_on_load(xml: &[u8]) -> Vec<u8> {
    let list = tokens(xml);
    let name = root_prefix(xml, &list);
    let wanted = order_in(WORKBOOK_ORDER, b"calcPr");
    for token in &list {
        if token.depth != 1 || !matches!(token.kind, Kind::Start | Kind::Empty) {
            continue;
        }
        let tag = &xml[token.start..token.end];
        let local = local_name(tag);
        if local == b"calcPr" {
            let edited = edit_tag(tag, &[("fullCalcOnLoad", Some("1"))]);
            return splice(xml, &[(token.start, token.end, edited)]);
        }
        let order = order_in(WORKBOOK_ORDER, local);
        if order.is_some() && order > wanted {
            let text = format!("<{name}calcPr fullCalcOnLoad=\"1\"/>").into_bytes();
            return splice(xml, &[(token.start, token.start, text)]);
        }
    }
    before_root_end(
        xml,
        format!("<{name}calcPr fullCalcOnLoad=\"1\"/>").as_bytes(),
    )
}

/// Sets `refreshOnLoad` on a pivot cache definition.
pub fn set_refresh_on_load(xml: &[u8]) -> Vec<u8> {
    let list = tokens(xml);
    let Some(root) = list
        .iter()
        .find(|token| matches!(token.kind, Kind::Start | Kind::Empty))
    else {
        return xml.to_vec();
    };
    let edited = edit_tag(&xml[root.start..root.end], &[("refreshOnLoad", Some("1"))]);
    splice(xml, &[(root.start, root.end, edited)])
}

/// Counts `<si>` in a shared strings part, which is where new strings start.
pub fn count_strings<R: Read>(reader: R) -> io::Result<u32> {
    let mut scanner = Scanner::new(reader);
    let mut count = 0;
    while let Some(kind) = scanner.next()? {
        if matches!(kind, Kind::Start | Kind::Empty) && local_name(scanner.token()) == b"si" {
            count += 1;
        }
    }
    Ok(count)
}

fn string_items(strings: &StringTable, si: &str, t: &str, ascii_only: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for (at, text) in strings.added.iter().enumerate() {
        if let Some(items) = strings.rich.get(&at) {
            // Rich strings made here go in without a prefix, declaring the namespace.
            if si == "si" {
                out.extend_from_slice(format!("<si>{items}</si>").as_bytes());
            } else {
                out.extend_from_slice(
                    format!(
                        "<si xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">{items}</si>"
                    )
                    .as_bytes(),
                );
            }
            continue;
        }
        let spaced = text.starts_with(char::is_whitespace)
            || text.ends_with(char::is_whitespace)
            || text.contains('\n');
        out.extend_from_slice(format!("<{si}><{t}").as_bytes());
        if spaced {
            out.extend_from_slice(b" xml:space=\"preserve\"");
        }
        out.push(b'>');
        let mut body = Vec::new();
        escape_text(&escape_x(text), &mut body);
        if ascii_only {
            for ch in String::from_utf8_lossy(&body).chars() {
                if ch.is_ascii() {
                    out.push(ch as u8);
                } else {
                    out.extend_from_slice(format!("&#{};", ch as u32).as_bytes());
                }
            }
        } else {
            out.extend_from_slice(&body);
        }
        out.extend_from_slice(format!("</{t}></{si}>").as_bytes());
    }
    out
}

/// Copies a shared strings part with new strings on its end. Existing indexes all stay valid,
/// and the part's counts go up by what was added.
pub fn append_strings<R: Read>(
    reader: R,
    strings: &StringTable,
    ascii_only: bool,
    out: &mut dyn Write,
) -> io::Result<()> {
    crate::pace::block_on(append_strings_paced(
        reader,
        strings,
        ascii_only,
        out,
        &crate::pace::Pace::unlimited(),
    ))
}

/// `append_strings`, stopping between tags at `pace`'s points.
pub async fn append_strings_paced<R: Read>(
    reader: R,
    strings: &StringTable,
    ascii_only: bool,
    out: &mut dyn Write,
    pace: &crate::pace::Pace,
) -> io::Result<()> {
    let mut scanner = Scanner::new(reader);
    let mut depth = 0;
    let mut name = String::new();
    let mut written = false;
    let raise = |tag: &[u8], key: &[u8], by: u64| {
        attr_value(tag, key)
            .and_then(parse_u32)
            .map(|count| (u64::from(count) + by).to_string())
    };
    loop {
        pace.point().await;
        let Some(kind) = scanner.next()? else {
            break;
        };
        let token = scanner.token();
        match kind {
            Kind::Start | Kind::Empty if depth == 0 => {
                name = prefix(token);
                let count = raise(token, b"count", strings.references);
                let unique = raise(token, b"uniqueCount", strings.added.len() as u64);
                let mut changes: Vec<(&str, Option<&str>)> = Vec::new();
                if let Some(count) = &count {
                    changes.push(("count", Some(count)));
                }
                if let Some(unique) = &unique {
                    changes.push(("uniqueCount", Some(unique)));
                }
                let tag = edit_tag(token, &changes);
                if kind == Kind::Start {
                    depth += 1;
                    out.write_all(&tag)?;
                } else {
                    out.write_all(&opened(&tag))?;
                    let si = format!("{name}si");
                    let t = format!("{name}t");
                    out.write_all(&string_items(strings, &si, &t, ascii_only))?;
                    out.write_all(format!("</{name}sst>").as_bytes())?;
                    written = true;
                }
            }
            Kind::Start => {
                depth += 1;
                out.write_all(token)?;
            }
            Kind::End => {
                depth -= 1;
                if depth == 0 {
                    let closing = token.to_vec();
                    let si = format!("{name}si");
                    let t = format!("{name}t");
                    out.write_all(&string_items(strings, &si, &t, ascii_only))?;
                    written = true;
                    out.write_all(&closing)?;
                } else {
                    out.write_all(token)?;
                }
            }
            Kind::Other => match utf8_declaration(token) {
                Some(fixed) => out.write_all(&fixed)?,
                None => out.write_all(token)?,
            },
            _ => out.write_all(token)?,
        }
    }
    if written {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the shared strings part never ends",
        ))
    }
}

/// A new shared strings part, for a workbook that had none.
pub fn new_strings_part(strings: &StringTable) -> Vec<u8> {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" count=\"{}\" uniqueCount=\"{}\">",
        strings.references,
        strings.added.len()
    )
    .into_bytes();
    out.extend_from_slice(&string_items(strings, "si", "t", false));
    out.extend_from_slice(b"</sst>");
    out
}

/// A table column's name as its `name` attribute stores it.
pub fn table_column_attr(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '\n' => out.push_str("_x000a_"),
            '\r' => out.push_str("_x000d_"),
            '\t' => out.push_str("_x0009_"),
            other => out.push(other),
        }
    }
    escape_attr(&out)
}

/// Sets table column names, by their position in the table.
pub fn set_table_columns(xml: &[u8], names: &[(usize, String)]) -> Vec<u8> {
    let list = tokens(xml);
    let mut changes = Vec::new();
    let mut index = 0;
    for token in &list {
        if !matches!(token.kind, Kind::Start | Kind::Empty) {
            continue;
        }
        let tag = &xml[token.start..token.end];
        if local_name(tag) != b"tableColumn" {
            continue;
        }
        if let Some((_, name)) = names.iter().find(|(at, _)| *at == index) {
            let value = table_column_attr(name);
            changes.push((
                token.start,
                token.end,
                edit_tag(tag, &[("name", Some(&value))]),
            ));
        }
        index += 1;
    }
    splice(xml, &changes)
}

/// A table part with its calculated and totals formulas following renamed columns, as (old, new).
pub fn rename_in_table_formulas(xml: &[u8], table: &str, renames: &[(String, String)]) -> Vec<u8> {
    let list = tokens(xml);
    let mut changes = Vec::new();
    for (at, token) in list.iter().enumerate() {
        if token.kind != Kind::Start {
            continue;
        }
        let tag = &xml[token.start..token.end];
        if !matches!(
            local_name(tag),
            b"calculatedColumnFormula" | b"totalsRowFormula"
        ) {
            continue;
        }
        let Some(text) = list.get(at + 1).filter(|next| next.kind == Kind::Text) else {
            continue;
        };
        let formula = unescape_attr(&xml[text.start..text.end]);
        let mut next = std::borrow::Cow::Borrowed(formula.as_str());
        for (old, new) in renames {
            if let std::borrow::Cow::Owned(changed) =
                cgs_refs::rename_table_column(&next, table, old, new, true)
            {
                next = std::borrow::Cow::Owned(changed);
            }
        }
        if let std::borrow::Cow::Owned(changed) = next {
            let mut escaped = Vec::new();
            escape_text(&changed, &mut escaped);
            changes.push((text.start, text.end, escaped));
        }
    }
    splice(xml, &changes)
}

/// A table's place and column names, read from its part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TablePart {
    pub reference: String,
    pub display_name: String,
    pub header_rows: u32,
    pub columns: Vec<String>,
}

pub fn read_table_part(xml: &[u8]) -> Option<TablePart> {
    let list = tokens(xml);
    let root = list
        .iter()
        .find(|token| matches!(token.kind, Kind::Start | Kind::Empty))?;
    let tag = &xml[root.start..root.end];
    let mut part = TablePart {
        reference: attr_value(tag, b"ref").map(unescape_attr)?,
        display_name: attr_value(tag, b"displayName")
            .map(unescape_attr)
            .unwrap_or_default(),
        header_rows: attr_value(tag, b"headerRowCount")
            .and_then(parse_u32)
            .unwrap_or(1),
        columns: Vec::new(),
    };
    for token in &list {
        if matches!(token.kind, Kind::Start | Kind::Empty) {
            let tag = &xml[token.start..token.end];
            if local_name(tag) == b"tableColumn" {
                let name = attr_value(tag, b"name")
                    .map(unescape_attr)
                    .unwrap_or_default();
                part.columns.push(unescape_x(&name).into_owned());
            }
        }
    }
    Some(part)
}

/// What differs in a new cell format from the one it starts from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XfChanges {
    pub font: Option<u32>,
    pub fill: Option<u32>,
    pub border: Option<u32>,
    pub num_fmt: Option<u32>,
    /// `<alignment>`'s attributes, starting with a space.
    pub alignment: Option<String>,
    /// Attributes to set or remove on the `<alignment>` the format starts from, keeping the rest.
    pub alignment_edits: Vec<(&'static str, Option<String>)>,
    /// The format's `<protection>`: whether it's locked and whether its formula is hidden.
    pub protection: Option<(bool, bool)>,
}

/// What changes in a font. The rest of it stays, including what we don't read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FontEdits {
    pub name: Option<String>,
    pub size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub strike: Option<bool>,
    /// `single`, `double`, `singleAccounting` or `doubleAccounting`, or nothing for none.
    pub underline: Option<Option<String>>,
    /// `<color>`'s attributes, starting with a space, or nothing for automatic.
    pub color: Option<Option<String>>,
}

/// What changes in a border. Each line is its style and `<color>`'s attributes, or nothing for none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BorderEdits {
    pub left: Option<Option<(String, String)>>,
    pub right: Option<Option<(String, String)>>,
    pub top: Option<Option<(String, String)>>,
    pub bottom: Option<Option<(String, String)>>,
    pub diagonal: Option<Option<(String, String)>>,
    pub diagonal_up: Option<bool>,
    pub diagonal_down: Option<bool>,
}

/// A font's children in the order Excel writes them, which the validator holds us to.
const FONT_ORDER: &[&str] = &[
    "b",
    "i",
    "strike",
    "condense",
    "extend",
    "outline",
    "shadow",
    "u",
    "vertAlign",
    "sz",
    "color",
    "name",
    "family",
    "charset",
    "scheme",
];

/// Writes a number as briefly as it reads back.
fn point_text(points: f32) -> String {
    let rounded = (f64::from(points) * 100.0).round() / 100.0;
    format!("{rounded}")
}

/// A flag such as `<b/>` is on unless it says `val="0"`.
fn flag_child(name: &str, on: bool) -> Option<String> {
    on.then(|| format!("<{name}/>"))
}

/// A cell format's XML with this alignment in place of any it had.
fn with_alignment(xf: &str, name: &str, attributes: &str) -> String {
    let element = format!("<{name}alignment{attributes}/>");
    // Without children it is closed in its first tag.
    if let Some(body) = xf.strip_suffix("/>") {
        return format!("{body}>{element}</{name}xf>");
    }
    let open = format!("<{name}alignment");
    if let Some(start) = xf.find(&open) {
        let tail = &xf[start..];
        if let Some(end) = tail.find("/>").map(|at| at + 2).or_else(|| {
            tail.find(&format!("</{name}alignment>"))
                .map(|at| at + name.len() + 12)
        }) {
            return format!("{}{element}{}", &xf[..start], &tail[end..]);
        }
    }
    // Alignment comes before anything else inside a format.
    match xf.find('>') {
        Some(at) => format!("{}{element}{}", &xf[..=at], &xf[at + 1..]),
        None => xf.to_string(),
    }
}

/// A format with its `<protection>` set, after its `<alignment>` as the schema orders them. A
/// format that is locked with its formula shown says nothing, as Excel writes it.
fn with_protection(xf: &str, name: &str, locked: bool, hidden: bool) -> String {
    let mut attributes = String::new();
    if !locked {
        attributes.push_str(" locked=\"0\"");
    }
    if hidden {
        attributes.push_str(" hidden=\"1\"");
    }
    let open = format!("<{name}protection");
    // Take out the one it had.
    let mut xf = xf.to_string();
    if let Some(start) = xf.find(&open) {
        let tail = &xf[start..];
        let end = tail.find("/>").map(|at| at + 2).or_else(|| {
            tail.find(&format!("</{name}protection>"))
                .map(|at| at + name.len() + 13)
        });
        if let Some(end) = end {
            xf = format!("{}{}", &xf[..start], &tail[end..]);
        }
    }
    let flag = if attributes.is_empty() {
        None
    } else {
        Some("1")
    };
    let tag_end = xf.find('>').map_or(xf.len(), |at| at + 1);
    let edited = edit_tag(&xf.as_bytes()[..tag_end], &[("applyProtection", flag)]);
    let xf = format!("{}{}", String::from_utf8_lossy(&edited), &xf[tag_end..]);
    if attributes.is_empty() {
        return xf;
    }
    let element = format!("<{name}protection{attributes}/>");
    if let Some(body) = xf.strip_suffix("/>") {
        return format!("{body}>{element}</{name}xf>");
    }
    // After `<alignment>` if there is one, and before anything else.
    let alignment = format!("<{name}alignment");
    let at = match xf.find(&alignment) {
        Some(start) => {
            let tail = &xf[start..];
            tail.find("/>").map(|end| start + end + 2).or_else(|| {
                tail.find(&format!("</{name}alignment>"))
                    .map(|end| start + end + name.len() + 12)
            })
        }
        None => xf.find('>').map(|at| at + 1),
    };
    match at {
        Some(at) => format!("{}{element}{}", &xf[..at], &xf[at..]),
        None => xf,
    }
}

/// `styles.xml`, which only grows, so every index in the file stays valid. It holds the whole
/// part as bytes: the part is small, and the model reads its new styles back from it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StylesXml {
    pub bytes: Vec<u8>,
    pub changed: bool,
}

/// Part of a style sheet: the span of one list and how many entries it holds.
struct ListAt {
    /// The list's start tag.
    tag: (usize, usize),
    /// Where a new entry goes.
    end_tag: usize,
    entries: Vec<(usize, usize)>,
    empty: bool,
}

/// XML written with no prefix, as it must read in a part that has one.
fn with_prefix(xml: &[u8], name: &str) -> String {
    if name.is_empty() {
        return String::from_utf8_lossy(xml).into_owned();
    }
    let mut out = Vec::with_capacity(xml.len() + 16);
    let mut scanner = Scanner::new(xml);
    while let Ok(Some(kind)) = scanner.next() {
        let token = scanner.token();
        match kind {
            Kind::Start | Kind::Empty => {
                out.push(b'<');
                out.extend_from_slice(name.as_bytes());
                out.extend_from_slice(&token[1..]);
            }
            Kind::End => {
                out.extend_from_slice(b"</");
                out.extend_from_slice(name.as_bytes());
                out.extend_from_slice(&token[2..]);
            }
            _ => out.extend_from_slice(token),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Standard number formats end at 163, and files number their own from there.
const FIRST_CUSTOM_FORMAT: u32 = 164;

impl StylesXml {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            changed: false,
        }
    }

    fn find_list(&self, list: &[Located], name: &[u8], entry: &[u8]) -> Option<ListAt> {
        let at = list.iter().position(|token| {
            token.depth == 1
                && matches!(token.kind, Kind::Start | Kind::Empty)
                && local_name(&self.bytes[token.start..token.end]) == name
        })?;
        let start = &list[at];
        if start.kind == Kind::Empty {
            return Some(ListAt {
                tag: (start.start, start.end),
                end_tag: start.end,
                entries: Vec::new(),
                empty: true,
            });
        }
        let mut entries = Vec::new();
        let mut open: Option<usize> = None;
        for token in &list[at + 1..] {
            if token.kind == Kind::End && token.depth == 1 {
                return Some(ListAt {
                    tag: (start.start, start.end),
                    end_tag: token.start,
                    entries,
                    empty: false,
                });
            }
            if token.depth == 2 {
                let tag = &self.bytes[token.start..token.end];
                match token.kind {
                    Kind::Empty if local_name(tag) == entry => {
                        entries.push((token.start, token.end))
                    }
                    Kind::Start if local_name(tag) == entry => open = Some(token.start),
                    Kind::End => {
                        if let Some(from) = open.take() {
                            entries.push((from, token.end));
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    /// Adds an entry to a list, creating the list in its place if the file has none. Returns
    /// the new entry's index.
    fn append(&mut self, name: &str, entry: &str, xml: &str) -> u32 {
        self.changed = true;
        let list = tokens(&self.bytes);
        let root = root_prefix(&self.bytes, &list);
        if let Some(found) = self.find_list(&list, name.as_bytes(), entry.as_bytes()) {
            let index = found.entries.len() as u32;
            let count = (index + 1).to_string();
            let tag = &self.bytes[found.tag.0..found.tag.1];
            let edited = edit_tag(tag, &[("count", Some(&count))]);
            let changes = if found.empty {
                let mut replacement = opened(&edited);
                replacement.extend_from_slice(xml.as_bytes());
                replacement.extend_from_slice(format!("</{root}{name}>").as_bytes());
                vec![(found.tag.0, found.tag.1, replacement)]
            } else {
                vec![
                    (found.tag.0, found.tag.1, edited),
                    (found.end_tag, found.end_tag, xml.as_bytes().to_vec()),
                ]
            };
            self.bytes = splice(&self.bytes, &changes);
            return index;
        }
        let text = format!("<{root}{name} count=\"1\">{xml}</{root}{name}>");
        let wanted = order_in(STYLES_ORDER, name.as_bytes());
        for token in &list {
            if token.depth == 1 && matches!(token.kind, Kind::Start | Kind::Empty) {
                let order = order_in(
                    STYLES_ORDER,
                    local_name(&self.bytes[token.start..token.end]),
                );
                if order.is_some() && order > wanted {
                    let at = token.start;
                    self.bytes = splice(&self.bytes, &[(at, at, text.into_bytes())]);
                    return 0;
                }
            }
        }
        self.bytes = before_root_end(&self.bytes, text.as_bytes());
        0
    }

    fn root(&self) -> String {
        root_prefix(&self.bytes, &tokens(&self.bytes))
    }

    /// The XML of a list's entries, in order.
    fn entries(&self, list: &[u8], entry: &[u8]) -> Vec<Vec<u8>> {
        let located = tokens(&self.bytes);
        self.find_list(&located, list, entry)
            .map(|found| {
                found
                    .entries
                    .iter()
                    .map(|&(from, to)| self.bytes[from..to].to_vec())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Adds an entry, unless the list already holds exactly this one.
    fn append_new(&mut self, name: &str, entry: &str, xml: &str) -> u32 {
        let existing = self.entries(name.as_bytes(), entry.as_bytes());
        if let Some(at) = existing.iter().position(|old| old == xml.as_bytes()) {
            return at as u32;
        }
        self.append(name, entry, xml)
    }

    /// A font like the one at `base`, with these changes, reusing one that's the same. Everything
    /// else it holds stays, except that a new name drops what described the old one.
    pub fn edit_font(&mut self, base: u32, edits: &FontEdits) -> u32 {
        let prefix = self.root();
        let fonts = self.entries(b"fonts", b"font");
        let old = fonts.get(base as usize).or_else(|| fonts.first());
        let mut children: Vec<(String, String)> = Vec::new();
        if let Some(old) = old {
            let mut scanner = Scanner::new(&old[..]);
            let mut depth = 0;
            while let Ok(Some(kind)) = scanner.next() {
                let token = scanner.token();
                match kind {
                    Kind::Start => {
                        if depth == 1 {
                            // A child with content, which fonts never have in practice.
                            children.push((
                                String::from_utf8_lossy(local_name(token)).into_owned(),
                                String::from_utf8_lossy(&closed(token)).into_owned(),
                            ));
                        }
                        depth += 1;
                    }
                    Kind::End => depth -= 1,
                    Kind::Empty if depth == 1 => {
                        children.push((
                            String::from_utf8_lossy(local_name(token)).into_owned(),
                            String::from_utf8_lossy(token).into_owned(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        let mut put = |name: &str, xml: Option<String>| {
            children.retain(|(child, _)| child != name);
            if let Some(xml) = xml {
                children.push((name.to_string(), with_prefix(xml.as_bytes(), &prefix)));
            }
        };
        if let Some(on) = edits.bold {
            put("b", flag_child("b", on));
        }
        if let Some(on) = edits.italic {
            put("i", flag_child("i", on));
        }
        if let Some(on) = edits.strike {
            put("strike", flag_child("strike", on));
        }
        if let Some(underline) = &edits.underline {
            put(
                "u",
                underline.as_ref().map(|kind| {
                    if kind == "single" {
                        "<u/>".to_string()
                    } else {
                        format!("<u val=\"{kind}\"/>")
                    }
                }),
            );
        }
        if let Some(size) = edits.size {
            put("sz", Some(format!("<sz val=\"{}\"/>", point_text(size))));
        }
        if let Some(color) = &edits.color {
            put(
                "color",
                color
                    .as_ref()
                    .map(|attributes| format!("<color{attributes}/>")),
            );
        }
        if let Some(name) = &edits.name {
            put(
                "name",
                Some(format!("<name val=\"{}\"/>", escape_attr(name))),
            );
            put("family", None);
            put("charset", None);
            put("scheme", None);
        }
        let place = |name: &str| FONT_ORDER.iter().position(|known| *known == name);
        children.sort_by_key(|(name, _)| place(name).unwrap_or(FONT_ORDER.len()));
        let inner: String = children.into_iter().map(|(_, xml)| xml).collect();
        self.append_new(
            "fonts",
            "font",
            &format!("<{prefix}font>{inner}</{prefix}font>"),
        )
    }

    /// Adds a fill from its `patternFill`'s children, reusing one that's the same. Returns its index.
    pub fn add_pattern_fill(&mut self, pattern: &str, colors: &str) -> u32 {
        // Excel keeps the first two fills for itself, whatever they hold.
        let name = self.root();
        for _ in self.entry_count(b"fills", b"fill")..2 {
            let reserved = if self.entry_count(b"fills", b"fill") == 0 {
                "none"
            } else {
                "gray125"
            };
            self.append(
                "fills",
                "fill",
                &format!(
                    "<{name}fill><{name}patternFill patternType=\"{reserved}\"/></{name}fill>"
                ),
            );
        }
        let colors = with_prefix(colors.as_bytes(), &name);
        let xml = if colors.is_empty() {
            format!("<{name}fill><{name}patternFill patternType=\"{pattern}\"/></{name}fill>")
        } else {
            format!(
                "<{name}fill><{name}patternFill patternType=\"{pattern}\">{colors}</{name}patternFill></{name}fill>"
            )
        };
        self.append_new("fills", "fill", &xml)
    }

    /// A border like the one at `base`, with these changes, reusing one that's the same.
    pub fn edit_border(&mut self, base: u32, edits: &BorderEdits) -> u32 {
        let prefix = self.root();
        let borders = self.entries(b"borders", b"border");
        let old = borders.get(base as usize).or_else(|| borders.first());
        // The sides as they stand, by name, and the border's own start tag.
        let mut sides: Vec<(String, String)> = Vec::new();
        let mut open = format!("<{prefix}border>").into_bytes();
        if let Some(old) = old {
            let mut scanner = Scanner::new(&old[..]);
            let mut depth = 0;
            let mut current: Option<(String, Vec<u8>)> = None;
            while let Ok(Some(kind)) = scanner.next() {
                let token = scanner.token();
                match kind {
                    Kind::Start if depth == 0 => {
                        open = token.to_vec();
                        depth = 1;
                    }
                    Kind::Empty if depth == 0 => open = opened(token),
                    Kind::Start if depth == 1 => {
                        current = Some((
                            String::from_utf8_lossy(local_name(token)).into_owned(),
                            token.to_vec(),
                        ));
                        depth = 2;
                    }
                    Kind::Empty if depth == 1 => {
                        sides.push((
                            String::from_utf8_lossy(local_name(token)).into_owned(),
                            String::from_utf8_lossy(token).into_owned(),
                        ));
                    }
                    Kind::End if depth == 2 => {
                        depth = 1;
                        if let Some((name, mut bytes)) = current.take() {
                            bytes.extend_from_slice(token);
                            sides.push((name, String::from_utf8_lossy(&bytes).into_owned()));
                        }
                    }
                    Kind::End => depth -= 1,
                    _ => {
                        if let Some((_, bytes)) = current.as_mut() {
                            bytes.extend_from_slice(token);
                        }
                    }
                }
            }
        }
        // A file can say `start` and `end`, which are the same sides.
        for (name, xml) in &mut sides {
            let plain = match name.as_str() {
                "start" => "left",
                "end" => "right",
                _ => continue,
            };
            *xml = xml.replace(&format!("{prefix}{name}"), &format!("{prefix}{plain}"));
            *name = plain.to_string();
        }
        let mut set = |name: &str, line: &Option<Option<(String, String)>>| {
            let Some(line) = line else {
                return;
            };
            let xml = match line {
                Some((style, color)) if color.is_empty() => {
                    format!("<{prefix}{name} style=\"{style}\"/>")
                }
                Some((style, color)) => format!(
                    "<{prefix}{name} style=\"{style}\"><{prefix}color{color}/></{prefix}{name}>"
                ),
                None => format!("<{prefix}{name}/>"),
            };
            match sides.iter_mut().find(|(side, _)| side == name) {
                Some(side) => side.1 = xml,
                None => sides.push((name.to_string(), xml)),
            }
        };
        set("left", &edits.left);
        set("right", &edits.right);
        set("top", &edits.top);
        set("bottom", &edits.bottom);
        set("diagonal", &edits.diagonal);
        const ORDER: &[&str] = &[
            "left",
            "right",
            "top",
            "bottom",
            "diagonal",
            "vertical",
            "horizontal",
        ];
        for name in ["left", "right", "top", "bottom", "diagonal"] {
            if !sides.iter().any(|(side, _)| side == name) {
                sides.push((name.to_string(), format!("<{prefix}{name}/>")));
            }
        }
        let place = |name: &str| ORDER.iter().position(|known| *known == name);
        sides.sort_by_key(|(name, _)| place(name).unwrap_or(ORDER.len()));
        let on = |flag: bool| if flag { Some("1") } else { None };
        let mut changes: Vec<(&str, Option<&str>)> = Vec::new();
        if let Some(flag) = edits.diagonal_up {
            changes.push(("diagonalUp", on(flag)));
        }
        if let Some(flag) = edits.diagonal_down {
            changes.push(("diagonalDown", on(flag)));
        }
        let open = String::from_utf8_lossy(&edit_tag(&open, &changes)).into_owned();
        let inner: String = sides.into_iter().map(|(_, xml)| xml).collect();
        // The first border is the default one, and a file with none needs it.
        if borders.is_empty() {
            let blank = with_prefix(b"<left/><right/><top/><bottom/><diagonal/>", &prefix);
            self.append(
                "borders",
                "border",
                &format!("<{prefix}border>{blank}</{prefix}border>"),
            );
        }
        self.append_new(
            "borders",
            "border",
            &format!("{open}{inner}</{prefix}border>"),
        )
    }

    /// Adds a font, whose children are written without a prefix. Returns its index.
    pub fn add_font(&mut self, inner: &str) -> u32 {
        let name = self.root();
        let inner = with_prefix(inner.as_bytes(), &name);
        self.append(
            "fonts",
            "font",
            &format!("<{name}font>{inner}</{name}font>"),
        )
    }

    /// Adds a differential format, whose children are written without a prefix. Returns its index.
    pub fn add_dxf(&mut self, inner: &str) -> u32 {
        let name = self.root();
        let inner = with_prefix(inner.as_bytes(), &name);
        self.append("dxfs", "dxf", &format!("<{name}dxf>{inner}</{name}dxf>"))
    }

    /// Adds a solid fill of this `RRGGBB` colour. Returns its index.
    pub fn add_solid_fill(&mut self, rgb: &str) -> u32 {
        // Excel keeps the first two fills for itself, whatever they hold.
        let list = tokens(&self.bytes);
        let have = self
            .find_list(&list, b"fills", b"fill")
            .map_or(0, |found| found.entries.len());
        let name = self.root();
        for _ in have..2 {
            let pattern = if self.entry_count(b"fills", b"fill") == 0 {
                "none"
            } else {
                "gray125"
            };
            self.append(
                "fills",
                "fill",
                &format!("<{name}fill><{name}patternFill patternType=\"{pattern}\"/></{name}fill>"),
            );
        }
        self.append(
            "fills",
            "fill",
            &format!(
                "<{name}fill><{name}patternFill patternType=\"solid\"><{name}fgColor rgb=\"FF{rgb}\"/><{name}bgColor indexed=\"64\"/></{name}patternFill></{name}fill>"
            ),
        )
    }

    /// Adds a border from its sides' XML. Returns its index.
    pub fn add_border(&mut self, inner: &str) -> u32 {
        let name = self.root();
        // The first border is the default one, and a file with none needs it.
        if self.entry_count(b"borders", b"border") == 0 {
            let blank = with_prefix(b"<left/><right/><top/><bottom/><diagonal/>", &name);
            self.append(
                "borders",
                "border",
                &format!("<{name}border>{blank}</{name}border>"),
            );
        }
        let inner = with_prefix(inner.as_bytes(), &name);
        self.append(
            "borders",
            "border",
            &format!("<{name}border>{inner}</{name}border>"),
        )
    }

    fn entry_count(&self, list: &[u8], entry: &[u8]) -> usize {
        let located = tokens(&self.bytes);
        self.find_list(&located, list, entry)
            .map_or(0, |found| found.entries.len())
    }

    /// Adds a number format, and returns its id, reusing one the file already has for this code.
    pub fn add_num_fmt(&mut self, code: &str) -> u32 {
        let list = tokens(&self.bytes);
        let mut next = FIRST_CUSTOM_FORMAT;
        if let Some(found) = self.find_list(&list, b"numFmts", b"numFmt") {
            for &(from, to) in &found.entries {
                let tag = &self.bytes[from..to];
                let id = attr_value(tag, b"numFmtId")
                    .and_then(parse_u32)
                    .unwrap_or(0);
                let existing = attr_value(tag, b"formatCode")
                    .map(unescape_attr)
                    .unwrap_or_default();
                if existing == code {
                    return id;
                }
                next = next.max(id + 1);
            }
        }
        let name = self.root();
        self.append(
            "numFmts",
            "numFmt",
            &format!(
                "<{name}numFmt numFmtId=\"{next}\" formatCode=\"{}\"/>",
                escape_attr(code)
            ),
        );
        next
    }

    /// Adds a cell format copied from `base`, with these differences. Everything else it holds
    /// stays, including alignment, protection and what we don't know. Returns its index.
    pub fn add_xf(&mut self, base: u32, changes: XfChanges) -> u32 {
        let list = tokens(&self.bytes);
        let name = self.root();
        let found = self.find_list(&list, b"cellXfs", b"xf");
        let template = found
            .as_ref()
            .and_then(|found| {
                found
                    .entries
                    .get(base as usize)
                    .or_else(|| found.entries.first())
            })
            .map(|&(from, to)| self.bytes[from..to].to_vec())
            .unwrap_or_else(|| {
                format!("<{name}xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>")
                    .into_bytes()
            });
        let tag_end = template
            .iter()
            .position(|&byte| byte == b'>')
            .map_or(template.len(), |at| at + 1);
        let values: Vec<(&str, &str, String)> = [
            ("numFmtId", "applyNumberFormat", changes.num_fmt),
            ("fontId", "applyFont", changes.font),
            ("fillId", "applyFill", changes.fill),
            ("borderId", "applyBorder", changes.border),
        ]
        .into_iter()
        .filter_map(|(key, apply, value)| value.map(|value| (key, apply, value.to_string())))
        .collect();
        let mut edits: Vec<(&str, Option<&str>)> = Vec::new();
        for (key, apply, value) in &values {
            edits.push((*key, Some(value.as_str())));
            edits.push((*apply, Some("1")));
        }
        // A style list the file doesn't have can't be referred to, by this new format at least.
        let has_style_list = self
            .find_list(&list, b"cellStyleXfs", b"xf")
            .is_some_and(|found| {
                attr_value(&template[..tag_end], b"xfId")
                    .and_then(parse_u32)
                    .is_some_and(|index| (index as usize) < found.entries.len())
            });
        if !has_style_list {
            edits.push(("xfId", None));
        }
        if changes.alignment.is_some() || !changes.alignment_edits.is_empty() {
            edits.push(("applyAlignment", Some("1")));
        }
        let mut xml = edit_tag(&template[..tag_end], &edits);
        xml.extend_from_slice(&template[tag_end..]);
        let mut xml = String::from_utf8_lossy(&xml).into_owned();
        if let Some(alignment) = &changes.alignment {
            xml = with_alignment(&xml, &name, alignment);
        }
        if !changes.alignment_edits.is_empty() {
            let open = format!("<{name}alignment");
            let held = xml.find(&open).and_then(|start| {
                let tail = &xml[start..];
                tail.find('>').map(|end| tail[..=end].to_string())
            });
            let tag = held.unwrap_or_else(|| format!("{open}/>"));
            let list: Vec<(&str, Option<&str>)> = changes
                .alignment_edits
                .iter()
                .map(|(key, value)| (*key, value.as_deref()))
                .collect();
            let edited = edit_tag(tag.as_bytes(), &list);
            let edited = String::from_utf8_lossy(&edited).into_owned();
            // What's left after the name is what `with_alignment` writes.
            let attributes = edited
                .strip_prefix(&open)
                .map(|rest| {
                    rest.trim_end_matches('>')
                        .trim_end_matches('/')
                        .trim_end()
                        .to_string()
                })
                .unwrap_or_default();
            xml = with_alignment(&xml, &name, &attributes);
        }
        if let Some((locked, hidden)) = changes.protection {
            xml = with_protection(&xml, &name, locked, hidden);
        }
        self.append_new("cellXfs", "xf", &xml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn content_types_lose_and_gain_only_what_is_named() {
        let xml = br#"<?xml version="1.0"?><Types xmlns="t"><Default Extension="xml" ContentType="a"/><Override PartName="/xl/calcChain.xml" ContentType="b"/><Override PartName="/xl/workbook.xml" ContentType="c"/></Types>"#;
        let less = remove_overrides(xml, &["xl/calcchain.xml".to_string()]);
        assert_eq!(
            text(&less),
            r#"<?xml version="1.0"?><Types xmlns="t"><Default Extension="xml" ContentType="a"/><Override PartName="/xl/workbook.xml" ContentType="c"/></Types>"#
        );
        let more = add_override(&less, "xl/sharedStrings.xml", "s&t");
        assert!(text(&more).ends_with(
            r#"<Override PartName="/xl/sharedStrings.xml" ContentType="s&amp;t"/></Types>"#
        ));
    }

    #[test]
    fn relationships_get_an_id_nothing_uses() {
        let xml = br#"<Relationships xmlns="r"><Relationship Id="rId2" Type="x/calcChain" Target="c.xml"/><Relationship Id="rId1" Type="x/styles" Target="s.xml"/></Relationships>"#;
        let less = remove_relationships(xml, "/calcChain");
        assert_eq!(
            text(&less),
            r#"<Relationships xmlns="r"><Relationship Id="rId1" Type="x/styles" Target="s.xml"/></Relationships>"#
        );
        let more = add_relationship(xml, "x/sharedStrings", "t.xml");
        assert!(text(&more).contains(r#"Id="rId3" Type="x/sharedStrings""#));
    }

    #[test]
    fn calculation_on_load_keeps_what_calc_pr_held_and_finds_its_place() {
        let held = br#"<workbook xmlns="w"><sheets/><calcPr calcId="191029" calcMode="manual"/></workbook>"#;
        assert_eq!(
            text(&set_full_calc_on_load(held)),
            r#"<workbook xmlns="w"><sheets/><calcPr calcId="191029" calcMode="manual" fullCalcOnLoad="1"/></workbook>"#
        );
        let none = br#"<x:workbook xmlns:x="w"><x:sheets/><x:definedNames/><x:pivotCaches/><x:unknown/></x:workbook>"#;
        assert_eq!(
            text(&set_full_calc_on_load(none)),
            r#"<x:workbook xmlns:x="w"><x:sheets/><x:definedNames/><x:calcPr fullCalcOnLoad="1"/><x:pivotCaches/><x:unknown/></x:workbook>"#
        );
        let bare = br#"<workbook xmlns="w"><sheets/></workbook>"#;
        assert_eq!(
            text(&set_full_calc_on_load(bare)),
            r#"<workbook xmlns="w"><sheets/><calcPr fullCalcOnLoad="1"/></workbook>"#
        );
    }

    #[test]
    fn strings_go_on_the_end_and_counts_go_up() {
        let mut strings = StringTable::new(2);
        assert_eq!(strings.intern("new"), 2);
        assert_eq!(strings.intern(" spaced <&>"), 3);
        assert_eq!(strings.intern("new"), 2);
        let xml = r#"<sst xmlns="s" count="5" uniqueCount="2"><si><t>a</t></si><si><r><t>b</t></r></si></sst>"#;
        assert_eq!(count_strings(xml.as_bytes()).unwrap(), 2);
        let mut out = Vec::new();
        append_strings(xml.as_bytes(), &strings, false, &mut out).unwrap();
        assert_eq!(
            text(&out),
            r#"<sst xmlns="s" count="8" uniqueCount="4"><si><t>a</t></si><si><r><t>b</t></r></si><si><t>new</t></si><si><t xml:space="preserve"> spaced &lt;&amp;&gt;</t></si></sst>"#
        );
    }

    #[test]
    fn strings_can_go_in_an_empty_prefixed_part_and_outside_utf8() {
        let mut strings = StringTable::new(0);
        strings.intern("ن");
        let mut out = Vec::new();
        append_strings(&br#"<x:sst xmlns:x="s"/>"#[..], &strings, true, &mut out).unwrap();
        assert_eq!(
            text(&out),
            r#"<x:sst xmlns:x="s"><x:si><x:t>&#1606;</x:t></x:si></x:sst>"#
        );
    }

    const STYLES: &str = r#"<styleSheet xmlns="s"><numFmts count="1"><numFmt numFmtId="170" formatCode="0.0"/></numFmts><fonts count="1" x14ac:knownFonts="1"><font><sz val="11"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border/></borders><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" applyAlignment="1"><alignment horizontal="center"/><extLst/></xf></cellXfs><dxfs count="0"/></styleSheet>"#;

    #[test]
    fn styles_only_grow_and_a_new_format_keeps_what_its_base_held() {
        let mut styles = StylesXml::new(STYLES.as_bytes().to_vec());
        assert_eq!(styles.add_font("<b/><sz val=\"11\"/>"), 1);
        assert_eq!(styles.add_solid_fill("FFEE00"), 2);
        assert_eq!(styles.add_num_fmt("0.0"), 170);
        assert_eq!(styles.add_num_fmt("0.000"), 171);
        let xf = styles.add_xf(
            1,
            XfChanges {
                font: Some(1),
                fill: Some(2),
                num_fmt: Some(171),
                ..XfChanges::default()
            },
        );
        assert_eq!(xf, 2);
        let out = text(&styles.bytes);
        assert!(out.contains(r#"<fonts count="2" x14ac:knownFonts="1">"#));
        assert!(out.contains(r#"<fills count="3">"#));
        assert!(out.contains(r#"<numFmts count="2">"#));
        assert!(out.contains(r#"<cellXfs count="3">"#));
        assert!(out.contains(
            r#"<xf numFmtId="171" fontId="1" fillId="2" borderId="0" applyAlignment="1" applyNumberFormat="1" applyFont="1" applyFill="1"><alignment horizontal="center"/><extLst/></xf></cellXfs>"#
        ));
        // Everything there before is still there, in the same order.
        let table = crate::styles::read_styles(&styles.bytes);
        assert_eq!(table.xfs.len(), 3);
        assert!(table.fonts[1].bold);
    }

    #[test]
    fn a_font_edit_keeps_what_it_does_not_change_and_is_reused() {
        let xml = r#"<styleSheet><fonts count="1"><font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font></fonts><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs></styleSheet>"#;
        let mut styles = StylesXml::new(xml.as_bytes().to_vec());
        let bold = FontEdits {
            bold: Some(true),
            ..FontEdits::default()
        };
        assert_eq!(styles.edit_font(0, &bold), 1);
        let out = text(&styles.bytes);
        assert!(out.contains(r#"<font><b/><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font>"#), "{out}");
        assert!(out.contains(r#"<fonts count="2">"#), "{out}");
        // Back to the first, and the same again, adds nothing.
        let plain = FontEdits {
            bold: Some(false),
            ..FontEdits::default()
        };
        assert_eq!(styles.edit_font(1, &plain), 0);
        assert_eq!(styles.edit_font(0, &bold), 1);
        let arial = FontEdits {
            name: Some("Arial".to_string()),
            size: Some(10.5),
            underline: Some(Some("double".to_string())),
            color: Some(Some(r#" rgb="FFFF0000""#.to_string())),
            ..FontEdits::default()
        };
        assert_eq!(styles.edit_font(0, &arial), 2);
        let out = text(&styles.bytes);
        assert!(out.contains(r#"<font><u val="double"/><sz val="10.5"/><color rgb="FFFF0000"/><name val="Arial"/></font>"#), "{out}");
    }

    #[test]
    fn a_format_keeps_what_its_alignment_said_and_the_same_format_is_reused() {
        let xml = r#"<styleSheet><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment horizontal="left" justifyLastLine="1"/><protection locked="0"/></xf></cellXfs></styleSheet>"#;
        let mut styles = StylesXml::new(xml.as_bytes().to_vec());
        let edits = XfChanges {
            alignment_edits: vec![("wrapText", Some("1".to_string())), ("horizontal", None)],
            ..XfChanges::default()
        };
        assert_eq!(styles.add_xf(0, edits.clone()), 1);
        let out = text(&styles.bytes);
        assert!(
            out.contains(
                r#"<alignment justifyLastLine="1" wrapText="1"/><protection locked="0"/>"#
            ),
            "{out}"
        );
        assert_eq!(styles.add_xf(0, edits), 1);
        assert!(out.contains(r#"<cellXfs count="2">"#), "{out}");
    }

    #[test]
    fn a_border_edit_changes_only_its_sides() {
        let xml = r#"<styleSheet><borders count="1"><border><left style="thin"><color auto="1"/></left><right/><top/><bottom/><diagonal/></border></borders></styleSheet>"#;
        let mut styles = StylesXml::new(xml.as_bytes().to_vec());
        let edits = BorderEdits {
            bottom: Some(Some((
                "double".to_string(),
                r#" rgb="FF00FF00""#.to_string(),
            ))),
            right: Some(Some(("thin".to_string(), String::new()))),
            diagonal_up: Some(true),
            ..BorderEdits::default()
        };
        assert_eq!(styles.edit_border(0, &edits), 1);
        let out = text(&styles.bytes);
        assert!(out.contains(r#"<border diagonalUp="1"><left style="thin"><color auto="1"/></left><right style="thin"/><top/><bottom style="double"><color rgb="FF00FF00"/></bottom><diagonal/></border>"#), "{out}");
        let clear = BorderEdits {
            left: Some(None),
            ..BorderEdits::default()
        };
        assert_eq!(styles.edit_border(0, &clear), 2);
        assert!(text(&styles.bytes)
            .contains(r#"<border><left/><right/><top/><bottom/><diagonal/></border>"#));
    }

    #[test]
    fn a_pattern_fill_leaves_the_first_two_alone_and_is_reused() {
        let mut styles = StylesXml::new(b"<styleSheet/>".to_vec());
        let colors = r#"<fgColor theme="4"/><bgColor rgb="FFFF0000"/>"#;
        assert_eq!(styles.add_pattern_fill("darkUp", colors), 2);
        assert_eq!(styles.add_pattern_fill("darkUp", colors), 2);
        assert_eq!(styles.add_pattern_fill("none", ""), 0);
    }

    #[test]
    fn a_prefixed_style_sheet_gets_prefixed_fonts() {
        let mut styles = StylesXml::new(
            br#"<x:styleSheet xmlns:x="s"><x:fonts count="1"><x:font/></x:fonts></x:styleSheet>"#
                .to_vec(),
        );
        assert_eq!(styles.add_font(r#"<b/><name val="Calibri"></name>"#), 1);
        assert_eq!(
            text(&styles.bytes),
            r#"<x:styleSheet xmlns:x="s"><x:fonts count="2"><x:font/><x:font><x:b/><x:name val="Calibri"></x:name></x:font></x:fonts></x:styleSheet>"#
        );
    }

    #[test]
    fn a_new_format_does_not_copy_a_reference_to_a_style_that_is_not_there() {
        let mut styles = StylesXml::new(
            br#"<styleSheet xmlns="s"><cellXfs count="1"><xf fontId="0" xfId="0"/></cellXfs></styleSheet>"#
                .to_vec(),
        );
        styles.add_xf(0, XfChanges::default());
        assert!(text(&styles.bytes).ends_with(r#"<xf fontId="0"/></cellXfs></styleSheet>"#));
        let mut with_list = StylesXml::new(
            br#"<styleSheet xmlns="s"><cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="1"><xf fontId="0" xfId="0"/></cellXfs></styleSheet>"#
                .to_vec(),
        );
        with_list.add_xf(0, XfChanges::default());
        assert!(
            text(&with_list.bytes).ends_with(r#"<xf fontId="0" xfId="0"/></cellXfs></styleSheet>"#)
        );
    }

    #[test]
    fn a_new_part_replaces_a_content_type_that_named_it_with_no_part() {
        let xml =
            br#"<Types xmlns="t"><Override PartName="/xl/styles.xml" ContentType="old"/></Types>"#;
        assert_eq!(
            text(&add_override(xml, "xl/styles.xml", "new")),
            r#"<Types xmlns="t"><Override PartName="/xl/styles.xml" ContentType="new"/></Types>"#
        );
    }

    #[test]
    fn styles_with_no_lists_get_them_in_the_schemas_order() {
        let mut styles =
            StylesXml::new(br#"<styleSheet xmlns="s"><dxfs count="0"/></styleSheet>"#.to_vec());
        assert_eq!(styles.add_xf(0, XfChanges::default()), 0);
        assert_eq!(styles.add_font("<b/>"), 0);
        assert_eq!(
            text(&styles.bytes),
            r#"<styleSheet xmlns="s"><fonts count="1"><font><b/></font></fonts><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellXfs><dxfs count="0"/></styleSheet>"#
        );
    }

    #[test]
    fn table_columns_rename_by_position_and_keep_the_rest() {
        let xml = br#"<table ref="A1:B3" displayName="T"><tableColumns count="2"><tableColumn id="1" name="One"/><tableColumn id="2" name="Two" dataDxfId="3"/></tableColumns></table>"#;
        let part = read_table_part(xml).unwrap();
        assert_eq!(part.columns, ["One", "Two"]);
        assert_eq!(part.header_rows, 1);
        assert_eq!(
            text(&set_table_columns(xml, &[(1, "New\nname".to_string())])),
            r#"<table ref="A1:B3" displayName="T"><tableColumns count="2"><tableColumn id="1" name="One"/><tableColumn id="2" name="New_x000a_name" dataDxfId="3"/></tableColumns></table>"#
        );
        assert_eq!(
            read_table_part(&set_table_columns(xml, &[(0, "A\nB".to_string())]))
                .unwrap()
                .columns[0],
            "A\nB"
        );
    }

    #[test]
    fn a_table_formula_follows_a_renamed_column() {
        let xml = br#"<table name="Sales" displayName="Sales"><tableColumns count="2"><tableColumn id="1" name="Amount"/><tableColumn id="2" name="Twice"><calculatedColumnFormula>Sales[[#This Row],[Amount]]*2&amp;"x"</calculatedColumnFormula></tableColumn></tableColumns></table>"#;
        let renames = vec![("Amount".to_string(), "Takings".to_string())];
        let out = text(&rename_in_table_formulas(xml, "Sales", &renames));
        assert!(out.contains(r#"<calculatedColumnFormula>Sales[[#This Row],[Takings]]*2&amp;"x"</calculatedColumnFormula>"#), "{out}");
    }

    #[test]
    fn a_pivot_cache_learns_to_refresh() {
        let xml = br#"<pivotCacheDefinition xmlns="p" r:id="rId1" refreshedBy="me"><cacheSource/></pivotCacheDefinition>"#;
        assert_eq!(
            text(&set_refresh_on_load(xml)),
            r#"<pivotCacheDefinition xmlns="p" r:id="rId1" refreshedBy="me" refreshOnLoad="1"><cacheSource/></pivotCacheDefinition>"#
        );
    }
}
