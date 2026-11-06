//! Refresh against Excel: opens the workbook Excel made (`bench/excel-make-pivots.ps1`), makes the
//! same source edits Excel made, refreshes every pivot table through the engine, and compares
//! each one, cell by cell and line by line, with the copy Excel refreshed. Then it saves the result
//! for Excel to open.
//! `pivotrefresh <excel-pivots.xlsx> <excel-refreshed.xlsx> <cases.json> <out.xlsx> <report.json> [--english]`

use std::rc::Rc;

use calamine::{open_workbook_auto, Data, Reader};
use cgs_calc::CalcValue;
use cgs_engine::{Engine, LoadPhase};
use cgs_xlsx::package::Package;
use serde_json::{json, Value};

#[path = "pivotcheck.rs"]
#[allow(dead_code)]
mod pivotcheck;

fn settle(engine: &mut Engine) {
    while engine.calc_step(10_000, &[]).unwrap_or(false) {}
    engine.take_dirty();
}

fn same(ours: &CalcValue, theirs: &Data) -> bool {
    let close = |a: f64, b: f64| a == b || (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    match (ours, theirs) {
        (CalcValue::Empty, Data::Empty) => true,
        (CalcValue::Text(t), Data::Empty) | (CalcValue::Text(t), Data::String(_))
            if t.is_empty() =>
        {
            matches!(theirs, Data::Empty) || matches!(theirs, Data::String(s) if s.is_empty())
        }
        (CalcValue::Text(a), Data::String(b)) => a == b,
        (CalcValue::Number(a), Data::Float(b)) => close(*a, *b),
        (CalcValue::Number(a), Data::Int(b)) => close(*a, *b as f64),
        (CalcValue::Number(a), Data::DateTime(b)) => close(*a, b.as_f64()),
        (CalcValue::Bool(a), Data::Bool(b)) => a == b,
        (CalcValue::Error(a), Data::Error(b)) => {
            let norm = |s: &str| {
                s.trim_start_matches('#')
                    .to_lowercase()
                    .replace(['/', '!', '?'], "")
            };
            norm(a) == norm(&format!("{b}"))
        }
        _ => false,
    }
}

/// Every pivot table's cells as the engine holds them, by sheet name.
fn dump(engine: &mut Engine) -> Vec<Value> {
    let Some(book) = engine.book() else {
        return Vec::new();
    };
    let names: Vec<String> = book.sheets.iter().map(|s| s.name.clone()).collect();
    let mut out = Vec::new();
    for (sheet, name) in names.iter().enumerate() {
        let list: Value =
            serde_json::from_str(&engine.pivot_list(sheet as u32)).unwrap_or(json!([]));
        for p in list.as_array().cloned().unwrap_or_default() {
            let (top, left) = (
                p["top"].as_u64().unwrap_or(1) as u32,
                p["left"].as_u64().unwrap_or(1) as u32,
            );
            let (bottom, right) = (
                p["bottom"].as_u64().unwrap_or(1) as u32,
                p["right"].as_u64().unwrap_or(1) as u32,
            );
            let rows: Vec<Vec<Value>> = (top..=bottom)
                .map(|row| {
                    (left..=right)
                        .map(|col| match engine.cell_value(sheet as u32, row, col) {
                            CalcValue::Empty => Value::Null,
                            CalcValue::Number(n) => json!(n),
                            CalcValue::Text(t) => json!(t),
                            CalcValue::Bool(b) => json!(b),
                            CalcValue::Error(e) => json!({ "error": e }),
                        })
                        .collect()
                })
                .collect();
            let empty_text = engine
                .pivot_info(sheet as u32, top, left)
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .and_then(|i| i["options"]["emptyText"].as_str().map(str::to_string));
            out.push(json!({ "sheet": name, "name": p["name"], "top": top, "left": left, "rows": rows, "emptyText": empty_text }));
        }
    }
    out
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        return Err("usage: pivotrefresh <excel-pivots.xlsx> <excel-refreshed.xlsx> <cases.json> <out.xlsx> <report.json> [--english]".into());
    }
    let english = args.iter().any(|a| a == "--english");
    let cases: Value =
        serde_json::from_str(&std::fs::read_to_string(&args[3]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut engine = Engine::new();
    if !english {
        engine.set_pivot_captions(pivotcheck::arabic());
    }
    let bytes = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    engine.open(Rc::new(bytes)).map_err(|e| e.to_string())?;
    while engine.load_step(10_000.0).map_err(|e| e.to_string())?.phase == LoadPhase::Reading {}
    while engine.prepare_step().map_err(|e| e.to_string())? {}
    settle(&mut engine);
    // The source edits, on the data sheet, as Excel made them.
    for edit in cases["edits"].as_array().cloned().unwrap_or_default() {
        let row = edit["row"].as_u64().unwrap_or(1);
        let col = edit["col"].as_u64().unwrap_or(1);
        let command = match &edit["value"] {
            Value::Null => {
                json!({ "kind": "clear", "sheet": 0, "areas": [{ "top": row, "left": col, "bottom": row, "right": col }] })
            }
            Value::String(s) if s.starts_with("date:") => {
                json!({ "kind": "input", "sheet": 0, "row": row, "col": col, "text": s[5..] })
            }
            Value::String(s) => {
                json!({ "kind": "input", "sheet": 0, "row": row, "col": col, "text": s })
            }
            other => {
                json!({ "kind": "input", "sheet": 0, "row": row, "col": col, "text": other.to_string() })
            }
        };
        engine
            .apply_json(&command.to_string())
            .map_err(|e| format!("{command}: {e}"))?;
        settle(&mut engine);
    }
    engine.apply_json(r#"{"kind":"refreshPivot"}"#)?;
    settle(&mut engine);

    // Excel's refreshed copy: its definitions and its cells.
    let theirs_bytes = std::fs::read(&args[2]).map_err(|e| e.to_string())?;
    let package = Package::open(Rc::new(theirs_bytes)).map_err(|e| e.to_string())?;
    let theirs_book = cgs_xlsx::workbook::read_workbook(&package).map_err(|e| e.to_string())?;
    let mut sheets = open_workbook_auto(&args[2]).map_err(|e| e.to_string())?;
    let ours_book = engine.book().ok_or("no book")?.clone();
    let mut report = Vec::new();
    let (mut tables_ok, mut lines_ok, mut cells, mut wrong_cells) = (0, 0, 0u64, 0u64);
    for theirs in &theirs_book.pivot_tables {
        let sheet_name = &theirs_book.sheets[theirs.sheet as usize].name;
        let Some(ours) = ours_book
            .pivot_tables
            .iter()
            .find(|t| ours_book.sheets[t.sheet as usize].name == *sheet_name)
        else {
            report.push(json!({ "sheet": sheet_name, "error": "missing" }));
            continue;
        };
        let range = sheets
            .worksheet_range(sheet_name)
            .map_err(|e| e.to_string())?;
        let a = theirs.location.range;
        let b = ours.location.range;
        let pages = theirs.page_fields.len() as u32;
        let top = a
            .top
            .min(b.top)
            .saturating_sub(if pages > 0 { pages + 1 } else { 0 })
            .max(1);
        let mut wrong = Vec::new();
        for row in top..=a.bottom.max(b.bottom) {
            for col in a.left.min(b.left)..=a.right.max(b.right) {
                let mine = engine.cell_value(ours.sheet, row, col);
                let their = range
                    .get_value((row - 1, col - 1))
                    .cloned()
                    .unwrap_or(Data::Empty);
                if mine == CalcValue::Empty && their == Data::Empty {
                    continue;
                }
                cells += 1;
                // Excel saves 0 where it shows the caption for empty cells.
                let caption = theirs.options.missing_caption.clone().unwrap_or_default();
                let shows_caption = matches!((&mine, &their), (CalcValue::Text(t), Data::Float(z)) if *z == 0.0 && *t == caption);
                if !same(&mine, &their) && !shows_caption {
                    wrong_cells += 1;
                    if wrong.len() < 12 {
                        wrong.push(format!("R{row}C{col}: ours {mine:?} theirs {their:?}"));
                    }
                }
            }
        }
        let lines = ours.location == theirs.location
            && ours.row_items == theirs.row_items
            && ours.col_items == theirs.col_items;
        if !lines {
            wrong.push(format!(
                "lines: location {} rowItems {} colItems {}",
                ours.location == theirs.location,
                ours.row_items == theirs.row_items,
                ours.col_items == theirs.col_items
            ));
        }
        let cells_ok = wrong.iter().all(|w| w.starts_with("lines"));
        tables_ok += u32::from(cells_ok);
        lines_ok += u32::from(lines);
        if !(cells_ok && lines) {
            println!("FAIL {sheet_name}");
            for w in wrong.iter().take(8) {
                println!("    {w}");
            }
        }
        report.push(json!({ "sheet": sheet_name, "cellsMatch": cells_ok, "linesMatch": lines, "wrong": wrong }));
    }
    let mut saved = Vec::new();
    engine.save(&mut saved).map_err(|e| e.to_string())?;
    std::fs::write(&args[4], saved).map_err(|e| e.to_string())?;
    let refreshed_values = dump(&mut engine);
    // Then rearranged as the cases say, for Excel to lay out the same definitions again.
    let mut changed = Vec::new();
    for change in cases["changes"].as_array().cloned().unwrap_or_default() {
        let result = engine.apply_json(&change.to_string());
        settle(&mut engine);
        changed.push(json!({ "command": change, "error": result.err() }));
    }
    let changed_path = args[4].replace(".xlsx", "-changed.xlsx");
    let mut saved = Vec::new();
    engine.save(&mut saved).map_err(|e| e.to_string())?;
    std::fs::write(&changed_path, saved).map_err(|e| e.to_string())?;
    let changed_values = dump(&mut engine);
    let total = theirs_book.pivot_tables.len();
    println!(
        "pivots {total} | cells match {tables_ok} | lines match {lines_ok} | cells compared {cells} wrong {wrong_cells}"
    );
    std::fs::write(
        &args[5],
        serde_json::to_string_pretty(&json!({
            "pivots": total,
            "cellsMatch": tables_ok,
            "linesMatch": lines_ok,
            "cellsCompared": cells,
            "cellsWrong": wrong_cells,
            "tables": report,
            "refreshedValues": refreshed_values,
            "changes": changed,
            "changedValues": changed_values,
        }))
        .unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
