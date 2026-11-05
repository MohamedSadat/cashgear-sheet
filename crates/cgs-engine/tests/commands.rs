//! Commands, undo and calculation through the engine, and what a save makes of them.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::rc::Rc;

use cgs_calc::{CalcValue, CellSink, Rect, SheetRect};
use cgs_engine::{Area, Command, Engine, LoadPhase};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

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

fn book(sheet1: &str, sheet2: &str, calc: &str) -> Vec<u8> {
    let types = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
    let root = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{RELS}"><sheets><sheet name="One" sheetId="1" r:id="rId1"/><sheet name="Two" sheetId="2" r:id="rId2"/></sheets><definedNames><definedName name="Total">One!$A$1:$A$3</definedName></definedNames><calcPr calcId="1"{calc}/></workbook>"#
    );
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELS}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{RELS}/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#
    );
    zip_of(&[
        ("[Content_Types].xml", types.to_string()),
        ("_rels/.rels", root),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        (
            "xl/worksheets/sheet1.xml",
            format!(r#"<worksheet xmlns="{MAIN}"><sheetData>{sheet1}</sheetData></worksheet>"#),
        ),
        (
            "xl/worksheets/sheet2.xml",
            format!(r#"<worksheet xmlns="{MAIN}"><sheetData>{sheet2}</sheetData></worksheet>"#),
        ),
    ])
}

const ONE: &str = r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1*2</f><v>2</v></c></row><row r="2"><c r="A2"><v>2</v></c><c r="B2"><f>A2*2</f><v>4</v></c></row><row r="3"><c r="A3"><v>3</v></c><c r="B3"><f>A3*2</f><v>6</v></c></row><row r="4"><c r="A4"><f>SUM(A1:A3)</f><v>6</v></c></row>"#;
const TWO: &str = r#"<row r="1"><c r="A1"><f>One!A3+1</f><v>4</v></c></row>"#;

fn opened(bytes: Vec<u8>) -> Engine {
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).unwrap();
    while engine.load_step(1000.0).unwrap().phase == LoadPhase::Reading {}
    engine
}

/// Both read off Excel: the copied part of a `{=...}` array is one array of its own, whose formula
/// has moved from where the original starts.
#[test]
fn part_of_an_array_copies_as_one_array() {
    let mut engine = opened(cgs_xlsx::blank::blank_workbook().unwrap());
    let run = |engine: &mut Engine, json: String| engine.apply_json(&json);
    for row in 1..=5u32 {
        let json = format!(
            r#"{{"kind":"input","sheet":0,"row":{row},"col":1,"text":"{}"}}"#,
            row * 10
        );
        run(&mut engine, json).unwrap();
    }
    run(
        &mut engine,
        r#"{"kind":"arrayInput","sheet":0,"area":{"top":2,"left":3,"bottom":4,"right":3},"text":"=A2:A4*2"}"#.to_string(),
    )
    .unwrap();
    settle(&mut engine);
    assert_eq!(engine.display_text(0, 3, 3), "60");

    for (row, text) in [(10, "1"), (11, "2")] {
        let json = format!(r#"{{"kind":"input","sheet":0,"row":{row},"col":3,"text":"{text}"}}"#);
        run(&mut engine, json).unwrap();
    }
    // C3:C4 of the array at C2:C4 goes to E10: `{=C10:C12*2}`, moved by 8 rows and 2 columns from C2.
    run(
        &mut engine,
        r#"{"kind":"paste","fromSheet":0,"from":{"top":3,"left":3,"bottom":4,"right":3},"sheet":0,"to":{"top":10,"left":5,"bottom":11,"right":5}}"#.to_string(),
    )
    .unwrap();
    settle(&mut engine);
    assert_eq!(
        engine.formula_text(0, 10, 5).as_deref(),
        Some("C10:C12 * 2")
    );

    // The two cells read the first two of what it comes to, not the first one twice.
    assert_eq!(engine.display_text(0, 10, 5), "2");
    assert_eq!(engine.display_text(0, 11, 5), "4");

    // Filling across makes one array for each column.
    run(
        &mut engine,
        r#"{"kind":"fill","sheet":0,"from":{"top":3,"left":3,"bottom":4,"right":3},"to":{"top":3,"left":3,"bottom":4,"right":4}}"#.to_string(),
    )
    .unwrap();
    settle(&mut engine);
    assert_eq!(engine.formula_text(0, 3, 4).as_deref(), Some("B3:B5 * 2"));
}

/// Someone carrying on typing while a calculation runs, a slice at a time, and then changing what
/// every formula reads. However many commands went in between, each change has to reach them all.
#[test]
fn edits_during_a_calculation_never_lose_a_later_one() {
    let mut engine = opened(cgs_xlsx::blank::blank_workbook().unwrap());
    const ROWS: u32 = 3_000;
    let input = |row: u32, col: u32, text: &str| {
        format!(r#"{{"kind":"input","sheet":0,"row":{row},"col":{col},"text":"{text}"}}"#)
    };
    engine.apply_json(&input(1, 7, "1")).unwrap();
    for row in 1..=ROWS {
        engine
            .apply_json(&input(row, 1, &(row % 1000).to_string()))
            .unwrap();
    }
    engine.apply_json(&input(1, 2, "=A1*$G$1+1")).unwrap();
    engine
        .apply_json(&format!(
            r#"{{"kind":"fill","sheet":0,"from":{{"top":1,"left":2,"bottom":1,"right":2}},"to":{{"top":1,"left":2,"bottom":{ROWS},"right":2}}}}"#
        ))
        .unwrap();
    settle(&mut engine);
    let mut spare = 0u32;
    for round in 0..6u32 {
        let value = round + 2;
        engine.apply_json(&input(1, 7, &value.to_string())).unwrap();
        // The second round is the long one, as it was in the app.
        let commands = if round == 1 { 4000 } else { 40 };
        for command in 0..commands {
            let _ = engine.calc_step(0, &[]).unwrap();
            spare += 1;
            engine
                .apply_json(&input(2 + command % 200, 8 + round, &spare.to_string()))
                .unwrap();
        }
        settle(&mut engine);
        for row in (1..=ROWS).step_by(997) {
            let expected = u64::from(row % 1000) * u64::from(value) + 1;
            assert_eq!(
                engine.display_text(0, row, 2),
                expected.to_string(),
                "row {row} of round {round}"
            );
        }
    }
}

fn settle(engine: &mut Engine) {
    while engine.calc_step(1000, &[]).unwrap() {}
    engine.take_dirty();
}

#[derive(Default)]
struct Grid(HashMap<(u32, u32), CalcValue>);

impl CellSink for Grid {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.insert((row, col), value.clone());
    }
}

fn cells(engine: &Engine, sheet: u32) -> HashMap<(u32, u32), CalcValue> {
    let mut grid = Grid::default();
    engine.read_cells(
        SheetRect {
            sheet,
            rect: Rect {
                top: 1,
                left: 1,
                bottom: 40,
                right: 12,
            },
        },
        &mut grid,
    );
    grid.0
}

fn num(engine: &Engine, sheet: u32, row: u32, col: u32) -> Option<f64> {
    match cells(engine, sheet).get(&(row, col)) {
        Some(CalcValue::Number(number)) => Some(*number),
        other => panic!("{row},{col}: {other:?}"),
    }
}

fn typed(engine: &mut Engine, sheet: u32, row: u32, col: u32, text: &str) {
    engine
        .apply(Command::Input {
            sheet,
            row,
            col,
            text: text.to_string(),
            areas: Vec::new(),
        })
        .unwrap();
    settle(engine);
}

#[test]
fn a_csv_is_part_of_what_a_save_writes() {
    let mut engine = opened(cgs_xlsx::blank::blank_workbook().unwrap());
    engine.load_csv("a,1\r\nb,=1+1\r\n").unwrap();
    assert!(engine.has_changes());
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    let mut again = opened(out);
    assert_eq!(again.display_text(0, 2, 1), "b");
    assert_eq!(again.display_text(0, 2, 2), "2");
}

fn axis(engine: &mut Engine, json: &str) {
    engine.apply_json(json).unwrap();
    settle(engine);
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

fn has_part(bytes: &[u8], name: &str) -> bool {
    ZipArchive::new(Cursor::new(bytes))
        .unwrap()
        .by_name(name)
        .is_ok()
}

fn undo(engine: &mut Engine) {
    engine.apply(Command::Undo).unwrap();
    settle(engine);
}

fn redo(engine: &mut Engine) {
    engine.apply(Command::Redo).unwrap();
    settle(engine);
}

#[test]
fn typing_calculates_and_undo_and_redo_go_back_and_forth() {
    let mut engine = opened(book(ONE, TWO, ""));
    typed(&mut engine, 0, 1, 1, "10");
    assert_eq!(num(&engine, 0, 1, 2), Some(20.0));
    assert_eq!(num(&engine, 0, 4, 1), Some(15.0));
    assert!(engine.has_changes());
    undo(&mut engine);
    assert_eq!(num(&engine, 0, 1, 1), Some(1.0));
    assert_eq!(num(&engine, 0, 1, 2), Some(2.0));
    assert!(!engine.has_changes());
    redo(&mut engine);
    assert_eq!(num(&engine, 0, 1, 2), Some(20.0));
    assert!(engine.has_changes());
    assert_eq!(engine.undo_state().undo, Some("typing"));
}

#[test]
fn undoing_everything_saves_the_file_as_it_was() {
    let source = book(ONE, TWO, "");
    let mut engine = opened(source.clone());
    typed(&mut engine, 0, 1, 1, "10");
    typed(&mut engine, 0, 5, 1, "=SUM(A1:A4)");
    axis(
        &mut engine,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":2,"count":1,"delete":false}"#,
    );
    undo(&mut engine);
    undo(&mut engine);
    undo(&mut engine);
    assert!(!engine.has_changes());
    assert_eq!(saved(&mut engine), source);
}

#[test]
fn a_new_edit_after_undo_drops_redo() {
    let mut engine = opened(book(ONE, TWO, ""));
    typed(&mut engine, 0, 1, 1, "10");
    undo(&mut engine);
    typed(&mut engine, 0, 1, 1, "20");
    let state = engine.undo_state();
    assert_eq!(state.redo, None);
    redo(&mut engine);
    assert_eq!(num(&engine, 0, 1, 1), Some(20.0));
}

#[test]
fn undo_holds_at_most_a_hundred_steps() {
    let mut engine = opened(book(ONE, TWO, ""));
    for number in 0..110 {
        typed(&mut engine, 0, 6, 1, &number.to_string());
    }
    let mut steps = 0;
    while engine.undo_state().undo.is_some() {
        engine.apply(Command::Undo).unwrap();
        steps += 1;
    }
    assert_eq!(steps, 100);
    settle(&mut engine);
    assert_eq!(num(&engine, 0, 6, 1), Some(9.0));
}

#[test]
fn undo_works_across_a_save() {
    let mut engine = opened(book(ONE, TWO, ""));
    typed(&mut engine, 0, 1, 1, "10");
    let file = saved(&mut engine);
    engine.commit_saved(Rc::new(file)).unwrap();
    assert!(!engine.has_changes());
    undo(&mut engine);
    assert_eq!(num(&engine, 0, 1, 1), Some(1.0));
    assert!(engine.has_changes());
    let file = saved(&mut engine);
    assert!(part(&file, "xl/worksheets/sheet1.xml").contains(r#"<c r="A1"><v>1</v></c>"#));
    engine.commit_saved(Rc::new(file)).unwrap();
    redo(&mut engine);
    assert_eq!(num(&engine, 0, 1, 2), Some(20.0));
    assert!(engine.has_changes());
    let file = saved(&mut engine);
    let reopened = opened(file);
    assert_eq!(num(&reopened, 0, 1, 1), Some(10.0));
    assert_eq!(num(&reopened, 0, 1, 2), Some(20.0));
}

#[test]
fn typing_a_date_gives_the_cell_a_format_once() {
    let mut engine = opened(book(ONE, TWO, ""));
    typed(&mut engine, 0, 7, 1, "1/2/2020");
    typed(&mut engine, 0, 8, 1, "1/3/2020");
    assert_eq!(num(&engine, 0, 7, 1), Some(43_832.0));
    let text = engine.display_text(0, 7, 1);
    assert!(text.contains("2020"), "{text}");
    let file = saved(&mut engine);
    let styles = part(&file, "xl/styles.xml");
    assert_eq!(styles.matches("numFmtId=\"14\"").count(), 1, "{styles}");
}

#[test]
fn a_bad_formula_is_refused_and_an_open_bracket_is_closed() {
    let mut engine = opened(book(ONE, TWO, ""));
    let bad = engine.apply(Command::Input {
        sheet: 0,
        row: 6,
        col: 1,
        text: "=1+)".to_string(),
        areas: Vec::new(),
    });
    assert!(bad.is_err());
    assert_eq!(engine.undo_state().undo, None);
    typed(&mut engine, 0, 6, 1, "=SUM(A1:A3");
    assert_eq!(num(&engine, 0, 6, 1), Some(6.0));
}

#[test]
fn deleting_rows_and_undoing_brings_formulas_back_exactly() {
    let mut engine = opened(book(ONE, TWO, ""));
    while engine.prepare_step().unwrap() {}
    settle(&mut engine);
    let before_sum = engine.formula_text(0, 4, 1);
    let before_two = engine.formula_text(1, 1, 1);
    axis(
        &mut engine,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":2,"count":2,"delete":true}"#,
    );
    assert_eq!(engine.formula_text(1, 1, 1).as_deref(), Some("#REF! + 1"));
    assert_eq!(num(&engine, 0, 2, 1), Some(1.0));
    undo(&mut engine);
    assert_eq!(engine.formula_text(0, 4, 1), before_sum);
    assert_eq!(engine.formula_text(1, 1, 1), before_two);
    assert_eq!(num(&engine, 0, 3, 1), Some(3.0));
    assert_eq!(num(&engine, 1, 1, 1), Some(4.0));
    redo(&mut engine);
    assert_eq!(engine.formula_text(1, 1, 1).as_deref(), Some("#REF! + 1"));
}

#[test]
fn inserting_rows_saves_moved_cells_and_formulas() {
    let mut engine = opened(book(ONE, TWO, ""));
    axis(
        &mut engine,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":2,"count":1,"delete":false}"#,
    );
    typed(&mut engine, 0, 2, 1, "100");
    assert_eq!(num(&engine, 0, 5, 1), Some(106.0));
    assert_eq!(num(&engine, 1, 1, 1), Some(4.0));
    let file = saved(&mut engine);
    let one = part(&file, "xl/worksheets/sheet1.xml");
    assert!(one.contains(r#"<c r="A3"><v>2</v></c>"#), "{one}");
    assert!(one.contains(r#"<f>A3*2</f>"#), "{one}");
    assert!(one.contains(r#"<f>SUM(A1:A4)</f><v>106</v>"#), "{one}");
    let two = part(&file, "xl/worksheets/sheet2.xml");
    assert!(two.contains("<f>One!A4+1</f>"), "{two}");
    let workbook = part(&file, "xl/workbook.xml");
    assert!(workbook.contains("One!$A$1:$A$4"), "{workbook}");
    let reopened = opened(file);
    assert_eq!(num(&reopened, 0, 5, 1), Some(106.0));
    assert_eq!(reopened.formula_text(1, 1, 1).as_deref(), Some("One!A4+1"));
}

#[test]
fn deleting_columns_saves_too() {
    let mut engine = opened(book(ONE, TWO, ""));
    axis(
        &mut engine,
        r#"{"kind":"axis","sheet":0,"axis":"col","at":1,"count":1,"delete":true}"#,
    );
    let file = saved(&mut engine);
    let one = part(&file, "xl/worksheets/sheet1.xml");
    assert!(one.contains(r#"<c r="A1" t="e"><f>#REF!*2</f>"#), "{one}");
    assert!(!one.contains(r#"<c r="B1""#), "{one}");
    let two = part(&file, "xl/worksheets/sheet2.xml");
    assert!(two.contains("<f>One!#REF!+1</f>"), "{two}");
    let reopened = opened(file);
    assert_eq!(
        cells(&reopened, 0).get(&(1, 1)),
        Some(&CalcValue::Error("#REF!".to_string()))
    );
}

#[test]
fn sheets_can_be_added_renamed_and_deleted_and_undone() {
    let mut engine = opened(book(ONE, TWO, ""));
    engine
        .apply_json(r#"{"kind":"insertSheet","position":1,"name":"Mid"}"#)
        .unwrap();
    typed(&mut engine, 2, 1, 1, "=One!A1+41");
    assert_eq!(num(&engine, 2, 1, 1), Some(42.0));
    engine
        .apply_json(r#"{"kind":"renameSheet","sheet":0,"name":"First Sheet"}"#)
        .unwrap();
    settle(&mut engine);
    assert_eq!(
        engine.formula_text(1, 1, 1).as_deref(),
        Some("'First Sheet'!A3 + 1")
    );
    let file = saved(&mut engine);
    assert!(has_part(&file, "xl/worksheets/sheet3.xml"));
    let workbook = part(&file, "xl/workbook.xml");
    let first = workbook.find("First Sheet").unwrap();
    let mid = workbook.find("\"Mid\"").unwrap();
    let two = workbook.find("\"Two\"").unwrap();
    assert!(first < mid && mid < two, "{workbook}");
    assert!(workbook.contains("'First Sheet'!$A$1:$A$3"), "{workbook}");
    assert!(part(&file, "[Content_Types].xml").contains("/xl/worksheets/sheet3.xml"));
    assert!(part(&file, "xl/_rels/workbook.xml.rels").contains("worksheets/sheet3.xml"));
    let reopened = opened(file.clone());
    assert_eq!(num(&reopened, 1, 1, 1), Some(42.0));
    assert_eq!(
        reopened.book().unwrap().sheets[1].name,
        "Mid",
        "tabs keep their order"
    );

    engine
        .apply_json(r#"{"kind":"deleteSheet","sheet":1}"#)
        .unwrap();
    settle(&mut engine);
    let file = saved(&mut engine);
    assert!(!has_part(&file, "xl/worksheets/sheet2.xml"));
    assert!(!part(&file, "[Content_Types].xml").contains("/xl/worksheets/sheet2.xml"));
    assert!(!part(&file, "xl/_rels/workbook.xml.rels").contains("worksheets/sheet2.xml"));
    assert!(!part(&file, "xl/workbook.xml").contains("\"Two\""));
    undo(&mut engine);
    assert_eq!(num(&engine, 1, 1, 1), Some(4.0));
}

#[test]
fn the_last_visible_sheet_stays_and_names_follow_excels_rules() {
    let mut engine = opened(book(ONE, TWO, ""));
    for name in ["", "a/b", "two", "'x", "History"] {
        let json = format!(r#"{{"kind":"renameSheet","sheet":0,"name":"{name}"}}"#);
        assert!(engine.apply_json(&json).is_err(), "{name}");
    }
    engine
        .apply_json(r#"{"kind":"deleteSheet","sheet":0}"#)
        .unwrap();
    assert!(engine
        .apply_json(r#"{"kind":"deleteSheet","sheet":1}"#)
        .is_err());
}

#[test]
fn a_formula_that_spills_is_saved_as_a_dynamic_array() {
    let mut engine = opened(book(ONE, TWO, ""));
    typed(&mut engine, 0, 10, 1, "=SEQUENCE(3)");
    assert_eq!(num(&engine, 0, 12, 1), Some(3.0));
    let file = saved(&mut engine);
    let one = part(&file, "xl/worksheets/sheet1.xml");
    assert!(
        one.contains(r#"<f t="array" ref="A10:A12">_xlfn.SEQUENCE(3)</f>"#),
        "{one}"
    );
    assert!(one.contains(r#"<c r="A10" cm="1">"#), "{one}");
    assert!(one.contains(r#"<c r="A12"><v>3</v></c>"#), "{one}");
    let metadata = part(&file, "xl/metadata.xml");
    assert!(metadata.contains("XLDAPR"));
    assert!(part(&file, "[Content_Types].xml").contains("sheetMetadata"));
    assert!(part(&file, "xl/_rels/workbook.xml.rels").contains("sheetMetadata"));
    let reopened = opened(file);
    assert_eq!(num(&reopened, 0, 12, 1), Some(3.0));
}

#[test]
fn a_full_calc_file_calculates_once_ready_and_a_saved_value_otherwise_stands() {
    // Wrong on purpose: only calculating can change it.
    let wrong = r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1*2</f><v>99</v></c></row>"#;
    let mut plain = opened(book(wrong, TWO, ""));
    while plain.prepare_step().unwrap() {}
    settle(&mut plain);
    assert_eq!(num(&plain, 0, 1, 2), Some(99.0));
    assert!(!plain.has_changes());

    let mut asked = opened(book(wrong, TWO, r#" fullCalcOnLoad="1""#));
    while asked.prepare_step().unwrap() {}
    settle(&mut asked);
    assert_eq!(num(&asked, 0, 1, 2), Some(2.0));
    assert!(!asked.has_changes());

    let no_value = r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1*2</f></c></row>"#;
    let mut empty = opened(book(no_value, TWO, ""));
    while empty.prepare_step().unwrap() {}
    settle(&mut empty);
    assert_eq!(num(&empty, 0, 1, 2), Some(2.0));
}

#[test]
fn volatile_functions_come_up_to_date_after_opening_and_a_manual_file_waits() {
    let sheet = r#"<row r="1"><c r="A1"><f>ROW()+NOW()*0+RAND()*0</f><v>99</v></c><c r="B1"><f>A1+1</f><v>100</v></c></row>"#;
    let mut engine = opened(book(sheet, TWO, ""));
    while engine.prepare_step().unwrap() {}
    settle(&mut engine);
    assert_eq!(num(&engine, 0, 1, 1), Some(1.0));
    assert_eq!(num(&engine, 0, 1, 2), Some(2.0));
    assert!(!engine.has_changes());

    let mut manual = opened(book(sheet, TWO, r#" calcMode="manual""#));
    while manual.prepare_step().unwrap() {}
    settle(&mut manual);
    assert!(manual.is_manual());
    assert_eq!(num(&manual, 0, 1, 1), Some(99.0));
    typed(&mut manual, 0, 2, 1, "=1+1");
    assert_eq!(cells(&manual, 0).get(&(2, 1)), None);
    assert!(manual.is_calculating());
    manual.calculate_now(false).unwrap();
    settle(&mut manual);
    assert_eq!(num(&manual, 0, 2, 1), Some(2.0));
    assert!(!manual.is_calculating());
}

#[test]
fn turning_calculation_to_manual_is_saved() {
    let mut engine = opened(book(ONE, TWO, ""));
    engine.set_manual(true);
    assert!(engine.has_changes());
    let file = saved(&mut engine);
    assert!(part(&file, "xl/workbook.xml").contains(r#"calcMode="manual""#));
}

#[test]
fn filling_carries_on_a_series_and_shifts_formulas() {
    let mut engine = opened(book(ONE, TWO, ""));
    let area = |top, left, bottom, right| Area {
        top,
        left,
        bottom,
        right,
    };
    engine
        .apply(Command::Fill {
            sheet: 0,
            from: area(1, 1, 2, 2),
            to: area(1, 1, 5, 2),
            copy: false,
        })
        .unwrap();
    settle(&mut engine);
    assert_eq!(num(&engine, 0, 5, 1), Some(5.0));
    assert_eq!(engine.formula_text(0, 5, 2).as_deref(), Some("A5 * 2"));
    assert_eq!(num(&engine, 0, 5, 2), Some(10.0));
    undo(&mut engine);
    assert_eq!(engine.formula_text(0, 4, 1).as_deref(), Some("SUM(A1:A3)"));
    assert_eq!(cells(&engine, 0).get(&(5, 1)), None);
}

#[test]
fn pasting_cells_shifts_formulas_and_goes_ref_off_the_sheet() {
    let mut engine = opened(book(ONE, TWO, ""));
    engine
        .apply_json(r#"{"kind":"paste","fromSheet":0,"from":{"top":1,"left":2,"bottom":1,"right":2},"sheet":0,"to":{"top":6,"left":3,"bottom":6,"right":3}}"#)
        .unwrap();
    settle(&mut engine);
    assert_eq!(engine.formula_text(0, 6, 3).as_deref(), Some("B6 * 2"));
    engine
        .apply_json(r#"{"kind":"paste","fromSheet":0,"from":{"top":2,"left":2,"bottom":2,"right":2},"sheet":0,"to":{"top":1,"left":1,"bottom":1,"right":1}}"#)
        .unwrap();
    settle(&mut engine);
    assert_eq!(engine.formula_text(0, 1, 1).as_deref(), Some("#REF! * 2"));
}

#[test]
fn a_paste_fills_a_destination_that_holds_it_a_whole_number_of_times() {
    let mut engine = opened(book(ONE, TWO, ""));
    engine
        .apply_json(r#"{"kind":"paste","fromSheet":0,"from":{"top":1,"left":1,"bottom":1,"right":1},"sheet":1,"to":{"top":3,"left":1,"bottom":5,"right":2},"mode":"values"}"#)
        .unwrap();
    settle(&mut engine);
    assert_eq!(num(&engine, 1, 5, 2), Some(1.0));
}

#[test]
fn moving_cells_takes_what_points_at_them_along() {
    let mut engine = opened(book(ONE, TWO, ""));
    engine
        .apply_json(r#"{"kind":"move","fromSheet":0,"from":{"top":1,"left":1,"bottom":3,"right":1},"sheet":0,"row":1,"col":5}"#)
        .unwrap();
    settle(&mut engine);
    assert_eq!(cells(&engine, 0).get(&(1, 1)), None);
    assert_eq!(num(&engine, 0, 3, 5), Some(3.0));
    assert_eq!(engine.formula_text(0, 1, 2).as_deref(), Some("E1 * 2"));
    assert_eq!(engine.formula_text(0, 4, 1).as_deref(), Some("SUM(E1:E3)"));
    assert_eq!(engine.formula_text(1, 1, 1).as_deref(), Some("One!E3 + 1"));
    assert_eq!(num(&engine, 1, 1, 1), Some(4.0));
    undo(&mut engine);
    assert_eq!(num(&engine, 0, 3, 1), Some(3.0));
    assert_eq!(engine.formula_text(1, 1, 1).as_deref(), Some("One!A3 + 1"));
}

#[test]
fn clearing_keeps_formats_and_can_be_undone() {
    let mut engine = opened(book(ONE, TWO, ""));
    engine
        .apply_json(
            r#"{"kind":"clear","sheet":0,"areas":[{"top":1,"left":1,"bottom":1048576,"right":1}]}"#,
        )
        .unwrap();
    settle(&mut engine);
    assert_eq!(cells(&engine, 0).get(&(1, 1)), None);
    assert_eq!(cells(&engine, 0).get(&(4, 1)), None);
    undo(&mut engine);
    assert_eq!(num(&engine, 0, 1, 1), Some(1.0));
    assert_eq!(engine.formula_text(0, 4, 1).as_deref(), Some("SUM(A1:A3)"));
}

#[test]
fn pasting_excels_html_keeps_numbers_and_looks() {
    let mut engine = opened(book(ONE, TWO, ""));
    let html = r#"<html><head><style>.xl65 {mso-number-format:"0\.00"; font-weight:700; background:#FFFF00; text-align:center;}</style></head><body><table><tr><td class=xl65 x:num="1.5">1.50</td><td>text</td></tr><tr><td colspan=2 x:str>2,5</td></tr></table></body></html>"#;
    engine
        .apply(Command::PasteExternal {
            sheet: 1,
            row: 5,
            col: 1,
            text: "1.50\ttext\n2,5\n".to_string(),
            html: Some(html.to_string()),
        })
        .unwrap();
    settle(&mut engine);
    assert_eq!(num(&engine, 1, 5, 1), Some(1.5));
    assert_eq!(engine.display_text(1, 5, 1), "1.50");
    assert_eq!(
        cells(&engine, 1).get(&(6, 1)),
        Some(&CalcValue::Text("2,5".to_string()))
    );
    let file = saved(&mut engine);
    let two = part(&file, "xl/worksheets/sheet2.xml");
    assert!(part(&file, "xl/styles.xml").contains("FFFFFF00"));
    assert!(part(&file, "xl/styles.xml").contains("horizontal=\"center\""));
    assert!(two.contains(r#"<mergeCell ref="A6:B6"/>"#), "{two}");
    let reopened = opened(file);
    assert_eq!(reopened.book().unwrap().sheets[1].merges.len(), 1);
    undo(&mut engine);
    assert_eq!(cells(&engine, 1).get(&(5, 1)), None);
    assert!(engine.book().unwrap().sheets[1].merges.is_empty());
}

#[test]
fn pasting_plain_text_reads_each_cell_as_typed() {
    let mut engine = opened(book(ONE, TWO, ""));
    engine
        .apply(Command::PasteExternal {
            sheet: 1,
            row: 3,
            col: 1,
            text: "1\t=1+1\r\n\"a\nb\"\t50%\r\n".to_string(),
            html: None,
        })
        .unwrap();
    settle(&mut engine);
    assert_eq!(num(&engine, 1, 3, 1), Some(1.0));
    assert_eq!(num(&engine, 1, 3, 2), Some(2.0));
    assert_eq!(
        cells(&engine, 1).get(&(4, 1)),
        Some(&CalcValue::Text("a\nb".to_string()))
    );
    assert_eq!(num(&engine, 1, 4, 2), Some(0.5));
    assert_eq!(engine.display_text(1, 4, 2), "50%");
}

#[test]
fn csv_comes_in_as_typed_and_goes_out_as_shown() {
    let mut engine = opened(book("", "", ""));
    engine
        .load_csv("\u{feff}name;amount\nقلم;1,5\n\"a;b\";50%\n")
        .unwrap();
    settle(&mut engine);
    assert_eq!(
        cells(&engine, 0).get(&(2, 1)),
        Some(&CalcValue::Text("قلم".to_string()))
    );
    assert_eq!(num(&engine, 0, 3, 2), Some(0.5));
    let out = engine.csv_text(0);
    assert!(out.starts_with('\u{feff}'));
    assert!(
        out.contains("\"a;b\",50%\r\n") || out.contains("a;b,50%\r\n"),
        "{out}"
    );
    assert!(out.contains("قلم"), "{out}");
}

#[test]
fn copying_gives_text_and_html_with_exact_numbers() {
    let mut engine = opened(book(ONE, TWO, ""));
    let (text, html) = engine.copy_text(
        0,
        Area {
            top: 1,
            left: 1,
            bottom: 2,
            right: 2,
        },
    );
    assert_eq!(text, "1\t2\r\n2\t4\r\n");
    assert!(html.contains(r#"<td x:num="1">1</td>"#), "{html}");
}

#[test]
fn changes_say_which_tiles_to_draw_again() {
    let mut engine = opened(book(ONE, TWO, ""));
    engine.take_dirty();
    typed_no_settle(&mut engine, 0, 100, 1, "5");
    let dirty = engine.take_dirty();
    assert!(dirty.tiles.contains(&(0, 64, 0)), "{dirty:?}");
    axis(
        &mut engine,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":1,"count":1,"delete":false}"#,
    );
    typed_no_settle(&mut engine, 0, 100, 1, "5");
    let dirty = engine.take_dirty();
    assert!(dirty.tiles.contains(&(0, 64, 0)), "{dirty:?}");
    assert!(dirty.layouts.is_empty());
    engine
        .apply_json(r#"{"kind":"axis","sheet":0,"axis":"row","at":1,"count":1,"delete":false}"#)
        .unwrap();
    let dirty = engine.take_dirty();
    assert!(
        dirty.sheets.contains(&0) && dirty.layouts.contains(&0),
        "{dirty:?}"
    );
}

fn typed_no_settle(engine: &mut Engine, sheet: u32, row: u32, col: u32, text: &str) {
    engine
        .apply(Command::Input {
            sheet,
            row,
            col,
            text: text.to_string(),
            areas: Vec::new(),
        })
        .unwrap();
}
