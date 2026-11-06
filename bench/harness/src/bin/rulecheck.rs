//! Builds Phase 6's rule cases through the engine for `bench/run-excel-rules.mjs`: each case's
//! cells go into a new workbook, its commands run, and the file is saved for Excel to judge. What
//! the engine made of it goes out beside: conditional formats' colours, the cells validation
//! refuses, the rows a filter hides, and what a sort left. A sort's case is also saved before its
//! commands run, for Excel to sort itself.
//!
//! Usage: rulecheck <cases.json> <folder> <out.json> <today serial>

use std::fs;
use std::path::Path;
use std::rc::Rc;

use cgs_engine::{Engine, LoadPhase};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Case {
    name: String,
    kind: String,
    /// (row, col, text as typed), 1-based.
    cells: Vec<(u32, u32, String)>,
    /// Commands that make the case's starting point, before a sort's unsorted copy is saved.
    #[serde(default)]
    setup: Vec<Value>,
    #[serde(default)]
    commands: Vec<Value>,
    area: Area,
}

#[derive(Deserialize, Clone, Copy)]
struct Area {
    top: u32,
    left: u32,
    bottom: u32,
    right: u32,
}

fn settle(engine: &mut Engine) {
    while engine.calc_step(10_000, &[]).unwrap_or(false) {}
    engine.take_dirty();
}

fn save(engine: &mut Engine, path: &Path) -> Result<(), String> {
    let mut bytes = Vec::new();
    engine.save(&mut bytes).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

fn run(case: &Case, folder: &Path, today: f64) -> Result<Value, String> {
    let mut engine = Engine::new();
    engine
        .open(Rc::new(
            cgs_xlsx::blank::blank_workbook().map_err(|error| error.to_string())?,
        ))
        .map_err(|error| error.to_string())?;
    while engine.load_step(10_000.0).map_err(|e| e.to_string())?.phase == LoadPhase::Reading {}
    engine.set_today(today);
    for (row, col, text) in &case.cells {
        let input = json!({ "kind": "input", "sheet": 0, "row": row, "col": col, "text": text });
        engine.apply_json(&input.to_string())?;
    }
    settle(&mut engine);
    for command in &case.setup {
        engine
            .apply_json(&command.to_string())
            .map_err(|error| format!("{command}: {error}"))?;
        settle(&mut engine);
    }
    let before = if case.kind == "sort" {
        let path = folder.join(format!("{}-before.xlsx", case.name));
        save(&mut engine, &path)?;
        Some(path.display().to_string())
    } else {
        None
    };
    for command in &case.commands {
        engine
            .apply_json(&command.to_string())
            .map_err(|error| format!("{command}: {error}"))?;
        settle(&mut engine);
    }
    let file = folder.join(format!("{}.xlsx", case.name));
    save(&mut engine, &file)?;
    let a = case.area;
    let values: Vec<Vec<String>> = (a.top..=a.bottom)
        .map(|row| {
            (a.left..=a.right)
                .map(|col| engine.display_text(0, row, col))
                .collect()
        })
        .collect();
    let layout = engine.layout(0).ok_or("no layout")?;
    let hidden: Vec<u32> = (a.top..=a.bottom)
        .filter(|&row| {
            layout
                .rows
                .iter()
                .any(|&(first, last, px)| px == 0 && row >= first && row <= last)
        })
        .collect();
    let area = json!({ "top": a.top, "left": a.left, "bottom": a.bottom, "right": a.right });
    let ask = |question: Value| -> Result<Value, String> {
        serde_json::from_str(&engine.query(&question.to_string())?).map_err(|e| e.to_string())
    };
    let cf = ask(json!({ "kind": "cfLook", "sheet": 0, "area": area }))?;
    let invalid = ask(json!({ "kind": "invalidCells", "sheet": 0 }))?;
    Ok(json!({
        "name": case.name,
        "kind": case.kind,
        "file": file.display().to_string(),
        "before": before,
        "values": values,
        "hidden": hidden,
        "cf": cf,
        "invalid": invalid,
    }))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: rulecheck <cases.json> <folder> <out.json> <today serial>");
        std::process::exit(2);
    }
    let cases: Vec<Case> =
        serde_json::from_str(&fs::read_to_string(&args[1]).expect("the cases")).expect("cases");
    let folder = Path::new(&args[2]);
    let today: f64 = args[4].parse().expect("today's serial");
    let results: Vec<Value> = cases
        .iter()
        .map(|case| {
            run(case, folder, today)
                .unwrap_or_else(|error| json!({ "name": case.name, "error": error }))
        })
        .collect();
    fs::write(&args[3], serde_json::to_string_pretty(&results).unwrap()).expect("the results");
}
