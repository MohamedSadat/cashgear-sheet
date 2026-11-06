//! Sorting and filtering through the engine's commands, each case read off Excel 2016 over COM.

use std::collections::HashMap;
use std::rc::Rc;

use cgs_calc::{CalcValue, CellSink, Rect, SheetRect};
use cgs_engine::{Command, Engine, LoadPhase};

fn opened() -> Engine {
    let mut engine = Engine::new();
    engine
        .open(Rc::new(cgs_xlsx::blank::blank_workbook().unwrap()))
        .unwrap();
    while engine.load_step(1000.0).unwrap().phase == LoadPhase::Reading {}
    engine
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

fn value(engine: &Engine, row: u32, col: u32) -> CalcValue {
    let mut grid = Grid::default();
    engine.read_cells(
        SheetRect {
            sheet: 0,
            rect: Rect {
                top: row,
                left: col,
                bottom: row,
                right: col,
            },
        },
        &mut grid,
    );
    grid.0.remove(&(row, col)).unwrap_or(CalcValue::Empty)
}

fn text(engine: &Engine, row: u32, col: u32) -> String {
    match value(engine, row, col) {
        CalcValue::Text(text) => text,
        CalcValue::Number(number) => number.to_string(),
        CalcValue::Empty => String::new(),
        other => format!("{other:?}"),
    }
}

fn typed(engine: &mut Engine, row: u32, col: u32, text: &str) {
    engine
        .apply(Command::Input {
            sheet: 0,
            row,
            col,
            text: text.to_string(),
            areas: Vec::new(),
        })
        .unwrap();
    settle(engine);
}

fn run(engine: &mut Engine, json: &str) {
    engine.apply_json(json).unwrap();
    settle(engine);
}

fn sort(engine: &mut Engine, area: &str, keys: &str, header: bool) {
    run(
        engine,
        &format!(r#"{{"kind":"sort","sheet":0,"area":{area},"keys":{keys},"header":{header}}}"#),
    );
}

fn column(engine: &Engine, col: u32, rows: u32) -> Vec<String> {
    (1..=rows).map(|row| text(engine, row, col)).collect()
}

#[test]
fn mixed_values_sort_numbers_text_logicals_errors_then_blanks() {
    let mut engine = opened();
    for (row, typed_text) in [
        "5", "x", "TRUE", "=1/0", "", "2", "a", "FALSE", "=NA()", "-3", "'TRUE",
    ]
    .iter()
    .enumerate()
    {
        let row = row as u32 + 1;
        if !typed_text.is_empty() {
            typed(&mut engine, row, 1, typed_text);
        }
        typed(&mut engine, row, 2, &row.to_string());
    }
    let area = r#"{"top":1,"left":1,"bottom":11,"right":2}"#;
    sort(&mut engine, area, r#"[{"index":1}]"#, false);
    assert_eq!(
        column(&engine, 2, 11),
        ["10", "6", "1", "7", "11", "2", "8", "3", "4", "9", "5"]
    );
    sort(
        &mut engine,
        area,
        r#"[{"index":1,"descending":true}]"#,
        false,
    );
    assert_eq!(
        column(&engine, 2, 11),
        ["4", "9", "3", "8", "2", "11", "7", "1", "6", "10", "5"]
    );
}

#[test]
fn hidden_rows_stay_and_formulas_move_as_copies() {
    let mut engine = opened();
    for row in 1..=6u32 {
        typed(&mut engine, row, 1, &(7 - row).to_string());
        typed(&mut engine, row, 2, &format!("=A{row}*10"));
        typed(&mut engine, row, 4, &format!("=SUM(A$1:A{row})"));
    }
    run(
        &mut engine,
        r#"{"kind":"lines","sheet":0,"axis":"row","first":3,"last":3,"hidden":true}"#,
    );
    sort(
        &mut engine,
        r#"{"top":1,"left":1,"bottom":6,"right":4}"#,
        r#"[{"index":1}]"#,
        false,
    );
    assert_eq!(column(&engine, 1, 6), ["1", "2", "4", "3", "5", "6"]);
    assert_eq!(
        engine.formula_text(0, 1, 2).unwrap().replace(' ', ""),
        "A1*10"
    );
    assert_eq!(
        engine.formula_text(0, 1, 4).unwrap().replace(' ', ""),
        "SUM(A$1:A1)"
    );
    assert_eq!(text(&engine, 2, 2), "20");
}

#[test]
fn a_header_stays_and_several_keys_and_lists_order_the_rest() {
    let mut engine = opened();
    let rows = [
        ("Name", "Month"),
        ("b", "Mar"),
        ("a", "Jan"),
        ("b", "Jan"),
        ("a", "Feb"),
    ];
    for (at, (name, month)) in rows.iter().enumerate() {
        typed(&mut engine, at as u32 + 1, 1, name);
        typed(&mut engine, at as u32 + 1, 2, month);
    }
    let area = r#"{"top":1,"left":1,"bottom":5,"right":2}"#;
    sort(
        &mut engine,
        area,
        r#"[{"index":2,"list":["Jan","Feb","Mar"]},{"index":1,"descending":true}]"#,
        true,
    );
    assert_eq!(column(&engine, 1, 5), ["Name", "b", "a", "a", "b"]);
    assert_eq!(column(&engine, 2, 5), ["Month", "Jan", "Jan", "Feb", "Mar"]);
}

#[test]
fn merged_cells_refuse_a_sort() {
    let mut engine = opened();
    typed(&mut engine, 1, 1, "2");
    typed(&mut engine, 2, 1, "1");
    run(
        &mut engine,
        r#"{"kind":"merge","sheet":0,"areas":[{"top":1,"left":2,"bottom":1,"right":3}]}"#,
    );
    let refused = engine.apply_json(
        r#"{"kind":"sort","sheet":0,"area":{"top":1,"left":1,"bottom":2,"right":3},"keys":[{"index":1}]}"#,
    );
    assert!(refused.unwrap_err().contains("merged cells"));
}

fn shows(engine: &Engine, row: u32) -> bool {
    engine.book().unwrap().sheets[0].rows.px(row) > 0
}

/// Name and Qty in A1:B7.
fn people() -> Engine {
    let mut engine = opened();
    let rows = [
        ("Name", "Qty"),
        ("ann", "5"),
        ("bob", "12"),
        ("Ann", "7"),
        ("cy", "3"),
        ("", "9"),
        ("dee", "20"),
    ];
    for (at, (name, qty)) in rows.iter().enumerate() {
        let row = at as u32 + 1;
        if !name.is_empty() {
            typed(&mut engine, row, 1, name);
        }
        typed(&mut engine, row, 2, qty);
    }
    run(
        &mut engine,
        r#"{"kind":"toggleFilter","sheet":0,"area":{"top":2,"left":1,"bottom":2,"right":1}}"#,
    );
    engine
}

fn set_filter(engine: &mut Engine, col: u32, criteria: &str) {
    run(
        engine,
        &format!(r#"{{"kind":"setFilter","sheet":0,"col":{col},"criteria":{criteria}}}"#),
    );
}

fn visible(engine: &Engine) -> Vec<u32> {
    (2..=7).filter(|&row| shows(engine, row)).collect()
}

#[test]
fn a_value_filter_keeps_what_it_lists_whatever_the_case() {
    let mut engine = people();
    set_filter(
        &mut engine,
        1,
        r#"{"kind":"values","values":["ann"],"blank":true}"#,
    );
    assert_eq!(visible(&engine), [2, 4, 6]);
    run(&mut engine, r#"{"kind":"undo"}"#);
    assert_eq!(visible(&engine), [2, 3, 4, 5, 6, 7]);
}

#[test]
fn custom_top_ten_and_average_filters_pick_rows_as_excel_does() {
    let mut engine = people();
    set_filter(
        &mut engine,
        2,
        r#"{"kind":"custom","and":true,"conditions":[{"op":"greaterThan","value":"4"},{"op":"lessThanOrEqual","value":"12"}]}"#,
    );
    assert_eq!(visible(&engine), [2, 3, 4, 6]);
    set_filter(&mut engine, 2, r#"{"kind":"top10","value":2}"#);
    assert_eq!(visible(&engine), [3, 7]);
    set_filter(
        &mut engine,
        2,
        r#"{"kind":"dynamic","name":"aboveAverage"}"#,
    );
    assert_eq!(visible(&engine), [3, 7]);
    set_filter(
        &mut engine,
        1,
        r#"{"kind":"custom","conditions":[{"op":"equal","value":"a*"}]}"#,
    );
    assert_eq!(visible(&engine), Vec::<u32>::new());
    run(&mut engine, r#"{"kind":"clearFilter","sheet":0}"#);
    assert_eq!(visible(&engine), [2, 3, 4, 5, 6, 7]);
}

#[test]
fn the_dropdown_lists_each_value_once_with_blanks_apart() {
    let mut engine = people();
    set_filter(
        &mut engine,
        2,
        r#"{"kind":"custom","conditions":[{"op":"greaterThan","value":"6"}]}"#,
    );
    let list: serde_json::Value = serde_json::from_str(
        &engine
            .query(r#"{"kind":"filterList","sheet":0,"col":1}"#)
            .unwrap(),
    )
    .unwrap();
    let names: Vec<&str> = list["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value["text"].as_str().unwrap())
        .collect();
    // Qty's filter leaves bob, Ann, the blank and dee.
    assert_eq!(names, ["Ann", "bob", "dee"]);
    assert_eq!(list["blanks"], true);
}

#[test]
fn a_filter_saves_its_criteria_and_hidden_rows_and_reads_back() {
    let mut engine = people();
    set_filter(&mut engine, 1, r#"{"kind":"values","values":["bob","cy"]}"#);
    let mut bytes = Vec::new();
    engine.save(&mut bytes).unwrap();
    let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains(r#"<autoFilter ref="A1:B7"><filterColumn colId="0"><filters><filter val="bob"/><filter val="cy"/></filters></filterColumn></autoFilter>"#), "{sheet}");
    assert!(sheet.contains(r#"<sheetPr filterMode="1"/>"#), "{sheet}");
    assert!(
        sheet.contains(r#"<row r="2""#) && sheet.contains(r#"hidden="1""#),
        "{sheet}"
    );
    let mut again = Engine::new();
    again.open(Rc::new(bytes)).unwrap();
    while again.load_step(1000.0).unwrap().phase == LoadPhase::Reading {}
    let filters: serde_json::Value =
        serde_json::from_str(&again.query(r#"{"kind":"filters","sheet":0}"#).unwrap()).unwrap();
    assert_eq!(filters[0]["filtered"][0], 1);
    assert_eq!(visible(&again), [3, 5]);
}

#[test]
fn a_tables_buttons_filter_its_rows() {
    let mut engine = people();
    run(
        &mut engine,
        r#"{"kind":"toggleFilter","sheet":0,"area":{"top":2,"left":1,"bottom":2,"right":1}}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"createTable","sheet":0,"area":{"top":1,"left":1,"bottom":7,"right":2},"name":"People"}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"setFilter","sheet":0,"table":"People","col":2,"criteria":{"kind":"top10","value":1}}"#,
    );
    assert_eq!(visible(&engine), [7]);
}

fn part(bytes: &[u8], name: &str) -> String {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

/// How long a value filter takes on a million rows, natively. Run with `--ignored --nocapture`.
#[test]
#[ignore]
fn timing_a_filter_on_a_million_rows() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/workbooks/generated/speed/rows-1m.xlsx"
    );
    let bytes = std::fs::read(path).unwrap();
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).unwrap();
    while engine.load_step(100_000.0).unwrap().phase == LoadPhase::Reading {}
    let started = std::time::Instant::now();
    run(
        &mut engine,
        r#"{"kind":"toggleFilter","sheet":0,"area":{"top":1,"left":1,"bottom":1,"right":1}}"#,
    );
    eprintln!("toggle {:?}", started.elapsed());
    let started = std::time::Instant::now();
    engine
        .apply_json(r#"{"kind":"setFilter","sheet":0,"col":8,"criteria":{"kind":"values","values":["7","300"]}}"#)
        .unwrap();
    eprintln!("values filter on H {:?}", started.elapsed());
    let started = std::time::Instant::now();
    engine
        .apply_json(r#"{"kind":"clearFilter","sheet":0}"#)
        .unwrap();
    eprintln!("clear {:?}", started.elapsed());
}
