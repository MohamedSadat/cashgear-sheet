//! Formatting through the engine: styles only grow, undo puts back exactly what was there, and what a
//! save writes reads back the same.

use std::io::{Cursor, Read, Write};
use std::rc::Rc;

use cgs_engine::{Command, Engine, LoadPhase};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const STYLES: &str = r#"<fonts count="2"><font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font><font><b/><sz val="14"/><name val="Arial"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/><bgColor indexed="64"/></patternFill></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="3"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="4" fontId="1" fillId="2" borderId="0" xfId="0" applyNumberFormat="1" applyFont="1" applyFill="1"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyProtection="1"><protection locked="0"/></xf></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>"#;

const SHEET: &str = r#"<dimension ref="A1:C3"/><sheetViews><sheetView workbookViewId="0"><selection activeCell="B2" sqref="B2"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight="15"/><cols><col min="2" max="2" width="20" customWidth="1"/></cols><sheetData><row r="1" ht="30" customHeight="1"><c r="A1"><v>1</v></c><c r="B1" s="1"><v>2.5</v></c></row><row r="2"><c r="A2" s="2"><v>3</v></c><c r="B2"><f>A2*2</f><v>6</v></c></row><row r="3"><c r="A3" t="s"><v>0</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="D5:E6"/></mergeCells><pageMargins left="1" right="1" top="1" bottom="1" header="0" footer="0"/>"#;

fn zip_of(parts: &[(&str, String)]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut out);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }
    out.into_inner()
}

fn book() -> Vec<u8> {
    let types = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>"#;
    let root = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{RELS}"><sheets><sheet name="One" sheetId="1" r:id="rId1"/></sheets><calcPr calcId="1"/></workbook>"#
    );
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{RELS}/styles" Target="styles.xml"/><Relationship Id="rId3" Type="{RELS}/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#
    );
    zip_of(&[
        ("[Content_Types].xml", types.to_string()),
        ("_rels/.rels", root),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        (
            "xl/worksheets/sheet1.xml",
            format!(r#"<worksheet xmlns="{MAIN}">{SHEET}</worksheet>"#),
        ),
        (
            "xl/styles.xml",
            format!(r#"<styleSheet xmlns="{MAIN}">{STYLES}</styleSheet>"#),
        ),
        (
            "xl/sharedStrings.xml",
            format!(r#"<sst xmlns="{MAIN}" count="1" uniqueCount="1"><si><t>Hello</t></si></sst>"#),
        ),
    ])
}

fn opened(bytes: Vec<u8>) -> Engine {
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).unwrap();
    while engine.load_step(1000.0).unwrap().phase == LoadPhase::Reading {}
    while engine.prepare_step().unwrap() {}
    engine
}

fn saved(engine: &mut Engine) -> Vec<u8> {
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    engine.verify_saved(&out).unwrap();
    out
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

fn sheet_part(bytes: &[u8]) -> String {
    part(bytes, "xl/worksheets/sheet1.xml")
}

fn run(engine: &mut Engine, json: &str) {
    engine.apply_json(json).unwrap();
}

fn undo(engine: &mut Engine) {
    engine.apply(Command::Undo).unwrap();
}

fn redo(engine: &mut Engine) {
    engine.apply(Command::Redo).unwrap();
}

/// The list of cell formats in a styles part, each as its own text.
fn xfs(styles: &str) -> Vec<String> {
    let start = styles.find("<cellXfs").unwrap();
    let start = start + styles[start..].find('>').unwrap() + 1;
    let end = styles.find("</cellXfs>").unwrap();
    let mut list = Vec::new();
    let mut rest = &styles[start..end];
    while !rest.is_empty() {
        let tag_end = rest.find('>').unwrap();
        let cut = if rest.as_bytes()[tag_end - 1] == b'/' {
            tag_end + 1
        } else {
            rest.find("</xf>").unwrap() + 5
        };
        list.push(rest[..cut].to_string());
        rest = &rest[cut..];
    }
    list
}

fn style_of(engine: &Engine, row: u32, col: u32) -> u32 {
    engine.book().unwrap().sheets[0].style_at(row, col)
}

const AREA_A1_C3: &str = r#"{"top":1,"left":1,"bottom":3,"right":3}"#;

#[test]
fn formats_add_styles_and_leave_the_old_ones_byte_for_byte() {
    let source = book();
    let mut engine = opened(source.clone());
    run(
        &mut engine,
        &format!(
            r#"{{"kind":"format","sheet":0,"areas":[{AREA_A1_C3}],"change":{{"bold":true,"fontSize":16,"fill":{{"pattern":"solid","color":{{"kind":"theme","theme":4,"tint":0.4}}}}}}}}"#
        ),
    );
    let out = saved(&mut engine);
    let before = xfs(&part(&source, "xl/styles.xml"));
    let after = xfs(&part(&out, "xl/styles.xml"));
    assert_eq!(after[..before.len()], before[..], "old formats moved");
    assert!(after.len() > before.len());
    let styles = part(&out, "xl/styles.xml");
    assert!(styles.contains(r#"<font><b/><sz val="16"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font>"#), "{styles}");
    assert!(
        styles.contains(r#"<fgColor theme="4" tint="0.3999755851924192"/>"#),
        "{styles}"
    );
    let sheet = sheet_part(&out);
    // A1 and blanks in the area get cells, and the protection on A2's format stays with it.
    assert!(sheet.contains(r#"<c r="C2" s=""#), "{sheet}");
    let a2 = sheet.split(r#"<c r="A2" s=""#).nth(1).unwrap();
    let a2_style: usize = a2[..a2.find('"').unwrap()].parse().unwrap();
    assert!(
        after[a2_style].contains(r#"<protection locked="0"/>"#),
        "{}",
        after[a2_style]
    );
    // Reading it back says the same.
    let again = opened(out);
    let info = &again.book().unwrap().sheets[0];
    let table = &again.book().unwrap().styles;
    let font = table.font(table.xf(info.style_at(1, 1)));
    assert!(font.bold);
    assert_eq!(font.size, 16.0);
    assert_eq!(font.name, "Calibri");
}

#[test]
fn undoing_a_format_saves_the_file_as_it_was() {
    let source = book();
    let mut engine = opened(source.clone());
    let plain = style_of(&engine, 1, 1);
    run(
        &mut engine,
        &format!(
            r#"{{"kind":"format","sheet":0,"areas":[{AREA_A1_C3}],"change":{{"italic":true}}}}"#
        ),
    );
    assert_ne!(style_of(&engine, 1, 1), plain);
    undo(&mut engine);
    assert_eq!(style_of(&engine, 1, 1), plain);
    let out = saved(&mut engine);
    assert_eq!(out, source);
    redo(&mut engine);
    assert_ne!(style_of(&engine, 1, 1), plain);
    let out = saved(&mut engine);
    assert!(sheet_part(&out).contains(r#"<c r="C3" s=""#));
}

#[test]
fn a_format_undone_after_a_save_goes_back_in_the_new_file_too() {
    let mut engine = opened(book());
    run(
        &mut engine,
        &format!(
            r#"{{"kind":"format","sheet":0,"areas":[{AREA_A1_C3}],"change":{{"italic":true}}}}"#
        ),
    );
    let first = saved(&mut engine);
    engine.commit_saved(Rc::new(first)).unwrap();
    undo(&mut engine);
    run(
        &mut engine,
        r#"{"kind":"input","sheet":0,"row":9,"col":9,"text":"x"}"#,
    );
    let second = saved(&mut engine);
    let again = opened(second);
    let table = &again.book().unwrap().styles;
    let info = &again.book().unwrap().sheets[0];
    assert!(!table.font(table.xf(info.style_at(1, 1))).italic);
    // A blank that only got a cell for the format doesn't keep the look.
    assert!(!table.font(table.xf(info.style_at(3, 3))).italic);
}

#[test]
fn a_whole_column_goes_on_its_col_and_cells_it_already_has() {
    let mut engine = opened(book());
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":1,"bottom":1048576,"right":2}],"change":{"horizontal":"center"}}"#,
    );
    let info = &engine.book().unwrap().sheets[0];
    let center = info.style_at(1, 1);
    assert_ne!(center, 0);
    // A cell nobody wrote takes it from the column.
    let far = info.style_at(1000, 1);
    assert_eq!(info.own_style(1000, 1), None);
    let table = &engine.book().unwrap().styles;
    assert_eq!(
        table.xf(far).alignment.horizontal,
        cgs_model::HAlign::Center
    );
    let out = saved(&mut engine);
    let sheet = sheet_part(&out);
    // Column B keeps its width, and neither column gets a million cells.
    assert!(
        sheet.contains(r#"<col min="1" max="1" width="9.140625" style=""#),
        "{sheet}"
    );
    assert!(
        sheet.contains(r#"<col min="2" max="2" width="20" customWidth="1" style=""#),
        "{sheet}"
    );
    assert!(!sheet.contains(r#"r="A1000""#));
    assert!(sheet.len() < 2000, "{}", sheet.len());
    undo(&mut engine);
    let info = &engine.book().unwrap().sheets[0];
    assert_eq!(info.style_at(1000, 1), 0);
    assert_eq!(info.style_at(1, 2), 1);
}

#[test]
fn a_whole_sheet_stays_small_and_row_styles_follow() {
    let mut engine = opened(book());
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":5,"left":1,"bottom":5,"right":16384}],"change":{"bold":true}}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":1,"bottom":1048576,"right":16384}],"change":{"italic":true}}"#,
    );
    let info = &engine.book().unwrap().sheets[0];
    let table = &engine.book().unwrap().styles;
    assert!(table.font(table.xf(info.style_at(5, 3))).italic);
    assert!(table.font(table.xf(info.style_at(5, 3))).bold);
    assert!(table.font(table.xf(info.style_at(9, 9))).italic);
    assert!(!table.font(table.xf(info.style_at(9, 9))).bold);
    let out = saved(&mut engine);
    let sheet = sheet_part(&out);
    assert!(sheet.contains(r#"max="16384""#), "{sheet}");
    assert!(sheet.contains(r#"customFormat="1""#), "{sheet}");
    assert!(sheet.len() < 3000, "{}", sheet.len());
    let again = opened(out);
    let info = &again.book().unwrap().sheets[0];
    let table = &again.book().unwrap().styles;
    assert!(table.font(table.xf(info.style_at(9, 9))).italic);
    assert!(table.font(table.xf(info.style_at(5, 9))).bold);
}

#[test]
fn a_whole_row_with_a_styled_column_keeps_the_column_where_they_cross() {
    let mut engine = opened(book());
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":3,"bottom":1048576,"right":3}],"change":{"fill":{"pattern":"solid","color":{"kind":"rgb","rgb":"00FF00"}}}}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":7,"left":1,"bottom":7,"right":16384}],"change":{"bold":true}}"#,
    );
    let info = &engine.book().unwrap().sheets[0];
    let table = &engine.book().unwrap().styles;
    let crossing = table.xf(info.style_at(7, 3));
    assert!(table.font(crossing).bold);
    assert_eq!(table.fill(crossing).pattern, cgs_model::Pattern::Solid);
    assert!(table.font(table.xf(info.style_at(7, 5))).bold);
    assert_eq!(
        table.fill(table.xf(info.style_at(7, 5))).pattern,
        cgs_model::Pattern::None
    );
    let out = saved(&mut engine);
    let again = opened(out);
    let info = &again.book().unwrap().sheets[0];
    let table = &again.book().unwrap().styles;
    assert!(table.font(table.xf(info.style_at(7, 3))).bold);
    assert_eq!(
        table.fill(table.xf(info.style_at(7, 3))).pattern,
        cgs_model::Pattern::Solid
    );
}

#[test]
fn borders_go_by_where_a_cell_sits_and_neighbours_let_go() {
    let mut engine = opened(book());
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":2,"bottom":1,"right":2}],"change":{"border":{"left":{"style":"thick"}}}}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":1,"bottom":2,"right":2}],"change":{"border":{"top":{"style":"thin"},"bottom":{"style":"double"},"left":{"style":"medium"},"right":{"style":"medium"},"insideHorizontal":{"style":"dashed"}}}}"#,
    );
    let info = &engine.book().unwrap().sheets[0];
    let table = &engine.book().unwrap().styles;
    let line = |row, col| *table.border(table.xf(info.style_at(row, col)));
    use cgs_model::LineStyle::*;
    assert_eq!(line(1, 1).top.style, Thin);
    assert_eq!(line(1, 1).bottom.style, Dashed);
    assert_eq!(line(1, 1).left.style, Medium);
    assert_eq!(line(1, 1).right.style, None);
    assert_eq!(line(2, 2).top.style, Dashed);
    assert_eq!(line(2, 2).bottom.style, Double);
    assert_eq!(line(2, 2).right.style, Medium);
    assert_eq!(line(2, 2).left.style, None);
    // The first format's thick left on B1 is on the inside now, with nothing said about it.
    assert_eq!(line(1, 2).left.style, Thick);
}

#[test]
fn sizes_and_hiding_save_and_undo() {
    let source = book();
    let mut engine = opened(source.clone());
    run(
        &mut engine,
        r#"{"kind":"lines","sheet":0,"axis":"col","first":3,"last":4,"size":100}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"lines","sheet":0,"axis":"row","first":3,"last":3,"size":40}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"lines","sheet":0,"axis":"row","first":2,"last":2,"hidden":true}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"lines","sheet":0,"axis":"col","first":1,"last":1,"hidden":true}"#,
    );
    let out = saved(&mut engine);
    let sheet = sheet_part(&out);
    assert!(
        sheet.contains(r#"<row r="3" ht="30" customHeight="1">"#),
        "{sheet}"
    );
    assert!(sheet.contains(r#"<row r="2" hidden="1">"#), "{sheet}");
    assert!(
        sheet.contains(r#"<col min="1" max="1" width="9.140625" hidden="1"/>"#),
        "{sheet}"
    );
    assert!(
        sheet.contains(r#"<col min="2" max="2" width="20" customWidth="1"/>"#),
        "{sheet}"
    );
    assert!(
        sheet.contains(r#"<col min="3" max="4" width="14.28515625" customWidth="1"/>"#),
        "{sheet}"
    );
    let again = opened(out);
    let info = &again.book().unwrap().sheets[0];
    assert_eq!(info.rows.px(3), 40);
    assert!(info.rows.info(3).unwrap().custom);
    assert_eq!(info.rows.px(2), 0);
    assert_eq!(info.cols.px(1), 0);
    assert_eq!(info.cols.px(4), 100);
    for _ in 0..4 {
        undo(&mut engine);
    }
    assert_eq!(saved(&mut engine), source);
}

#[test]
fn showing_a_line_gives_it_its_size_back() {
    let mut engine = opened(book());
    run(
        &mut engine,
        r#"{"kind":"lines","sheet":0,"axis":"col","first":2,"last":2,"size":0}"#,
    );
    assert_eq!(engine.book().unwrap().sheets[0].cols.px(2), 0);
    run(
        &mut engine,
        r#"{"kind":"lines","sheet":0,"axis":"col","first":2,"last":2,"hidden":false}"#,
    );
    assert_eq!(engine.book().unwrap().sheets[0].cols.px(2), 64);
}

#[test]
fn a_bigger_font_makes_the_row_taller_unless_someone_set_it() {
    let mut engine = opened(book());
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":2,"left":1,"bottom":2,"right":1}],"change":{"fontSize":24}}"#,
    );
    let info = &engine.book().unwrap().sheets[0];
    assert_eq!(info.rows.px(2), 42);
    assert!(!info.rows.info(2).unwrap().custom);
    // Row 1 has a fixed height.
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":1,"bottom":1,"right":1}],"change":{"fontSize":48}}"#,
    );
    assert_eq!(engine.book().unwrap().sheets[0].rows.px(1), 40);
    let out = saved(&mut engine);
    let sheet = sheet_part(&out);
    assert!(sheet.contains(r#"<row r="2" ht="31.5">"#), "{sheet}");
    undo(&mut engine);
    undo(&mut engine);
    assert_eq!(engine.book().unwrap().sheets[0].rows.px(2), 20);
}

#[test]
fn merging_keeps_the_first_thing_and_goes_back() {
    let source = book();
    let mut engine = opened(source.clone());
    run(
        &mut engine,
        r#"{"kind":"merge","sheet":0,"areas":[{"top":2,"left":1,"bottom":3,"right":3}],"center":true}"#,
    );
    let info = &engine.book().unwrap().sheets[0];
    assert!(info
        .merges
        .iter()
        .any(|merge| merge.top == 2 && merge.right == 3));
    let table = &engine.book().unwrap().styles;
    assert_eq!(
        table.xf(info.style_at(3, 3)).alignment.horizontal,
        cgs_model::HAlign::Center
    );
    let out = saved(&mut engine);
    let sheet = sheet_part(&out);
    assert!(
        sheet.contains(
            r#"<mergeCells count="2"><mergeCell ref="D5:E6"/><mergeCell ref="A2:C3"/></mergeCells>"#
        ),
        "{sheet}"
    );
    assert!(!sheet.contains("<f>A2*2</f>"), "{sheet}");
    undo(&mut engine);
    assert_eq!(engine.book().unwrap().sheets[0].merges.len(), 1);
    assert_eq!(saved(&mut engine), source);
    redo(&mut engine);
    run(
        &mut engine,
        r#"{"kind":"unmerge","sheet":0,"areas":[{"top":2,"left":1,"bottom":2,"right":1}]}"#,
    );
    assert_eq!(engine.book().unwrap().sheets[0].merges.len(), 1);
}

#[test]
fn a_merge_that_cuts_another_is_refused() {
    let mut engine = opened(book());
    let result = engine.apply_json(
        r#"{"kind":"merge","sheet":0,"areas":[{"top":5,"left":4,"bottom":5,"right":6}]}"#,
    );
    assert!(result.is_err());
}

#[test]
fn panes_and_view_flags_save_and_read_back() {
    let source = book();
    let mut engine = opened(source.clone());
    run(
        &mut engine,
        r#"{"kind":"freeze","sheet":0,"rows":2,"cols":1}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"viewFlags","sheet":0,"grid":false,"headings":false}"#,
    );
    let out = saved(&mut engine);
    let sheet = sheet_part(&out);
    assert!(sheet.contains(r#"state="frozen""#), "{sheet}");
    assert!(sheet.contains(r#"showGridLines="0""#), "{sheet}");
    assert!(sheet.contains(r#"showRowColHeaders="0""#), "{sheet}");
    let again = opened(out);
    let view = again.book().unwrap().sheets[0].view;
    assert_eq!(view.freeze.unwrap().rows, 2);
    assert_eq!(view.freeze.unwrap().cols, 1);
    assert!(!view.show_grid);
    undo(&mut engine);
    undo(&mut engine);
    assert_eq!(saved(&mut engine), source);
}

#[test]
fn more_decimals_start_from_what_general_showed() {
    let mut engine = opened(book());
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":1,"bottom":1,"right":2}],"change":{"decimals":1},"active":[1,2]}"#,
    );
    let info = &engine.book().unwrap().sheets[0];
    let table = &engine.book().unwrap().styles;
    // B1 is 2.5 in #,##0.00, so its own format grows. A1 was General, and goes from B1's 2 places
    // of nothing: General shows no decimals in a value of 2.5 here only through its format.
    let b1 = table.xf(info.style_at(1, 2)).num_fmt;
    assert_eq!(
        table.num_fmts.get(&b1).map(String::as_str),
        Some("#,##0.000")
    );
    let a1 = table.xf(info.style_at(1, 1)).num_fmt;
    assert_eq!(a1, 2, "{:?}", table.num_fmts.get(&a1));
}

#[test]
fn a_bad_number_format_is_refused() {
    let mut engine = opened(book());
    assert!(engine
        .apply_json(
            r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":1,"bottom":1,"right":1}],"change":{"numberFormat":"0.0.0[["}}"#,
        )
        .is_err());
}

#[test]
fn inserting_a_row_moves_what_a_format_said() {
    let mut engine = opened(book());
    run(
        &mut engine,
        r#"{"kind":"lines","sheet":0,"axis":"row","first":3,"last":3,"size":50}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":3,"left":1,"bottom":3,"right":2}],"change":{"bold":true}}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":1,"count":2,"delete":false}"#,
    );
    let out = saved(&mut engine);
    let again = opened(out);
    let info = &again.book().unwrap().sheets[0];
    let table = &again.book().unwrap().styles;
    assert_eq!(info.rows.px(5), 50);
    assert_eq!(info.rows.px(3), 40);
    assert!(table.font(table.xf(info.style_at(5, 1))).bold);
    assert!(table.font(table.xf(info.style_at(5, 2))).bold);
    assert!(!table.font(table.xf(info.style_at(3, 1))).bold);
}

#[test]
fn find_goes_round_and_lists_what_it_finds() {
    let mut engine = opened(book());
    let query = r#"{"text":"hello","sheet":0}"#;
    let found = engine.find_next_json(query, 1, 1, false).unwrap();
    assert!(found.contains(r#""row":3"#), "{found}");
    // From past it, going round.
    let found = engine.find_next_json(query, 3, 1, false).unwrap();
    assert!(found.contains(r#""row":3"#), "{found}");
    let none = engine
        .find_next_json(r#"{"text":"nope","sheet":0}"#, 1, 1, false)
        .unwrap();
    assert_eq!(none, "null");
    // What a cell shows, against what its formula says.
    let shown = engine
        .find_next_json(r#"{"text":"6","sheet":0}"#, 1, 1, false)
        .unwrap();
    assert!(shown.contains(r#""row":2,"col":2"#), "{shown}");
    let formula = engine
        .find_all_json(r#"{"text":"a2*","sheet":0,"formulas":true}"#)
        .unwrap();
    assert!(formula.contains(r#""text":"=A2*2""#), "{formula}");
    let values = engine.find_all_json(r#"{"text":"a2*","sheet":0}"#).unwrap();
    assert_eq!(values, "[]");
}

#[test]
fn replacing_retypes_and_counts() {
    let mut engine = opened(book());
    run(
        &mut engine,
        r#"{"kind":"replaceAll","query":{"text":"A2","sheet":0,"formulas":true},"with":"A1"}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"replaceAll","query":{"text":"ell","sheet":0,"formulas":true},"with":"ipp"}"#,
    );
    let out = saved(&mut engine);
    let sheet = sheet_part(&out);
    assert!(sheet.contains("<f>A1*2</f>"), "{sheet}");
    assert!(part(&out, "xl/sharedStrings.xml").contains("Hippo"));
}

#[test]
fn cells_shift_down_and_what_reads_them_follows() {
    let mut engine = opened(book());
    // B2 reads A2. Move A2 down by one: the formula in B2 follows it, and stays where it is.
    run(
        &mut engine,
        r#"{"kind":"shiftCells","sheet":0,"area":{"top":2,"left":1,"bottom":2,"right":1},"down":true}"#,
    );
    let out = saved(&mut engine);
    let sheet = sheet_part(&out);
    assert!(sheet.contains("<f>A3*2</f>"), "{sheet}");
    let a3 = sheet.split(r#"<c r="A3""#).nth(1).unwrap();
    assert!(a3.starts_with(r#" s="2""#), "{a3}");
    let a4 = sheet.split(r#"<c r="A4""#).nth(1).unwrap();
    assert!(a4.contains(r#"t="s""#), "{a4}");
    undo(&mut engine);
    run(
        &mut engine,
        r#"{"kind":"shiftCells","sheet":0,"area":{"top":1,"left":1,"bottom":1,"right":1},"down":false,"delete":true}"#,
    );
    let out = saved(&mut engine);
    let sheet = sheet_part(&out);
    // A1 went, so B1 took its place, keeping its look.
    let a1 = sheet.split(r#"<c r="A1""#).nth(1).unwrap();
    assert!(a1.starts_with(r#" s="1""#), "{a1}");
}

#[test]
fn a_shift_through_part_of_a_merge_is_refused() {
    let mut engine = opened(book());
    let result = engine.apply_json(
        r#"{"kind":"shiftCells","sheet":0,"area":{"top":5,"left":4,"bottom":5,"right":4},"down":true}"#,
    );
    assert!(result.is_err());
}

#[test]
fn undoing_a_shift_saves_the_file_as_it_was() {
    let source = book();
    let mut engine = opened(source.clone());
    run(
        &mut engine,
        r#"{"kind":"shiftCells","sheet":0,"area":{"top":1,"left":1,"bottom":2,"right":1},"down":false}"#,
    );
    undo(&mut engine);
    assert_eq!(saved(&mut engine), source);
}
