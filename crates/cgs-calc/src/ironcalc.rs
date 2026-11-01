//! IronCalc behind `CalcEngine`.
//!
//! IronCalc keeps the value with each formula cell, so cached values load directly and
//! nothing calculates until `evaluate`. That call recomputes every formula and can't be
//! divided, so without an upstream change a "slice" is a whole workbook pass.

use std::collections::HashMap;

use ironcalc_base::cell::CellValue;
use ironcalc_base::expressions::parser::static_analysis::add_implicit_intersection;
use ironcalc_base::expressions::parser::stringify::to_rc_format;
use ironcalc_base::expressions::parser::{new_parser_english, DefinedNameS, Parser};
use ironcalc_base::expressions::types::CellReferenceRC;
use ironcalc_base::types::{
    ArrayKind, Cell, CellType, DefinedName, FormulaValue, Row, SheetData, SpillValue, Table,
    TableColumn, TableStyleInfo, Workbook,
};
use ironcalc_base::Model;

use crate::{
    err, BookInfo, CalcEngine, CalcProgress, CalcValue, CellEdit, CellInput, CellRef, CellSink,
    FormulaShape, Rect, Result, SheetCells, SheetRect, StructuralOp, StructuralUndo,
};

const LANGUAGE: &str = "en";

struct Loading {
    workbook: Workbook,
    parser: Parser<'static>,
    /// Per sheet, formula text to its index in `shared_formulas`.
    formula_index: Vec<HashMap<String, i32>>,
    strings: HashMap<String, i32>,
}

pub struct IronCalcEngine {
    model: Option<Model<'static>>,
    loading: Option<Loading>,
    dirty: bool,
    changed: Vec<SheetRect>,
    sheets: Vec<String>,
}

impl Default for IronCalcEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn a1(col: u32, row: u32) -> String {
    let mut letters = Vec::new();
    let mut rest = col;
    while rest > 0 {
        letters.push(b'A' + ((rest - 1) % 26) as u8);
        rest = (rest - 1) / 26;
    }
    letters.reverse();
    format!("{}{row}", String::from_utf8_lossy(&letters))
}

fn table_of(info: &crate::TableInfo, sheet_name: &str) -> Table {
    let range = info.range;
    Table {
        name: info.name.clone(),
        display_name: info.name.clone(),
        sheet_name: sheet_name.to_string(),
        reference: format!(
            "{}:{}",
            a1(range.left, range.top),
            a1(range.right, range.bottom)
        ),
        totals_row_count: u32::from(info.totals_row),
        header_row_count: u32::from(info.header_row),
        header_row_dxf_id: None,
        data_dxf_id: None,
        totals_row_dxf_id: None,
        columns: info
            .columns
            .iter()
            .enumerate()
            .map(|(index, name)| TableColumn {
                id: index as u32 + 1,
                name: name.clone(),
                ..TableColumn::default()
            })
            .collect(),
        style_info: TableStyleInfo::default(),
        has_filters: false,
    }
}

fn error_of(text: &str) -> ironcalc_base::expressions::token::Error {
    use ironcalc_base::expressions::token::Error;
    match text {
        "#REF!" => Error::REF,
        "#NAME?" => Error::NAME,
        "#VALUE!" => Error::VALUE,
        "#DIV/0!" => Error::DIV,
        "#N/A" => Error::NA,
        "#NUM!" => Error::NUM,
        "#NULL!" => Error::NULL,
        "#SPILL!" => Error::SPILL,
        "#CALC!" => Error::CALC,
        _ => Error::ERROR,
    }
}

fn formula_value(cached: Option<&CalcValue>, at: &str) -> FormulaValue {
    match cached {
        None => FormulaValue::Unevaluated,
        Some(CalcValue::Empty) => FormulaValue::Number(0.0),
        Some(CalcValue::Number(number)) => FormulaValue::Number(*number),
        Some(CalcValue::Bool(flag)) => FormulaValue::Boolean(*flag),
        Some(CalcValue::Text(text)) => FormulaValue::Text(text.clone()),
        Some(CalcValue::Error(text)) => {
            FormulaValue::new_error(error_of(text), at.to_string(), String::new())
        }
    }
}

fn spill_value(cached: &CalcValue) -> SpillValue {
    match cached {
        CalcValue::Empty => SpillValue::Number(0.0),
        CalcValue::Number(number) => SpillValue::Number(*number),
        CalcValue::Bool(flag) => SpillValue::Boolean(*flag),
        CalcValue::Text(text) => SpillValue::Text(text.clone()),
        CalcValue::Error(text) => SpillValue::Error(error_of(text)),
    }
}

impl IronCalcEngine {
    pub fn new() -> Self {
        Self {
            model: None,
            loading: None,
            dirty: false,
            changed: Vec::new(),
            sheets: Vec::new(),
        }
    }

    fn model(&mut self) -> Result<&mut Model<'static>> {
        self.model.as_mut().ok_or_else(|| err("not loaded"))
    }

    fn whole_sheet(&self, sheet: u32) -> SheetRect {
        SheetRect {
            sheet,
            rect: Rect {
                top: 1,
                left: 1,
                bottom: 1_048_576,
                right: 16_384,
            },
        }
    }
}

impl Loading {
    fn intern(&mut self, text: &str) -> i32 {
        if let Some(index) = self.strings.get(text) {
            return *index;
        }
        let index = self.workbook.shared_strings.len() as i32;
        self.workbook.shared_strings.push(text.to_string());
        self.strings.insert(text.to_string(), index);
        index
    }

    fn formula(&mut self, sheet: usize, row: u32, col: u32, text: &str, array: bool) -> i32 {
        let context = CellReferenceRC {
            sheet: self.workbook.worksheets[sheet].name.clone(),
            row: row as i32,
            column: col as i32,
        };
        let mut parsed = self.parser.parse(text, &context);
        if !array {
            add_implicit_intersection(&mut parsed, true);
        }
        let rc = to_rc_format(&parsed);
        if let Some(index) = self.formula_index[sheet].get(&rc) {
            return *index;
        }
        let worksheet = &mut self.workbook.worksheets[sheet];
        let index = worksheet.shared_formulas.len() as i32;
        worksheet.shared_formulas.push(rc.clone());
        self.formula_index[sheet].insert(rc, index);
        index
    }

    fn cell_of(&mut self, sheet: usize, row: u32, col: u32, input: &CellInput) -> Option<Cell> {
        let at = format!("{}!{}", self.workbook.worksheets[sheet].name, a1(col, row));
        Some(match input {
            CellInput::Clear => return None,
            CellInput::Value(CalcValue::Empty) => return None,
            CellInput::Value(CalcValue::Number(number)) => Cell::NumberCell { v: *number, s: 0 },
            CellInput::Value(CalcValue::Bool(flag)) => Cell::BooleanCell { v: *flag, s: 0 },
            CellInput::Value(CalcValue::Text(text)) => Cell::SharedString {
                si: self.intern(text),
                s: 0,
            },
            CellInput::Value(CalcValue::Error(text)) => Cell::ErrorCell {
                ei: error_of(text),
                s: 0,
            },
            CellInput::Spill { anchor, cached } => Cell::SpillCell {
                s: 0,
                a: (anchor.0 as i32, anchor.1 as i32),
                v: spill_value(cached),
            },
            CellInput::Formula {
                text,
                shape,
                cached,
            } => {
                let value = formula_value(cached.as_ref(), &at);
                match shape {
                    FormulaShape::Normal => Cell::CellFormula {
                        f: self.formula(sheet, row, col, text, false),
                        s: 0,
                        v: value,
                    },
                    FormulaShape::Array(range) | FormulaShape::Dynamic(range) => {
                        Cell::ArrayFormula {
                            f: self.formula(sheet, row, col, text, true),
                            s: 0,
                            r: (
                                (range.right - range.left + 1) as i32,
                                (range.bottom - range.top + 1) as i32,
                            ),
                            kind: if matches!(shape, FormulaShape::Dynamic(_)) {
                                ArrayKind::Dynamic
                            } else {
                                ArrayKind::Cse
                            },
                            v: value,
                        }
                    }
                }
            }
        })
    }
}

impl CalcEngine for IronCalcEngine {
    fn begin_load(&mut self, book: &BookInfo) -> Result<()> {
        let mut model = Model::new_empty("workbook", "en", "UTC", LANGUAGE).map_err(err)?;
        // Temporary names avoid clashing with the default sheet. The file's own names
        // then go in directly, because whatever Excel saved is valid.
        for index in 1..book.sheets.len() {
            model
                .add_sheet(&format!("cgs-sheet-{index}"))
                .map_err(err)?;
        }
        let mut workbook = model.workbook;
        for (worksheet, name) in workbook.worksheets.iter_mut().zip(&book.sheets) {
            worksheet.name.clone_from(name);
            worksheet.sheet_data = SheetData::new();
        }
        for name in &book.names {
            let sheet_id = name
                .local_sheet
                .and_then(|index| workbook.worksheets.get(index as usize))
                .map(|sheet| sheet.sheet_id);
            workbook.defined_names.push(DefinedName {
                name: name.name.clone(),
                formula: name.formula.clone(),
                sheet_id,
            });
        }
        for table in &book.tables {
            let sheet_name = book
                .sheets
                .get(table.sheet as usize)
                .cloned()
                .unwrap_or_default();
            workbook
                .tables
                .insert(table.name.clone(), table_of(table, &sheet_name));
        }
        for &(sheet, row) in &book.hidden_rows {
            if let Some(worksheet) = workbook.worksheets.get_mut(sheet as usize) {
                worksheet.rows.push(Row {
                    r: row as i32,
                    height: 15.0,
                    custom_format: false,
                    custom_height: false,
                    s: 0,
                    hidden: true,
                });
            }
        }
        let names: Vec<DefinedNameS> = workbook.get_defined_names_with_scope();
        let parser = new_parser_english(book.sheets.clone(), names, workbook.tables.clone());
        self.sheets = book.sheets.clone();
        self.loading = Some(Loading {
            parser,
            formula_index: vec![HashMap::new(); workbook.worksheets.len()],
            strings: HashMap::new(),
            workbook,
        });
        self.model = None;
        Ok(())
    }

    fn load_batch(&mut self, cells: &SheetCells<'_>) -> Result<()> {
        let loading = self
            .loading
            .as_mut()
            .ok_or_else(|| err("load not started"))?;
        let sheet = cells.sheet as usize;
        for (row, col, input) in cells.cells {
            if let Some(cell) = loading.cell_of(sheet, *row, *col, input) {
                loading.workbook.worksheets[sheet].sheet_data.set_cell(
                    *row as i32,
                    *col as i32,
                    cell,
                );
            }
        }
        Ok(())
    }

    fn finish_load(&mut self) -> Result<()> {
        let loading = self.loading.take().ok_or_else(|| err("load not started"))?;
        self.model = Some(Model::from_workbook(loading.workbook, LANGUAGE).map_err(err)?);
        Ok(())
    }

    fn set_cells(&mut self, edits: &[CellEdit]) -> Result<Vec<CellEdit>> {
        let mut previous = Vec::with_capacity(edits.len());
        for edit in edits {
            let (sheet, row, col) = (edit.at.sheet, edit.at.row as i32, edit.at.col as i32);
            let old_text = self.formula_text(edit.at);
            let model = self.model()?;
            let old_value = value_at(model, sheet, row, col);
            previous.push(CellEdit {
                at: edit.at,
                input: match old_text {
                    Some(text) => CellInput::Formula {
                        text,
                        shape: FormulaShape::Normal,
                        cached: Some(old_value),
                    },
                    None if old_value == CalcValue::Empty => CellInput::Clear,
                    None => CellInput::Value(old_value),
                },
            });
            match &edit.input {
                CellInput::Value(CalcValue::Number(number)) => {
                    model
                        .update_cell_with_number(sheet, row, col, *number)
                        .map_err(err)?;
                }
                CellInput::Value(CalcValue::Bool(flag)) => {
                    model
                        .update_cell_with_bool(sheet, row, col, *flag)
                        .map_err(err)?;
                }
                CellInput::Value(CalcValue::Text(text)) => {
                    model
                        .update_cell_with_text(sheet, row, col, text)
                        .map_err(err)?;
                }
                CellInput::Value(CalcValue::Error(text)) => {
                    model
                        .update_cell_with_text(sheet, row, col, text)
                        .map_err(err)?;
                }
                CellInput::Value(CalcValue::Empty) | CellInput::Clear => {
                    model
                        .workbook
                        .worksheet_mut(sheet)
                        .and_then(|worksheet| worksheet.cell_clear_contents(row, col))
                        .map_err(err)?;
                }
                CellInput::Formula { text, .. } => {
                    model
                        .update_cell_with_formula(sheet, row, col, format!("={text}"))
                        .map_err(err)?;
                }
                CellInput::Spill { .. } => {}
            }
        }
        self.dirty = true;
        Ok(previous)
    }

    fn structural(&mut self, op: &StructuralOp) -> Result<StructuralUndo> {
        let model = self.model()?;
        let inverse = match *op {
            StructuralOp::InsertRows { sheet, at, count } => {
                model
                    .insert_rows(sheet, at as i32, count as i32)
                    .map_err(err)?;
                StructuralOp::DeleteRows { sheet, at, count }
            }
            StructuralOp::DeleteRows { sheet, at, count } => {
                model
                    .delete_rows(sheet, at as i32, count as i32)
                    .map_err(err)?;
                StructuralOp::InsertRows { sheet, at, count }
            }
            StructuralOp::InsertCols { sheet, at, count } => {
                model
                    .insert_columns(sheet, at as i32, count as i32)
                    .map_err(err)?;
                StructuralOp::DeleteCols { sheet, at, count }
            }
            StructuralOp::DeleteCols { sheet, at, count } => {
                model
                    .delete_columns(sheet, at as i32, count as i32)
                    .map_err(err)?;
                StructuralOp::InsertCols { sheet, at, count }
            }
        };
        self.dirty = true;
        Ok(StructuralUndo {
            inverse,
            removed: Vec::new(),
        })
    }

    fn recalc_all(&mut self) -> Result<()> {
        self.dirty = true;
        Ok(())
    }

    fn calc_slice(&mut self, _ms: u32, _first: &[SheetRect]) -> Result<CalcProgress> {
        if self.dirty {
            self.dirty = false;
            self.model()?.evaluate();
            for sheet in 0..self.sheets.len() as u32 {
                let whole = self.whole_sheet(sheet);
                self.changed.push(whole);
            }
        }
        Ok(CalcProgress::Done)
    }

    fn take_changed(&mut self) -> Vec<SheetRect> {
        std::mem::take(&mut self.changed)
    }

    fn read_block(&self, r: SheetRect, out: &mut dyn CellSink) {
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let Ok(worksheet) = model.workbook.worksheet(r.sheet) else {
            return;
        };
        let (top, bottom) = (r.rect.top as i32, r.rect.bottom as i32);
        for row in worksheet.sheet_data.rows() {
            if row < top || row > bottom {
                continue;
            }
            for (col, _) in worksheet.sheet_data.cells_in_row_between(
                row,
                r.rect.left as i32,
                r.rect.right as i32,
            ) {
                let value = value_at(model, r.sheet, row, col);
                if value != CalcValue::Empty {
                    out.cell(row as u32, col as u32, &value);
                }
            }
        }
    }

    fn formula_text(&self, at: CellRef) -> Option<String> {
        let model = self.model.as_ref()?;
        let text = model
            .get_cell_formula(at.sheet, at.row as i32, at.col as i32)
            .ok()??;
        Some(text.trim_start_matches('=').to_string())
    }
}

fn value_at(model: &Model<'_>, sheet: u32, row: i32, col: i32) -> CalcValue {
    match model.get_cell_value_by_index(sheet, row, col) {
        Ok(CellValue::Number(number)) => CalcValue::Number(number),
        Ok(CellValue::Boolean(flag)) => CalcValue::Bool(flag),
        Ok(CellValue::String(text)) => {
            if matches!(
                model.get_cell_type(sheet, row, col),
                Ok(CellType::ErrorValue)
            ) {
                CalcValue::Error(text)
            } else {
                CalcValue::Text(text)
            }
        }
        Ok(CellValue::None) | Err(_) => CalcValue::Empty,
    }
}
