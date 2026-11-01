//! A neutral xlsx reader for the bake-off: both engines load through the same cells,
//! so neither engine's own importer decides the result. It keeps what the formula
//! engine needs, plus the provenance needed to judge whether stored values are Excel's.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use zip::ZipArchive;

use crate::formula::{parse_a1, parse_range, shift};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Empty,
    Number(f64),
    Bool(bool),
    Text(String),
    /// As stored, such as `#N/A`.
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormulaKind {
    Normal,
    /// A legacy `{=...}` array over this range, which is ((top, left), (bottom, right)).
    Array(((u32, u32), (u32, u32))),
    /// A dynamic array whose last spill covered this range.
    Dynamic(((u32, u32), (u32, u32))),
    /// `{=TABLE(...)}` what-if cells, which neither engine computes.
    DataTable,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Value(Value),
    Formula {
        text: String,
        kind: FormulaKind,
        /// The stored `<v>`; `None` when the file has none.
        cached: Option<Value>,
    },
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub row: u32,
    pub col: u32,
    pub content: Content,
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub name: String,
    pub cells: Vec<Cell>,
    /// Stored values of the non-anchor cells of arrays, by (row, column). They are
    /// not loaded as constants, because the array formula produces them.
    pub spill_values: HashMap<(u32, u32), Value>,
    pub hidden_rows: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct DefinedName {
    pub name: String,
    pub formula: String,
    pub local_sheet: Option<u32>,
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub struct TableDef {
    pub name: String,
    pub display_name: String,
    pub sheet: u32,
    pub reference: String,
    pub header_rows: u32,
    pub totals_rows: u32,
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Book {
    /// `Application` and `AppVersion` from `docProps/app.xml`.
    pub application: String,
    pub app_version: String,
    /// `appName` and `rupBuild` of `fileVersion`.
    pub file_app: String,
    pub rup_build: String,
    pub calc_id: String,
    pub calc_mode: String,
    pub full_calc_on_load: bool,
    pub iterate: bool,
    pub date1904: bool,
    pub has_external_links: bool,
    pub has_excel_only_markup: bool,
    pub sheets: Vec<Sheet>,
    pub names: Vec<DefinedName>,
    pub tables: Vec<TableDef>,
}

impl Book {
    pub fn formula_cells(&self) -> usize {
        self.sheets
            .iter()
            .flat_map(|sheet| &sheet.cells)
            .filter(|cell| matches!(cell.content, Content::Formula { .. }))
            .count()
    }
}

type Err = String;

fn attr(element: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    for attribute in element.attributes().flatten() {
        if attribute.key.local_name().as_ref() == name {
            let raw = String::from_utf8_lossy(&attribute.value);
            return Some(
                quick_xml::escape::unescape(&raw)
                    .map_or_else(|_| raw.to_string(), std::borrow::Cow::into_owned),
            );
        }
    }
    None
}

fn read_part(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    // Some writers use a different case or a leading slash in the zip entry name.
    let wanted = name.trim_start_matches('/').to_ascii_lowercase();
    let index = (0..archive.len()).find(|&index| {
        archive.name_for_index(index).is_some_and(|entry| {
            entry.is_ok_and(|entry| entry.trim_start_matches('/').eq_ignore_ascii_case(&wanted))
        })
    })?;
    let mut file = archive.by_index(index).ok()?;
    let mut out = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut out).ok()?;
    Some(out)
}

/// The text of an element, including entity references, up to its end tag.
fn push_text(event: &Event<'_>, out: &mut String) {
    match event {
        Event::Text(text) => {
            if let Ok(decoded) = text.decode() {
                out.push_str(&decoded);
            }
        }
        Event::CData(text) => {
            if let Ok(decoded) = text.decode() {
                out.push_str(&decoded);
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
fn unescape_x(text: String) -> String {
    if !text.contains("_x") {
        return text;
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
    out
}

fn read_shared_strings(xml: &[u8]) -> Vec<String> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    let mut in_phonetic = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element)) => match element.local_name().as_ref() {
                b"si" => current.clear(),
                b"rPh" => in_phonetic = true,
                b"t" => in_text = !in_phonetic,
                _ => {}
            },
            Ok(Event::Empty(element)) if element.local_name().as_ref() == b"si" => {
                out.push(String::new());
            }
            Ok(Event::End(element)) => match element.local_name().as_ref() {
                b"si" => out.push(unescape_x(std::mem::take(&mut current))),
                b"rPh" => in_phonetic = false,
                b"t" => in_text = false,
                _ => {}
            },
            Ok(event) => {
                if in_text {
                    push_text(&event, &mut current);
                }
            }
        }
        buf.clear();
    }
    out
}

fn read_relationships(xml: &[u8]) -> HashMap<String, (String, String)> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut out = HashMap::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"Relationship" =>
            {
                if let (Some(id), Some(kind), Some(target)) = (
                    attr(&element, b"Id"),
                    attr(&element, b"Type"),
                    attr(&element, b"Target"),
                ) {
                    out.insert(id, (kind, target));
                }
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

fn resolve_target(base: &str, target: &str) -> String {
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

fn parse_stored(kind: &str, text: &str, strings: &[String]) -> Value {
    match kind {
        "s" => text
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|index| strings.get(index).cloned())
            .map_or(Value::Empty, Value::Text),
        "str" | "inlineStr" => Value::Text(unescape_x(text.to_string())),
        "b" => Value::Bool(matches!(text.trim(), "1" | "true" | "TRUE")),
        "e" => Value::Error(text.trim().to_string()),
        _ => text
            .trim()
            .parse::<f64>()
            .map_or(Value::Empty, Value::Number),
    }
}

/// A `<f t="shared" si="n"/>` child waiting for its master's text.
struct PendingShared {
    cell: usize,
    index: String,
}

fn read_sheet(name: String, xml: &[u8], strings: &[String]) -> Sheet {
    let mut sheet = Sheet {
        name,
        ..Sheet::default()
    };
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();

    let mut row = 0u32;
    let mut col = 0u32;
    let mut cell_kind = String::new();
    let mut cell_dynamic = false;
    let mut value_text = String::new();
    let mut has_value = false;
    let mut formula_text = String::new();
    let mut formula_kind = String::new();
    let mut formula_ref = String::new();
    let mut formula_si = String::new();
    let mut has_formula = false;
    let mut inline_text = String::new();
    let mut has_inline = false;
    let mut in_value = false;
    let mut in_formula = false;
    let mut in_inline_t = false;
    let mut in_cell = false;

    // Masters: si -> (text, row, col). Children can come before their master.
    let mut masters: HashMap<String, (String, u32, u32)> = HashMap::new();
    let mut pending: Vec<PendingShared> = Vec::new();
    let mut array_ranges: Vec<((u32, u32), (u32, u32))> = Vec::new();

    macro_rules! finish_cell {
        () => {{
            let stored = if has_inline {
                Some(Value::Text(unescape_x(std::mem::take(&mut inline_text))))
            } else if has_value {
                Some(parse_stored(&cell_kind, &value_text, strings))
            } else if cell_kind == "str" {
                // Excel stores an empty string result as `t="str"` with no text.
                Some(Value::Text(String::new()))
            } else {
                None
            };
            if has_formula {
                let range = parse_range(&formula_ref);
                let (kind, text) = match formula_kind.as_str() {
                    "array" => match range {
                        Some(range) => {
                            array_ranges.push(range);
                            (
                                if cell_dynamic {
                                    FormulaKind::Dynamic(range)
                                } else {
                                    FormulaKind::Array(range)
                                },
                                std::mem::take(&mut formula_text),
                            )
                        }
                        None => (FormulaKind::Normal, std::mem::take(&mut formula_text)),
                    },
                    "dataTable" => (FormulaKind::DataTable, String::new()),
                    "shared" => {
                        if !formula_text.is_empty() {
                            masters.insert(
                                formula_si.clone(),
                                (std::mem::take(&mut formula_text), row, col),
                            );
                            let master = &masters[&formula_si];
                            (FormulaKind::Normal, master.0.clone())
                        } else {
                            pending.push(PendingShared {
                                cell: sheet.cells.len(),
                                index: formula_si.clone(),
                            });
                            (FormulaKind::Normal, String::new())
                        }
                    }
                    _ => (FormulaKind::Normal, std::mem::take(&mut formula_text)),
                };
                sheet.cells.push(Cell {
                    row,
                    col,
                    content: Content::Formula {
                        text,
                        kind,
                        cached: stored,
                    },
                });
            } else if let Some(value) = stored {
                if value != Value::Empty {
                    sheet.cells.push(Cell {
                        row,
                        col,
                        content: Content::Value(value),
                    });
                }
            }
        }};
    }

    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(ref element)) => match element.local_name().as_ref() {
                b"row" => {
                    row = attr(element, b"r")
                        .and_then(|r| r.parse().ok())
                        .unwrap_or(row + 1);
                    col = 0;
                    if matches!(attr(element, b"hidden").as_deref(), Some("1" | "true")) {
                        sheet.hidden_rows.push(row);
                    }
                }
                b"c" => {
                    in_cell = true;
                    col =
                        attr(element, b"r")
                            .and_then(|r| parse_a1(&r))
                            .map_or(col + 1, |(r, c)| {
                                row = r;
                                c
                            });
                    cell_kind = attr(element, b"t").unwrap_or_default();
                    cell_dynamic = attr(element, b"cm").is_some();
                    value_text.clear();
                    has_value = false;
                    has_formula = false;
                    has_inline = false;
                    inline_text.clear();
                    formula_text.clear();
                    formula_kind.clear();
                    formula_ref.clear();
                    formula_si.clear();
                }
                b"f" if in_cell => {
                    has_formula = true;
                    in_formula = true;
                    formula_kind = attr(element, b"t").unwrap_or_default();
                    formula_ref = attr(element, b"ref").unwrap_or_default();
                    formula_si = attr(element, b"si").unwrap_or_default();
                }
                b"v" if in_cell => {
                    in_value = true;
                    has_value = true;
                }
                b"is" if in_cell => has_inline = true,
                b"t" if has_inline => in_inline_t = true,
                _ => {}
            },
            Ok(Event::Empty(ref element)) => match element.local_name().as_ref() {
                b"row" => {
                    row = attr(element, b"r")
                        .and_then(|r| r.parse().ok())
                        .unwrap_or(row + 1);
                    if matches!(attr(element, b"hidden").as_deref(), Some("1" | "true")) {
                        sheet.hidden_rows.push(row);
                    }
                }
                b"c" => {
                    // A styled blank cell.
                    col =
                        attr(element, b"r")
                            .and_then(|r| parse_a1(&r))
                            .map_or(col + 1, |(r, c)| {
                                row = r;
                                c
                            });
                }
                b"f" if in_cell => {
                    has_formula = true;
                    formula_kind = attr(element, b"t").unwrap_or_default();
                    formula_ref = attr(element, b"ref").unwrap_or_default();
                    formula_si = attr(element, b"si").unwrap_or_default();
                }
                _ => {}
            },
            Ok(Event::End(ref element)) => match element.local_name().as_ref() {
                b"c" => {
                    finish_cell!();
                    in_cell = false;
                }
                b"f" => in_formula = false,
                b"v" => in_value = false,
                b"t" => in_inline_t = false,
                _ => {}
            },
            Ok(ref other) => {
                if in_formula {
                    push_text(other, &mut formula_text);
                } else if in_value {
                    push_text(other, &mut value_text);
                } else if in_inline_t {
                    push_text(other, &mut inline_text);
                }
            }
        }
        buf.clear();
    }

    for child in pending {
        let cell = &mut sheet.cells[child.cell];
        let (row, col) = (cell.row, cell.col);
        if let Content::Formula { text, .. } = &mut cell.content {
            if let Some((master, master_row, master_col)) = masters.get(&child.index) {
                *text = shift(
                    master,
                    i64::from(row) - i64::from(*master_row),
                    i64::from(col) - i64::from(*master_col),
                );
            }
        }
    }

    // The other cells of an array are the array's output, not constants.
    if !array_ranges.is_empty() {
        let in_array = |row: u32, col: u32| {
            array_ranges.iter().any(|&((top, left), (bottom, right))| {
                row >= top && row <= bottom && col >= left && col <= right
            })
        };
        let mut kept = Vec::with_capacity(sheet.cells.len());
        for cell in std::mem::take(&mut sheet.cells) {
            let spilled = match &cell.content {
                Content::Value(_) => in_array(cell.row, cell.col),
                Content::Formula { .. } => false,
            };
            if spilled {
                if let Content::Value(value) = cell.content {
                    sheet.spill_values.insert((cell.row, cell.col), value);
                }
            } else {
                kept.push(cell);
            }
        }
        sheet.cells = kept;
    }
    sheet
}

fn read_table(xml: &[u8], sheet: u32) -> Option<TableDef> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut table: Option<TableDef> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element) | Event::Empty(element)) => {
                match element.local_name().as_ref() {
                    // Only the root: an extension can hold an <x14:table> too.
                    b"table" if table.is_none() => {
                        table = Some(TableDef {
                            name: attr(&element, b"name").unwrap_or_default(),
                            display_name: attr(&element, b"displayName").unwrap_or_default(),
                            sheet,
                            reference: attr(&element, b"ref").unwrap_or_default(),
                            header_rows: attr(&element, b"headerRowCount")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(1),
                            totals_rows: attr(&element, b"totalsRowCount")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(0),
                            columns: Vec::new(),
                        });
                    }
                    b"tableColumn" => {
                        if let (Some(table), Some(name)) = (table.as_mut(), attr(&element, b"name"))
                        {
                            table.columns.push(unescape_x(name));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    table
}

pub fn read_book(bytes: &[u8]) -> Result<Book, Err> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|error| format!("zip: {error}"))?;
    let mut book = Book::default();

    if let Some(xml) = read_part(&mut archive, "docProps/app.xml") {
        let mut reader = Reader::from_reader(xml.as_slice());
        let mut buf = Vec::new();
        let mut current = String::new();
        let mut field = 0;
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Eof) | Err(_) => break,
                Ok(Event::Start(element)) => {
                    field = match element.local_name().as_ref() {
                        b"Application" => 1,
                        b"AppVersion" => 2,
                        _ => 0,
                    };
                    current.clear();
                }
                Ok(Event::End(_)) => {
                    match field {
                        1 => book.application = current.trim().to_string(),
                        2 => book.app_version = current.trim().to_string(),
                        _ => {}
                    }
                    field = 0;
                }
                Ok(event) => {
                    if field != 0 {
                        push_text(&event, &mut current);
                    }
                }
            }
            buf.clear();
        }
    }

    let workbook_xml =
        read_part(&mut archive, "xl/workbook.xml").ok_or("no xl/workbook.xml".to_string())?;
    let workbook_rels = read_part(&mut archive, "xl/_rels/workbook.xml.rels")
        .map(|xml| read_relationships(&xml))
        .unwrap_or_default();
    let strings = read_part(&mut archive, "xl/sharedStrings.xml")
        .map(|xml| read_shared_strings(&xml))
        .unwrap_or_default();

    let workbook_text = String::from_utf8_lossy(&workbook_xml);
    book.has_excel_only_markup =
        workbook_text.contains("xr:revisionPtr") || workbook_text.contains("x15ac:absPath");

    let mut sheet_rows: Vec<(String, String)> = Vec::new();
    {
        let mut reader = Reader::from_reader(workbook_xml.as_slice());
        let mut buf = Vec::new();
        let mut name: Option<DefinedName> = None;
        let mut current = String::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Eof) | Err(_) => break,
                Ok(Event::Start(ref element) | Event::Empty(ref element)) => {
                    match element.local_name().as_ref() {
                        b"sheet" => {
                            if let (Some(sheet_name), Some(id)) =
                                (attr(element, b"name"), attr(element, b"id"))
                            {
                                sheet_rows.push((sheet_name, id));
                            }
                        }
                        b"fileVersion" => {
                            book.file_app = attr(element, b"appName").unwrap_or_default();
                            book.rup_build = attr(element, b"rupBuild").unwrap_or_default();
                        }
                        b"workbookPr" => {
                            book.date1904 =
                                matches!(attr(element, b"date1904").as_deref(), Some("1" | "true"));
                        }
                        b"calcPr" => {
                            book.calc_id = attr(element, b"calcId").unwrap_or_default();
                            book.calc_mode = attr(element, b"calcMode").unwrap_or_default();
                            book.full_calc_on_load = matches!(
                                attr(element, b"fullCalcOnLoad").as_deref(),
                                Some("1" | "true")
                            );
                            book.iterate =
                                matches!(attr(element, b"iterate").as_deref(), Some("1" | "true"));
                        }
                        b"externalReference" => book.has_external_links = true,
                        b"definedName" => {
                            name = Some(DefinedName {
                                name: attr(element, b"name").unwrap_or_default(),
                                formula: String::new(),
                                local_sheet: attr(element, b"localSheetId")
                                    .and_then(|id| id.parse().ok()),
                                hidden: matches!(
                                    attr(element, b"hidden").as_deref(),
                                    Some("1" | "true")
                                ),
                            });
                            current.clear();
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(ref element)) if element.local_name().as_ref() == b"definedName" => {
                    if let Some(mut done) = name.take() {
                        done.formula = std::mem::take(&mut current);
                        book.names.push(done);
                    }
                }
                Ok(ref event) => {
                    if name.is_some() {
                        push_text(event, &mut current);
                    }
                }
            }
            buf.clear();
        }
    }

    for (index, (sheet_name, id)) in sheet_rows.into_iter().enumerate() {
        let Some((kind, target)) = workbook_rels.get(&id) else {
            continue;
        };
        if !kind.ends_with("/worksheet") {
            // A chart sheet or a macro sheet has no cells to load, but keeps its index.
            book.sheets.push(Sheet {
                name: sheet_name,
                ..Sheet::default()
            });
            continue;
        }
        let path = resolve_target("xl/workbook.xml", target);
        let xml = read_part(&mut archive, &path).ok_or(format!("missing part {path}"))?;
        book.sheets.push(read_sheet(sheet_name, &xml, &strings));

        let rels_path = match path.rsplit_once('/') {
            Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
            None => format!("_rels/{path}.rels"),
        };
        if let Some(rels) = read_part(&mut archive, &rels_path) {
            for (rel_kind, rel_target) in read_relationships(&rels).values() {
                if rel_kind.ends_with("/table") {
                    let table_path = resolve_target(&path, rel_target);
                    if let Some(table_xml) = read_part(&mut archive, &table_path) {
                        if let Some(table) = read_table(&table_xml, index as u32) {
                            book.tables.push(table);
                        }
                    }
                }
            }
        }
    }
    Ok(book)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHEET: &[u8] = br#"<worksheet><sheetData>
      <row r="1"><c r="A1"><v>1</v></c><c r="B1" t="s"><v>0</v></c>
        <c r="C1"><f t="shared" ref="C1:C3" si="0">A1*2</f><v>2</v></c></row>
      <row r="2"><c r="C2"><f t="shared" si="0"/><v>4</v></c></row>
      <row r="3"><c r="C3" t="str"><f t="shared" si="0"/><v>x</v></c>
        <c r="D3" t="e"><f>1/0</f><v>#DIV/0!</v></c>
        <c r="E3" t="b"><f>TRUE</f><v>1</v></c>
        <c r="F3" t="inlineStr"><is><t>inline &amp; text</t></is></c></row>
      <row r="4"><c r="A4"><f t="array" ref="A4:A5">ROW(A1:A2)</f><v>1</v></c></row>
      <row r="5"><c r="A5"><v>2</v></c><c r="B5" s="3"/><c r="C5"><f>A1</f></c></row>
    </sheetData></worksheet>"#;

    fn formula_at(sheet: &Sheet, row: u32, col: u32) -> (String, Option<Value>) {
        let cell = sheet
            .cells
            .iter()
            .find(|cell| cell.row == row && cell.col == col)
            .unwrap();
        match &cell.content {
            Content::Formula { text, cached, .. } => (text.clone(), cached.clone()),
            other => panic!("not a formula: {other:?}"),
        }
    }

    #[test]
    fn reads_cells_and_expands_shared_formulas() {
        let sheet = read_sheet("S".into(), SHEET, &["hello".to_string()]);
        assert_eq!(formula_at(&sheet, 2, 3).0, "A2*2");
        assert_eq!(formula_at(&sheet, 3, 3).1, Some(Value::Text("x".into())));
        assert_eq!(
            formula_at(&sheet, 3, 4).1,
            Some(Value::Error("#DIV/0!".into()))
        );
        assert_eq!(formula_at(&sheet, 3, 5).1, Some(Value::Bool(true)));
        assert_eq!(formula_at(&sheet, 5, 3).1, None);
        let inline = sheet.cells.iter().find(|c| (c.row, c.col) == (3, 6));
        assert_eq!(
            inline.unwrap().content,
            Content::Value(Value::Text("inline & text".into()))
        );
    }

    #[test]
    fn array_output_is_not_a_constant() {
        let sheet = read_sheet("S".into(), SHEET, &[]);
        assert!(sheet.spill_values.contains_key(&(5, 1)));
        assert!(!sheet.cells.iter().any(|c| (c.row, c.col) == (5, 1)));
    }

    #[test]
    fn unescapes_control_characters() {
        assert_eq!(unescape_x("a_x000D_b".into()), "a\rb");
        assert_eq!(unescape_x("a_xZZZZ_b".into()), "a_xZZZZ_b");
    }
}
