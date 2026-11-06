//! Pivot tables through the engine's commands: made from a range, laid out, refreshed after the
//! source changes, undone, and saved and read back. What Excel makes of the same layouts is
//! checked by `bench/run-pivot-check.mjs`.

use std::rc::Rc;

use cgs_calc::CalcValue;
use cgs_engine::{Engine, LoadPhase};
use serde_json::{json, Value};

fn open(bytes: Vec<u8>) -> Engine {
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).unwrap();
    while engine.load_step(1000.0).unwrap().phase == LoadPhase::Reading {}
    while engine.prepare_step().unwrap() {}
    settle(&mut engine);
    engine
}

fn settle(engine: &mut Engine) {
    while engine.calc_step(1000, &[]).unwrap() {}
    engine.take_dirty();
}

fn run(engine: &mut Engine, command: Value) -> Result<(), String> {
    let result = engine.apply_json(&command.to_string()).map(|_| ());
    settle(engine);
    result
}

fn typed(engine: &mut Engine, row: u32, col: u32, text: &str) {
    run(
        engine,
        json!({ "kind": "input", "sheet": 0, "row": row, "col": col, "text": text }),
    )
    .unwrap();
}

/// Region, Product and Sales: North 10, 30 (Apple, Pear); South 20, 40.
fn sales() -> Engine {
    let mut engine = open(cgs_xlsx::blank::blank_workbook().unwrap());
    let rows = [
        ["Region", "Product", "Sales"],
        ["North", "Apple", "10"],
        ["South", "Apple", "20"],
        ["North", "Pear", "30"],
        ["South", "Pear", "40"],
    ];
    for (r, row) in rows.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            typed(&mut engine, r as u32 + 1, c as u32 + 1, text);
        }
    }
    engine
}

fn shown(engine: &Engine, sheet: u32, row: u32, col: u32) -> String {
    match engine.cell_value(sheet, row, col) {
        CalcValue::Text(t) => t,
        CalcValue::Number(n) => n.to_string(),
        CalcValue::Empty => String::new(),
        other => format!("{other:?}"),
    }
}

fn pivot() -> Value {
    json!({ "sheet": 0, "name": "PivotTable1" })
}

fn made(engine: &mut Engine) {
    run(
        engine,
        json!({
            "kind": "createPivot",
            "sheet": 0,
            "area": { "top": 1, "left": 1, "bottom": 1, "right": 1 },
            "to": { "kind": "existing", "sheet": 0, "row": 3, "col": 6 },
        }),
    )
    .unwrap();
    run(
        engine,
        json!({
            "kind": "pivotLayout",
            "pivot": pivot(),
            "layout": { "rows": [0], "values": [{ "field": 2 }] },
        }),
    )
    .unwrap();
}

#[test]
fn a_pivot_made_from_a_range_lays_out_as_excel_does() {
    let mut engine = sales();
    made(&mut engine);
    let column = |engine: &Engine, col| -> Vec<String> {
        (3..=6).map(|row| shown(engine, 0, row, col)).collect()
    };
    assert_eq!(
        column(&engine, 6),
        ["Row Labels", "North", "South", "Grand Total"]
    );
    assert_eq!(column(&engine, 7), ["Sum of Sales", "40", "60", "100"]);
    // Products across: compact form with a column field.
    run(
        &mut engine,
        json!({
            "kind": "pivotLayout",
            "pivot": pivot(),
            "layout": { "rows": [0], "cols": [1], "values": [{ "field": 2 }] },
        }),
    )
    .unwrap();
    let row =
        |engine: &Engine, r| -> Vec<String> { (6..=9).map(|c| shown(engine, 0, r, c)).collect() };
    assert_eq!(row(&engine, 3), ["Sum of Sales", "Column Labels", "", ""]);
    assert_eq!(
        row(&engine, 4),
        ["Row Labels", "Apple", "Pear", "Grand Total"]
    );
    assert_eq!(row(&engine, 5), ["North", "10", "30", "40"]);
    assert_eq!(row(&engine, 7), ["Grand Total", "30", "70", "100"]);
    // Undo takes it back to the rows-only layout, cells and all.
    run(&mut engine, json!({ "kind": "undo" })).unwrap();
    assert_eq!(column(&engine, 7), ["Sum of Sales", "40", "60", "100"]);
    assert_eq!(shown(&engine, 0, 4, 8), "");
}

#[test]
fn a_refresh_reads_the_source_again() {
    let mut engine = sales();
    made(&mut engine);
    typed(&mut engine, 2, 3, "15");
    // The cache keeps what it read until a refresh.
    assert_eq!(shown(&engine, 0, 4, 7), "40");
    typed(&mut engine, 6, 1, "East");
    typed(&mut engine, 6, 2, "Apple");
    typed(&mut engine, 6, 3, "5");
    // A refresh reads the same range, so the new row isn't in it.
    run(&mut engine, json!({ "kind": "refreshPivot" })).unwrap();
    assert_eq!(shown(&engine, 0, 4, 7), "45");
    assert_eq!(shown(&engine, 0, 6, 7), "105");
    run(&mut engine, json!({ "kind": "undo" })).unwrap();
    assert_eq!(shown(&engine, 0, 4, 7), "40");
}

#[test]
fn summaries_filters_and_forms_follow_their_commands() {
    let mut engine = sales();
    made(&mut engine);
    run(
        &mut engine,
        json!({
            "kind": "pivotLayout",
            "pivot": pivot(),
            "layout": { "rows": [0, 1], "values": [{ "field": 2, "summary": "average" }] },
        }),
    )
    .unwrap();
    assert_eq!(shown(&engine, 0, 3, 7), "Average of Sales");
    assert_eq!(shown(&engine, 0, 4, 7), "20");
    run(
        &mut engine,
        json!({ "kind": "pivotOptions", "pivot": pivot(), "options": { "form": "tabular" } }),
    )
    .unwrap();
    assert_eq!(shown(&engine, 0, 3, 6), "Region");
    assert_eq!(shown(&engine, 0, 3, 7), "Product");
    assert_eq!(shown(&engine, 0, 4, 6), "North");
    assert_eq!(shown(&engine, 0, 4, 7), "Apple");
    assert_eq!(shown(&engine, 0, 6, 6), "North Total");
    // Hiding South leaves it out of the totals too.
    run(
        &mut engine,
        json!({ "kind": "pivotField", "pivot": pivot(), "field": 0, "change": { "hidden": [1] } }),
    )
    .unwrap();
    assert_eq!(shown(&engine, 0, 7, 6), "Grand Total");
    assert_eq!(shown(&engine, 0, 7, 8), "20");
    let info: Value = serde_json::from_str(&engine.pivot_info(0, 3, 6).unwrap()).unwrap();
    assert_eq!(info["options"]["form"], "tabular");
    assert_eq!(info["fields"][0]["items"][1]["hidden"], true);
    assert_eq!(info["values"][0]["summary"], "average");
}

#[test]
fn a_saved_pivot_reads_back_and_its_parts_are_linked() {
    let mut engine = sales();
    made(&mut engine);
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    let mut again = open(out.clone());
    assert_eq!(shown(&again, 0, 4, 6), "North");
    assert_eq!(shown(&again, 0, 6, 7), "100");
    let info: Value = serde_json::from_str(&again.pivot_info(0, 4, 6).unwrap()).unwrap();
    assert_eq!(info["name"], "PivotTable1");
    assert_eq!(info["rows"], json!([0]));
    // The file asks Excel to read the source when it opens, and keeps no records.
    let package = cgs_xlsx::package::Package::open(Rc::new(out)).unwrap();
    let names: Vec<String> = package.parts().iter().map(|p| p.name.clone()).collect();
    assert!(names.iter().any(|n| n == "xl/pivotTables/pivotTable1.xml"));
    assert!(names
        .iter()
        .any(|n| n == "xl/pivotCache/pivotCacheDefinition1.xml"));
    let cache = String::from_utf8(
        package
            .read("xl/pivotCache/pivotCacheDefinition1.xml")
            .unwrap(),
    )
    .unwrap();
    assert!(cache.contains("refreshOnLoad=\"1\""));
    let workbook = String::from_utf8(package.read("xl/workbook.xml").unwrap()).unwrap();
    assert!(workbook.contains("<pivotCaches><pivotCache cacheId=\"1\""));
    let types = String::from_utf8(package.read("[Content_Types].xml").unwrap()).unwrap();
    assert!(types.contains("/xl/pivotTables/pivotTable1.xml"));
    // And a refresh after reopening reads the cells again.
    run(&mut again, json!({ "kind": "refreshPivot" })).unwrap();
    assert_eq!(shown(&again, 0, 6, 7), "100");
}

#[test]
fn a_pivot_cannot_spill_over_data() {
    let mut engine = sales();
    typed(&mut engine, 5, 6, "in the way");
    let made = run(
        &mut engine,
        json!({
            "kind": "createPivot",
            "sheet": 0,
            "area": { "top": 1, "left": 1, "bottom": 5, "right": 3 },
            "to": { "kind": "existing", "sheet": 0, "row": 3, "col": 6 },
        }),
    );
    assert!(made.is_ok());
    let laid = run(
        &mut engine,
        json!({
            "kind": "pivotLayout",
            "pivot": pivot(),
            "layout": { "rows": [0], "values": [{ "field": 2 }] },
        }),
    );
    assert!(laid.unwrap_err().contains("already data"));
    assert_eq!(shown(&engine, 0, 5, 6), "in the way");
}
