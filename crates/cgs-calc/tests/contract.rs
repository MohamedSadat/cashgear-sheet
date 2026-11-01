//! What every adapter must do. Each test runs once per engine that is compiled in.
#![cfg(any(feature = "formualizer", feature = "ironcalc"))]

use std::collections::HashMap;

use cgs_calc::{
    BookInfo, CalcEngine, CalcProgress, CalcValue, CellEdit, CellInput, CellRef, CellSink,
    FormulaShape, NameInfo, Rect, SheetCells, SheetRect, StructuralOp, TableInfo,
};

fn engines() -> Vec<(&'static str, Box<dyn CalcEngine>)> {
    vec![
        #[cfg(feature = "formualizer")]
        (
            "formualizer",
            Box::new(cgs_calc::formualizer::FormualizerEngine::new()),
        ),
        #[cfg(feature = "ironcalc")]
        (
            "ironcalc",
            Box::new(cgs_calc::ironcalc::IronCalcEngine::new()),
        ),
    ]
}

fn num(value: f64) -> CellInput {
    CellInput::Value(CalcValue::Number(value))
}

fn text(value: &str) -> CellInput {
    CellInput::Value(CalcValue::Text(value.to_string()))
}

fn formula(formula: &str, cached: f64) -> CellInput {
    CellInput::Formula {
        text: formula.to_string(),
        shape: FormulaShape::Normal,
        cached: Some(CalcValue::Number(cached)),
    }
}

fn info() -> BookInfo {
    BookInfo {
        sheets: vec!["Sheet1".to_string(), "Other Sheet".to_string()],
        ..BookInfo::default()
    }
}

fn load(engine: &mut dyn CalcEngine, info: &BookInfo, sheets: &[Vec<(u32, u32, CellInput)>]) {
    engine.begin_load(info).unwrap();
    for (index, cells) in sheets.iter().enumerate() {
        engine
            .load_batch(&SheetCells {
                sheet: index as u32,
                cells,
            })
            .unwrap();
    }
    engine.finish_load().unwrap();
}

fn settle(engine: &mut dyn CalcEngine) {
    for _ in 0..100_000 {
        if engine.calc_slice(8, &[]).unwrap() == CalcProgress::Done {
            return;
        }
    }
    panic!("calculation never finished");
}

#[derive(Default)]
struct Grid(HashMap<(u32, u32), CalcValue>);

impl CellSink for Grid {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.insert((row, col), value.clone());
    }
}

fn grid(engine: &dyn CalcEngine, sheet: u32) -> Grid {
    let mut out = Grid::default();
    engine.read_block(
        SheetRect {
            sheet,
            rect: Rect {
                top: 1,
                left: 1,
                bottom: 40,
                right: 12,
            },
        },
        &mut out,
    );
    out
}

fn at(engine: &dyn CalcEngine, sheet: u32, row: u32, col: u32) -> CalcValue {
    grid(engine, sheet)
        .0
        .get(&(row, col))
        .cloned()
        .unwrap_or(CalcValue::Empty)
}

#[test]
fn cached_values_read_back_without_calculating() {
    for (name, mut engine) in engines() {
        // The cached value is wrong on purpose: seeing it proves nothing calculated.
        let sheet = vec![(1, 1, num(2.0)), (1, 2, formula("A1*10", 999.0))];
        load(engine.as_mut(), &info(), &[sheet, Vec::new()]);
        assert_eq!(
            at(engine.as_ref(), 0, 1, 1),
            CalcValue::Number(2.0),
            "{name}"
        );
        assert_eq!(
            at(engine.as_ref(), 0, 1, 2),
            CalcValue::Number(999.0),
            "{name}"
        );
    }
}

#[test]
fn recalc_all_replaces_cached_values() {
    for (name, mut engine) in engines() {
        let sheet = vec![
            (1, 1, num(2.0)),
            (1, 2, formula("A1*10", 999.0)),
            (2, 2, formula("B1+'Other Sheet'!A1", 999.0)),
        ];
        load(engine.as_mut(), &info(), &[sheet, vec![(1, 1, num(5.0))]]);
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 1, 2),
            CalcValue::Number(20.0),
            "{name}"
        );
        assert_eq!(
            at(engine.as_ref(), 0, 2, 2),
            CalcValue::Number(25.0),
            "{name}"
        );
    }
}

#[test]
fn an_edit_reaches_its_dependents_and_returns_the_pre_image() {
    for (name, mut engine) in engines() {
        let sheet = vec![(1, 1, num(2.0)), (1, 2, formula("A1*10", 20.0))];
        load(engine.as_mut(), &info(), &[sheet, Vec::new()]);
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        let before = engine
            .set_cells(&[CellEdit {
                at: CellRef {
                    sheet: 0,
                    row: 1,
                    col: 1,
                },
                input: num(3.0),
            }])
            .unwrap();
        assert_eq!(before[0].input, num(2.0), "{name}");
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 1, 2),
            CalcValue::Number(30.0),
            "{name}"
        );
        assert!(!engine.take_changed().is_empty(), "{name}");
    }
}

#[test]
fn an_edit_right_after_loading_recalculates_dependents() {
    for (name, mut engine) in engines() {
        let sheet = vec![(1, 1, num(2.0)), (1, 2, formula("A1*10", 20.0))];
        load(engine.as_mut(), &info(), &[sheet, Vec::new()]);
        engine
            .set_cells(&[CellEdit {
                at: CellRef {
                    sheet: 0,
                    row: 1,
                    col: 1,
                },
                input: num(3.0),
            }])
            .unwrap();
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 1, 2),
            CalcValue::Number(30.0),
            "{name}"
        );
    }
}

#[test]
fn names_and_tables_resolve() {
    for (name, mut engine) in engines() {
        let info = BookInfo {
            names: vec![
                NameInfo {
                    name: "Rate".to_string(),
                    formula: "Sheet1!$D$1".to_string(),
                    local_sheet: None,
                },
                NameInfo {
                    name: "Doubled".to_string(),
                    formula: "Sheet1!$D$1*2".to_string(),
                    local_sheet: None,
                },
            ],
            tables: vec![TableInfo {
                name: "Sales".to_string(),
                sheet: 0,
                range: Rect {
                    top: 1,
                    left: 1,
                    bottom: 4,
                    right: 2,
                },
                header_row: true,
                totals_row: true,
                columns: vec!["Item".to_string(), "Total Cost".to_string()],
            }],
            ..info()
        };
        let sheet = vec![
            (1, 1, text("Item")),
            (1, 2, text("Total Cost")),
            (2, 1, text("a")),
            (2, 2, num(10.0)),
            (3, 1, text("b")),
            (3, 2, num(20.0)),
            (4, 1, text("Total")),
            (4, 2, formula("SUBTOTAL(109,Sales[Total Cost])", 0.0)),
            (1, 4, num(0.5)),
            (1, 5, formula("Rate*4", 0.0)),
            (1, 6, formula("Doubled", 0.0)),
            (1, 7, formula("SUM(Sales[Total Cost])", 0.0)),
        ];
        load(engine.as_mut(), &info, &[sheet, Vec::new()]);
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        let read = grid(engine.as_ref(), 0);
        // Formualizer 0.13.0 reports #CIRC! for a totals row that reads its own table.
        if name != "formualizer" {
            assert_eq!(read.0[&(4, 2)], CalcValue::Number(30.0), "{name} subtotal");
        }
        assert_eq!(read.0[&(1, 5)], CalcValue::Number(2.0), "{name} name");
        // IronCalc 0.9.1 gives #NAME? for a name that isn't a plain reference or LAMBDA.
        if name != "ironcalc" {
            assert_eq!(
                read.0[&(1, 6)],
                CalcValue::Number(1.0),
                "{name} formula name"
            );
        }
        assert_eq!(read.0[&(1, 7)], CalcValue::Number(30.0), "{name} table sum");
    }
}

#[test]
fn hidden_rows_reach_subtotal() {
    for (name, mut engine) in engines() {
        let info = BookInfo {
            hidden_rows: vec![(0, 2)],
            ..info()
        };
        let sheet = vec![
            (1, 1, num(1.0)),
            (2, 1, num(2.0)),
            (3, 1, num(4.0)),
            (1, 2, formula("SUBTOTAL(109,A1:A3)", 0.0)),
            (2, 2, formula("SUBTOTAL(9,A1:A3)", 0.0)),
        ];
        load(engine.as_mut(), &info, &[sheet, Vec::new()]);
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 1, 2),
            CalcValue::Number(5.0),
            "{name}"
        );
        assert_eq!(
            at(engine.as_ref(), 0, 2, 2),
            CalcValue::Number(7.0),
            "{name}"
        );
    }
}

#[test]
fn arrays_fill_their_range() {
    for (name, mut engine) in engines() {
        let range = Rect {
            top: 1,
            left: 3,
            bottom: 2,
            right: 3,
        };
        let sheet = vec![
            (1, 1, num(1.0)),
            (2, 1, num(2.0)),
            (
                1,
                3,
                CellInput::Formula {
                    text: "A1:A2*10".to_string(),
                    shape: FormulaShape::Array(range),
                    cached: Some(CalcValue::Number(0.0)),
                },
            ),
            (
                2,
                3,
                CellInput::Spill {
                    anchor: (1, 3),
                    cached: CalcValue::Number(0.0),
                },
            ),
        ];
        load(engine.as_mut(), &info(), &[sheet, Vec::new()]);
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 1, 3),
            CalcValue::Number(10.0),
            "{name}"
        );
        assert_eq!(
            at(engine.as_ref(), 0, 2, 3),
            CalcValue::Number(20.0),
            "{name}"
        );
    }
}

#[test]
fn dynamic_text_arrays_spill() {
    for (name, mut engine) in engines() {
        let range = Rect {
            top: 1,
            left: 3,
            bottom: 2,
            right: 3,
        };
        let sheet = vec![
            (1, 1, text("hello")),
            (2, 1, text("world")),
            (
                1,
                3,
                CellInput::Formula {
                    text: "LEFT(A1:A2,2)".to_string(),
                    shape: FormulaShape::Dynamic(range),
                    cached: Some(CalcValue::Text("he".to_string())),
                },
            ),
            (
                2,
                3,
                CellInput::Spill {
                    anchor: (1, 3),
                    cached: CalcValue::Text("wo".to_string()),
                },
            ),
        ];
        load(engine.as_mut(), &info(), &[sheet, Vec::new()]);
        assert_eq!(
            at(engine.as_ref(), 0, 2, 3),
            CalcValue::Text("wo".to_string()),
            "{name} cached spill"
        );
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 1, 3),
            CalcValue::Text("he".to_string()),
            "{name} anchor"
        );
        // Formualizer 0.13.0 doesn't lift a text function over a range: LEFT gives one value.
        if name != "formualizer" {
            assert_eq!(
                at(engine.as_ref(), 0, 2, 3),
                CalcValue::Text("wo".to_string()),
                "{name} spill"
            );
        }
    }
}

#[test]
fn inserting_a_row_moves_cells_and_keeps_references() {
    for (name, mut engine) in engines() {
        let sheet = vec![
            (1, 1, num(2.0)),
            (2, 1, num(3.0)),
            (3, 1, formula("SUM(A1:A2)", 5.0)),
        ];
        load(engine.as_mut(), &info(), &[sheet, Vec::new()]);
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        let undo = engine
            .structural(&StructuralOp::InsertRows {
                sheet: 0,
                at: 1,
                count: 1,
            })
            .unwrap();
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 4, 1),
            CalcValue::Number(5.0),
            "{name}"
        );
        assert_eq!(
            engine.formula_text(CellRef {
                sheet: 0,
                row: 4,
                col: 1
            }),
            Some("SUM(A2:A3)".to_string()),
            "{name}"
        );
        assert_eq!(
            undo.inverse,
            StructuralOp::DeleteRows {
                sheet: 0,
                at: 1,
                count: 1
            },
            "{name}"
        );
    }
}

#[test]
fn deleting_a_row_a_formula_needs_gives_ref_error() {
    for (name, mut engine) in engines() {
        let sheet = vec![(1, 1, num(2.0)), (2, 1, formula("A1+1", 3.0))];
        load(engine.as_mut(), &info(), &[sheet, Vec::new()]);
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        engine
            .structural(&StructuralOp::DeleteRows {
                sheet: 0,
                at: 1,
                count: 1,
            })
            .unwrap();
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 1, 1),
            CalcValue::Error("#REF!".to_string()),
            "{name}"
        );
    }
}
