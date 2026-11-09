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

#[test]
fn a_number_field_on_an_axis_lists_its_items_in_the_cache() {
    let mut engine = sales();
    made(&mut engine);
    // Sales as a report filter: its items must be in the cache for the table to point at.
    run(
        &mut engine,
        json!({
            "kind": "pivotLayout",
            "pivot": pivot(),
            "layout": { "rows": [0], "pages": [2], "values": [{ "field": 2, "summary": "count" }] },
        }),
    )
    .unwrap();
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    let package = cgs_xlsx::package::Package::open(Rc::new(out)).unwrap();
    let cache = String::from_utf8(
        package
            .read("xl/pivotCache/pivotCacheDefinition1.xml")
            .unwrap(),
    )
    .unwrap();
    let sales = cache.split("<cacheField name=\"Sales\"").nth(1).unwrap();
    assert!(sales.starts_with(" numFmtId=\"0\"><sharedItems"));
    assert!(sales.contains("count=\"4\"><n v=\"10\"/>"));
}

/// Excel's own probe book for grouping: 20 sales over 2016 in three regions and four products.
fn dated() -> Engine {
    let mut engine = open(cgs_xlsx::blank::blank_workbook().unwrap());
    for (c, text) in ["Region", "Product", "Date", "Sales"].iter().enumerate() {
        typed(&mut engine, 1, c as u32 + 1, text);
    }
    let regions = ["East", "West", "North"];
    let products = ["Apple", "Pear", "Plum", "Fig"];
    for i in 0..20u32 {
        let row = i + 2;
        typed(&mut engine, row, 1, regions[i as usize % 3]);
        typed(&mut engine, row, 2, products[i as usize % 4]);
        let date = format!("{}/{}/2016", i % 12 + 1, i % 27 + 1);
        typed(&mut engine, row, 3, &date);
        typed(&mut engine, row, 4, &(10 * (i + 1) + 5).to_string());
    }
    run(
        &mut engine,
        json!({
            "kind": "createPivot",
            "sheet": 0,
            "area": { "top": 1, "left": 1, "bottom": 21, "right": 4 },
            "to": { "kind": "existing", "sheet": 0, "row": 3, "col": 7 },
        }),
    )
    .unwrap();
    engine
}

fn labels(engine: &Engine) -> Vec<String> {
    (3..60)
        .map(|row| shown(engine, 0, row, 7))
        .take_while(|text| !text.is_empty())
        .collect()
}

fn saved_cache(engine: &mut Engine) -> String {
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    let package = cgs_xlsx::package::Package::open(Rc::new(out)).unwrap();
    String::from_utf8(
        package
            .read("xl/pivotCache/pivotCacheDefinition1.xml")
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn dates_group_by_period_into_new_fields_as_excel_2016_does() {
    let mut engine = dated();
    let layout = json!({ "kind": "pivotLayout", "pivot": pivot(), "layout": { "rows": [2], "values": [{ "field": 3 }] } });
    run(&mut engine, layout).unwrap();
    run(
        &mut engine,
        json!({ "kind": "groupPivot", "pivot": pivot(), "field": 2, "group": { "kind": "dates", "parts": ["months", "quarters", "years"] } }),
    )
    .unwrap();
    // As Excel 2016 lays it out over COM.
    assert_eq!(
        labels(&engine),
        [
            "Row Labels",
            "2016",
            "Qtr1",
            "Jan",
            "Feb",
            "Mar",
            "Qtr2",
            "Apr",
            "May",
            "Jun",
            "Qtr3",
            "Jul",
            "Aug",
            "Sep",
            "Qtr4",
            "Oct",
            "Nov",
            "Dec",
            "Grand Total"
        ]
    );
    let cache = saved_cache(&mut engine);
    assert!(cache.contains("<fieldGroup par=\"6\"/>"), "{cache}");
    assert!(cache.contains("<cacheField name=\"Months (Date)\" numFmtId=\"0\" databaseField=\"0\"><fieldGroup base=\"2\"><rangePr groupBy=\"months\" startDate=\"2016-01-01T00:00:00\" endDate=\"2016-12-13T00:00:00\"/><groupItems count=\"14\"><s v=\"&lt;1/1/2016\"/><s v=\"Jan\"/>"), "{cache}");
    assert!(
        cache.contains("<s v=\"2016\"/><s v=\"&gt;12/13/2016\"/></groupItems>"),
        "{cache}"
    );
    // Ungroup brings the dates back.
    run(
        &mut engine,
        json!({ "kind": "ungroupPivot", "pivot": pivot(), "field": 4 }),
    )
    .unwrap();
    assert_eq!(labels(&engine).len(), 22);
    assert!(!saved_cache(&mut engine).contains("fieldGroup"));
    // Undo goes back to the groups.
    run(&mut engine, json!({ "kind": "undo" })).unwrap();
    assert_eq!(labels(&engine)[1], "2016");
}

#[test]
fn numbers_group_in_steps_and_items_by_hand_as_excel_2016_does() {
    let mut engine = dated();
    run(
        &mut engine,
        json!({ "kind": "pivotLayout", "pivot": pivot(), "layout": { "rows": [3], "values": [{ "field": 0, "summary": "count" }] } }),
    )
    .unwrap();
    run(
        &mut engine,
        json!({ "kind": "groupPivot", "pivot": pivot(), "field": 3, "group": { "kind": "numbers", "start": 0, "end": 250, "by": 50 } }),
    )
    .unwrap();
    assert_eq!(
        labels(&engine),
        [
            "Row Labels",
            "0-49",
            "50-99",
            "100-149",
            "150-199",
            "200-250",
            "Grand Total"
        ]
    );
    let cache = saved_cache(&mut engine);
    assert!(cache.contains("<fieldGroup base=\"3\"><rangePr autoStart=\"0\" autoEnd=\"0\" startNum=\"0\" endNum=\"250\" groupInterval=\"50\"/><groupItems count=\"7\"><s v=\"&lt;0\"/><s v=\"0-49\"/>"), "{cache}");

    // Apple and Fig grouped by hand, in a new field before Product.
    run(
        &mut engine,
        json!({ "kind": "pivotLayout", "pivot": pivot(), "layout": { "rows": [1], "values": [{ "field": 3 }] } }),
    )
    .unwrap();
    run(
        &mut engine,
        json!({ "kind": "groupPivot", "pivot": pivot(), "field": 1, "group": { "kind": "items", "items": [0, 1] } }),
    )
    .unwrap();
    assert_eq!(
        labels(&engine),
        [
            "Row Labels",
            "Group1",
            "Apple",
            "Fig",
            "Pear",
            "Pear",
            "Plum",
            "Plum",
            "Grand Total"
        ]
    );
    assert_eq!(shown(&engine, 0, 4, 8), "1100");
    assert_eq!(shown(&engine, 0, 5, 8), "475");
    let cache = saved_cache(&mut engine);
    assert!(cache.contains("<cacheField name=\"Product2\" numFmtId=\"0\" databaseField=\"0\"><fieldGroup base=\"1\"><discretePr count=\"4\"><x v=\"2\"/><x v=\"0\"/><x v=\"1\"/><x v=\"2\"/></discretePr><groupItems count=\"3\"><s v=\"Pear\"/><s v=\"Plum\"/><s v=\"Group1\"/></groupItems>"), "{cache}");
    run(
        &mut engine,
        json!({ "kind": "ungroupPivot", "pivot": pivot(), "field": 4 }),
    )
    .unwrap();
    assert_eq!(
        labels(&engine),
        ["Row Labels", "Apple", "Fig", "Pear", "Plum", "Grand Total"]
    );
}

fn sheet_names(engine: &Engine) -> Vec<String> {
    let book = engine.book().unwrap();
    book.tab_order()
        .iter()
        .map(|&s| book.sheets[s as usize].name.clone())
        .collect()
}

#[test]
fn show_details_lists_a_values_records_on_a_new_sheet_as_excel_2016_does() {
    let mut engine = dated();
    run(
        &mut engine,
        json!({ "kind": "pivotLayout", "pivot": pivot(), "layout": { "rows": [0], "cols": [1], "values": [{ "field": 3 }] } }),
    )
    .unwrap();
    // East and Apple: 150, as in Excel's B5.
    assert_eq!(shown(&engine, 0, 5, 8), "150");
    run(
        &mut engine,
        json!({ "kind": "pivotDetails", "sheet": 0, "row": 5, "col": 8 }),
    )
    .unwrap();
    assert_eq!(sheet_names(&engine), ["Sheet2", "Sheet1"]);
    let row =
        |engine: &Engine, r| -> Vec<String> { (1..=4).map(|c| shown(engine, 1, r, c)).collect() };
    assert_eq!(row(&engine, 1), ["Region", "Product", "Date", "Sales"]);
    assert_eq!(row(&engine, 2), ["East", "Apple", "42370", "15"]);
    assert_eq!(row(&engine, 3), ["East", "Apple", "42382", "135"]);
    assert_eq!(row(&engine, 4), ["", "", "", ""]);
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    let package = cgs_xlsx::package::Package::open(Rc::new(out)).unwrap();
    let part = package
        .parts()
        .iter()
        .map(|p| p.name.clone())
        .find(|n| n.starts_with("xl/tables/"))
        .unwrap_or_else(|| {
            panic!(
                "{:?}",
                package
                    .parts()
                    .iter()
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>()
            )
        });
    let table = String::from_utf8(package.read(&part).unwrap()).unwrap();
    assert!(table.contains("name=\"Table1\""), "{table}");
    assert!(table.contains("ref=\"A1:D3\""), "{table}");
    assert!(table.contains("TableStyleMedium2"), "{table}");
    // The grand total's details are every record; a label isn't a value.
    run(
        &mut engine,
        json!({ "kind": "pivotDetails", "sheet": 0, "row": 8, "col": 12 }),
    )
    .unwrap();
    assert_eq!(sheet_names(&engine), ["Sheet2", "Sheet3", "Sheet1"]);
    assert_eq!(shown(&engine, 2, 21, 4), "205");
    assert!(run(
        &mut engine,
        json!({ "kind": "pivotDetails", "sheet": 0, "row": 5, "col": 7 })
    )
    .is_err());
    // One undo step each.
    run(&mut engine, json!({ "kind": "undo" })).unwrap();
    run(&mut engine, json!({ "kind": "undo" })).unwrap();
    assert_eq!(sheet_names(&engine), ["Sheet1"]);
}

#[test]
fn report_filter_pages_copy_the_pivot_per_item_as_excel_2016_does() {
    let mut engine = dated();
    run(
        &mut engine,
        json!({ "kind": "pivotLayout", "pivot": pivot(), "layout": { "pages": [0], "rows": [1], "values": [{ "field": 3 }] } }),
    )
    .unwrap();
    run(
        &mut engine,
        json!({ "kind": "pivotFilterPages", "pivot": pivot(), "field": 0 }),
    )
    .unwrap();
    assert_eq!(sheet_names(&engine), ["East", "North", "West", "Sheet1"]);
    let book = engine.book().unwrap();
    let on = |name: &str| book.sheets.iter().position(|s| s.name == name).unwrap() as u32;
    let (east, west) = (on("East"), on("West"));
    // Excel makes West's first, so it is PivotTable2 after the original's PivotTable1.
    let named = |sheet: u32| {
        book.pivot_tables
            .iter()
            .find(|t| t.sheet == sheet)
            .unwrap()
            .name
            .clone()
    };
    assert_eq!(named(west), "PivotTable2");
    assert_eq!(named(east), "PivotTable4");
    assert_eq!(shown(&engine, east, 1, 7), "Region");
    assert_eq!(shown(&engine, east, 1, 8), "East");
    let total = |sheet: u32| {
        (3..20)
            .find(|&r| shown(&engine, sheet, r, 7) == "Grand Total")
            .map(|r| shown(&engine, sheet, r, 8))
            .unwrap()
    };
    // East's sales: rows 2, 5, 8, ... of the source.
    let east_total: u32 = (0..20u32)
        .filter(|i| i % 3 == 0)
        .map(|i| 10 * (i + 1) + 5)
        .sum();
    assert_eq!(total(east), east_total.to_string());
    assert_eq!(total(0), "2200");
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    let again = open(out);
    assert_eq!(again.book().unwrap().pivot_tables.len(), 4);
}

fn pivot_formula(engine: &Engine, row: u32, col: u32, from: u32) -> Value {
    let question =
        json!({ "kind": "pivotDataFormula", "sheet": 0, "row": row, "col": col, "from": from });
    serde_json::from_str(&engine.query(&question.to_string()).unwrap()).unwrap()
}

#[test]
fn a_click_on_a_value_writes_getpivotdata_which_reads_the_pivot() {
    let mut engine = dated();
    run(
        &mut engine,
        json!({ "kind": "pivotLayout", "pivot": pivot(), "layout": { "rows": [0], "cols": [1], "values": [{ "field": 3 }] } }),
    )
    .unwrap();
    let east_apple = pivot_formula(&engine, 5, 8, 0);
    assert_eq!(
        east_apple,
        "GETPIVOTDATA(\"Sales\",$G$3,\"Region\",\"East\",\"Product\",\"Apple\")"
    );
    assert_eq!(
        pivot_formula(&engine, 8, 12, 0),
        "GETPIVOTDATA(\"Sales\",$G$3)"
    );
    assert_eq!(pivot_formula(&engine, 5, 7, 0), Value::Null);
    typed(
        &mut engine,
        30,
        1,
        &format!("={}", east_apple.as_str().unwrap()),
    );
    typed(
        &mut engine,
        31,
        1,
        "=GETPIVOTDATA(\"Sum of Sales\",G5,\"Product\",\"pear\")",
    );
    typed(
        &mut engine,
        32,
        1,
        "=GETPIVOTDATA(\"Sales\",$G$3,\"Region\",\"Nowhere\")",
    );
    assert_eq!(shown(&engine, 0, 30, 1), "150");
    assert_eq!(shown(&engine, 0, 31, 1), "525");
    assert!(
        shown(&engine, 0, 32, 1).contains("#REF!"),
        "{}",
        shown(&engine, 0, 32, 1)
    );
    // A change to the source, refreshed, reaches it.
    typed(&mut engine, 2, 4, "1015");
    run(&mut engine, json!({ "kind": "refreshPivot" })).unwrap();
    assert_eq!(shown(&engine, 0, 30, 1), "1150");
    // Dates are written as DATE(), as Excel writes them.
    run(
        &mut engine,
        json!({ "kind": "pivotLayout", "pivot": pivot(), "layout": { "rows": [2], "values": [{ "field": 3 }] } }),
    )
    .unwrap();
    assert_eq!(
        pivot_formula(&engine, 4, 8, 1),
        "GETPIVOTDATA(\"Sales\",Sheet1!$G$3,\"Date\",DATE(2016,1,1))"
    );
}

#[test]
fn two_pivots_made_on_one_sheet_are_both_linked_from_it() {
    let mut engine = dated();
    run(
        &mut engine,
        json!({ "kind": "pivotLayout", "pivot": pivot(), "layout": { "rows": [0], "values": [{ "field": 3 }] } }),
    )
    .unwrap();
    run(
        &mut engine,
        json!({
            "kind": "createPivot",
            "sheet": 0,
            "area": { "top": 1, "left": 1, "bottom": 21, "right": 4 },
            "to": { "kind": "existing", "sheet": 0, "row": 3, "col": 11 },
        }),
    )
    .unwrap();
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    let package = cgs_xlsx::package::Package::open(Rc::new(out.clone())).unwrap();
    let rels =
        String::from_utf8(package.read("xl/worksheets/_rels/sheet1.xml.rels").unwrap()).unwrap();
    assert!(
        rels.contains("pivotTable1.xml") && rels.contains("pivotTable2.xml"),
        "{rels}"
    );
    assert_eq!(open(out).book().unwrap().pivot_tables.len(), 2);
}
