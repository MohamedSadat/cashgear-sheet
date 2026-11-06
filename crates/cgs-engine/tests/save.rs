//! Saving through the engine: edits go in through the Rust API, as the phase's plan says, and the
//! result is read back by both our reader and the zip crate.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::rc::Rc;

use cgs_calc::CalcValue;
use cgs_engine::{EditCell, EditInput, Engine, NewStyle};
use cgs_xlsx::patch_sheet::NewElement;
use cgs_xlsx::save::{Kept, SaveError};
use cgs_xlsx::ByteSource;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn zip_of(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut out);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    out.into_inner()
}

/// Two sheets and a macro, a chart, a calculation chain, a table and shared strings.
fn rich_book() -> Vec<u8> {
    let types = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="bin" ContentType="application/vnd.ms-office.vbaProject"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.ms-excel.sheet.macroEnabled.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="w"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="w"/><Override PartName="/xl/styles.xml" ContentType="s"/><Override PartName="/xl/sharedStrings.xml" ContentType="t"/><Override PartName="/xl/calcChain.xml" ContentType="c"/><Override PartName="/xl/tables/table1.xml" ContentType="tb"/></Types>"#;
    let root = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{RELS}"><sheets><sheet name="One" sheetId="1" r:id="rId1"/><sheet name="Two" sheetId="2" r:id="rId2"/></sheets><calcPr calcId="191029"/></workbook>"#
    );
    let workbook_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{RELS}/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="{RELS}/styles" Target="styles.xml"/><Relationship Id="rId4" Type="{RELS}/sharedStrings" Target="sharedStrings.xml"/><Relationship Id="rId5" Type="{RELS}/calcChain" Target="calcChain.xml"/></Relationships>"#
    );
    let sheet1 = format!(
        r#"<worksheet xmlns="{MAIN}"><dimension ref="A1:B3"/><sheetData><row r="1"><c r="A1"><v>2</v></c><c r="B1"><f>A1*10</f><v>20</v></c></row><row r="2"><c r="A2" t="s"><v>0</v></c><c r="B2" t="s"><v>1</v></c></row><row r="3"><c r="A3"><v>1</v></c><c r="B3"><v>2</v></c></row></sheetData><tableParts count="1"><tablePart r:id="rId1" xmlns:r="{RELS}"/></tableParts></worksheet>"#
    );
    let sheet1_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/table" Target="../tables/table1.xml"/></Relationships>"#
    );
    let sheet2 = format!(
        r#"<worksheet xmlns="{MAIN}"><sheetData><row r="1"><c r="A1"><f>1+1</f><v>2</v></c></row></sheetData></worksheet>"#
    );
    let table = format!(
        r#"<table xmlns="{MAIN}" id="1" name="T" displayName="T" ref="A2:B3"><autoFilter ref="A2:B3"/><tableColumns count="2"><tableColumn id="1" name="Head"/><tableColumn id="2" name="Foot"/></tableColumns></table>"#
    );
    let styles = format!(
        r#"<styleSheet xmlns="{MAIN}"><fonts count="1"><font><sz val="11"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border/></borders><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellXfs></styleSheet>"#
    );
    let strings = format!(
        r#"<sst xmlns="{MAIN}" count="2" uniqueCount="2"><si><t>Head</t></si><si><t>Foot</t></si></sst>"#
    );
    let chain =
        format!(r#"<calcChain xmlns="{MAIN}"><c r="B1" i="1"/><c r="A1" i="2"/></calcChain>"#);
    zip_of(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", root.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet1.as_bytes()),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            sheet1_rels.as_bytes(),
        ),
        ("xl/worksheets/sheet2.xml", sheet2.as_bytes()),
        ("xl/tables/table1.xml", table.as_bytes()),
        ("xl/styles.xml", styles.as_bytes()),
        ("xl/sharedStrings.xml", strings.as_bytes()),
        ("xl/calcChain.xml", chain.as_bytes()),
        ("xl/vbaProject.bin", &[0xD0, 0xCF, 0x11, 0xE0, 1, 2, 3, 255]),
        (
            "xl/charts/chart1.xml",
            b"<c:chartSpace xmlns:c=\"c\"><unknown/></c:chartSpace>",
        ),
        ("customXml/item1.xml", b"<mine/>"),
    ])
}

fn opened(bytes: Vec<u8>) -> Engine {
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).unwrap();
    loop {
        let status = engine.load_step(1000.0).unwrap();
        if status.phase != cgs_engine::LoadPhase::Reading {
            break;
        }
    }
    engine
}

fn value(sheet: u32, row: u32, col: u32, value: CalcValue) -> EditCell {
    EditCell {
        sheet,
        row,
        col,
        input: Some(EditInput::Value(value)),
        style: None,
    }
}

fn parts(bytes: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..archive.len())
        .map(|index| {
            let mut file = archive.by_index(index).unwrap();
            let mut body = Vec::new();
            file.read_to_end(&mut body).unwrap();
            (file.name().unwrap().to_string(), body)
        })
        .collect()
}

fn text(parts: &HashMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(parts[name].clone()).unwrap()
}

/// Saves, checks, and returns the bytes of the file along with the engine still open on it.
fn saved(engine: &mut Engine) -> (Vec<u8>, cgs_xlsx::save::SaveReport) {
    let mut out = Vec::new();
    let report = engine.save(&mut out).unwrap();
    engine.verify_saved(&out).unwrap();
    (out, report)
}

#[test]
fn a_save_with_no_edits_writes_the_same_bytes() {
    let source = rich_book();
    let mut engine = opened(source.clone());
    assert!(!engine.has_changes());
    let (out, report) = saved(&mut engine);
    assert_eq!(out, source);
    assert!(!report.recalculates_on_open);
    assert!(report.changed_parts.is_empty());
    assert_eq!(report.kept_parts, 14);
    assert!(report.kept_content.contains(&(Kept::Macros, 1)));
    assert!(report.kept_content.contains(&(Kept::Charts, 1)));
    assert!(report.kept_content.contains(&(Kept::CustomXml, 1)));
}

#[test]
fn an_edit_changes_only_what_it_must() {
    let source = rich_book();
    let before = parts(&source);
    let mut engine = opened(source);
    engine
        .edit_cells(&[
            value(0, 1, 1, CalcValue::Number(7.0)),
            value(0, 9, 1, CalcValue::Text("new <text>".to_string())),
        ])
        .unwrap();
    assert!(engine.has_changes());
    let (out, report) = saved(&mut engine);
    let after = parts(&out);

    // What changed had to.
    let mut changed: Vec<&str> = report.changed_parts.iter().map(String::as_str).collect();
    changed.sort_unstable();
    assert_eq!(
        changed,
        [
            "[Content_Types].xml",
            "xl/_rels/workbook.xml.rels",
            "xl/sharedStrings.xml",
            "xl/workbook.xml",
            "xl/worksheets/sheet1.xml",
        ]
    );
    assert_eq!(report.removed_parts, ["xl/calcChain.xml"]);
    assert!(report.recalculates_on_open);

    // The cell, what depends on it, and the new text cell are in the sheet, and nothing else moved.
    let sheet = text(&after, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains(r#"<c r="A1"><v>7</v></c><c r="B1"><f>A1*10</f><v>70</v></c>"#),
        "{sheet}"
    );
    assert!(
        sheet.contains(r#"<row r="9"><c r="A9" t="s"><v>2</v></c></row>"#),
        "{sheet}"
    );
    assert!(sheet.contains(r#"<dimension ref="A1:B9"/>"#), "{sheet}");
    assert!(sheet.contains(r#"<row r="3"><c r="A3"><v>1</v></c><c r="B3"><v>2</v></c></row>"#));
    assert!(sheet.contains("<tableParts"));
    let strings = text(&after, "xl/sharedStrings.xml");
    assert!(
        strings.contains(r#"count="3" uniqueCount="3""#),
        "{strings}"
    );
    assert!(
        strings.ends_with("<si><t>new &lt;text&gt;</t></si></sst>"),
        "{strings}"
    );

    // The calculation chain has gone from everywhere that named it, and Excel recalculates.
    assert!(!after.contains_key("xl/calcChain.xml"));
    assert!(!text(&after, "[Content_Types].xml").contains("calcChain"));
    assert!(!text(&after, "xl/_rels/workbook.xml.rels").contains("calcChain"));
    assert!(
        text(&after, "xl/workbook.xml").contains(r#"<calcPr calcId="191029" fullCalcOnLoad="1"/>"#)
    );

    // Everything else is the same, including what the app has no idea about.
    for (name, body) in &before {
        if !report.changed_parts.contains(name) && !report.removed_parts.contains(name) {
            assert_eq!(after.get(name), Some(body), "{name}");
        }
    }
}

#[test]
fn a_file_reads_back_as_it_was_edited_and_can_be_saved_again() {
    let mut engine = opened(rich_book());
    engine
        .edit_cells(&[
            value(0, 1, 1, CalcValue::Number(7.0)),
            value(1, 5, 2, CalcValue::Bool(true)),
            value(1, 6, 2, CalcValue::Text(" ع_x000D_".to_string())),
            value(0, 3, 1, CalcValue::Error("#N/A".to_string())),
            EditCell {
                sheet: 1,
                row: 7,
                col: 2,
                input: Some(EditInput::Formula("A1&\"<\"".to_string())),
                style: None,
            },
        ])
        .unwrap();
    let (out, _) = saved(&mut engine);

    // The engine moves on to the saved file, with nothing left to save.
    engine.commit_saved(Rc::new(out.clone())).unwrap();
    assert!(!engine.has_changes());
    assert_eq!(saved(&mut engine).0, out);

    let mut again = opened(out.clone());
    assert_eq!(again.display_text(0, 1, 2), "70");
    assert_eq!(again.display_text(1, 5, 2), "TRUE");
    assert_eq!(again.display_text(1, 6, 2), " ع_x000D_");
    assert_eq!(again.display_text(0, 3, 1), "#N/A");
    assert_eq!(again.display_text(1, 7, 2), "2<");
    assert_eq!(again.edit_text(1, 7, 2), "=A1&\"<\"");

    // A second round of edits starts from the first, and appends to what the first appended.
    again
        .edit_cells(&[value(1, 8, 2, CalcValue::Text("second".to_string()))])
        .unwrap();
    let (out, _) = saved(&mut again);
    let strings = text(&parts(&out), "xl/sharedStrings.xml");
    assert!(strings.contains(r#"uniqueCount="4""#), "{strings}");
    let sheet = text(&parts(&out), "xl/worksheets/sheet2.xml");
    assert!(sheet.contains(r#"<c r="B8" t="s"><v>3</v></c>"#), "{sheet}");
}

#[test]
fn a_style_only_edit_recalculates_nothing_and_keeps_the_chain() {
    let mut engine = opened(rich_book());
    let style = engine
        .add_style(
            0,
            &NewStyle {
                font: Some("<b/><sz val=\"11\"/>".to_string()),
                fill: Some("FFEE00".to_string()),
                number_format: Some("0.000".to_string()),
                ..NewStyle::default()
            },
        )
        .unwrap();
    assert_eq!(style, 1);
    engine
        .edit_cells(&[EditCell {
            sheet: 0,
            row: 1,
            col: 1,
            input: None,
            style: Some(style),
        }])
        .unwrap();
    let (out, report) = saved(&mut engine);
    assert!(!report.recalculates_on_open);
    assert!(report.removed_parts.is_empty());
    let after = parts(&out);
    assert!(after.contains_key("xl/calcChain.xml"));
    assert!(text(&after, "xl/worksheets/sheet1.xml").contains(r#"<c r="A1" s="1"><v>2</v></c>"#));
    assert_eq!(
        after["xl/workbook.xml"],
        parts(&rich_book())["xl/workbook.xml"]
    );
    let mut again = opened(out);
    assert_eq!(again.display_text(0, 1, 1), "2.000");
}

#[test]
fn table_headers_stay_in_step_with_their_table() {
    let mut engine = opened(rich_book());
    engine
        .edit_cells(&[value(0, 2, 1, CalcValue::Text("Renamed".to_string()))])
        .unwrap();
    let (out, report) = saved(&mut engine);
    let table = text(&parts(&out), "xl/tables/table1.xml");
    assert!(table.contains(r#"name="Renamed""#), "{table}");
    assert!(table.contains(r#"name="Foot""#), "{table}");
    assert_eq!(report.table_headers_fixed, 1);

    // Excel needs headers to be different strings, so it gets them.
    let mut engine = opened(rich_book());
    engine
        .edit_cells(&[
            value(0, 2, 1, CalcValue::Text("Foot".to_string())),
            EditCell {
                sheet: 0,
                row: 2,
                col: 2,
                input: Some(EditInput::Clear),
                style: None,
            },
        ])
        .unwrap();
    let (out, _) = saved(&mut engine);
    let all = parts(&out);
    let table = text(&all, "xl/tables/table1.xml");
    assert!(table.contains(r#"name="Foot""#), "{table}");
    assert!(table.contains(r#"name="Column2""#), "{table}");
    let strings = text(&all, "xl/sharedStrings.xml");
    assert!(strings.contains("<si><t>Column2</t></si>"), "{strings}");
    let sheet = text(&all, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains(r#"<c r="B2" t="s"><v>"#), "{sheet}");
}

#[test]
fn elements_and_pivots_save_through_the_same_path() {
    let mut engine = opened(rich_book());
    engine
        .add_sheet_element(
            1,
            NewElement {
                name: "mergeCells".to_string(),
                xml: r#"<mergeCells count="1"><mergeCell ref="C1:D1"/></mergeCells>"#.to_string(),
                replace: true,
            },
        )
        .unwrap();
    let (out, report) = saved(&mut engine);
    assert_eq!(report.changed_parts, ["xl/worksheets/sheet2.xml"]);
    assert!(text(&parts(&out), "xl/worksheets/sheet2.xml").contains("</sheetData><mergeCells"));
    assert!(!report.recalculates_on_open);
}

#[test]
fn a_workbook_without_shared_strings_gets_them_in_full() {
    let bytes = cgs_xlsx::blank::blank_workbook().unwrap();
    let mut engine = opened(bytes);
    engine
        .edit_cells(&[value(0, 2, 2, CalcValue::Text("first".to_string()))])
        .unwrap();
    let (out, report) = saved(&mut engine);
    assert_eq!(report.added_parts, ["xl/sharedStrings.xml"]);
    let all = parts(&out);
    assert!(text(&all, "[Content_Types].xml").contains(r#"PartName="/xl/sharedStrings.xml""#));
    assert!(text(&all, "xl/_rels/workbook.xml.rels").contains(r#"Target="sharedStrings.xml""#));
    let mut again = opened(out);
    assert_eq!(again.display_text(0, 2, 2), "first");
}

#[test]
fn a_check_catches_a_save_that_went_wrong() {
    let mut engine = opened(rich_book());
    engine
        .edit_cells(&[value(0, 1, 1, CalcValue::Number(7.0))])
        .unwrap();
    let (mut out, _) = saved(&mut engine);
    // Flip a byte inside a changed part's data.
    let directory = cgs_xlsx::zipdir::Directory::read(&out).unwrap();
    let entry = directory
        .entries
        .iter()
        .find(|entry| entry.name == "xl/worksheets/sheet1.xml")
        .unwrap();
    let at = entry.local as usize + 30 + entry.name.len() + 5;
    out[at] ^= 0xFF;
    assert!(matches!(
        engine.verify_saved(&out),
        Err(SaveError::Verify(_))
    ));
    // A file with another part missing, or that isn't the plan at all.
    assert!(engine.verify_saved(&rich_book()).is_err());
}

#[test]
fn a_source_that_changed_is_refused() {
    let mut engine = opened(rich_book());
    let other = zip_of(&[("[Content_Types].xml", b"<a/>")]);
    assert!(matches!(
        engine.use_source(Rc::new(other)),
        Err(SaveError::SourceUnreadable(_))
    ));
    engine.use_source(Rc::new(rich_book())).unwrap();
    engine
        .edit_cells(&[value(0, 1, 1, CalcValue::Number(7.0))])
        .unwrap();
    saved(&mut engine);
}

/// A source that fails like a browser's `File` after the file has changed.
struct Gone(u64);

impl ByteSource for Gone {
    fn len(&self) -> u64 {
        self.0
    }

    fn read_at(&self, _offset: u64, _out: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("NotReadableError"))
    }
}

#[test]
fn a_source_that_cannot_be_read_stops_the_save_and_keeps_the_edits() {
    let source = rich_book();
    let size = source.len() as u64;
    let mut engine = opened(source);
    engine
        .edit_cells(&[value(0, 1, 1, CalcValue::Number(7.0))])
        .unwrap();
    assert!(engine.use_source(Rc::new(Gone(size))).is_err());
    // The working source is still the good one, so the edits can still be saved.
    assert!(engine.has_changes());
    saved(&mut engine);
}

#[test]
fn a_new_blank_workbook_saves_and_reopens() {
    let source = cgs_xlsx::blank::blank_workbook().unwrap();
    let mut engine = opened(source.clone());
    let (out, _) = saved(&mut engine);
    assert_eq!(out, source);
    let mut engine = opened(source);
    engine
        .edit_cells(&[value(0, 1, 1, CalcValue::Number(1.5))])
        .unwrap();
    let (out, _) = saved(&mut engine);
    let mut again = opened(out);
    assert_eq!(again.display_text(0, 1, 1), "1.5");
}

/// A workbook with a formula that reads another one, as business files often have.
fn linked_book() -> Vec<u8> {
    let types = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
    let root = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{RELS}"><sheets><sheet name="One" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
    );
    let sheet = format!(
        r#"<worksheet xmlns="{MAIN}"><sheetData><row r="1"><c r="A1"><f>[1]Sheet1!A1</f><v>5</v></c><c r="B1"><f>A1*2</f><v>10</v></c><c r="C1"><v>1</v></c></row></sheetData></worksheet>"#
    );
    zip_of(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", root.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ])
}

#[test]
fn a_link_to_another_workbook_keeps_its_value_and_saves_as_it_was() {
    let mut engine = opened(linked_book());
    while engine.prepare_step().unwrap() {}
    assert_eq!(engine.display_text(0, 1, 1), "5");
    assert_eq!(engine.edit_text(0, 1, 1), "=[1]Sheet1!A1");

    // What reads the link recalculates from its saved value, and the link itself isn't touched.
    engine
        .edit_cells(&[value(0, 1, 3, CalcValue::Number(2.0))])
        .unwrap();
    assert_eq!(engine.display_text(0, 1, 2), "10");
    engine
        .edit_cells(&[EditCell {
            sheet: 0,
            row: 1,
            col: 2,
            input: Some(EditInput::Formula("A1*3".to_string())),
            style: None,
        }])
        .unwrap();
    assert_eq!(engine.display_text(0, 1, 2), "15");
    let (out, _) = saved(&mut engine);
    let sheet = text(&parts(&out), "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains("<c r=\"A1\"><f>[1]Sheet1!A1</f><v>5</v></c>"),
        "{sheet}"
    );
    assert!(
        sheet.contains("<c r=\"B1\"><f>A1*3</f><v>15</v></c>"),
        "{sheet}"
    );
}
