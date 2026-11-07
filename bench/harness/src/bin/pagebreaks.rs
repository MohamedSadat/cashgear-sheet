//! Where the engine breaks each worksheet into pages, for comparing with Excel's `HPageBreaks` and
//! `VPageBreaks` over COM. A row break is the first row of a new page, as Excel's `Location.Row`
//! says it, and a column break the first column.
//!
//! Usage: pagebreaks <workbook> [--ignore-print-area]

use std::fs;
use std::rc::Rc;

use cgs_engine::{Engine, LoadPhase};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SheetPages {
    sheet: u32,
    name: String,
    pages: usize,
    zoom: u64,
    rows: Vec<u64>,
    cols: Vec<u64>,
    manual_rows: Vec<u64>,
    manual_cols: Vec<u64>,
    /// The sheet says anything about printing: a page setup other than the default, breaks,
    /// a print area or titles.
    has_setup: bool,
    /// The sheet's page setup as the engine reads it.
    setup: Value,
    /// The first and last row and column any page prints: [top, left, bottom, right].
    extent: [u64; 4],
}

fn has_setup(setup: &Value) -> bool {
    let margins = &setup["margins"];
    let normal = [
        ("left", 0.7),
        ("right", 0.7),
        ("top", 0.75),
        ("bottom", 0.75),
    ]
    .iter()
    .all(|(key, value)| (margins[*key].as_f64().unwrap_or(*value) - value).abs() < 1e-9);
    setup["paper"] != 1
        || setup["orientation"] == "landscape"
        || setup["scale"] != 100
        || setup["fitToPage"] == true
        || setup["headings"] == true
        || setup["centerHorizontally"] == true
        || setup["centerVertically"] == true
        || !normal
        || setup["rowBreaks"].as_array().is_some_and(|b| !b.is_empty())
        || setup["colBreaks"].as_array().is_some_and(|b| !b.is_empty())
        || setup["printArea"].as_str().is_some_and(|a| !a.is_empty())
        || !setup["titleRows"].is_null()
        || !setup["titleCols"].is_null()
}

fn numbers(value: &Value) -> Vec<u64> {
    value
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ignore = args.iter().any(|arg| arg == "--ignore-print-area");
    let bytes = fs::read(&args[0]).expect("read the workbook");
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).expect("open the workbook");
    while engine.load_step(1000.0).expect("read the workbook").phase == LoadPhase::Reading {}
    let book: Value = serde_json::from_str(&engine.book_json()).expect("the book's sheets");
    let mut out = Vec::new();
    for (index, sheet) in book["sheets"].as_array().into_iter().flatten().enumerate() {
        if sheet["kind"] != "worksheet" || sheet["deleted"] == true {
            continue;
        }
        let question = format!(
            r#"{{"kind":"pages","sheet":{index},"request":{{"ignorePrintArea":{ignore}}}}}"#
        );
        let answer: Value =
            serde_json::from_str(&engine.query(&question).expect("pages")).expect("pages as JSON");
        let setup: Value = serde_json::from_str(
            &engine
                .query(&format!(r#"{{"kind":"pageSetup","sheet":{index}}}"#))
                .expect("page setup"),
        )
        .expect("page setup as JSON");
        // Breaks as the first line of the next page, as Excel's locations say them.
        let after = |key: &str| numbers(&answer[key]).into_iter().map(|n| n + 1).collect();
        let mut extent = [u64::MAX, u64::MAX, 0, 0];
        for page in answer["pages"].as_array().into_iter().flatten() {
            let rows = numbers(&page["rows"]);
            let cols = numbers(&page["cols"]);
            if let (Some(&top), Some(&bottom), Some(&left), Some(&right)) =
                (rows.first(), rows.last(), cols.first(), cols.last())
            {
                extent = [
                    extent[0].min(top),
                    extent[1].min(left),
                    extent[2].max(bottom),
                    extent[3].max(right),
                ];
            }
        }
        out.push(SheetPages {
            sheet: index as u32,
            name: sheet["name"].as_str().unwrap_or_default().to_string(),
            pages: answer["pages"].as_array().map_or(0, Vec::len),
            zoom: answer["zoom"].as_u64().unwrap_or(100),
            rows: after("rowBreaks"),
            cols: after("colBreaks"),
            manual_rows: after("manualRows"),
            manual_cols: after("manualCols"),
            has_setup: has_setup(&setup),
            setup,
            extent,
        });
    }
    println!("{}", serde_json::to_string(&out).expect("pages as JSON"));
}
