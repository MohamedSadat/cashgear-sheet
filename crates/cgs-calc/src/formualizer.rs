//! Formualizer behind `CalcEngine`.
//!
//! Loading uses deferred graph building: cached values go into the Arrow store, and
//! formula text is staged without being parsed. So opening calculates nothing, and a
//! staged formula keeps its cached value. The first edit builds the graph and adopts
//! those values as current, so that only what depends on the edit recalculates.
//! `evaluate_slice` keeps what it finished, unlike a cancelled full evaluation, which
//! puts everything it did back to dirty.

use std::collections::HashMap;
use std::time::Duration;

use formualizer_common::{DateSystem, ExcelError, ExcelErrorKind, LiteralValue};
use formualizer_eval::arrow_store::IngestBuilder;
use formualizer_eval::engine::eval::{Engine, EvaluationSlice};
use formualizer_eval::engine::named_range::{NameScope, NamedDefinition};
use formualizer_eval::engine::{EvalConfig, FormulaParsePolicy, TemporalEgress};
use formualizer_eval::reference::{CellRef as FzCell, Coord, RangeRef};
use formualizer_parse::parser::{parse, ASTNodeType, ReferenceType};
use formualizer_workbook::workbook::WBResolver;
use formualizer_workbook::{Workbook, WorkbookConfig};

use crate::{
    err, BookInfo, CalcEngine, CalcProgress, CalcValue, CellEdit, CellInput, CellRef, CellSink,
    FormulaShape, Rect, Result, SheetCells, SheetRect, StructuralOp, StructuralUndo,
};

pub struct FormualizerEngine {
    workbook: Workbook,
    sheets: Vec<String>,
    /// Constants and cached values wait here until `finish_load` builds each sheet.
    loading: Vec<HashMap<(u32, u32), LiteralValue>>,
    extents: Vec<(u32, u32)>,
    /// Every formula cell, kept in sheet order for full recalculation.
    formulas: Vec<Vec<(u32, u32)>>,
    /// Array formulas, which must run again to rebuild their spills.
    array_anchors: Vec<(CellRef, String)>,
    /// Cells showing a saved spill. A stored value would block the real spill, so
    /// these empty out when calculation first starts.
    cached_spills: Vec<CellRef>,
    /// Staged formulas haven't been adopted or calculated yet.
    fresh: bool,
    slice: EvaluationSlice,
    idle: bool,
    changed: Vec<SheetRect>,
}

impl Default for FormualizerEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn config(date1904: bool) -> WorkbookConfig {
    let eval = EvalConfig {
        defer_graph_building: true,
        enable_parallel: false,
        // A formula the parser can't read becomes an error cell instead of failing the load.
        formula_parse_policy: FormulaParsePolicy::CoerceToError,
        temporal_egress: TemporalEgress::Serial,
        date_system: if date1904 {
            DateSystem::Excel1904
        } else {
            DateSystem::Excel1900
        },
        ..EvalConfig::default()
    };
    WorkbookConfig {
        eval,
        enable_changelog: false,
        ingest_limits: Default::default(),
    }
}

fn to_literal(value: &CalcValue) -> LiteralValue {
    match value {
        CalcValue::Empty => LiteralValue::Empty,
        CalcValue::Number(number) => LiteralValue::Number(*number),
        CalcValue::Bool(flag) => LiteralValue::Boolean(*flag),
        CalcValue::Text(text) => LiteralValue::Text(text.clone()),
        CalcValue::Error(text) => LiteralValue::Error(ExcelError::new(error_kind(text))),
    }
}

fn error_kind(text: &str) -> ExcelErrorKind {
    match text {
        "#NULL!" => ExcelErrorKind::Null,
        "#REF!" => ExcelErrorKind::Ref,
        "#NAME?" => ExcelErrorKind::Name,
        "#VALUE!" => ExcelErrorKind::Value,
        "#DIV/0!" => ExcelErrorKind::Div,
        "#N/A" => ExcelErrorKind::Na,
        "#NUM!" => ExcelErrorKind::Num,
        "#SPILL!" => ExcelErrorKind::Spill,
        "#CALC!" => ExcelErrorKind::Calc,
        _ => ExcelErrorKind::Error,
    }
}

pub fn from_literal(value: &LiteralValue) -> CalcValue {
    match value {
        LiteralValue::Empty | LiteralValue::Pending => CalcValue::Empty,
        LiteralValue::Int(number) => CalcValue::Number(*number as f64),
        LiteralValue::Number(number) => CalcValue::Number(*number),
        LiteralValue::Boolean(flag) => CalcValue::Bool(*flag),
        LiteralValue::Text(text) => CalcValue::Text(text.clone()),
        LiteralValue::Error(error) => CalcValue::Error(error.kind.to_string()),
        LiteralValue::Array(rows) => rows
            .first()
            .and_then(|row| row.first())
            .map_or(CalcValue::Empty, from_literal),
        other => other
            .as_serial_number()
            .map_or(CalcValue::Empty, CalcValue::Number),
    }
}

impl FormualizerEngine {
    pub fn new() -> Self {
        Self {
            workbook: Workbook::new_with_config(config(false)),
            sheets: Vec::new(),
            loading: Vec::new(),
            extents: Vec::new(),
            formulas: Vec::new(),
            array_anchors: Vec::new(),
            cached_spills: Vec::new(),
            fresh: true,
            slice: EvaluationSlice::default(),
            idle: true,
            changed: Vec::new(),
        }
    }

    fn engine(&mut self) -> &mut Engine<WBResolver> {
        self.workbook.engine_mut()
    }

    fn sheet_name(&self, sheet: u32) -> Result<&str> {
        self.sheets
            .get(sheet as usize)
            .map(String::as_str)
            .ok_or_else(|| err(format!("no sheet {sheet}")))
    }

    fn define_name(&mut self, name: &crate::NameInfo) -> Result<()> {
        let scope = match name.local_sheet {
            Some(index) => {
                let sheet = self.sheet_name(index)?.to_string();
                let id = self
                    .engine()
                    .sheet_id(&sheet)
                    .ok_or_else(|| err("no sheet id"))?;
                NameScope::Sheet(id)
            }
            None => NameScope::Workbook,
        };
        let text = format!("={}", name.formula.trim().trim_start_matches('='));
        let ast = parse(&text).map_err(err)?;
        let definition = self
            .simple_reference(&ast)
            .unwrap_or_else(|| NamedDefinition::Formula {
                ast,
                dependencies: Vec::new(),
                range_deps: Vec::new(),
            });
        self.engine()
            .define_name(&name.name, definition, scope)
            .map_err(err)
    }

    /// A name that is a plain absolute reference gets the engine's direct form.
    fn simple_reference(
        &mut self,
        ast: &formualizer_parse::parser::ASTNode,
    ) -> Option<NamedDefinition> {
        let ASTNodeType::Reference { reference, .. } = &ast.node_type else {
            return None;
        };
        match reference {
            ReferenceType::Cell {
                sheet: Some(sheet),
                row,
                col,
                ..
            } => {
                let id = self.engine().sheet_id(sheet)?;
                Some(NamedDefinition::Cell(FzCell::new(
                    id,
                    Coord::from_excel(*row, *col, true, true),
                )))
            }
            ReferenceType::Range {
                sheet: Some(sheet),
                start_row: Some(top),
                start_col: Some(left),
                end_row: Some(bottom),
                end_col: Some(right),
                ..
            } => {
                let id = self.engine().sheet_id(sheet)?;
                let start = FzCell::new(id, Coord::from_excel(*top, *left, true, true));
                let end = FzCell::new(id, Coord::from_excel(*bottom, *right, true, true));
                Some(NamedDefinition::Range(RangeRef::new(start, end)))
            }
            _ => None,
        }
    }

    /// Makes staged formulas part of the graph, keeping their saved values, before
    /// the first edit. Array formulas stay due, because their spills need rebuilding.
    fn adopt(&mut self) -> Result<()> {
        if !self.fresh {
            return Ok(());
        }
        self.fresh = false;
        self.engine().adopt_stored_values().map_err(err)?;
        self.rearm_arrays()
    }

    fn rearm_arrays(&mut self) -> Result<()> {
        for cell in std::mem::take(&mut self.cached_spills) {
            let name = self.sheets[cell.sheet as usize].clone();
            self.workbook
                .set_value(&name, cell.row, cell.col, LiteralValue::Empty)
                .map_err(err)?;
        }
        for (cell, text) in self.array_anchors.clone() {
            let name = self.sheets[cell.sheet as usize].clone();
            self.workbook
                .set_formula(&name, cell.row, cell.col, &text)
                .map_err(err)?;
        }
        Ok(())
    }

    fn install_values(&mut self) -> Result<()> {
        let date_system = self.workbook.eval_config().date_system;
        let loading = std::mem::take(&mut self.loading);
        for (index, values) in loading.into_iter().enumerate() {
            let (rows, cols) = self.extents[index];
            if rows == 0 || cols == 0 {
                continue;
            }
            let name = self.sheets[index].clone();
            let mut builder = IngestBuilder::new(&name, cols as usize, 32 * 1024, date_system);
            let mut by_row: HashMap<u32, Vec<(u32, LiteralValue)>> = HashMap::new();
            for ((row, col), value) in values {
                by_row.entry(row).or_default().push((col, value));
            }
            for row in 1..=rows {
                let mut cells = vec![LiteralValue::Empty; cols as usize];
                if let Some(entries) = by_row.remove(&row) {
                    for (col, value) in entries {
                        cells[(col - 1) as usize] = value;
                    }
                }
                builder.append_row(&cells).map_err(err)?;
            }
            let sheet = builder.finish();
            let store = self.engine().sheet_store_mut();
            match store.sheets.iter().position(|s| s.name.as_ref() == name) {
                Some(position) => store.sheets[position] = sheet,
                None => store.sheets.push(sheet),
            }
            self.engine().finalize_sheet_index(&name);
        }
        self.engine().set_first_load_assume_new(false);
        self.engine().reset_ensure_touched();
        Ok(())
    }
}

impl CalcEngine for FormualizerEngine {
    fn begin_load(&mut self, book: &BookInfo) -> Result<()> {
        self.workbook = Workbook::new_with_config(config(book.date1904));
        self.sheets = book.sheets.clone();
        self.loading = vec![HashMap::new(); book.sheets.len()];
        self.extents = vec![(0, 0); book.sheets.len()];
        self.formulas = vec![Vec::new(); book.sheets.len()];
        self.array_anchors.clear();
        self.cached_spills.clear();
        self.fresh = true;
        self.slice.reset();
        self.idle = true;
        let names: Vec<&str> = book.sheets.iter().map(String::as_str).collect();
        self.engine().adopt_file_sheets(names).map_err(err)?;
        for name in &book.names {
            // A name the engine can't take only hurts the formulas that use it, and
            // those show up in the per-function report.
            let _ = self.define_name(name);
        }
        for &(sheet, row) in &book.hidden_rows {
            let name = self.sheet_name(sheet)?.to_string();
            let _ = self.workbook.set_row_hidden(&name, row, true);
        }
        for table in &book.tables {
            let sheet = self.sheet_name(table.sheet)?.to_string();
            let Some(id) = self.engine().sheet_id(&sheet) else {
                continue;
            };
            let start = FzCell::new(
                id,
                Coord::from_excel(table.range.top, table.range.left, true, true),
            );
            let end = FzCell::new(
                id,
                Coord::from_excel(table.range.bottom, table.range.right, true, true),
            );
            let _ = self.engine().define_table(
                &table.name,
                RangeRef::new(start, end),
                table.header_row,
                table.columns.clone(),
                table.totals_row,
            );
        }
        Ok(())
    }

    fn load_batch(&mut self, cells: &SheetCells<'_>) -> Result<()> {
        let index = cells.sheet as usize;
        let name = self.sheet_name(cells.sheet)?.to_string();
        for (row, col, input) in cells.cells {
            let extent = &mut self.extents[index];
            extent.0 = extent.0.max(*row);
            extent.1 = extent.1.max(*col);
            match input {
                CellInput::Value(value) => {
                    self.loading[index].insert((*row, *col), to_literal(value));
                }
                CellInput::Formula {
                    text,
                    shape,
                    cached,
                } => {
                    if let Some(value) = cached {
                        self.loading[index].insert((*row, *col), to_literal(value));
                    }
                    self.formulas[index].push((*row, *col));
                    if !matches!(shape, FormulaShape::Normal) {
                        self.array_anchors.push((
                            CellRef {
                                sheet: cells.sheet,
                                row: *row,
                                col: *col,
                            },
                            format!("={text}"),
                        ));
                    }
                    self.engine()
                        .stage_formula_text(&name, *row, *col, format!("={text}"));
                }
                CellInput::Spill { cached, .. } => {
                    self.loading[index].insert((*row, *col), to_literal(cached));
                    self.cached_spills.push(CellRef {
                        sheet: cells.sheet,
                        row: *row,
                        col: *col,
                    });
                }
                CellInput::Clear => {}
            }
        }
        Ok(())
    }

    fn finish_load(&mut self) -> Result<()> {
        self.install_values()
    }

    fn set_cells(&mut self, edits: &[CellEdit]) -> Result<Vec<CellEdit>> {
        self.adopt()?;
        self.slice.reset();
        self.idle = false;
        let mut previous = Vec::with_capacity(edits.len());
        for edit in edits {
            let name = self.sheet_name(edit.at.sheet)?.to_string();
            let (row, col) = (edit.at.row, edit.at.col);
            let old_formula = self.workbook.get_formula(&name, row, col);
            let old_value = self
                .workbook
                .get_value(&name, row, col)
                .map_or(CalcValue::Empty, |value| from_literal(&value));
            previous.push(CellEdit {
                at: edit.at,
                input: match old_formula {
                    Some(text) => CellInput::Formula {
                        text: text.trim_start_matches('=').to_string(),
                        shape: FormulaShape::Normal,
                        cached: Some(old_value),
                    },
                    None if old_value == CalcValue::Empty => CellInput::Clear,
                    None => CellInput::Value(old_value),
                },
            });
            let sheet_formulas = &mut self.formulas[edit.at.sheet as usize];
            match &edit.input {
                CellInput::Value(value) => {
                    sheet_formulas.retain(|&at| at != (row, col));
                    self.workbook
                        .set_value(&name, row, col, to_literal(value))
                        .map_err(err)?;
                }
                CellInput::Clear => {
                    sheet_formulas.retain(|&at| at != (row, col));
                    self.workbook
                        .set_value(&name, row, col, LiteralValue::Empty)
                        .map_err(err)?;
                }
                CellInput::Formula { text, .. } => {
                    if !sheet_formulas.contains(&(row, col)) {
                        sheet_formulas.push((row, col));
                    }
                    self.workbook
                        .set_formula(&name, row, col, &format!("={text}"))
                        .map_err(err)?;
                }
                CellInput::Spill { .. } => {}
            }
        }
        Ok(previous)
    }

    fn structural(&mut self, op: &StructuralOp) -> Result<StructuralUndo> {
        self.adopt()?;
        self.slice.reset();
        self.idle = false;
        let (sheet, inverse) = match *op {
            StructuralOp::InsertRows { sheet, at, count } => {
                (sheet, StructuralOp::DeleteRows { sheet, at, count })
            }
            StructuralOp::DeleteRows { sheet, at, count } => {
                (sheet, StructuralOp::InsertRows { sheet, at, count })
            }
            StructuralOp::InsertCols { sheet, at, count } => {
                (sheet, StructuralOp::DeleteCols { sheet, at, count })
            }
            StructuralOp::DeleteCols { sheet, at, count } => {
                (sheet, StructuralOp::InsertCols { sheet, at, count })
            }
        };
        let name = self.sheet_name(sheet)?.to_string();
        let engine = self.engine();
        match *op {
            StructuralOp::InsertRows { at, count, .. } => {
                engine.insert_rows(&name, at, count).map_err(err)?;
            }
            StructuralOp::DeleteRows { at, count, .. } => {
                engine.delete_rows(&name, at, count).map_err(err)?;
            }
            StructuralOp::InsertCols { at, count, .. } => {
                engine.insert_columns(&name, at, count).map_err(err)?;
            }
            StructuralOp::DeleteCols { at, count, .. } => {
                engine.delete_columns(&name, at, count).map_err(err)?;
            }
        }
        Ok(StructuralUndo {
            inverse,
            removed: Vec::new(),
        })
    }

    fn recalc_all(&mut self) -> Result<()> {
        self.slice.reset();
        self.idle = false;
        if self.fresh {
            // Everything staged is dirty when it is built, so nothing needs marking.
            self.fresh = false;
            return self.rearm_arrays();
        }
        self.engine().mark_all_formulas_dirty();
        Ok(())
    }

    fn calc_slice(&mut self, ms: u32, first: &[SheetRect]) -> Result<CalcProgress> {
        if self.idle {
            return Ok(CalcProgress::Done);
        }
        // Volatile and cycle work can stay due; a slice that finds nothing dirty ends it.
        let budget = Duration::from_secs_f64(f64::from(ms) / 1000.0);
        if !first.is_empty() {
            let sheets = &self.sheets;
            let targets: Vec<(&str, u32, u32)> = first
                .iter()
                .flat_map(|area| {
                    let name = sheets[area.sheet as usize].as_str();
                    (area.rect.top..=area.rect.bottom).flat_map(move |row| {
                        (area.rect.left..=area.rect.right).map(move |col| (name, row, col))
                    })
                })
                .collect();
            self.workbook.evaluate_cells(&targets).map_err(err)?;
            // Those cells are clean now, so a pass planned before them is out of date.
            self.slice.reset();
        }
        let result = self
            .workbook
            .engine_mut()
            .evaluate_slice(&mut self.slice, budget)
            .map_err(err)?;
        if result.computed_vertices > 0 {
            self.changed.extend(
                self.extents
                    .iter()
                    .enumerate()
                    .map(|(sheet, &(rows, cols))| SheetRect {
                        sheet: sheet as u32,
                        rect: Rect {
                            top: 1,
                            left: 1,
                            bottom: rows.max(1),
                            right: cols.max(1),
                        },
                    }),
            );
        }
        if result.finished {
            self.idle = true;
            return Ok(CalcProgress::Done);
        }
        Ok(CalcProgress::More { remaining: None })
    }

    fn take_changed(&mut self) -> Vec<SheetRect> {
        std::mem::take(&mut self.changed)
    }

    fn read_block(&self, r: SheetRect, out: &mut dyn CellSink) {
        let Some(name) = self.sheets.get(r.sheet as usize) else {
            return;
        };
        let rows = self.workbook.engine().get_range_values(
            name,
            r.rect.top,
            r.rect.left,
            r.rect.bottom,
            r.rect.right,
        );
        for (row_offset, row) in rows.iter().enumerate() {
            for (col_offset, value) in row.iter().enumerate() {
                if !matches!(value, LiteralValue::Empty) {
                    out.cell(
                        r.rect.top + row_offset as u32,
                        r.rect.left + col_offset as u32,
                        &from_literal(value),
                    );
                }
            }
        }
    }

    fn formula_text(&self, at: CellRef) -> Option<String> {
        let name = self.sheets.get(at.sheet as usize)?;
        self.workbook
            .get_formula(name, at.row, at.col)
            .map(|text| text.trim_start_matches('=').to_string())
    }
}
