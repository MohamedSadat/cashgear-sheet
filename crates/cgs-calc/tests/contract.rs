//! What the engine adapter must do.

use std::collections::HashMap;

use cgs_calc::{
    BookInfo, CalcEngine, CalcProgress, CalcValue, CellEdit, CellInput, CellRef, CellSink,
    FormulaShape, NameInfo, Rect, SheetCells, SheetRect, StructuralOp, TableInfo,
};

fn engines() -> Vec<(&'static str, Box<dyn CalcEngine>)> {
    vec![(
        "formualizer",
        Box::new(cgs_calc::formualizer::FormualizerEngine::new()),
    )]
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
fn slices_finish_and_visible_cells_can_come_first() {
    for (name, mut engine) in engines() {
        let mut sheet = vec![(1, 1, num(1.0))];
        for row in 2..2000 {
            sheet.push((row, 1, formula(&format!("A{}+1", row - 1), 0.0)));
        }
        load(engine.as_mut(), &info(), &[sheet, Vec::new()]);
        engine.recalc_all().unwrap();
        let visible = SheetRect {
            sheet: 0,
            rect: Rect {
                top: 1000,
                left: 1,
                bottom: 1000,
                right: 1,
            },
        };
        // A very short budget forces many calls, and nothing may be lost between them.
        let mut calls = 0;
        loop {
            calls += 1;
            let first = if calls == 1 {
                vec![visible]
            } else {
                Vec::new()
            };
            if engine.calc_slice(0, &first).unwrap() == CalcProgress::Done {
                break;
            }
            assert!(calls < 100_000, "{name} never finished");
            if calls == 1 {
                let mut out = Grid::default();
                engine.read_block(visible, &mut out);
                assert_eq!(
                    out.0[&(1000, 1)],
                    CalcValue::Number(1000.0),
                    "{name} visible first"
                );
            }
        }
        assert_eq!(
            at(engine.as_ref(), 0, 40, 1),
            CalcValue::Number(40.0),
            "{name}"
        );
        let mut out = Grid::default();
        engine.read_block(
            SheetRect {
                sheet: 0,
                rect: Rect {
                    top: 1999,
                    left: 1,
                    bottom: 1999,
                    right: 1,
                },
            },
            &mut out,
        );
        assert_eq!(out.0[&(1999, 1)], CalcValue::Number(1999.0), "{name}");
    }
}

#[test]
fn an_edit_recalculates_only_dependents_after_a_cached_load() {
    for (name, mut engine) in engines() {
        // B1's cached value is wrong on purpose. It must stay, because nothing it
        // depends on changed.
        let sheet = vec![
            (1, 1, num(2.0)),
            (1, 2, formula("A1*10", 999.0)),
            (2, 1, num(3.0)),
            (2, 2, formula("A2*10", 30.0)),
        ];
        load(engine.as_mut(), &info(), &[sheet, Vec::new()]);
        engine
            .set_cells(&[CellEdit {
                at: CellRef {
                    sheet: 0,
                    row: 2,
                    col: 1,
                },
                input: num(4.0),
            }])
            .unwrap();
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 2, 2),
            CalcValue::Number(40.0),
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
        assert_eq!(read.0[&(4, 2)], CalcValue::Number(30.0), "{name} subtotal");
        assert_eq!(read.0[&(1, 5)], CalcValue::Number(2.0), "{name} name");
        assert_eq!(
            read.0[&(1, 6)],
            CalcValue::Number(1.0),
            "{name} formula name"
        );
        assert_eq!(read.0[&(1, 7)], CalcValue::Number(30.0), "{name} table sum");
    }
}

#[test]
fn this_row_references_read_their_row() {
    for (name, mut engine) in engines() {
        let info = BookInfo {
            tables: vec![TableInfo {
                name: "Sales".to_string(),
                sheet: 0,
                range: Rect {
                    top: 1,
                    left: 1,
                    bottom: 3,
                    right: 4,
                },
                header_row: true,
                totals_row: false,
                columns: vec![
                    "Q1".to_string(),
                    "Q2".to_string(),
                    "Sum".to_string(),
                    "Twice".to_string(),
                ],
            }],
            ..info()
        };
        let sheet = vec![
            (1, 1, text("Q1")),
            (1, 2, text("Q2")),
            (1, 3, text("Sum")),
            (1, 4, text("Twice")),
            (2, 1, num(1.0)),
            (2, 2, num(2.0)),
            (2, 3, formula("SUM(Sales[[#This Row],[Q1]:[Q2]])", 0.0)),
            (2, 4, formula("Sales[[#This Row],[Sum]]*2", 0.0)),
            (3, 1, num(3.0)),
            (3, 2, num(4.0)),
            (3, 3, formula("SUM(Sales[[#This Row],[Q1]:[Q2]])", 0.0)),
            (3, 4, formula("Sales[[#This Row],[Sum]]*2", 0.0)),
        ];
        load(engine.as_mut(), &info, &[sheet, Vec::new()]);
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        let read = grid(engine.as_ref(), 0);
        assert_eq!(read.0[&(2, 3)], CalcValue::Number(3.0), "{name}");
        assert_eq!(read.0[&(3, 3)], CalcValue::Number(7.0), "{name}");
        assert_eq!(read.0[&(3, 4)], CalcValue::Number(14.0), "{name}");
    }
}

#[test]
fn a_plain_formula_reads_a_range_at_its_own_row() {
    for (name, mut engine) in engines() {
        let sheet = vec![
            (1, 1, text("one")),
            (2, 1, text("two")),
            (3, 1, text("three")),
            (2, 3, formula("A:A&\"!\"", 0.0)),
            (3, 3, formula("A1:A3&\"?\"", 0.0)),
        ];
        load(engine.as_mut(), &info(), &[sheet, Vec::new()]);
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 2, 3),
            CalcValue::Text("two!".to_string()),
            "{name}"
        );
        assert_eq!(
            at(engine.as_ref(), 0, 3, 3),
            CalcValue::Text("three?".to_string()),
            "{name}"
        );
    }
}

#[test]
fn a_name_can_read_a_table_or_a_later_name() {
    for (name, mut engine) in engines() {
        let info = BookInfo {
            names: vec![
                NameInfo {
                    name: "Doubled".to_string(),
                    formula: "Total*2".to_string(),
                    local_sheet: None,
                },
                NameInfo {
                    name: "Total".to_string(),
                    formula: "SUM(Sales[Cost])".to_string(),
                    local_sheet: None,
                },
            ],
            tables: vec![TableInfo {
                name: "Sales".to_string(),
                sheet: 0,
                range: Rect {
                    top: 1,
                    left: 1,
                    bottom: 3,
                    right: 1,
                },
                header_row: true,
                totals_row: false,
                columns: vec!["Cost".to_string()],
            }],
            ..info()
        };
        let sheet = vec![
            (1, 1, text("Cost")),
            (2, 1, num(10.0)),
            (3, 1, num(20.0)),
            (1, 3, formula("Total", 0.0)),
            (2, 3, formula("Doubled", 0.0)),
        ];
        load(engine.as_mut(), &info, &[sheet, Vec::new()]);
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 1, 3),
            CalcValue::Number(30.0),
            "{name}"
        );
        assert_eq!(
            at(engine.as_ref(), 0, 2, 3),
            CalcValue::Number(60.0),
            "{name}"
        );
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
        // Formualizer doesn't spread a scalar function over a range yet, so LEFT gives one
        // value here. Upstream tracks it as PSU3D0/formualizer#529. When that lands, this
        // should say "wo" instead.
        assert_eq!(
            at(engine.as_ref(), 0, 2, 3),
            CalcValue::Empty,
            "{name} spill"
        );
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

fn stream(engine: &mut dyn CalcEngine, sparse: bool) {
    use cgs_calc::{LoadEntry, LoadValue, SheetShape};
    engine.begin_load(&info()).unwrap();
    let shape = SheetShape {
        rows: 3,
        cols: 3,
        sparse,
    };
    engine.begin_sheet(0, shape).unwrap();
    engine
        .load_rows(
            0,
            &[
                LoadEntry {
                    row: 1,
                    col: 1,
                    value: LoadValue::Number(2.0),
                },
                LoadEntry {
                    row: 1,
                    col: 2,
                    value: LoadValue::Number(999.0),
                },
            ],
        )
        .unwrap();
    engine
        .load_rows(
            0,
            &[LoadEntry {
                row: 3,
                col: 3,
                value: LoadValue::Text("later"),
            }],
        )
        .unwrap();
    engine
        .load_formula(0, 1, 2, "A1*10", FormulaShape::Normal)
        .unwrap();
    engine.finish_sheet(0, &[]).unwrap();
    engine.begin_sheet(1, shape).unwrap();
    engine
        .load_rows(
            1,
            &[LoadEntry {
                row: 1,
                col: 1,
                value: LoadValue::Number(1998.0),
            }],
        )
        .unwrap();
    engine
        .load_formula(1, 1, 1, "Sheet1!B1*2", FormulaShape::Normal)
        .unwrap();
    engine.finish_sheet(1, &[]).unwrap();
    engine.finish_load().unwrap();
}

#[test]
fn streamed_rows_read_back_without_calculating() {
    for sparse in [false, true] {
        for (name, mut engine) in engines() {
            stream(engine.as_mut(), sparse);
            assert_eq!(
                at(engine.as_ref(), 0, 1, 2),
                CalcValue::Number(999.0),
                "{name}"
            );
            assert_eq!(
                at(engine.as_ref(), 0, 3, 3),
                CalcValue::Text("later".to_string()),
                "{name}"
            );
            assert_eq!(
                engine.formula_text(CellRef {
                    sheet: 0,
                    row: 1,
                    col: 2
                }),
                Some("A1*10".to_string()),
                "{name}"
            );
        }
    }
}

#[test]
fn preparing_changes_no_value_and_an_edit_still_reaches_every_sheet() {
    for (name, mut engine) in engines() {
        stream(engine.as_mut(), false);
        let mut steps = 0;
        while engine.prepare_step().unwrap() {
            steps += 1;
        }
        assert!(steps >= 2, "{name}: one step per sheet");
        assert!(!engine.prepare_step().unwrap(), "{name}: stays finished");
        // The saved value is deliberately wrong, so a calculation would show here.
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 1, 2),
            CalcValue::Number(999.0),
            "{name}"
        );

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
        assert_eq!(
            at(engine.as_ref(), 1, 1, 1),
            CalcValue::Number(60.0),
            "{name}"
        );
    }
}

#[test]
fn a_full_recalculation_after_preparing_still_runs() {
    for (name, mut engine) in engines() {
        stream(engine.as_mut(), false);
        while engine.prepare_step().unwrap() {}
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        assert_eq!(
            at(engine.as_ref(), 0, 1, 2),
            CalcValue::Number(20.0),
            "{name}"
        );
    }
}

#[test]
fn a_sparse_sheet_keeps_a_far_corner() {
    use cgs_calc::{LoadEntry, LoadValue, SheetShape};
    for (name, mut engine) in engines() {
        engine.begin_load(&info()).unwrap();
        engine
            .begin_sheet(
                0,
                SheetShape {
                    rows: 1_048_576,
                    cols: 16_384,
                    sparse: true,
                },
            )
            .unwrap();
        engine
            .load_rows(
                0,
                &[LoadEntry {
                    row: 1_048_576,
                    col: 16_384,
                    value: LoadValue::Number(7.0),
                }],
            )
            .unwrap();
        engine.finish_sheet(0, &[]).unwrap();
        engine.finish_load().unwrap();
        let mut out = Grid::default();
        engine.read_block(
            SheetRect {
                sheet: 0,
                rect: Rect {
                    top: 1_048_570,
                    left: 16_380,
                    bottom: 1_048_576,
                    right: 16_384,
                },
            },
            &mut out,
        );
        assert_eq!(
            out.0.get(&(1_048_576, 16_384)),
            Some(&CalcValue::Number(7.0)),
            "{name}"
        );
    }
}

#[test]
fn text_reads_format_codes_through_cgs_numfmt() {
    let cases: [(&str, CalcValue); 8] = [
        (
            r#"TEXT(45306,"yyyy-mm-dd")"#,
            CalcValue::Text("2024-01-15".to_string()),
        ),
        (
            r##"TEXT(1234.5,"#,##0.00")"##,
            CalcValue::Text("1,234.50".to_string()),
        ),
        (
            r#"TEXT(0.256,"0.0%")"#,
            CalcValue::Text("25.6%".to_string()),
        ),
        (r#"TEXT("abc","@")"#, CalcValue::Text("abc".to_string())),
        (
            r#"TEXT("12.5","0.00")"#,
            CalcValue::Text("12.50".to_string()),
        ),
        (
            r#"TEXT(-1.5,"0.0;(0.0)")"#,
            CalcValue::Text("(1.5)".to_string()),
        ),
        (
            r#"TEXT(45306.5,"h:mm AM/PM")"#,
            CalcValue::Text("12:00 PM".to_string()),
        ),
        (r#"TEXT(1/0,"0")"#, CalcValue::Error("#DIV/0!".to_string())),
    ];
    for (name, mut engine) in engines() {
        let sheet: Vec<_> = cases
            .iter()
            .enumerate()
            .map(|(index, (text, _))| (index as u32 + 1, 1, formula(text, 0.0)))
            .collect();
        load(engine.as_mut(), &info(), &[sheet, Vec::new()]);
        engine.recalc_all().unwrap();
        settle(engine.as_mut());
        for (index, (text, expected)) in cases.iter().enumerate() {
            let value = at(engine.as_ref(), 0, index as u32 + 1, 1);
            assert_eq!(&value, expected, "{name}: {text}");
        }
    }
}

#[test]
fn every_cell_an_edit_changes_is_in_the_change_list() {
    // The engine keeps its own spelling of a sheet named `sheet1`. Two public workbooks lost a
    // changed result to that, when the list was matched to sheets by name.
    let mut engine = cgs_calc::formualizer::FormualizerEngine::new();
    let book = BookInfo {
        sheets: vec!["sheet1".to_string()],
        ..BookInfo::default()
    };
    let cells = vec![
        (
            1,
            1,
            CellInput::Formula {
                text: "IF(ISBLANK(B1),\"\",B1)".to_string(),
                shape: FormulaShape::Normal,
                cached: Some(CalcValue::Text("Peter".to_string())),
            },
        ),
        (1, 2, text("Peter")),
        (
            2,
            1,
            CellInput::Formula {
                text: "IF(ISBLANK(B2),\"\",B2)".to_string(),
                shape: FormulaShape::Normal,
                cached: Some(CalcValue::Text("Tom".to_string())),
            },
        ),
        (2, 2, text("Tom")),
        (
            3,
            1,
            CellInput::Formula {
                text: "COUNTBLANK(A1:A2)".to_string(),
                shape: FormulaShape::Normal,
                cached: Some(CalcValue::Number(0.0)),
            },
        ),
    ];
    load(&mut engine, &book, &[cells]);
    while engine.prepare_step().unwrap() {}
    engine
        .set_cells(&[CellEdit {
            at: CellRef {
                sheet: 0,
                row: 1,
                col: 2,
            },
            input: text("New"),
        }])
        .unwrap();
    let changed = engine.calc_all_changes().unwrap();
    assert_eq!(
        grid(&engine, 0).0[&(1, 1)],
        CalcValue::Text("New".to_string())
    );
    assert!(changed.contains(&CellRef {
        sheet: 0,
        row: 1,
        col: 1
    }));
}

fn fresh(sheets: &[Vec<(u32, u32, CellInput)>]) -> cgs_calc::formualizer::FormualizerEngine {
    let mut engine = cgs_calc::formualizer::FormualizerEngine::new();
    load(&mut engine, &info(), sheets);
    while engine.prepare_step().unwrap() {}
    engine
}

fn cell(sheet: u32, row: u32, col: u32) -> CellRef {
    CellRef { sheet, row, col }
}

fn value_at(engine: &dyn CalcEngine, sheet: u32, row: u32, col: u32) -> Option<CalcValue> {
    grid(engine, sheet).0.get(&(row, col)).cloned()
}

#[test]
fn slices_report_exactly_what_changed() {
    let mut engine = fresh(&[vec![
        (1, 1, num(1.0)),
        (2, 1, formula("A1*2", 2.0)),
        (3, 1, formula("A2+1", 3.0)),
        (4, 1, num(9.0)),
    ]]);
    engine
        .set_cells(&[CellEdit {
            at: cell(0, 1, 1),
            input: num(5.0),
        }])
        .unwrap();
    settle(&mut engine);
    let mut changed = engine.take_changed();
    changed.sort();
    assert_eq!(changed, vec![cell(0, 2, 1), cell(0, 3, 1)]);
    assert!(engine.take_changed().is_empty());
}

#[test]
fn slices_report_spills_too() {
    let mut engine = fresh(&[vec![(1, 1, num(1.0))]]);
    engine
        .set_cells(&[CellEdit {
            at: cell(0, 3, 1),
            input: CellInput::Formula {
                text: "SEQUENCE(3)".to_string(),
                shape: FormulaShape::Normal,
                cached: None,
            },
        }])
        .unwrap();
    settle(&mut engine);
    let changed = engine.take_changed();
    assert!(changed.contains(&cell(0, 5, 1)), "{changed:?}");
    assert_eq!(value_at(&engine, 0, 5, 1), Some(CalcValue::Number(3.0)));
    let areas = engine.spill_areas();
    assert_eq!(areas.len(), 1);
    assert_eq!(
        areas[0].rect,
        Rect {
            top: 3,
            left: 1,
            bottom: 5,
            right: 1
        }
    );
}

#[test]
fn a_delete_gives_back_what_putting_the_rows_back_needs() {
    let mut engine = fresh(&[
        vec![
            (1, 1, num(1.0)),
            (2, 1, num(2.0)),
            (3, 1, num(3.0)),
            (4, 1, num(4.0)),
            (5, 2, formula("SUM(A1:A3)", 6.0)),
            (6, 2, formula("A2", 2.0)),
            (7, 2, formula("A4", 4.0)),
            (2, 3, formula("A1", 1.0)),
        ],
        vec![(1, 1, formula("Sheet1!A3", 3.0))],
    ]);
    let before: Vec<Option<String>> = [(5, 2), (6, 2), (7, 2), (2, 3)]
        .iter()
        .map(|&(row, col)| engine.formula_text(cell(0, row, col)))
        .collect();
    let undo = engine
        .structural(&StructuralOp::DeleteRows {
            sheet: 0,
            at: 2,
            count: 2,
        })
        .unwrap();
    settle(&mut engine);
    assert_eq!(
        engine.formula_text(cell(0, 3, 2)).as_deref(),
        Some("SUM(A1:A1)")
    );
    assert_eq!(engine.formula_text(cell(0, 4, 2)).as_deref(), Some("#REF!"));
    assert_eq!(engine.formula_text(cell(1, 1, 1)).as_deref(), Some("#REF!"));
    assert_eq!(value_at(&engine, 0, 2, 1), Some(CalcValue::Number(4.0)));
    // The cells and formulas it took or changed, and not the one that only moved.
    let held: Vec<CellRef> = undo.removed.iter().map(|edit| edit.at).collect();
    for cell in [
        cell(0, 2, 1),
        cell(0, 3, 1),
        cell(0, 5, 2),
        cell(0, 6, 2),
        cell(0, 2, 3),
        cell(1, 1, 1),
    ] {
        assert!(held.contains(&cell), "{cell:?} in {held:?}");
    }
    assert!(!held.contains(&cell(0, 7, 2)));

    engine.structural(&undo.inverse).unwrap();
    engine.set_cells(&undo.removed).unwrap();
    settle(&mut engine);
    let after: Vec<Option<String>> = [(5, 2), (6, 2), (7, 2), (2, 3)]
        .iter()
        .map(|&(row, col)| engine.formula_text(cell(0, row, col)))
        .collect();
    assert_eq!(before, after);
    assert_eq!(value_at(&engine, 0, 5, 2), Some(CalcValue::Number(6.0)));
    assert_eq!(value_at(&engine, 1, 1, 1), Some(CalcValue::Number(3.0)));
}

#[test]
fn renaming_a_sheet_carries_what_reads_it() {
    let mut engine = fresh(&[
        vec![(1, 1, num(7.0))],
        vec![(1, 1, formula("Sheet1!A1+1", 8.0))],
    ]);
    let old = engine.rename_sheet(0, "New Name").unwrap();
    assert_eq!(old, "Sheet1");
    settle(&mut engine);
    assert_eq!(
        engine.formula_text(cell(1, 1, 1)).as_deref(),
        Some("'New Name'!A1 + 1")
    );
    assert_eq!(value_at(&engine, 1, 1, 1), Some(CalcValue::Number(8.0)));
    engine
        .set_cells(&[CellEdit {
            at: cell(0, 1, 1),
            input: num(10.0),
        }])
        .unwrap();
    settle(&mut engine);
    assert_eq!(value_at(&engine, 1, 1, 1), Some(CalcValue::Number(11.0)));
}

#[test]
fn deleting_a_sheet_leaves_ref_and_can_be_undone() {
    let mut engine = fresh(&[
        vec![(1, 1, num(7.0)), (2, 1, formula("A1*2", 14.0))],
        vec![(1, 1, formula("Sheet1!A2+1", 15.0))],
    ]);
    let undo = engine.delete_sheet(0).unwrap();
    settle(&mut engine);
    assert_eq!(
        engine.formula_text(cell(1, 1, 1)).as_deref(),
        Some("#REF! + 1")
    );
    assert_eq!(
        value_at(&engine, 1, 1, 1),
        Some(CalcValue::Error("#REF!".to_string()))
    );
    assert_eq!(value_at(&engine, 0, 1, 1), None);
    engine.restore_sheet(0).unwrap();
    engine.set_cells(&undo.cells).unwrap();
    engine.set_cells(&undo.readers).unwrap();
    settle(&mut engine);
    assert_eq!(value_at(&engine, 0, 2, 1), Some(CalcValue::Number(14.0)));
    assert_eq!(value_at(&engine, 1, 1, 1), Some(CalcValue::Number(15.0)));
}

#[test]
fn a_new_sheet_can_be_read_and_written() {
    let mut engine = fresh(&[vec![(1, 1, num(7.0))], vec![]]);
    let index = engine.add_sheet("Third").unwrap();
    assert_eq!(index, 2);
    engine
        .set_cells(&[
            CellEdit {
                at: cell(2, 1, 1),
                input: CellInput::Formula {
                    text: "Sheet1!A1*3".to_string(),
                    shape: FormulaShape::Normal,
                    cached: None,
                },
            },
            CellEdit {
                at: cell(0, 2, 1),
                input: CellInput::Formula {
                    text: "Third!A1+1".to_string(),
                    shape: FormulaShape::Normal,
                    cached: None,
                },
            },
        ])
        .unwrap();
    settle(&mut engine);
    assert_eq!(value_at(&engine, 2, 1, 1), Some(CalcValue::Number(21.0)));
    assert_eq!(value_at(&engine, 0, 2, 1), Some(CalcValue::Number(22.0)));
}

/// A small generator, so that a failure comes back the same each time.
struct Rng(u64);

impl Rng {
    fn next(&mut self, below: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % u64::from(below)) as u32
    }
}

fn random_reference(rng: &mut Rng) -> String {
    let col = |rng: &mut Rng| char::from(b'A' + rng.next(8) as u8);
    let row = |rng: &mut Rng| rng.next(12) + 1;
    let dollar = |rng: &mut Rng| if rng.next(4) == 0 { "$" } else { "" };
    match rng.next(6) {
        0 | 1 => format!("{}{}{}{}", dollar(rng), col(rng), dollar(rng), row(rng)),
        2 | 3 => {
            let (a, b) = (rng.next(12) + 1, rng.next(12) + 1);
            let (c, d) = (rng.next(8), rng.next(8));
            format!(
                "{}{}:{}{}",
                char::from(b'A' + c.min(d) as u8),
                a.min(b),
                char::from(b'A' + c.max(d) as u8),
                a.max(b)
            )
        }
        4 => {
            let (c, d) = (rng.next(8), rng.next(8));
            format!(
                "{}:{}",
                char::from(b'A' + c.min(d) as u8),
                char::from(b'A' + c.max(d) as u8)
            )
        }
        _ => {
            let (a, b) = (rng.next(12) + 1, rng.next(12) + 1);
            format!("{}:{}", a.min(b), a.max(b))
        }
    }
}

#[test]
fn rewriting_text_agrees_with_the_engine() {
    // How the two print a range of one cell, `#REF!` or spaces makes no difference to its meaning.
    let normal = |text: &str| {
        let plain = text.replace(' ', "").replace("Sheet1!#REF!", "#REF!");
        let mut out = String::new();
        for (index, part) in plain.split(':').enumerate() {
            let last = out
                .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '$')
                .next()
                .unwrap_or("");
            let same = !last.is_empty()
                && part.starts_with(last)
                && !part[last.len()..].starts_with(|c: char| c.is_ascii_alphanumeric());
            if same {
                out.push_str(&part[last.len()..]);
            } else {
                if index > 0 {
                    out.push(':');
                }
                out.push_str(part);
            }
        }
        out
    };
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut compared = 0;
    for _ in 0..300 {
        let mut cells = Vec::new();
        for row in 20..40 {
            let (a, b) = (random_reference(&mut rng), random_reference(&mut rng));
            cells.push((row, 20, formula(&format!("SUM({a},{b})"), 0.0)));
        }
        let other = vec![(
            1,
            1,
            formula(&format!("SUM(Sheet1!{})", random_reference(&mut rng)), 0.0),
        )];
        let mut engine = fresh(&[cells, other]);
        let axis = if rng.next(2) == 0 {
            cgs_refs::Axis::Row
        } else {
            cgs_refs::Axis::Col
        };
        let (start, count) = (rng.next(9) + 1, rng.next(4) + 1);
        let edit = if rng.next(2) == 0 {
            cgs_refs::AxisEdit::insert(axis, start, count)
        } else {
            cgs_refs::AxisEdit::delete(axis, start, count)
        };
        let before: Vec<String> = (20..40)
            .map(|row| engine.formula_text(cell(0, row, 20)).unwrap())
            .collect();
        let other_before = engine.formula_text(cell(1, 1, 1)).unwrap();
        engine.structural(&StructuralOp::new(0, edit)).unwrap();
        settle(&mut engine);
        for (index, text) in before.iter().enumerate() {
            let row = 20 + index as u32;
            let (row, col) = match axis {
                cgs_refs::Axis::Row => (edit.point(row).unwrap(), 20),
                cgs_refs::Axis::Col => (row, edit.point(20).unwrap()),
            };
            let actual = engine.formula_text(cell(0, row, col)).unwrap();
            let expected = cgs_refs::edit_formula(text, "Sheet1", "Sheet1", &edit);
            assert_eq!(normal(&expected), normal(&actual), "{text} after {edit:?}");
            compared += 1;
        }
        let actual = engine.formula_text(cell(1, 1, 1)).unwrap();
        let expected = cgs_refs::edit_formula(&other_before, "Other Sheet", "Sheet1", &edit);
        assert_eq!(
            normal(&expected),
            normal(&actual),
            "{other_before} after {edit:?}"
        );
    }
    assert_eq!(compared, 6000);
}
