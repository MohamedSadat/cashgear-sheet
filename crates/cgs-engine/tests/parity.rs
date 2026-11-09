//! Phase 13's Excel parity: each change made through the engine, saved as Excel 2016 saves it, read
//! back, and undone.

use std::io::{Cursor, Read, Write};
use std::rc::Rc;

use cgs_engine::{Command, Engine, LoadPhase};
use cgs_model::SheetState;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn zip_of(parts: &[(String, String)]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut out);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in parts {
            writer.start_file(name.as_str(), options).unwrap();
            writer.write_all(bytes.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }
    out.into_inner()
}

/// A workbook of these sheets, each given as its name and what goes inside `<worksheet>`, with
/// `book_views` inside `<workbook>` before `<sheets>`.
fn workbook(sheets: &[(&str, &str)], book_views: &str) -> Vec<u8> {
    let mut types = String::from(
        r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
    );
    let mut list = String::new();
    let mut rels = String::new();
    let mut parts = Vec::new();
    for (index, (name, body)) in sheets.iter().enumerate() {
        let n = index + 1;
        types.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
        list.push_str(&format!(
            r#"<sheet name="{name}" sheetId="{n}" r:id="rId{n}"/>"#
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{n}" Type="{RELS}/worksheet" Target="worksheets/sheet{n}.xml"/>"#
        ));
        parts.push((
            format!("xl/worksheets/sheet{n}.xml"),
            format!(r#"<worksheet xmlns="{MAIN}" xmlns:r="{RELS}">{body}</worksheet>"#),
        ));
    }
    types.push_str("</Types>");
    let styles_id = sheets.len() + 1;
    rels.push_str(&format!(
        r#"<Relationship Id="rId{styles_id}" Type="{RELS}/styles" Target="styles.xml"/>"#
    ));
    let mut all = vec![
        ("[Content_Types].xml".to_string(), types),
        (
            "_rels/.rels".to_string(),
            format!(
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/workbook.xml".to_string(),
            format!(
                r#"<workbook xmlns="{MAIN}" xmlns:r="{RELS}">{book_views}<sheets>{list}</sheets><calcPr calcId="1"/></workbook>"#
            ),
        ),
        (
            "xl/_rels/workbook.xml.rels".to_string(),
            format!(
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
            ),
        ),
        (
            "xl/styles.xml".to_string(),
            format!(
                r#"<styleSheet xmlns="{MAIN}"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#
            ),
        ),
    ];
    all.extend(parts);
    zip_of(&all)
}

const PLAIN: &str = r#"<sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>"#;
const SELECTED: &str = r#"<sheetViews><sheetView tabSelected="1" workbookViewId="0"/></sheetViews><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>"#;
const VIEWS: &str = r#"<bookViews><workbookView xWindow="0" yWindow="0" windowWidth="100" windowHeight="100"/></bookViews>"#;

fn three() -> Vec<u8> {
    workbook(
        &[("One", SELECTED), ("Two", PLAIN), ("Three", PLAIN)],
        VIEWS,
    )
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

fn run(engine: &mut Engine, json: &str) {
    engine.apply_json(json).unwrap();
}

fn undo(engine: &mut Engine) {
    engine.apply(Command::Undo).unwrap();
}

fn states(engine: &Engine) -> Vec<SheetState> {
    engine
        .book()
        .unwrap()
        .sheets
        .iter()
        .map(|sheet| sheet.state)
        .collect()
}

#[test]
fn hiding_the_active_sheet_saves_as_excel_does_and_undoes() {
    let source = three();
    let mut engine = opened(source.clone());
    run(
        &mut engine,
        r#"{"kind":"sheetState","sheet":0,"hidden":true}"#,
    );
    assert_eq!(
        states(&engine),
        [SheetState::Hidden, SheetState::Visible, SheetState::Visible]
    );
    // The next visible sheet to the right is active.
    assert_eq!(engine.book().unwrap().active_sheet, 1);
    let out = saved(&mut engine);
    let book = part(&out, "xl/workbook.xml");
    assert!(
        book.contains(r#"<sheet name="One" sheetId="1" r:id="rId1" state="hidden"/>"#),
        "{book}"
    );
    assert!(book.contains(r#"activeTab="1""#), "{book}");
    assert!(book.contains(r#"firstSheet="1""#), "{book}");
    assert!(!part(&out, "xl/worksheets/sheet1.xml").contains("tabSelected"));
    assert!(part(&out, "xl/worksheets/sheet2.xml").contains(r#"tabSelected="1""#));
    let again = opened(out);
    assert_eq!(states(&again)[0], SheetState::Hidden);
    assert_eq!(again.book().unwrap().active_sheet, 1);
    undo(&mut engine);
    assert_eq!(states(&engine)[0], SheetState::Visible);
    assert_eq!(engine.book().unwrap().active_sheet, 0);
    assert_eq!(saved(&mut engine), source);
}

#[test]
fn the_last_tab_hidden_hands_over_to_its_left_and_unhiding_shows_the_sheet() {
    let mut engine = opened(three());
    run(
        &mut engine,
        r#"{"kind":"sheetState","sheet":1,"hidden":true}"#,
    );
    // Not the active sheet, so the active one stays.
    assert_eq!(engine.book().unwrap().active_sheet, 0);
    run(
        &mut engine,
        r#"{"kind":"sheetState","sheet":1,"hidden":false}"#,
    );
    assert_eq!(engine.book().unwrap().active_sheet, 1);
    run(
        &mut engine,
        r#"{"kind":"sheetState","sheet":2,"hidden":false}"#,
    );
    // Already visible: nothing to do.
    assert_eq!(engine.book().unwrap().active_sheet, 1);
    let out = saved(&mut engine);
    let book = part(&out, "xl/workbook.xml");
    assert!(!book.contains("state="), "{book}");
    assert!(book.contains(r#"activeTab="1""#), "{book}");
    // Hiding the active last tab goes to the nearest visible on its left.
    let mut engine = opened(three());
    run(&mut engine, r#"{"kind":"activeSheet","sheet":2}"#);
    run(
        &mut engine,
        r#"{"kind":"sheetState","sheet":1,"hidden":true}"#,
    );
    assert_eq!(engine.book().unwrap().active_sheet, 2);
    run(
        &mut engine,
        r#"{"kind":"sheetState","sheet":2,"hidden":true}"#,
    );
    assert_eq!(engine.book().unwrap().active_sheet, 0);
}

#[test]
fn the_sheet_showing_is_saved_without_being_a_change() {
    let source = three();
    let mut engine = opened(source.clone());
    run(&mut engine, r#"{"kind":"activeSheet","sheet":2}"#);
    assert!(!engine.has_changes());
    let out = saved(&mut engine);
    assert!(part(&out, "xl/workbook.xml").contains(r#"activeTab="2""#));
    assert!(!part(&out, "xl/worksheets/sheet1.xml").contains("tabSelected"));
    assert!(part(&out, "xl/worksheets/sheet3.xml").contains(r#"tabSelected="1""#));
    // Back where the file had it, the save is the file.
    run(&mut engine, r#"{"kind":"activeSheet","sheet":0}"#);
    let mut engine = opened(source.clone());
    run(&mut engine, r#"{"kind":"activeSheet","sheet":2}"#);
    run(&mut engine, r#"{"kind":"activeSheet","sheet":0}"#);
    assert_eq!(saved(&mut engine), source);
}

#[test]
fn the_last_visible_sheet_stays() {
    let mut engine = opened(three());
    run(
        &mut engine,
        r#"{"kind":"sheetState","sheet":0,"hidden":true}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"sheetState","sheet":2,"hidden":true}"#,
    );
    assert_eq!(engine.book().unwrap().active_sheet, 1);
    let refused = engine
        .apply_json(r#"{"kind":"sheetState","sheet":1,"hidden":true}"#)
        .unwrap_err();
    assert!(refused.contains("at least one visible"), "{refused}");
    // Nor deleted while the others are hidden.
    assert!(engine
        .apply_json(r#"{"kind":"deleteSheet","sheet":1}"#)
        .is_err());
}

#[test]
fn zoom_is_saved_with_nothing_else_changed_and_isnt_undone() {
    let source = three();
    let mut engine = opened(source.clone());
    run(
        &mut engine,
        r#"{"kind":"viewFlags","sheet":1,"grid":false}"#,
    );
    run(&mut engine, r#"{"kind":"zoom","sheet":1,"zoom":150}"#);
    assert_eq!(engine.layout(1).unwrap().zoom, 150);
    assert!(engine.has_changes());
    // Undoing the gridlines leaves the zoom, and with nothing left to undo the book is unchanged.
    undo(&mut engine);
    assert!(!engine.has_changes());
    assert_eq!(engine.layout(1).unwrap().zoom, 150);
    run(&mut engine, r#"{"kind":"zoom","sheet":2,"zoom":1000}"#);
    let out = saved(&mut engine);
    let two = part(&out, "xl/worksheets/sheet2.xml");
    assert!(
        two.contains(r#"<sheetView workbookViewId="0" zoomScale="150" zoomScaleNormal="150"/>"#),
        "{two}"
    );
    assert!(part(&out, "xl/worksheets/sheet3.xml").contains(r#"zoomScale="400""#));
    assert_eq!(
        part(&out, "xl/worksheets/sheet1.xml"),
        part(&source, "xl/worksheets/sheet1.xml")
    );
    let again = opened(out.clone());
    assert_eq!(again.layout(1).unwrap().zoom, 150);
    // Back to 100% says nothing, as Excel writes it.
    let mut engine = opened(out);
    run(&mut engine, r#"{"kind":"zoom","sheet":1,"zoom":100}"#);
    let back = saved(&mut engine);
    assert!(!part(&back, "xl/worksheets/sheet2.xml").contains("zoomScale"));
}

#[test]
fn zoom_in_another_view_is_normal_views() {
    let sheet = r#"<sheetViews><sheetView view="pageBreakPreview" zoomScale="60" zoomScaleNormal="85" workbookViewId="0"/></sheetViews><sheetData/>"#;
    let mut engine = opened(workbook(&[("One", sheet)], VIEWS));
    assert_eq!(engine.layout(0).unwrap().zoom, 85);
    run(&mut engine, r#"{"kind":"zoom","sheet":0,"zoom":120}"#);
    let one = part(&saved(&mut engine), "xl/worksheets/sheet1.xml");
    assert!(
        one.contains(r#"view="pageBreakPreview" zoomScale="60" zoomScaleNormal="120""#),
        "{one}"
    );
}

/// As Excel 2016 saved them over COM: `Protect "abc"` and `Workbook.Protect "wb", True`.
const EXCEL_SHEET_PROTECTION: &str = r#"<sheetProtection algorithmName="SHA-512" hashValue="lE+oY/K8O9RzxhHbfj+59f6OKjW/HyUpDp1tBw9LKbUXQ2NlFMdltyninyJ7dov5g+VPTaGpfgt/Z1zYKcAFyQ==" saltValue="mZgorKXCxX0pnb7n04x0bA==" spinCount="100000" sheet="1" objects="1" scenarios="1"/>"#;
const EXCEL_BOOK_PROTECTION: &str = r#"<workbookProtection workbookAlgorithmName="SHA-512" workbookHashValue="SHy4/btZfFwmRsjQc5i0Tt0EsAatkk5D41w/SBmvMDUwoOD1Pzy3kRvtIUMUbrwoZyeuVxX0qBjbFgpke2OOdg==" workbookSaltValue="C8wlzqnyyPxIKcG6QlN/HQ==" workbookSpinCount="100000" lockStructure="1"/>"#;
const PROTECTED_MESSAGE: &str = "on a protected sheet";

/// A styles part whose second format is unlocked, as Format Cells' Protection tab leaves it.
fn with_unlocked_style(bytes: Vec<u8>) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = Vec::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        let name = file.name().unwrap().to_string();
        if name == "xl/styles.xml" {
            text = text.replace(
                r#"<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>"#,
                r#"<cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyProtection="1"><protection locked="0"/></xf></cellXfs>"#,
            );
        }
        parts.push((name, text));
    }
    zip_of(&parts)
}

fn excel_protected() -> Vec<u8> {
    let sheet = format!(
        r#"<sheetViews><sheetView tabSelected="1" workbookViewId="0"/></sheetViews><sheetData><row r="1"><c r="A1"><v>1</v></c><c r="C1" s="1"><v>5</v></c></row></sheetData>{EXCEL_SHEET_PROTECTION}"#
    );
    with_unlocked_style(workbook(
        &[("One", &sheet), ("Two", PLAIN)],
        &format!("{EXCEL_BOOK_PROTECTION}{VIEWS}"),
    ))
}

#[test]
fn a_sheet_excel_protected_keeps_its_locked_cells_until_its_password_unprotects_it() {
    let mut engine = opened(excel_protected());
    let refused = engine
        .apply_json(r#"{"kind":"input","sheet":0,"row":1,"col":1,"text":"9"}"#)
        .unwrap_err();
    assert!(refused.contains(PROTECTED_MESSAGE), "{refused}");
    // C1 is unlocked; an empty cell has the default format, which is locked.
    run(
        &mut engine,
        r#"{"kind":"input","sheet":0,"row":1,"col":3,"text":"7"}"#,
    );
    assert!(engine
        .apply_json(r#"{"kind":"input","sheet":0,"row":2,"col":3,"text":"7"}"#)
        .is_err());
    for refused in [
        r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":3,"bottom":1,"right":3}],"change":{"bold":true}}"#,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":1,"count":1,"delete":false}"#,
        r#"{"kind":"merge","sheet":0,"areas":[{"top":5,"left":1,"bottom":6,"right":2}]}"#,
        r#"{"kind":"clear","sheet":0,"areas":[{"top":1,"left":1,"bottom":1,"right":3}]}"#,
    ] {
        let error = engine.apply_json(refused).unwrap_err();
        assert!(error.contains(PROTECTED_MESSAGE), "{refused}: {error}");
    }
    // The other sheet is not protected.
    run(
        &mut engine,
        r#"{"kind":"input","sheet":1,"row":1,"col":1,"text":"9"}"#,
    );
    // The structure is locked.
    for refused in [
        r#"{"kind":"insertSheet","position":2}"#,
        r#"{"kind":"renameSheet","sheet":1,"name":"X"}"#,
        r#"{"kind":"sheetState","sheet":1,"hidden":true}"#,
        r#"{"kind":"deleteSheet","sheet":1}"#,
    ] {
        let error = engine.apply_json(refused).unwrap_err();
        assert!(
            error.contains("Workbook is protected"),
            "{refused}: {error}"
        );
    }
    let wrong = engine
        .apply_json(r#"{"kind":"unprotectSheet","sheet":0,"password":"ABC"}"#)
        .unwrap_err();
    assert!(wrong.contains("not correct"), "{wrong}");
    run(
        &mut engine,
        r#"{"kind":"unprotectSheet","sheet":0,"password":"abc"}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"unprotectWorkbook","password":"wb"}"#,
    );
    // Unprotecting leaves nothing to undo, as in Excel.
    assert_eq!(engine.undo_state().undo, None);
    run(
        &mut engine,
        r#"{"kind":"input","sheet":0,"row":1,"col":1,"text":"9"}"#,
    );
    run(&mut engine, r#"{"kind":"insertSheet","position":2}"#);
    let out = saved(&mut engine);
    assert!(!part(&out, "xl/worksheets/sheet1.xml").contains("sheetProtection"));
    assert!(!part(&out, "xl/workbook.xml").contains("workbookProtection"));
}

#[test]
fn protecting_here_saves_excels_hash_and_options() {
    let source = with_unlocked_style(three());
    let mut engine = opened(source);
    run(
        &mut engine,
        r#"{"kind":"protectSheet","sheet":0,"password":"abc","salt":"mZgorKXCxX0pnb7n04x0bA==","allow":{"selectLocked":true,"selectUnlocked":true,"formatCells":true,"insertRows":true,"sort":true}}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"protectWorkbook","structure":true,"password":"wb","salt":"C8wlzqnyyPxIKcG6QlN/HQ=="}"#,
    );
    assert!(engine.has_changes());
    assert_eq!(engine.undo_state().undo, None);
    // What the options allow goes on working.
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":1,"bottom":1,"right":1}],"change":{"bold":true}}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":3,"count":1,"delete":false}"#,
    );
    assert!(engine
        .apply_json(r#"{"kind":"axis","sheet":0,"axis":"row","at":3,"count":1,"delete":true}"#)
        .is_err());
    let out = saved(&mut engine);
    let sheet = part(&out, "xl/worksheets/sheet1.xml");
    // Excel's own text for "abc" with that salt, and its order of options.
    let expected = EXCEL_SHEET_PROTECTION.replace(
        r#" scenarios="1"/>"#,
        r#" scenarios="1" formatCells="0" insertRows="0" sort="0"/>"#,
    );
    assert!(sheet.contains(&expected), "{sheet}");
    let book = part(&out, "xl/workbook.xml");
    assert!(
        book.contains(&format!("{EXCEL_BOOK_PROTECTION}<bookViews>")),
        "{book}"
    );
    // Read back, the password works.
    let mut again = opened(out);
    assert!(again
        .apply_json(r#"{"kind":"input","sheet":0,"row":1,"col":1,"text":"9"}"#)
        .is_err());
    run(
        &mut again,
        r#"{"kind":"unprotectSheet","sheet":0,"password":"abc"}"#,
    );
    run(
        &mut again,
        r#"{"kind":"input","sheet":0,"row":1,"col":1,"text":"9"}"#,
    );
}

#[test]
fn format_cells_unlocks_and_hides_formulas() {
    let mut engine = opened(three());
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":1,"bottom":1,"right":1}],"change":{"locked":false}}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":2,"left":1,"bottom":2,"right":1}],"change":{"formulaHidden":true}}"#,
    );
    let out = saved(&mut engine);
    let styles = part(&out, "xl/styles.xml");
    assert!(
        styles.contains(r#"applyProtection="1"><protection locked="0"/></xf>"#),
        "{styles}"
    );
    assert!(
        styles.contains(r#"applyProtection="1"><protection hidden="1"/></xf>"#),
        "{styles}"
    );
    let mut again = opened(out);
    run(&mut again, r#"{"kind":"protectSheet","sheet":0}"#);
    run(
        &mut again,
        r#"{"kind":"input","sheet":0,"row":1,"col":1,"text":"ok"}"#,
    );
    assert!(again
        .apply_json(r#"{"kind":"input","sheet":0,"row":2,"col":1,"text":"no"}"#)
        .is_err());
    // With no password, Unprotect takes none.
    run(&mut again, r#"{"kind":"unprotectSheet","sheet":0}"#);
}

#[test]
fn a_legacy_password_unprotects() {
    let sheet =
        format!(r#"{PLAIN}<sheetProtection password="CBEB" sheet="1" objects="1" scenarios="1"/>"#);
    let mut engine = opened(workbook(&[("One", &sheet)], VIEWS));
    assert!(engine
        .apply_json(r#"{"kind":"unprotectSheet","sheet":0,"password":"tes"}"#)
        .is_err());
    run(
        &mut engine,
        r#"{"kind":"unprotectSheet","sheet":0,"password":"test"}"#,
    );
}

#[test]
fn show_hide_note_saves_excels_shape_and_an_edit_keeps_its_box() {
    let mut engine = opened(three());
    run(
        &mut engine,
        r#"{"kind":"setNote","sheet":0,"row":2,"col":2,"text":"Ann:\nfirst","author":"Ann"}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"noteVisible","sheet":0,"row":2,"col":2,"visible":true}"#,
    );
    let out = saved(&mut engine);
    let names: Vec<String> = {
        let zip = ZipArchive::new(Cursor::new(out.clone())).unwrap();
        zip.file_names()
            .map(|name| name.unwrap().to_string())
            .collect()
    };
    let vml = names.iter().find(|name| name.ends_with(".vml")).unwrap();
    let shape = part(&out, vml);
    // Excel 2016's anchor for a new note on B2, and its mark for one that shows.
    assert!(
        shape.contains("<x:Anchor>2, 15, 0, 10, 4, 31, 4, 9</x:Anchor>"),
        "{shape}"
    );
    assert!(
        shape.contains("visibility:visible") && shape.contains("<x:Visible/>"),
        "{shape}"
    );
    let comments = names.iter().find(|name| name.contains("comments")).unwrap();
    // The author's name in bold, as Excel's box writes it.
    assert!(part(&out, comments).contains("<b/>"));
    // Read back, it still shows; edited, it keeps showing and keeps its author.
    let mut again = opened(out);
    run(
        &mut again,
        r#"{"kind":"setNote","sheet":0,"row":2,"col":2,"text":"Ann:\nsecond","author":"Bob"}"#,
    );
    let note = &again.book().unwrap().sheets[0].notes[&(2, 2)];
    assert!(note.visible);
    assert_eq!(note.author, "Ann");
    assert_eq!(note.text, "Ann:\nsecond");
    assert_eq!(note.anchor, Some([2, 15, 0, 10, 4, 31, 4, 9]));
    run(
        &mut again,
        r#"{"kind":"noteVisible","sheet":0,"row":2,"col":2,"visible":false}"#,
    );
    assert!(!again.book().unwrap().sheets[0].notes[&(2, 2)].visible);
    undo(&mut again);
    assert!(again.book().unwrap().sheets[0].notes[&(2, 2)].visible);
}

#[test]
fn remove_hyperlink_takes_its_look_too_and_clear_hyperlinks_leaves_it() {
    let mut engine = opened(three());
    let link = |row: u32| {
        format!(
            r#"{{"kind":"setLink","sheet":0,"area":{{"top":{row},"left":2,"bottom":{row},"right":2}},"link":{{"address":"https://example.com/","text":"site"}}}}"#
        )
    };
    run(&mut engine, &link(8));
    run(&mut engine, &link(9));
    let look = |engine: &mut Engine, row: u32| {
        let format = engine.cell_format_json(0, row, 2);
        (
            format.contains(r#""underline":"single""#),
            format.contains(r#""fontColor":{"kind":"theme""#),
        )
    };
    assert_eq!(look(&mut engine, 8), (true, true));
    run(
        &mut engine,
        r#"{"kind":"setLink","sheet":0,"area":{"top":8,"left":2,"bottom":8,"right":2},"plain":true}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"setLink","sheet":0,"area":{"top":9,"left":2,"bottom":9,"right":2}}"#,
    );
    assert!(engine.book().unwrap().sheets[0].links.is_empty());
    assert_eq!(look(&mut engine, 8), (false, false));
    assert_eq!(look(&mut engine, 9), (true, true));
}

/// The sheet Excel 2016 sorted and filtered over COM: N with fills and red text, the same numbers
/// under 3 Arrows and under a rule filling more than 6 green, and an id for each row.
fn colored() -> Engine {
    let mut engine = opened(three());
    let values = [5, 1, 8, 3, 9, 2, 7, 4];
    for (col, head) in ["N", "Arrow", "Rule", "Id"].iter().enumerate() {
        run(
            &mut engine,
            &format!(
                r#"{{"kind":"input","sheet":0,"row":1,"col":{},"text":"{head}"}}"#,
                col + 1
            ),
        );
    }
    for (at, value) in values.iter().enumerate() {
        let row = at + 2;
        for col in 1..=3 {
            run(
                &mut engine,
                &format!(
                    r#"{{"kind":"input","sheet":0,"row":{row},"col":{col},"text":"{value}"}}"#
                ),
            );
        }
        run(
            &mut engine,
            &format!(
                r#"{{"kind":"input","sheet":0,"row":{row},"col":4,"text":"{}"}}"#,
                at + 1
            ),
        );
    }
    let fill = |row: u32, rgb: &str| {
        format!(
            r#"{{"kind":"format","sheet":0,"areas":[{{"top":{row},"left":1,"bottom":{row},"right":1}}],"change":{{"fill":{{"pattern":"solid","color":{{"kind":"rgb","rgb":"{rgb}"}}}}}}}}"#
        )
    };
    let font = |row: u32| {
        format!(
            r#"{{"kind":"format","sheet":0,"areas":[{{"top":{row},"left":1,"bottom":{row},"right":1}}],"change":{{"fontColor":{{"kind":"rgb","rgb":"FF0000"}}}}}}"#
        )
    };
    run(&mut engine, &fill(3, "FFFF00"));
    run(&mut engine, &fill(6, "FFFF00"));
    run(&mut engine, &fill(8, "FF0000"));
    run(&mut engine, &font(5));
    run(&mut engine, &font(7));
    run(
        &mut engine,
        r#"{"kind":"addCf","sheet":0,"areas":[{"top":2,"left":2,"bottom":9,"right":2}],"rule":{"type":"iconSet","set":"3Arrows"}}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"addCf","sheet":0,"areas":[{"top":2,"left":3,"bottom":9,"right":3}],"rule":{"type":"cellIs","op":"greaterThan","formulas":["6"],"format":{"fill":"00B050"}}}"#,
    );
    engine
}

fn ids(engine: &mut Engine) -> String {
    (2..=9)
        .map(|row| engine.display_text(0, row, 4))
        .collect::<Vec<_>>()
        .join(",")
}

fn shown_ids(engine: &mut Engine) -> String {
    let hidden: Vec<bool> = (2..=9)
        .map(|row| {
            engine.book().unwrap().sheets[0]
                .rows
                .info(row)
                .is_some_and(|line| line.hidden)
        })
        .collect();
    (2..=9)
        .zip(hidden)
        .filter(|(_, hidden)| !hidden)
        .map(|(row, _)| engine.display_text(0, row, 4))
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn sorts_by_colour_and_icon_as_excel_2016_did() {
    // What Excel 2016 gave over COM for the same sheet.
    let cases = [
        (
            r#"{"index":1,"by":{"kind":"cellColor","color":"FFFF00"}}"#,
            "2,5,1,3,4,6,7,8",
        ),
        (
            r#"{"index":1,"descending":true,"by":{"kind":"cellColor","color":"FF0000"}}"#,
            "1,2,3,4,5,6,8,7",
        ),
        (
            r#"{"index":1,"by":{"kind":"fontColor","color":"FF0000"}}"#,
            "4,6,1,2,3,5,7,8",
        ),
        (
            r#"{"index":3,"by":{"kind":"cellColor","color":"00B050"}}"#,
            "3,5,7,1,2,4,6,8",
        ),
        (
            r#"{"index":2,"by":{"kind":"icon","set":"3Arrows","id":0}}"#,
            "2,4,6,1,3,5,7,8",
        ),
    ];
    for (key, expected) in cases {
        let mut engine = colored();
        run(
            &mut engine,
            &format!(
                r#"{{"kind":"sort","sheet":0,"area":{{"top":1,"left":1,"bottom":9,"right":4}},"keys":[{key}],"header":true}}"#
            ),
        );
        assert_eq!(ids(&mut engine), expected, "{key}");
    }
}

#[test]
fn filters_by_colour_and_icon_as_excel_2016_did() {
    let cases = [
        (1, r#"{"kind":"color","cell":true,"color":"FFFF00"}"#, "2,5"),
        (
            1,
            r#"{"kind":"color","cell":false,"color":"FF0000"}"#,
            "4,6",
        ),
        (1, r#"{"kind":"color","cell":true}"#, "1,3,4,6,8"),
        (
            3,
            r#"{"kind":"color","cell":true,"color":"00B050"}"#,
            "3,5,7",
        ),
        (2, r#"{"kind":"icon","set":"3Arrows","id":0}"#, "2,4,6"),
    ];
    for (col, criteria, expected) in cases {
        let mut engine = colored();
        run(
            &mut engine,
            r#"{"kind":"toggleFilter","sheet":0,"area":{"top":1,"left":1,"bottom":9,"right":4}}"#,
        );
        run(
            &mut engine,
            &format!(r#"{{"kind":"setFilter","sheet":0,"col":{col},"criteria":{criteria}}}"#),
        );
        assert_eq!(shown_ids(&mut engine), expected, "{criteria}");
        // The filter reads back the same from what it saved.
        let mut again = opened(saved(&mut engine));
        run(&mut again, r#"{"kind":"reapplyFilter","sheet":0}"#);
        assert_eq!(shown_ids(&mut again), expected, "{criteria}, read back");
    }
    // The menu offers the colours and icons a column has.
    let mut engine = colored();
    run(
        &mut engine,
        r#"{"kind":"toggleFilter","sheet":0,"area":{"top":1,"left":1,"bottom":9,"right":4}}"#,
    );
    let list = engine
        .query(r#"{"kind":"filterList","sheet":0,"col":1}"#)
        .unwrap();
    assert!(
        list.contains(r#""fills":[null,"FFFF00","FF0000"]"#),
        "{list}"
    );
    assert!(list.contains(r#""fonts":["000000","FF0000"]"#), "{list}");
    let arrows = engine
        .query(r#"{"kind":"filterList","sheet":0,"col":2}"#)
        .unwrap();
    assert!(arrows.contains(r#""set":"3Arrows""#), "{arrows}");
}

/// The sheet Excel 2016 checked over COM, with what `Range.Errors` reported for each cell.
const ERROR_CASES: &[(&str, &str)] = &[
    ("A5", ""),
    ("B5", ""),
    ("C5", "inconsistentFormula,omittedCells"),
    ("D5", ""),
    ("F1", "evaluateToError"),
    ("F2", ""),
    ("F3", "numberAsText"),
    ("F4", "numberAsText"),
    ("F5", "textDate"),
    ("F6", "textDate"),
    ("F7", ""),
    ("H1", "unlockedFormula"),
    ("H2", "emptyReference"),
    ("B10", ""),
    ("B11", ""),
    ("B12", "inconsistentFormula"),
    ("B13", ""),
    ("B14", ""),
    ("B15", ""),
    ("A21", ""),
    ("B21", ""),
    ("C21", "inconsistentFormula"),
    ("D21", ""),
    ("E21", ""),
    ("J4", "omittedCells"),
];

fn checked() -> Engine {
    let mut engine = opened(three());
    let input = |engine: &mut Engine, cell: &str, text: &str| {
        let (row, col) = cgs_refs::parse_a1(cell).unwrap();
        let text = serde_json::to_string(text).unwrap();
        run(
            engine,
            &format!(r#"{{"kind":"input","sheet":0,"row":{row},"col":{col},"text":{text}}}"#),
        );
    };
    for r in 1..=4 {
        for c in 1..=4 {
            input(
                &mut engine,
                &format!("{}{r}", (b'A' + c as u8 - 1) as char),
                &(r * c).to_string(),
            );
        }
    }
    for (cell, text) in [
        ("A5", "=SUM(A1:A4)"),
        ("B5", "=SUM(B1:B4)"),
        ("C5", "=SUM(C1:C3)"),
        ("D5", "=SUM(D1:D4)"),
        ("F1", "=1/0"),
        ("F2", "#N/A"),
        ("F4", "'456"),
        ("F5", "'1/1/25"),
        ("F6", "=DATEVALUE(\"1/1/25\")"),
        ("F7", "'abc"),
        ("H1", "=A1+1"),
        ("H2", "=Z99+1"),
        ("J1", "1"),
        ("J2", "2"),
        ("J3", "3"),
        ("J4", "=SUM(J2:J3)"),
    ] {
        input(&mut engine, cell, text);
    }
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":3,"left":6,"bottom":3,"right":6}],"change":{"numberFormat":"@"}}"#,
    );
    input(&mut engine, "F3", "123");
    run(
        &mut engine,
        r#"{"kind":"format","sheet":0,"areas":[{"top":1,"left":8,"bottom":1,"right":8}],"change":{"locked":false}}"#,
    );
    for r in 10..=15 {
        input(&mut engine, &format!("A{r}"), &r.to_string());
        input(&mut engine, &format!("B{r}"), &format!("=A{r}*2"));
    }
    input(&mut engine, "B12", "=A12*3");
    input(&mut engine, "B15", "=A15+0");
    for c in 1..=5u8 {
        let letter = (b'A' + c - 1) as char;
        input(&mut engine, &format!("{letter}20"), &c.to_string());
        input(
            &mut engine,
            &format!("{letter}21"),
            &format!("={letter}20*2"),
        );
    }
    input(&mut engine, "C21", "=C20+5");
    settle(&mut engine);
    engine
}

fn settle(engine: &mut Engine) {
    while engine.calc_step(1000, &[]).unwrap() {}
}

fn errors_at(engine: &Engine, cell: &str) -> String {
    let (row, col) = cgs_refs::parse_a1(cell).unwrap();
    let answer = engine
        .query(&format!(
            r#"{{"kind":"cellErrors","sheet":0,"row":{row},"col":{col}}}"#
        ))
        .unwrap();
    let list: Vec<serde_json::Value> = serde_json::from_str(&answer).unwrap();
    list.iter()
        .map(|entry| entry["rule"].as_str().unwrap().to_string())
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn error_checking_finds_what_excel_2016_found() {
    let mut engine = checked();
    // Every rule on, as `Range.Errors` reports them whether or not they are.
    run(&mut engine, r#"{"kind":"errorRules","rules":127}"#);
    for &(cell, expected) in ERROR_CASES {
        assert_eq!(errors_at(&engine, cell), expected, "{cell}");
    }
    // Excel's own rules leave empty references out.
    run(&mut engine, r#"{"kind":"errorRules","rules":63}"#);
    assert_eq!(errors_at(&engine, "H2"), "");
    // Ignore Error is saved as Excel saves it, and read back.
    run(
        &mut engine,
        r#"{"kind":"ignoreError","sheet":0,"row":3,"col":6}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"ignoreError","sheet":0,"row":12,"col":2}"#,
    );
    assert_eq!(errors_at(&engine, "F3"), "");
    let out = saved(&mut engine);
    let sheet = part(&out, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains(r#"<ignoredErrors><ignoredError sqref="F3" numberStoredAsText="1"/><ignoredError sqref="B12" formula="1"/></ignoredErrors>"#),
        "{sheet}"
    );
    let again = opened(out);
    assert_eq!(errors_at(&again, "F3"), "");
    assert_eq!(errors_at(&again, "B12"), "");
    assert_eq!(errors_at(&again, "F4"), "numberAsText");
    // Error Checking goes from cell to cell in reading order, round the sheet.
    let next = |engine: &Engine, row: u32, col: u32, back: bool| {
        engine
            .query(&format!(
                r#"{{"kind":"nextError","sheet":0,"row":{row},"col":{col},"back":{back}}}"#
            ))
            .unwrap()
    };
    assert_eq!(next(&again, 1, 1, false), "[1,6]");
    assert_eq!(next(&again, 1, 6, false), "[1,8]");
    assert_eq!(next(&again, 1, 6, true), "[21,3]");
}

#[test]
fn error_checking_fixes_what_it_finds() {
    let mut engine = checked();
    run(
        &mut engine,
        r#"{"kind":"convertToNumber","sheet":0,"areas":[{"top":4,"left":6,"bottom":4,"right":6}]}"#,
    );
    assert_eq!(errors_at(&engine, "F4"), "");
    assert_eq!(engine.display_text(0, 4, 6), "456");
    run(
        &mut engine,
        r#"{"kind":"includeCells","sheet":0,"row":4,"col":10}"#,
    );
    settle(&mut engine);
    assert_eq!(engine.edit_text(0, 4, 10), "=SUM(J1:J3)");
    assert_eq!(engine.display_text(0, 4, 10), "6");
    assert_eq!(errors_at(&engine, "J4"), "");
}

#[test]
fn circular_references_name_the_loops_cells() {
    let mut engine = opened(three());
    assert_eq!(engine.query(r#"{"kind":"circular"}"#).unwrap(), "[]");
    // As Excel 2016 was given over COM: A1 and B1 read each other.
    run(
        &mut engine,
        r#"{"kind":"input","sheet":0,"row":1,"col":1,"text":"=B1+1"}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"input","sheet":0,"row":1,"col":2,"text":"=A1+1"}"#,
    );
    // A sum over a range on another sheet that holds a formula reading it back.
    run(
        &mut engine,
        r#"{"kind":"input","sheet":1,"row":2,"col":1,"text":"=SUM(Three!A1:A5)"}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"input","sheet":2,"row":3,"col":1,"text":"=Two!A2*2"}"#,
    );
    // Not on a loop: it reads one.
    run(
        &mut engine,
        r#"{"kind":"input","sheet":0,"row":2,"col":1,"text":"=A1"}"#,
    );
    settle(&mut engine);
    assert_eq!(
        engine.query(r#"{"kind":"circular"}"#).unwrap(),
        "[[0,1,1],[0,1,2],[1,2,1],[2,3,1]]"
    );
    // Breaking a loop takes its cells off.
    run(
        &mut engine,
        r#"{"kind":"input","sheet":0,"row":1,"col":2,"text":"5"}"#,
    );
    assert_eq!(
        engine.query(r#"{"kind":"circular"}"#).unwrap(),
        "[[1,2,1],[2,3,1]]"
    );
}

/// One sheet whose A1 shows a rich shared string: "Bold" in bold, then " plain".
fn rich_book() -> Vec<u8> {
    let bytes = workbook(
        &[(
            "One",
            r#"<sheetViews><sheetView tabSelected="1" workbookViewId="0"/></sheetViews><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c></row></sheetData>"#,
        )],
        VIEWS,
    );
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = Vec::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        let name = file.name().unwrap().to_string();
        let text = match name.as_str() {
            "[Content_Types].xml" => text.replace(
                "</Types>",
                r#"<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>"#,
            ),
            "xl/_rels/workbook.xml.rels" => text.replace(
                "</Relationships>",
                &format!(r#"<Relationship Id="rId99" Type="{RELS}/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#),
            ),
            _ => text,
        };
        parts.push((name, text));
    }
    parts.push((
        "xl/sharedStrings.xml".to_string(),
        format!(
            r#"<sst xmlns="{MAIN}" count="1" uniqueCount="1"><si><r><rPr><b/><sz val="11"/></rPr><t>Bold</t></r><r><t xml:space="preserve"> plain</t></r></si></sst>"#
        ),
    ));
    zip_of(&parts)
}

#[test]
fn a_rich_cell_keeps_its_runs_when_pasted_or_moved() {
    let mut engine = opened(rich_book());
    assert_eq!(engine.book().unwrap().sheets[0].rich.get(&(1, 1)), Some(&0));
    run(
        &mut engine,
        r#"{"kind":"paste","fromSheet":0,"from":{"top":1,"left":1,"bottom":1,"right":1},"sheet":0,"to":{"top":1,"left":3,"bottom":2,"right":3}}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"move","fromSheet":0,"from":{"top":1,"left":1,"bottom":1,"right":1},"sheet":0,"row":5,"col":1}"#,
    );
    let rich = &engine.book().unwrap().sheets[0].rich;
    assert_eq!(rich.get(&(1, 3)), Some(&0));
    assert_eq!(rich.get(&(2, 3)), Some(&0));
    assert_eq!(rich.get(&(5, 1)), Some(&0));
    assert_eq!(rich.get(&(1, 1)), None);
    assert_eq!(engine.display_text(0, 5, 1), "Bold plain");
    let out = saved(&mut engine);
    let sheet = part(&out, "xl/worksheets/sheet1.xml");
    for cell in ["C1", "C2", "A5"] {
        assert!(
            sheet.contains(&format!(r#"<c r="{cell}" t="s"><v>0</v></c>"#)),
            "{sheet}"
        );
    }
    let again = opened(out);
    assert_eq!(again.book().unwrap().sheets[0].rich.get(&(5, 1)), Some(&0));
    // Undone, the move puts the rich cell back, and the paste takes its copies away.
    undo(&mut engine);
    undo(&mut engine);
    let rich = &engine.book().unwrap().sheets[0].rich;
    assert_eq!(rich.get(&(1, 1)), Some(&0));
    assert_eq!(rich.get(&(1, 3)), None);
    assert_eq!(rich.get(&(5, 1)), None);
}

#[test]
fn rich_text_typed_in_the_cell_saves_its_runs() {
    let mut engine = opened(rich_book());
    // "H2O is wet": the 2 in subscript, "wet" in bold red.
    run(
        &mut engine,
        r#"{"kind":"richInput","sheet":0,"row":2,"col":1,"text":"H2O is wet","runs":[{"start":0},{"start":1,"font":{"vertAlign":"subscript"}},{"start":2},{"start":7,"font":{"bold":true,"color":"FF0000"}}]}"#,
    );
    assert_eq!(engine.display_text(0, 2, 1), "H2O is wet");
    let runs = engine
        .query(r#"{"kind":"richAt","sheet":0,"row":2,"col":1}"#)
        .unwrap();
    assert!(runs.contains(r#""start":1"#) && runs.contains(r#""vertAlign":"subscript""#), "{runs}");
    // Pasted, the copy is rich too.
    run(
        &mut engine,
        r#"{"kind":"paste","fromSheet":0,"from":{"top":2,"left":1,"bottom":2,"right":1},"sheet":0,"to":{"top":3,"left":1,"bottom":3,"right":1}}"#,
    );
    let out = saved(&mut engine);
    let strings = part(&out, "xl/sharedStrings.xml");
    assert!(
        strings.contains(r#"<si><r><t xml:space="preserve">H</t></r><r><rPr><vertAlign val="subscript"/></rPr><t xml:space="preserve">2</t></r><r><t xml:space="preserve">O is </t></r><r><rPr><b/><color rgb="FFFF0000"/></rPr><t xml:space="preserve">wet</t></r></si>"#),
        "{strings}"
    );
    let mut again = opened(out);
    let info = &again.book().unwrap().sheets[0];
    assert!(info.rich.contains_key(&(2, 1)) && info.rich.contains_key(&(3, 1)));
    let index = info.rich[&(2, 1)];
    let fonts: Vec<_> = again.book().unwrap().rich_strings[&index]
        .iter()
        .map(|run| run.font.as_ref().map(|font| (font.bold, font.vert_align)))
        .collect();
    assert_eq!(
        fonts,
        [
            None,
            Some((false, cgs_model::VertAlign::Subscript)),
            None,
            Some((true, cgs_model::VertAlign::Baseline))
        ]
    );
    assert_eq!(again.display_text(0, 3, 1), "H2O is wet");
}
