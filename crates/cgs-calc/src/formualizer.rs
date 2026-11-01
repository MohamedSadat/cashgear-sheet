//! Formualizer behind `CalcEngine`.
//!
//! Loading uses deferred graph building: cached values go into the Arrow store, and
//! formula text is staged without being parsed. So opening calculates nothing and a
//! staged formula keeps its cached value. `evaluate_cells` then builds and calculates only
//! what a chunk of cells needs, and each call commits what it did. A cancelled full
//! evaluation can't be used for slices: it puts everything it did back to dirty.

use std::collections::{HashMap, VecDeque};

use formualizer_common::{DateSystem, ExcelError, ExcelErrorKind, LiteralValue};
use formualizer_eval::arrow_store::IngestBuilder;
use formualizer_eval::engine::eval::Engine;
use formualizer_eval::engine::named_range::{NameScope, NamedDefinition};
use formualizer_eval::engine::{EvalConfig, FormulaParsePolicy, TemporalEgress};
use formualizer_eval::reference::{CellRef as FzCell, Coord, RangeRef};
use formualizer_parse::parser::{parse, ASTNodeType, ReferenceType};
use formualizer_workbook::workbook::WBResolver;
use formualizer_workbook::{Workbook, WorkbookConfig};

use crate::{
    err, now_ms, BookInfo, CalcEngine, CalcProgress, CalcValue, CellEdit, CellInput, CellRef,
    CellSink, FormulaShape, Rect, Result, SheetCells, SheetRect, StructuralOp, StructuralUndo,
};

const START_CHUNK: usize = 16;
const MAX_CHUNK: usize = 65_536;

pub struct FormualizerEngine {
    workbook: Workbook,
    sheets: Vec<String>,
    /// Constants and cached values wait here until `finish_load` builds each sheet.
    loading: Vec<HashMap<(u32, u32), LiteralValue>>,
    extents: Vec<(u32, u32)>,
    /// Every formula cell, kept in sheet order for full recalculation.
    formulas: Vec<Vec<(u32, u32)>>,
    dynamic_anchors: Vec<(u32, u32, u32)>,
    /// Cells showing a saved spill. A stored value would block the real spill, so
    /// these empty out when calculation first starts.
    cached_spills: Vec<CellRef>,
    pending: VecDeque<CellRef>,
    chunk: usize,
    changed: Vec<SheetRect>,
    /// True once an edit may have dirtied cells that the pending list doesn't hold.
    needs_settle: bool,
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
            dynamic_anchors: Vec::new(),
            cached_spills: Vec::new(),
            pending: VecDeque::new(),
            chunk: START_CHUNK,
            changed: Vec::new(),
            needs_settle: false,
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
        self.pending.clear();
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
                    if let FormulaShape::Dynamic(_) = shape {
                        self.dynamic_anchors.push((cells.sheet, *row, *col));
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
        self.needs_settle = true;
        Ok(previous)
    }

    fn structural(&mut self, op: &StructuralOp) -> Result<StructuralUndo> {
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
        // Formula positions moved. Reading them back would need engine internals,
        // and only full recalculation uses the list, so it is rebuilt there.
        self.needs_settle = true;
        Ok(StructuralUndo {
            inverse,
            removed: Vec::new(),
        })
    }

    fn recalc_all(&mut self) -> Result<()> {
        self.pending.clear();
        for (sheet, cells) in self.formulas.iter().enumerate() {
            for &(row, col) in cells {
                self.pending.push_back(CellRef {
                    sheet: sheet as u32,
                    row,
                    col,
                });
            }
        }
        // Whatever was staged gets built and calculated by its first request. Cells
        // already built must be marked by hand.
        self.workbook
            .engine_mut()
            .mark_all_formulas_dirty_for_test();
        Ok(())
    }

    fn calc_slice(&mut self, ms: u32, first: &[SheetRect]) -> Result<CalcProgress> {
        let budget = f64::from(ms);
        for cell in std::mem::take(&mut self.cached_spills) {
            let name = self.sheets[cell.sheet as usize].clone();
            self.workbook
                .set_value(&name, cell.row, cell.col, LiteralValue::Empty)
                .map_err(err)?;
        }
        if self.pending.is_empty() {
            if self.needs_settle {
                // No public list of dirty cells yet: an edit settles in one call.
                self.needs_settle = false;
                self.workbook.evaluate_all().map_err(err)?;
                self.changed.extend(self.extents.iter().enumerate().map(
                    |(sheet, &(rows, cols))| SheetRect {
                        sheet: sheet as u32,
                        rect: Rect {
                            top: 1,
                            left: 1,
                            bottom: rows.max(1),
                            right: cols.max(1),
                        },
                    },
                ));
            }
            return Ok(CalcProgress::Done);
        }
        if !first.is_empty() {
            let visible = |cell: &CellRef| {
                first
                    .iter()
                    .any(|area| area.sheet == cell.sheet && area.rect.contains(cell.row, cell.col))
            };
            let (front, back): (Vec<CellRef>, Vec<CellRef>) = std::mem::take(&mut self.pending)
                .into_iter()
                .partition(visible);
            self.pending = front.into_iter().chain(back).collect();
        }
        let start = now_ms();
        let mut spent = 0.0;
        while spent < budget && !self.pending.is_empty() {
            let take = self.chunk.min(self.pending.len());
            let batch: Vec<CellRef> = self.pending.drain(..take).collect();
            let sheets = &self.sheets;
            let targets: Vec<(&str, u32, u32)> = batch
                .iter()
                .map(|cell| (sheets[cell.sheet as usize].as_str(), cell.row, cell.col))
                .collect();
            let chunk_start = now_ms();
            self.workbook.evaluate_cells(&targets).map_err(err)?;
            let took = now_ms() - chunk_start;
            for cell in &batch {
                self.changed.push(SheetRect {
                    sheet: cell.sheet,
                    rect: Rect {
                        top: cell.row,
                        left: cell.col,
                        bottom: cell.row,
                        right: cell.col,
                    },
                });
            }
            // Aim each call at a quarter of the budget.
            let aim = (budget / 4.0).max(1.0);
            self.chunk = if took < aim / 2.0 {
                (self.chunk * 2).min(MAX_CHUNK)
            } else if took > aim * 2.0 {
                (self.chunk / 2).max(1)
            } else {
                self.chunk
            };
            spent = now_ms() - start;
        }
        Ok(if self.pending.is_empty() && !self.needs_settle {
            CalcProgress::Done
        } else {
            CalcProgress::More {
                remaining: Some(self.pending.len()),
            }
        })
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
