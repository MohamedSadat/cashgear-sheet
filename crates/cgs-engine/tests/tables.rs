//! Tables through the engine's commands, each case read off Excel 2016 over COM.

use std::collections::HashMap;
use std::rc::Rc;

use cgs_calc::{CalcValue, CellSink, Rect, SheetRect};
use cgs_engine::{Area, Command, Engine, LoadPhase};

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

fn table(engine: &Engine, name: &str) -> serde_json::Value {
    let all: serde_json::Value =
        serde_json::from_str(&engine.query(r#"{"kind":"tables"}"#).unwrap()).unwrap();
    all.as_array()
        .unwrap()
        .iter()
        .find(|table| table["name"] == name)
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

fn area_of(engine: &Engine, name: &str) -> (u64, u64, u64, u64) {
    let area = &table(engine, name)["area"];
    (
        area["top"].as_u64().unwrap(),
        area["left"].as_u64().unwrap(),
        area["bottom"].as_u64().unwrap(),
        area["right"].as_u64().unwrap(),
    )
}

/// Sales in H1:J6: Item, Cost and Qty, as the probes had it.
fn sales() -> Engine {
    let mut engine = opened();
    for (col, name) in [(8, "Item"), (9, "Cost"), (10, "Qty")] {
        typed(&mut engine, 1, col, name);
    }
    for row in 2..=6 {
        typed(&mut engine, row, 8, &format!("t{row}"));
        typed(&mut engine, row, 9, &(row * 10).to_string());
        typed(&mut engine, row, 10, &row.to_string());
    }
    run(
        &mut engine,
        r#"{"kind":"createTable","sheet":0,"area":{"top":1,"left":8,"bottom":6,"right":10}}"#,
    );
    run(
        &mut engine,
        r#"{"kind":"renameTable","table":"Table1","name":"Sales"}"#,
    );
    engine
}

#[test]
fn a_new_table_takes_its_headings_and_excels_default_look() {
    let engine = sales();
    let sales = table(&engine, "Sales");
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 6, 10));
    assert_eq!(sales["style"], "TableStyleMedium2");
    assert_eq!(sales["rowStripes"], true);
    assert_eq!(sales["filterButton"], true);
    assert_eq!(sales["columns"][1]["name"], "Cost");
}

#[test]
fn without_headings_a_row_of_them_goes_on_top() {
    let mut engine = opened();
    for row in 1..=4 {
        typed(&mut engine, row, 8, &row.to_string());
    }
    typed(&mut engine, 6, 8, "below");
    run(
        &mut engine,
        r#"{"kind":"createTable","sheet":0,"area":{"top":1,"left":8,"bottom":4,"right":8}}"#,
    );
    assert_eq!(area_of(&engine, "Table1"), (1, 8, 5, 8));
    assert_eq!(text(&engine, 1, 8), "Column1");
    assert_eq!(text(&engine, 2, 8), "1");
    assert_eq!(text(&engine, 5, 8), "4");
    // The row below was free, so what's further down stays.
    assert_eq!(text(&engine, 6, 8), "below");
    run(&mut engine, r#"{"kind":"undo"}"#);
    assert_eq!(text(&engine, 1, 8), "1");
    assert!(table(&engine, "Table1").is_null());
}

#[test]
fn headings_become_text_and_all_different() {
    let mut engine = opened();
    typed(&mut engine, 1, 1, "Item");
    typed(&mut engine, 1, 2, "Item");
    typed(&mut engine, 2, 1, "1");
    run(
        &mut engine,
        r#"{"kind":"createTable","sheet":0,"area":{"top":1,"left":1,"bottom":2,"right":3},"headers":true}"#,
    );
    assert_eq!(text(&engine, 1, 2), "Item2");
    assert_eq!(text(&engine, 1, 3), "Column3");
}

#[test]
fn tables_cant_overlap() {
    let mut engine = sales();
    let refused = engine.apply_json(
        r#"{"kind":"createTable","sheet":0,"area":{"top":5,"left":10,"bottom":8,"right":11},"headers":true}"#,
    );
    assert_eq!(refused.unwrap_err(), "A table can't overlap another table.");
}

#[test]
fn deleting_cells_in_a_table_takes_its_rows() {
    let mut engine = sales();
    // Two rows down, which doesn't grow the table.
    typed(&mut engine, 8, 8, "below");
    run(
        &mut engine,
        r#"{"kind":"shiftCells","sheet":0,"area":{"top":3,"left":8,"bottom":4,"right":8},"down":true,"delete":true}"#,
    );
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 4, 10));
    assert_eq!(text(&engine, 3, 8), "t5");
    assert_eq!(text(&engine, 3, 9), "50");
    assert_eq!(text(&engine, 6, 8), "below");
}

#[test]
fn inserting_part_of_a_tables_width_is_refused_and_all_of_it_adds_rows() {
    let mut engine = sales();
    let refused = engine.apply_json(
        r#"{"kind":"shiftCells","sheet":0,"area":{"top":3,"left":8,"bottom":3,"right":8},"down":true}"#,
    );
    assert!(refused.unwrap_err().contains("shift cells in a table"));
    run(
        &mut engine,
        r#"{"kind":"shiftCells","sheet":0,"area":{"top":3,"left":8,"bottom":3,"right":10},"down":true}"#,
    );
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 7, 10));
    assert_eq!(text(&engine, 4, 8), "t3");
}

#[test]
fn deleting_a_cell_sideways_takes_its_table_column() {
    let mut engine = sales();
    typed(&mut engine, 2, 2, "=SUM(Sales[Item])");
    run(
        &mut engine,
        r#"{"kind":"shiftCells","sheet":0,"area":{"top":3,"left":8,"bottom":3,"right":8},"down":false,"delete":true}"#,
    );
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 6, 9));
    assert_eq!(text(&engine, 1, 8), "Cost");
    assert_eq!(engine.formula_text(0, 2, 2).unwrap(), "SUM(#REF!)");
}

#[test]
fn filling_across_moves_a_column_round_the_table() {
    let mut engine = sales();
    typed(&mut engine, 7, 2, "=SUM(Sales[Cost])");
    run(
        &mut engine,
        r#"{"kind":"fill","sheet":0,"from":{"top":7,"left":2,"bottom":7,"right":2},"to":{"top":7,"left":2,"bottom":7,"right":4}}"#,
    );
    assert!(engine.formula_text(0, 7, 3).unwrap().contains("Sales[Qty]"));
    assert!(engine
        .formula_text(0, 7, 4)
        .unwrap()
        .contains("Sales[Item]"));
    // Down, nothing moves.
    run(
        &mut engine,
        r#"{"kind":"fill","sheet":0,"from":{"top":7,"left":2,"bottom":7,"right":2},"to":{"top":7,"left":2,"bottom":8,"right":2}}"#,
    );
    assert!(engine
        .formula_text(0, 8, 2)
        .unwrap()
        .contains("Sales[Cost]"));
}

#[test]
fn inside_its_table_a_formula_shows_its_own_columns_without_the_name() {
    let mut engine = sales();
    typed(
        &mut engine,
        3,
        8,
        "=SUM(Sales[Cost])+Sales[[#Headers],[Item]]",
    );
    let shown = engine.formula_text(0, 3, 8).unwrap().replace(' ', "");
    assert!(shown.starts_with("SUM([Cost])"), "{shown}");
    assert!(shown.contains("Sales[[#Headers],[Item]]"), "{shown}");
}

#[test]
fn typing_below_or_beside_grows_the_table() {
    let mut engine = sales();
    typed(&mut engine, 7, 9, "70");
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 7, 10));
    typed(&mut engine, 4, 11, "x");
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 7, 11));
    assert_eq!(text(&engine, 1, 11), "Column1");
    // Not diagonally, nor two away.
    typed(&mut engine, 8, 12, "1");
    typed(&mut engine, 9, 8, "1");
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 7, 11));
}

#[test]
fn a_row_below_with_something_in_it_doesnt_grow_the_table() {
    let mut engine = sales();
    // A fill doesn't grow a table, so H7 holds something beside it.
    run(
        &mut engine,
        r#"{"kind":"fill","sheet":0,"from":{"top":6,"left":8,"bottom":6,"right":8},"to":{"top":6,"left":8,"bottom":7,"right":8}}"#,
    );
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 6, 10));
    typed(&mut engine, 7, 9, "1");
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 6, 10));
}

#[test]
fn a_formula_in_an_empty_column_fills_it_and_new_rows() {
    let mut engine = sales();
    run(
        &mut engine,
        r#"{"kind":"clear","sheet":0,"areas":[{"top":2,"left":10,"bottom":6,"right":10}]}"#,
    );
    typed(&mut engine, 3, 10, "=[@Cost]*2");
    assert_eq!(text(&engine, 2, 10), "40");
    assert_eq!(text(&engine, 6, 10), "120");
    assert_eq!(table(&engine, "Sales")["columns"][2]["calculated"], true);
    typed(&mut engine, 7, 8, "more");
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 7, 10));
    assert!(engine.formula_text(0, 7, 10).is_some());
}

#[test]
fn a_totals_row_comes_and_goes() {
    let mut engine = sales();
    typed(&mut engine, 7, 8, "taken");
    run(&mut engine, r#"{"kind":"undo"}"#);
    typed(&mut engine, 8, 8, "after");
    run(
        &mut engine,
        r#"{"kind":"tableOptions","table":"Sales","totalsRow":true}"#,
    );
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 7, 10));
    assert_eq!(text(&engine, 7, 8), "Total");
    assert_eq!(text(&engine, 7, 10), "20");
    assert!(engine
        .formula_text(0, 7, 10)
        .unwrap()
        .contains("SUBTOTAL(109"));
    assert_eq!(text(&engine, 8, 8), "after");
    run(
        &mut engine,
        r#"{"kind":"totalsFunction","table":"Sales","column":"Cost","function":"average"}"#,
    );
    assert_eq!(text(&engine, 7, 9), "40");
    run(
        &mut engine,
        r#"{"kind":"tableOptions","table":"Sales","totalsRow":false}"#,
    );
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 6, 10));
    assert_eq!(text(&engine, 7, 8), "after");
    // Functions are remembered.
    run(
        &mut engine,
        r#"{"kind":"tableOptions","table":"Sales","totalsRow":true}"#,
    );
    assert_eq!(text(&engine, 7, 9), "40");
}

#[test]
fn the_header_row_goes_and_comes_back_above() {
    let mut engine = sales();
    run(
        &mut engine,
        r#"{"kind":"tableOptions","table":"Sales","headerRow":false}"#,
    );
    assert_eq!(area_of(&engine, "Sales"), (2, 8, 6, 10));
    assert_eq!(text(&engine, 1, 8), "");
    run(
        &mut engine,
        r#"{"kind":"tableOptions","table":"Sales","headerRow":true}"#,
    );
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 6, 10));
    assert_eq!(text(&engine, 1, 9), "Cost");
}

#[test]
fn renaming_a_table_follows_into_formulas() {
    let mut engine = sales();
    typed(&mut engine, 2, 2, "=SUM(Sales[Qty])+COUNTA(Sales)");
    run(
        &mut engine,
        r#"{"kind":"renameTable","table":"Sales","name":"Orders"}"#,
    );
    let text = engine.formula_text(0, 2, 2).unwrap();
    assert!(
        text.contains("Orders[Qty]") && text.contains("COUNTA(Orders)"),
        "{text}"
    );
    for bad in ["A1", "1x", "a b", "R1C1"] {
        assert!(engine
            .apply_json(&format!(
                r#"{{"kind":"renameTable","table":"Orders","name":"{bad}"}}"#
            ))
            .is_err());
    }
}

#[test]
fn resizing_names_new_columns_and_loses_old_ones() {
    let mut engine = sales();
    typed(&mut engine, 2, 2, "=SUM(Sales[Qty])");
    run(
        &mut engine,
        r#"{"kind":"resizeTable","table":"Sales","area":{"top":1,"left":8,"bottom":8,"right":9}}"#,
    );
    assert_eq!(area_of(&engine, "Sales"), (1, 8, 8, 9));
    assert_eq!(engine.formula_text(0, 2, 2).unwrap(), "SUM(#REF!)");
    run(
        &mut engine,
        r#"{"kind":"resizeTable","table":"Sales","area":{"top":1,"left":8,"bottom":8,"right":11}}"#,
    );
    // A heading cell that still holds text names its column.
    assert_eq!(text(&engine, 1, 10), "Qty");
    assert_eq!(text(&engine, 1, 11), "Column1");
    assert!(engine
        .apply_json(
            r#"{"kind":"resizeTable","table":"Sales","area":{"top":2,"left":8,"bottom":8,"right":11}}"#
        )
        .is_err());
}

#[test]
fn converting_to_a_range_writes_the_cells_references_meant() {
    let mut engine = sales();
    typed(&mut engine, 2, 2, "=SUM(Sales[Cost])");
    typed(&mut engine, 3, 11, "=Sales[@Cost]");
    run(&mut engine, r#"{"kind":"convertToRange","table":"Sales"}"#);
    assert!(table(&engine, "Sales").is_null());
    assert_eq!(
        engine.formula_text(0, 2, 2).unwrap(),
        "SUM(Sheet1!$I$2:$I$6)"
    );
    assert_eq!(engine.formula_text(0, 3, 11).unwrap(), "Sheet1!$I3");
    assert_eq!(text(&engine, 2, 2), "200");
    run(&mut engine, r#"{"kind":"undo"}"#);
    assert!(!table(&engine, "Sales").is_null());
    assert!(engine
        .formula_text(0, 2, 2)
        .unwrap()
        .contains("Sales[Cost]"));
}

#[test]
fn guessing_takes_the_block_around_a_cell() {
    let engine = sales();
    let guess: serde_json::Value = serde_json::from_str(
        &engine
            .query(
                r#"{"kind":"guessTable","sheet":0,"area":{"top":3,"left":9,"bottom":3,"right":9}}"#,
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(guess["area"]["top"], 1);
    assert_eq!(guess["area"]["right"], 10);
    assert_eq!(guess["headers"], true);
    let _ = Area::cell(1, 1);
}

#[test]
fn this_row_references_keep_their_spelling() {
    let mut engine = sales();
    typed(&mut engine, 3, 12, "=Sales[@Cost]");
    assert_eq!(engine.formula_text(0, 3, 12).unwrap(), "Sales[@Cost]");
    typed(&mut engine, 3, 9, "=[@Qty]+1");
    assert_eq!(engine.formula_text(0, 3, 9).unwrap(), "[@Qty]+1");
    assert_eq!(text(&engine, 3, 9), "4");
    run(
        &mut engine,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":1,"count":1,"delete":false}"#,
    );
    assert_eq!(engine.formula_text(0, 4, 9).unwrap(), "[@Qty]+1");
    assert_eq!(text(&engine, 4, 9), "4");
}
