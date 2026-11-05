//! Formualizer behind `CalcEngine`.
//!
//! Loading uses deferred graph building: cached values go into the Arrow store, and
//! formula text is staged without being parsed. So opening calculates nothing, and a
//! staged formula keeps its cached value. The first edit builds the graph and adopts
//! those values as current, so that only what depends on the edit recalculates.
//! `evaluate_slice` keeps what it finished, unlike a cancelled full evaluation, which
//! puts everything it did back to dirty.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use cgs_numfmt::{Locale, Options, Value as FormatValue};

use formualizer_common::{DateSystem, ExcelError, ExcelErrorKind, LiteralValue};
use formualizer_eval::arrow_store::{
    map_error_code, ArrowSheet, CellIngest, IngestBuilder, OverlayValue,
};
use formualizer_eval::engine::eval::{Engine, EvaluationSlice};
use formualizer_eval::engine::named_range::{NameScope, NamedDefinition};
use formualizer_eval::engine::{EvalConfig, FormulaParsePolicy, TemporalEgress};
use formualizer_eval::reference::{CellRef as FzCell, Coord, RangeRef};
use formualizer_parse::parser::{
    parse, ASTNode, ASTNodeType, BinaryOperator, ReferenceType, UnaryOperator,
};
use formualizer_parse::pretty::canonical_formula;
use formualizer_workbook::workbook::WBResolver;
use formualizer_workbook::{CustomFnOptions, Workbook, WorkbookConfig};

use crate::{
    err, wants_sparse, BookInfo, CalcEngine, CalcProgress, CalcValue, CellEdit, CellInput, CellRef,
    CellSink, FormulaShape, LoadEntry, LoadValue, Rect, Result, SheetCells, SheetRect, SheetShape,
    StructuralOp, StructuralUndo,
};

/// Rows in one chunk of the Arrow store.
const CHUNK_ROWS: usize = 32 * 1024;

/// A sheet on its way into the Arrow store.
enum SheetLoad {
    /// Full columns, written row by row. Costs bytes for every cell in the area.
    Dense {
        builder: Box<IngestBuilder>,
        next_row: u32,
        cols: usize,
    },
    /// Cell by cell, for few cells over a large area.
    Sparse(Box<ArrowSheet>),
}

pub struct FormualizerEngine {
    workbook: Workbook,
    /// The conventions `TEXT` follows, which the shell sets from the user's language.
    locale: Arc<RwLock<Locale>>,
    sheets: Vec<String>,
    /// Sheets between `begin_sheet` and `finish_sheet`.
    loads: Vec<Option<SheetLoad>>,
    /// What `load_batch` holds until `finish_load`.
    pending: Vec<Vec<(u32, u32, CellInput)>>,
    extents: Vec<(u32, u32)>,
    /// Array formulas, which must run again to rebuild their spills.
    array_anchors: Vec<(CellRef, String)>,
    /// Cells showing a saved spill. A stored value would block the real spill, so
    /// these empty out when calculation first starts.
    cached_spills: Vec<CellRef>,
    /// Formulas that read another workbook, which keep their saved value, as they do in Excel
    /// until its links are updated. The engine would refuse them, and the whole sheet with them.
    linked: HashMap<CellRef, String>,
    /// Staged formulas haven't been adopted or calculated yet.
    fresh: bool,
    /// The next sheet `prepare_step` builds a graph for.
    prepare_next: usize,
    /// The graph is built and its saved values count as current.
    prepared: bool,
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

/// `TEXT`, through `cgs-numfmt`, which knows the format codes the engine's own doesn't.
fn text_function(
    args: &[LiteralValue],
    date1904: bool,
    locale: &RwLock<Locale>,
) -> std::result::Result<LiteralValue, ExcelError> {
    let [value, code] = args else {
        return Err(ExcelError::new(ExcelErrorKind::Value));
    };
    // A range reaches here as an array, and a scalar function reads its first cell.
    let scalar = |argument: &LiteralValue| -> LiteralValue {
        match argument {
            LiteralValue::Array(rows) => rows
                .first()
                .and_then(|row| row.first())
                .cloned()
                .unwrap_or(LiteralValue::Empty),
            other => other.clone(),
        }
    };
    let value = scalar(value);
    let code = scalar(code);
    if let LiteralValue::Error(error) = &value {
        return Err(error.clone());
    }
    if let LiteralValue::Error(error) = &code {
        return Err(error.clone());
    }
    let code = match from_literal(&code) {
        CalcValue::Text(text) => text,
        CalcValue::Number(number) => number.to_string(),
        CalcValue::Bool(flag) => if flag { "TRUE" } else { "FALSE" }.to_string(),
        _ => String::new(),
    };
    let value = from_literal(&value);
    let text;
    let input = match &value {
        CalcValue::Empty => FormatValue::Number(0.0),
        CalcValue::Number(number) => FormatValue::Number(*number),
        CalcValue::Bool(flag) => FormatValue::Bool(*flag),
        // Text that reads as a number formats as one, as in Excel.
        CalcValue::Text(inner) => match inner.trim().parse::<f64>() {
            Ok(number) => FormatValue::Number(number),
            Err(_) => {
                text = inner;
                FormatValue::Text(text)
            }
        },
        CalcValue::Error(inner) => FormatValue::Error(inner),
    };
    let locale = locale
        .read()
        .map_err(|_| ExcelError::new(ExcelErrorKind::Value))?;
    let options = Options {
        locale: &locale,
        date1904,
    };
    cgs_numfmt::text_function(input, &code, &options)
        .map(LiteralValue::Text)
        .map_err(|()| ExcelError::new(ExcelErrorKind::Value))
}

fn new_workbook(date1904: bool, locale: &Arc<RwLock<Locale>>) -> Workbook {
    let mut workbook = Workbook::new_with_config(config(date1904));
    let locale = Arc::clone(locale);
    let handler = move |args: &[LiteralValue]| text_function(args, date1904, &locale);
    let options = CustomFnOptions {
        min_args: 2,
        max_args: Some(2),
        volatile: false,
        thread_safe: true,
        deterministic: true,
        allow_override_builtin: true,
    };
    // It can only fail on a clash of names, and this is the first thing on a new workbook.
    let _ = workbook.register_custom_function("TEXT", options, Arc::new(handler));
    workbook
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

/// A file marks a formula that spills as an array, so a plain formula such as
/// `A:A&"/"&B:B` reads each range at its own row or column. Excel shows this as
/// `@A:A`. Only a formula of operators and references is rewritten here; what a function
/// does with a range belongs to the function. Returns the text if anything changed.
fn implicit_intersection(text: &str) -> Option<String> {
    if !text.contains(':') || text.contains('(') {
        return None;
    }
    let mut ast = parse(format!("={text}")).ok()?;
    wrap_ranges(&mut ast).then(|| canonical_formula(&ast))
}

fn wrap_ranges(node: &mut ASTNode) -> bool {
    match &mut node.node_type {
        ASTNodeType::Reference {
            reference: ReferenceType::Range { .. },
            ..
        } => {
            let inner = node.clone();
            node.node_type = ASTNodeType::UnaryOp {
                op: UnaryOperator::ImplicitIntersection,
                expr: Box::new(inner),
            };
            true
        }
        ASTNodeType::UnaryOp { expr, .. } => wrap_ranges(expr),
        ASTNodeType::BinaryOp { op, left, right } if *op != BinaryOperator::Range => {
            let left = wrap_ranges(left);
            let right = wrap_ranges(right);
            left || right
        }
        _ => false,
    }
}

/// Whether a formula reads a workbook other than this one: `[1]Sheet1!A1`, `[1]!Name`, or
/// `'C:\\files\\[book.xlsx]Sheet 1'!A1`. A table's `Table1[Column]` doesn't count.
fn reads_another_workbook(text: &str) -> bool {
    if !text.contains('[') {
        return false;
    }
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                at += 1;
                while at < bytes.len() && bytes[at] != b'"' {
                    at += 1;
                }
            }
            b'\'' => {
                let start = at + 1;
                at += 1;
                while at < bytes.len() {
                    if bytes[at] == b'\'' {
                        // Two quotes in a row are one quote in the name.
                        if bytes.get(at + 1) == Some(&b'\'') {
                            at += 2;
                            continue;
                        }
                        break;
                    }
                    at += 1;
                }
                if bytes.get(at + 1) == Some(&b'!')
                    && bytes[start..at.min(bytes.len())].contains(&b'[')
                {
                    return true;
                }
            }
            b'[' => {
                let after_name = at > 0
                    && (bytes[at - 1].is_ascii_alphanumeric()
                        || bytes[at - 1] == b'_'
                        || bytes[at - 1] == b'.'
                        || bytes[at - 1] == b']'
                        || bytes[at - 1] >= 0x80);
                let Some(length) = bytes[at..].iter().position(|&byte| byte == b']') else {
                    return false;
                };
                let end = at + length;
                if !after_name {
                    // From the bracket's end, a sheet name and then `!` make this a workbook.
                    let mut next = end + 1;
                    while next < bytes.len()
                        && (bytes[next].is_ascii_alphanumeric()
                            || bytes[next] == b'_'
                            || bytes[next] == b'.'
                            || bytes[next] >= 0x80)
                    {
                        next += 1;
                    }
                    if bytes.get(next) == Some(&b'!') {
                        return true;
                    }
                }
                at = end;
            }
            _ => {}
        }
        at += 1;
    }
    false
}

impl FormualizerEngine {
    pub fn new() -> Self {
        let locale = Arc::new(RwLock::new(Locale::en_us()));
        Self {
            workbook: new_workbook(false, &locale),
            locale,
            sheets: Vec::new(),
            loads: Vec::new(),
            pending: Vec::new(),
            extents: Vec::new(),
            array_anchors: Vec::new(),
            cached_spills: Vec::new(),
            linked: HashMap::new(),
            fresh: true,
            prepare_next: 0,
            prepared: false,
            slice: EvaluationSlice::default(),
            idle: true,
            changed: Vec::new(),
        }
    }

    /// Sets the conventions `TEXT` follows for built-in date and number forms.
    pub fn set_locale(&mut self, locale: Locale) {
        if let Ok(mut current) = self.locale.write() {
            *current = locale;
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
        while self.prepare_step()? {}
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

    fn date_system(&self) -> DateSystem {
        self.workbook.eval_config().date_system
    }

    /// Puts a finished sheet in the store, in place of any empty one.
    fn install_sheet(&mut self, sheet: ArrowSheet) {
        let name = sheet.name.to_string();
        let store = self.engine().sheet_store_mut();
        match store.sheets.iter().position(|s| s.name.as_ref() == name) {
            Some(position) => store.sheets[position] = sheet,
            None => store.sheets.push(sheet),
        }
        self.engine().finalize_sheet_index(&name);
    }
}

fn error_code(text: &str) -> u8 {
    map_error_code(error_kind(text))
}

fn to_ingest<'a>(value: &LoadValue<'a>) -> CellIngest<'a> {
    match *value {
        LoadValue::Number(number) => CellIngest::Number(number),
        LoadValue::Bool(flag) => CellIngest::Boolean(flag),
        LoadValue::Text(text) => CellIngest::Text(text),
        LoadValue::Error(text) => CellIngest::ErrorCode(error_code(text)),
    }
}

fn to_overlay(value: &LoadValue<'_>) -> OverlayValue {
    match *value {
        LoadValue::Number(number) => OverlayValue::Number(number),
        LoadValue::Bool(flag) => OverlayValue::Boolean(flag),
        LoadValue::Text(text) => OverlayValue::Text(text.into()),
        LoadValue::Error(text) => OverlayValue::Error(error_code(text)),
    }
}

fn borrow_value(value: &CalcValue) -> Option<LoadValue<'_>> {
    match value {
        CalcValue::Empty => None,
        CalcValue::Number(number) => Some(LoadValue::Number(*number)),
        CalcValue::Bool(flag) => Some(LoadValue::Bool(*flag)),
        CalcValue::Text(text) => Some(LoadValue::Text(text)),
        CalcValue::Error(text) => Some(LoadValue::Error(text)),
    }
}

impl CalcEngine for FormualizerEngine {
    fn begin_load(&mut self, book: &BookInfo) -> Result<()> {
        self.workbook = new_workbook(book.date1904, &self.locale);
        self.sheets = book.sheets.clone();
        self.loads = book.sheets.iter().map(|_| None).collect();
        self.pending = vec![Vec::new(); book.sheets.len()];
        self.extents = vec![(0, 0); book.sheets.len()];
        self.array_anchors.clear();
        self.cached_spills.clear();
        self.linked.clear();
        self.fresh = true;
        self.prepare_next = 0;
        self.prepared = false;
        self.slice.reset();
        self.idle = true;
        let names: Vec<&str> = book.sheets.iter().map(String::as_str).collect();
        self.engine().adopt_file_sheets(names).map_err(err)?;
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
        // After the tables, because a name can read one. A name can also read a name
        // that comes later, so failures go round again while any succeed. One the engine
        // still can't take only hurts the formulas that use it, and those show up in the
        // per-function report.
        let mut waiting: Vec<&crate::NameInfo> = book.names.iter().collect();
        loop {
            let before = waiting.len();
            waiting.retain(|name| self.define_name(name).is_err());
            if waiting.is_empty() || waiting.len() == before {
                break;
            }
        }
        Ok(())
    }

    fn load_batch(&mut self, cells: &SheetCells<'_>) -> Result<()> {
        self.sheet_name(cells.sheet)?;
        self.pending[cells.sheet as usize].extend_from_slice(cells.cells);
        Ok(())
    }

    fn finish_load(&mut self) -> Result<()> {
        for index in 0..self.pending.len() {
            let mut cells = std::mem::take(&mut self.pending[index]);
            if cells.is_empty() {
                continue;
            }
            let sheet = index as u32;
            // The sort is stable, so that a later write to a cell still wins.
            cells.sort_by_key(|&(row, _, _)| row);
            let rows = cells.iter().map(|&(row, _, _)| row).max().unwrap_or(1);
            let cols = cells.iter().map(|&(_, col, _)| col).max().unwrap_or(1);
            let sparse = wants_sparse(cells.len() as u64, rows, cols);
            self.begin_sheet(sheet, SheetShape { rows, cols, sparse })?;
            let mut entries = Vec::with_capacity(cells.len());
            for (row, col, input) in &cells {
                let value = match input {
                    CellInput::Value(value) => borrow_value(value),
                    CellInput::Formula { cached, .. } => cached.as_ref().and_then(borrow_value),
                    CellInput::Spill { cached, .. } => borrow_value(cached),
                    CellInput::Clear => None,
                };
                if let Some(value) = value {
                    entries.push(LoadEntry {
                        row: *row,
                        col: *col,
                        value,
                    });
                }
            }
            self.load_rows(sheet, &entries)?;
            for (row, col, input) in &cells {
                match input {
                    CellInput::Formula { text, shape, .. } => {
                        self.load_formula(sheet, *row, *col, text, *shape)?;
                    }
                    CellInput::Spill { .. } => self.mark_spill(sheet, *row, *col)?,
                    _ => {}
                }
            }
            self.finish_sheet(sheet, &[])?;
        }
        self.engine().set_first_load_assume_new(false);
        self.engine().reset_ensure_touched();
        Ok(())
    }

    fn begin_sheet(&mut self, sheet: u32, shape: SheetShape) -> Result<()> {
        let name = self.sheet_name(sheet)?.to_string();
        let date_system = self.date_system();
        let cols = shape.cols.max(1) as usize;
        self.array_anchors.retain(|(cell, _)| cell.sheet != sheet);
        self.cached_spills.retain(|cell| cell.sheet != sheet);
        self.linked.retain(|cell, _| cell.sheet != sheet);
        self.loads[sheet as usize] = Some(if shape.sparse {
            SheetLoad::Sparse(Box::new(ArrowSheet::new_sparse_with_date_system(
                &name,
                cols,
                shape.rows.max(1) as usize,
                CHUNK_ROWS,
                date_system,
            )))
        } else {
            SheetLoad::Dense {
                builder: Box::new(IngestBuilder::new(&name, cols, CHUNK_ROWS, date_system)),
                next_row: 1,
                cols,
            }
        });
        Ok(())
    }

    fn load_rows(&mut self, sheet: u32, cells: &[LoadEntry<'_>]) -> Result<()> {
        let index = sheet as usize;
        let mut load = self
            .loads
            .get_mut(index)
            .and_then(Option::take)
            .ok_or_else(|| err(format!("sheet {sheet} has not begun")))?;
        let extent = &mut self.extents[index];
        match &mut load {
            SheetLoad::Dense {
                builder,
                next_row,
                cols,
            } => {
                let empty: Vec<CellIngest<'_>> = (0..*cols).map(|_| CellIngest::Empty).collect();
                let mut row: Vec<CellIngest<'_>> = (0..*cols).map(|_| CellIngest::Empty).collect();
                let mut filled = Vec::new();
                let mut at = 0;
                while at < cells.len() {
                    let number = cells[at].row;
                    if number < *next_row {
                        return Err(err(format!("row {number} arrived out of order")));
                    }
                    while *next_row < number {
                        builder.append_row_cells(&empty).map_err(err)?;
                        *next_row += 1;
                    }
                    while at < cells.len() && cells[at].row == number {
                        let cell = &cells[at];
                        let slot = (cell.col as usize).wrapping_sub(1);
                        let target = row
                            .get_mut(slot)
                            .ok_or_else(|| err("a cell is outside the sheet's shape"))?;
                        *target = to_ingest(&cell.value);
                        filled.push(slot);
                        extent.1 = extent.1.max(cell.col);
                        at += 1;
                    }
                    builder.append_row_cells(&row).map_err(err)?;
                    *next_row += 1;
                    extent.0 = extent.0.max(number);
                    for slot in filled.drain(..) {
                        row[slot] = CellIngest::Empty;
                    }
                }
            }
            SheetLoad::Sparse(store) => {
                for cell in cells {
                    store.set_sparse_overlay_value(
                        (cell.row - 1) as usize,
                        (cell.col - 1) as usize,
                        to_overlay(&cell.value),
                    );
                    extent.0 = extent.0.max(cell.row);
                    extent.1 = extent.1.max(cell.col);
                }
            }
        }
        self.loads[index] = Some(load);
        Ok(())
    }

    fn load_formula(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        text: &str,
        shape: FormulaShape,
    ) -> Result<()> {
        let name = self.sheet_name(sheet)?.to_string();
        let extent = &mut self.extents[sheet as usize];
        extent.0 = extent.0.max(row);
        extent.1 = extent.1.max(col);
        if reads_another_workbook(text) {
            self.linked
                .insert(CellRef { sheet, row, col }, text.to_string());
            return Ok(());
        }
        let staged = match shape {
            FormulaShape::Normal => {
                implicit_intersection(text).unwrap_or_else(|| format!("={text}"))
            }
            FormulaShape::Array(_) | FormulaShape::Dynamic(_) => {
                let staged = format!("={text}");
                self.array_anchors
                    .push((CellRef { sheet, row, col }, staged.clone()));
                staged
            }
        };
        self.engine().stage_formula_text(&name, row, col, staged);
        Ok(())
    }

    fn mark_spill(&mut self, sheet: u32, row: u32, col: u32) -> Result<()> {
        self.cached_spills.push(CellRef { sheet, row, col });
        Ok(())
    }

    fn finish_sheet(&mut self, sheet: u32, hidden_rows: &[u32]) -> Result<()> {
        let name = self.sheet_name(sheet)?.to_string();
        let load = self
            .loads
            .get_mut(sheet as usize)
            .and_then(Option::take)
            .ok_or_else(|| err(format!("sheet {sheet} has not begun")))?;
        let store = match load {
            SheetLoad::Dense { builder, .. } => builder.finish(),
            SheetLoad::Sparse(store) => *store,
        };
        self.install_sheet(store);
        for &row in hidden_rows {
            let _ = self.workbook.set_row_hidden(&name, row, true);
        }
        Ok(())
    }

    fn prepare_step(&mut self) -> Result<bool> {
        if self.prepared {
            return Ok(false);
        }
        if self.prepare_next < self.sheets.len() {
            let name = self.sheets[self.prepare_next].clone();
            self.prepare_next += 1;
            self.engine()
                .build_graph_for_sheets([name.as_str()])
                .map_err(err)?;
            return Ok(true);
        }
        self.engine().adopt_stored_values().map_err(err)?;
        self.prepared = true;
        Ok(false)
    }

    fn set_cells(&mut self, edits: &[CellEdit]) -> Result<Vec<CellEdit>> {
        self.adopt()?;
        self.slice.reset();
        self.idle = false;
        let mut previous = Vec::with_capacity(edits.len());
        for edit in edits {
            let name = self.sheet_name(edit.at.sheet)?.to_string();
            let (row, col) = (edit.at.row, edit.at.col);
            self.linked.remove(&edit.at);
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
            match &edit.input {
                CellInput::Value(value) => {
                    self.workbook
                        .set_value(&name, row, col, to_literal(value))
                        .map_err(err)?;
                }
                CellInput::Clear => {
                    self.workbook
                        .set_value(&name, row, col, LiteralValue::Empty)
                        .map_err(err)?;
                }
                CellInput::Formula { text, .. } => {
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
            // Everything staged is dirty when it is built, so nothing needs marking
            // unless `prepare_step` has already adopted the saved values.
            self.fresh = false;
            if self.prepared {
                self.engine().mark_all_formulas_dirty();
            }
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

    fn calc_all_changes(&mut self) -> Result<Vec<CellRef>> {
        self.slice.reset();
        self.idle = true;
        let (_, delta) = self
            .workbook
            .engine_mut()
            .evaluate_all_with_delta()
            .map_err(err)?;
        // By the engine's own id for each sheet: it keeps its own spelling of a name that differs
        // only in case, such as `Sheet1` for a file's `sheet1`.
        let mut sheets = std::collections::HashMap::new();
        for (index, name) in self.sheets.iter().enumerate() {
            if let Some(id) = self.workbook.engine().sheet_id(name) {
                sheets.insert(id, index);
            }
        }
        let mut cells = Vec::with_capacity(delta.changed_cells.len());
        for cell in &delta.changed_cells {
            if let Some(&index) = sheets.get(&cell.sheet_id()) {
                cells.push(CellRef {
                    sheet: index as u32,
                    row: cell.row0() + 1,
                    col: cell.col0() + 1,
                });
            }
        }
        Ok(cells)
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
        if let Some(text) = self.linked.get(&at) {
            return Some(text.clone());
        }
        let name = self.sheets.get(at.sheet as usize)?;
        self.workbook
            .get_formula(name, at.row, at.col)
            .map(|text| text.trim_start_matches('=').to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::reads_another_workbook;

    #[test]
    fn tells_a_link_to_another_workbook_from_a_table_reference() {
        for linked in [
            "[1]Sheet1!A1",
            "[1]Sheet1!$A$1:$B$2+1",
            "SUM([2]Data!A1:A6)",
            "[1]!Total",
            "'[1]Params Tab 1'!$B$1",
            "'C:\\files\\[book.xlsx]Sheet 1'!A1",
            "[link-external-workbook-a.xlsx]Sheet0!A1",
            "'[1](0)泰州市'!B8",
            "[1]Лист1!A3",
        ] {
            assert!(reads_another_workbook(linked), "{linked}");
        }
        for local in [
            "A1+1",
            "Table1[Column]",
            "SUM(Table1[[#All],[Sales]])",
            "Table1[@Sales]*2",
            "\"[1]Sheet1!A1\"",
            "'Sheet [1]'&\"x\"",
            "'It''s'!A1",
            "[@Sales]",
            "[Sales]*2",
            "'My [Sheet'&\"!\"",
        ] {
            assert!(!reads_another_workbook(local), "{local}");
        }
    }
}
