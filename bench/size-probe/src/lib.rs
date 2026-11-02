//! Loads one formula given at run time, so the linker must keep the parser and every
//! built-in function. Its size is what the engine and adapter add to the shipped WASM.

#![cfg(target_arch = "wasm32")]

use cgs_calc::{
    BookInfo, CalcEngine, CalcProgress, CalcValue, CellInput, FormulaShape, SheetCells,
};
use cgs_calc::{CellSink, Rect, SheetRect};
use wasm_bindgen::prelude::wasm_bindgen;

struct First(f64);

impl CellSink for First {
    fn cell(&mut self, _row: u32, _col: u32, value: &CalcValue) {
        if let CalcValue::Number(number) = value {
            self.0 = *number;
        }
    }
}

#[wasm_bindgen]
pub fn probe(formula: &str) -> f64 {
    let mut engine = cgs_calc::formualizer::FormualizerEngine::new();
    let info = BookInfo {
        sheets: vec!["Sheet1".to_string()],
        ..BookInfo::default()
    };
    let cells = vec![(
        1,
        1,
        CellInput::Formula {
            text: formula.to_string(),
            shape: FormulaShape::Normal,
            cached: None,
        },
    )];
    if engine.begin_load(&info).is_err()
        || engine
            .load_batch(&SheetCells {
                sheet: 0,
                cells: &cells,
            })
            .is_err()
        || engine.finish_load().is_err()
        || engine.recalc_all().is_err()
    {
        return f64::NAN;
    }
    while !matches!(engine.calc_slice(8, &[]), Ok(CalcProgress::Done) | Err(_)) {}
    let mut out = First(f64::NAN);
    engine.read_block(
        SheetRect {
            sheet: 0,
            rect: Rect {
                top: 1,
                left: 1,
                bottom: 1,
                right: 1,
            },
        },
        &mut out,
    );
    out.0
}
