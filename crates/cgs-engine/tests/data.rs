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
