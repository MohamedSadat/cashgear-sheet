//! Runs a workbook through an engine's own xlsx importer and scores it the same way,
//! to tell adapter bugs from engine gaps. Run: `own-loader <engine> <file>`.

use std::collections::HashMap;

use cgs_bench::book::read_book;
use cgs_bench::run::score_with;
use cgs_calc::CalcValue;
use formualizer_eval::engine::{EvalConfig, TemporalEgress};
use formualizer_workbook::traits::{LoadStrategy, SpreadsheetReader};
use formualizer_workbook::{CalamineAdapter, Workbook, WorkbookConfig};
use ironcalc::base::cell::CellValue;
use ironcalc::base::types::CellType;
use ironcalc::base::Model;

fn formualizer(bytes: &[u8]) -> Result<Workbook, String> {
    let adapter = CalamineAdapter::open_bytes(bytes.to_vec()).map_err(|e| e.to_string())?;
    let config = WorkbookConfig {
        eval: EvalConfig {
            enable_parallel: false,
            temporal_egress: TemporalEgress::Serial,
            ..EvalConfig::default()
        },
        enable_changelog: false,
        ingest_limits: Default::default(),
    };
    let mut workbook = Workbook::from_reader(adapter, LoadStrategy::EagerAll, config)
        .map_err(|e| e.to_string())?;
    workbook.evaluate_all().map_err(|e| e.to_string())?;
    Ok(workbook)
}

fn ironcalc_model(bytes: &[u8]) -> Result<Model<'static>, String> {
    let workbook = ironcalc::import::load_from_xlsx_bytes(bytes, "workbook", "en", "UTC")
        .map_err(|e| e.to_string())?;
    let mut model = Model::from_workbook(workbook, "en")?;
    model.evaluate();
    Ok(model)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let engine = args[1].clone();
    let bytes = std::fs::read(&args[2]).expect("read workbook");
    let worker = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || -> Result<String, String> {
            let book = read_book(&bytes)?;
            let names: Vec<String> = book.sheets.iter().map(|s| s.name.clone()).collect();
            let score = if engine == "formualizer" {
                let workbook = formualizer(&bytes)?;
                score_with(&book, &|sheet, (rows, cols)| {
                    let mut out = HashMap::new();
                    let values = workbook.engine().get_range_values(
                        &names[sheet as usize],
                        1,
                        1,
                        rows,
                        cols,
                    );
                    for (r, row) in values.iter().enumerate() {
                        for (c, value) in row.iter().enumerate() {
                            let value = cgs_calc::formualizer::from_literal(value);
                            if value != CalcValue::Empty {
                                out.insert((r as u32 + 1, c as u32 + 1), value);
                            }
                        }
                    }
                    out
                })
            } else {
                let model = ironcalc_model(&bytes)?;
                score_with(&book, &|sheet, (rows, cols)| {
                    let mut out = HashMap::new();
                    for row in 1..=rows {
                        for col in 1..=cols {
                            let (r, c) = (row as i32, col as i32);
                            let value = match model.get_cell_value_by_index(sheet, r, c) {
                                Ok(CellValue::Number(n)) => CalcValue::Number(n),
                                Ok(CellValue::Boolean(b)) => CalcValue::Bool(b),
                                Ok(CellValue::String(s)) => {
                                    if matches!(
                                        model.get_cell_type(sheet, r, c),
                                        Ok(CellType::ErrorValue)
                                    ) {
                                        CalcValue::Error(s)
                                    } else {
                                        CalcValue::Text(s)
                                    }
                                }
                                _ => continue,
                            };
                            out.insert((row, col), value);
                        }
                    }
                    out
                })
            };
            serde_json::to_string(&score).map_err(|e| e.to_string())
        })
        .expect("spawn");
    match worker.join() {
        Ok(Ok(json)) => println!("{{\"ok\":true,\"score\":{json}}}"),
        Ok(Err(error)) => println!("{}", serde_json::json!({ "ok": false, "error": error })),
        Err(_) => println!("{{\"ok\":false,\"error\":\"panic\"}}"),
    }
}
