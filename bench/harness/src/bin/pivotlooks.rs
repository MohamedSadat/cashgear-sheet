//! What the app's pivot styles do to each pivot table cell of a workbook, for comparing with what
//! Excel shows (`bench/run-pivot-looks.mjs`).
//! `pivotlooks <workbook.xlsx> <out.json>`

use std::rc::Rc;

use cgs_engine::{Engine, LoadPhase};
use serde_json::{json, Value};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err("usage: pivotlooks <workbook.xlsx> <out.json>".into());
    }
    let mut engine = Engine::new();
    engine
        .open(Rc::new(std::fs::read(&args[1]).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    while engine.load_step(10_000.0).map_err(|e| e.to_string())?.phase == LoadPhase::Reading {}
    let names: Vec<String> = engine
        .book()
        .ok_or("no book")?
        .sheets
        .iter()
        .map(|s| s.name.clone())
        .collect();
    let mut out = Vec::new();
    for (sheet, name) in names.iter().enumerate() {
        let list: Value =
            serde_json::from_str(&engine.pivot_look_json(sheet as u32)).unwrap_or(json!([]));
        for mut pivot in list.as_array().cloned().unwrap_or_default() {
            pivot["sheet"] = json!(name);
            out.push(pivot);
        }
    }
    std::fs::write(&args[2], serde_json::to_string(&out).unwrap()).map_err(|e| e.to_string())?;
    Ok(())
}
