//! Formualizer behind `CalcEngine`.
//!
//! Loading uses deferred graph building: cached values go into the Arrow store, and
//! formula text is staged without being parsed. So opening calculates nothing, and a
//! staged formula keeps its cached value. The first edit builds the graph and adopts
//! those values as current, so that only what depends on the edit recalculates.
//! `evaluate_slice` keeps what it finished, unlike a cancelled full evaluation, which
//! puts everything it did back to dirty.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use cgs_numfmt::{Locale, Options, Value as FormatValue};

use formualizer_common::{DateSystem, ExcelError, ExcelErrorKind, LiteralValue};
use formualizer_eval::arrow_store::{
    map_error_code, ArrowSheet, CellIngest, IngestBuilder, OverlayValue,
};
use formualizer_eval::engine::eval::{Engine, EvaluationSlice};
use formualizer_eval::engine::named_range::{NameScope, NamedDefinition};
use formualizer_eval::engine::{
    CycleConfig, CycleDetection, CyclePolicy, EvalConfig, FormulaParsePolicy, TemporalEgress,
};
use formualizer_eval::reference::{CellRef as FzCell, Coord, RangeRef};
use formualizer_parse::parser::{parse, ASTNode, ASTNodeType, ReferenceType};
use formualizer_workbook::workbook::WBResolver;
use formualizer_workbook::{CustomFnOptions, Workbook, WorkbookConfig};

use cgs_refs::{
    delete_sheet_in_formula, delete_tables_in_formula, edit_formula, reads_sheet, reads_tables,
    rename_in_formula, touches_band,
};

use crate::file_formula::{expand_three_d, qualify_bare_columns, MovingNames};
use crate::{
    err, wants_sparse, BookInfo, CalcEngine, CalcError, CalcProgress, CalcValue, CellEdit,
    CellInput, CellRef, CellSink, FormulaShape, LoadEntry, LoadValue, NameInfo, Rect, Result,
    SheetCells, SheetRect, SheetShape, SheetUndo, StructuralOp, StructuralUndo,
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
    /// The pivot tables GETPIVOTDATA reads, which the engine sets after laying them out.
    pivots: crate::pivot_data::PivotIndex,
    sheets: Vec<String>,
    /// Sheets that have been deleted, whose indexes stay taken so that an undo can bring them back.
    gone: Vec<bool>,
    /// Defined names as they read now, for telling what a change to the workbook's shape does.
    names: Vec<NameInfo>,
    /// Names, in lower case, that stand for more than one cell.
    many_names: HashSet<String>,
    /// Names, in lower case, that read another workbook. Formulas that use them keep their saved
    /// value, as a formula that reads the workbook itself does.
    linked_names: HashSet<String>,
    /// Names that read something different from one formula to the next.
    moving_names: MovingNames,
    /// Tables as they stand now, as the engine doesn't move them with rows and columns.
    tables: Vec<crate::TableInfo>,
    /// Sheets between `begin_sheet` and `finish_sheet`.
    loads: Vec<Option<SheetLoad>>,
    /// What `load_batch` holds until `finish_load`.
    pending: Vec<Vec<(u32, u32, CellInput)>>,
    extents: Vec<(u32, u32)>,
    /// Array formulas, which must run again to rebuild their spills, with how they are stored.
    array_anchors: Vec<(CellRef, String, FormulaShape)>,
    /// Cells showing a saved spill. A stored value would block the real spill, so
    /// these empty out when calculation first starts.
    cached_spills: Vec<CellRef>,
    /// Formulas that read another workbook, which keep their saved value, as they do in Excel
    /// until its links are updated. The engine would refuse them, and the whole sheet with them.
    linked: HashMap<CellRef, String>,
    /// Formulas with this-row table references as they were given. The engine turns those into
    /// the cells they read, so its own text would lose them.
    this_row: HashMap<CellRef, String>,
    /// Formulas conditional formats and validation evaluate, parsed once each.
    parsed: std::cell::RefCell<HashMap<String, Option<std::rc::Rc<ASTNode>>>>,
    /// Staged formulas haven't been adopted or calculated yet.
    fresh: bool,
    /// The next sheet `prepare_step` builds a graph for.
    prepare_next: usize,
    /// The graph is built and its saved values count as current.
    prepared: bool,
    slice: EvaluationSlice,
    idle: bool,
    changed: Vec<CellRef>,
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
        circular_as_zero: true,
        // A formula that only names itself in a range it never reads isn't circular, as in Excel.
        cycle: CycleConfig {
            detection: CycleDetection::Runtime,
            policy: CyclePolicy::Error,
        },
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

fn new_workbook(
    date1904: bool,
    locale: &Arc<RwLock<Locale>>,
    pivots: &crate::pivot_data::PivotIndex,
) -> Workbook {
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
    // GETPIVOTDATA reads the pivot tables as last laid out, so it runs on every recalculation.
    let pivots = Arc::clone(pivots);
    let pivot_data = move |args: &[LiteralValue]| crate::pivot_data::get_pivot_data(args, &pivots);
    let options = CustomFnOptions {
        min_args: 2,
        max_args: None,
        volatile: true,
        thread_safe: true,
        deterministic: false,
        allow_override_builtin: true,
    };
    let _ = workbook.register_custom_function_with_references(
        "GETPIVOTDATA",
        options,
        &[1],
        Arc::new(pivot_data),
    );
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

/// Every function the engine knows, sorted.
pub fn function_names() -> Vec<String> {
    formualizer_eval::builtins::load_builtins();
    let mut names: Vec<String> = formualizer_eval::function_registry::snapshot_registered()
        .into_iter()
        .filter(|(namespace, _, _)| namespace.is_empty())
        .map(|(_, name, _)| name)
        .collect();
    names.sort();
    names.dedup();
    names
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

/// A file writes characters XML can't hold in a table's column names as `_x000a_`, and a formula
/// names the column with the character itself.
fn decoded_xml_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(at) = rest.find("_x") {
        let code = rest
            .get(at + 2..at + 6)
            .filter(|_| rest.get(at + 6..at + 7) == Some("_"))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .and_then(char::from_u32);
        match code {
            Some(ch) => {
                out.push_str(&rest[..at]);
                out.push(ch);
                rest = &rest[at + 7..];
            }
            None => {
                out.push_str(&rest[..at + 2]);
                rest = &rest[at + 2..];
            }
        }
    }
    out.push_str(rest);
    out
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
        let pivots = crate::pivot_data::PivotIndex::default();
        Self {
            workbook: new_workbook(false, &locale, &pivots),
            locale,
            pivots,
            sheets: Vec::new(),
            gone: Vec::new(),
            names: Vec::new(),
            many_names: HashSet::new(),
            linked_names: HashSet::new(),
            moving_names: MovingNames::default(),
            tables: Vec::new(),
            loads: Vec::new(),
            pending: Vec::new(),
            extents: Vec::new(),
            array_anchors: Vec::new(),
            cached_spills: Vec::new(),
            linked: HashMap::new(),
            this_row: HashMap::new(),
            parsed: std::cell::RefCell::new(HashMap::new()),
            fresh: true,
            prepare_next: 0,
            prepared: false,
            slice: EvaluationSlice::tracking(),
            idle: true,
            changed: Vec::new(),
        }
    }

    /// What the engine needs to read a file's formula, if that isn't the text itself.
    fn file_formula(&self, text: &str, sheet: u32, row: u32, col: u32) -> Option<String> {
        let mut text = std::borrow::Cow::Borrowed(text);
        let mut changed = false;
        if let Some(next) = crate::file_formula::column_line_ends(&text) {
            text = std::borrow::Cow::Owned(next);
            changed = true;
        }
        if let Some(next) = self.moving_names.expand(&text, sheet, row, col) {
            text = std::borrow::Cow::Owned(next.trim_start_matches('=').to_string());
            changed = true;
        }
        if let Some(next) = qualify_bare_columns(&text, &self.tables, sheet, row, col) {
            text = std::borrow::Cow::Owned(next);
            changed = true;
        }
        if let Some(next) = expand_three_d(&text, &self.sheets) {
            text = std::borrow::Cow::Owned(next.trim_start_matches('=').to_string());
            changed = true;
        }
        changed.then(|| text.into_owned())
    }

    /// The pivot tables as GETPIVOTDATA reads them.
    pub fn pivots(&self) -> Vec<crate::pivot_data::PivotLookup> {
        self.pivots
            .read()
            .map(|tables| tables.clone())
            .unwrap_or_default()
    }

    /// The pivot tables GETPIVOTDATA reads from now on.
    pub fn set_pivots(&mut self, tables: Vec<crate::pivot_data::PivotLookup>) {
        if let Ok(mut current) = self.pivots.write() {
            *current = tables;
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
            .filter(|_| !self.gone[sheet as usize])
            .map(String::as_str)
            .ok_or_else(|| err(format!("no sheet {sheet}")))
    }

    /// Our index for each of the engine's sheet ids. It keeps its own spelling of a name that
    /// differs only in case, such as `Sheet1` for a file's `sheet1`.
    fn sheet_indexes(&self) -> HashMap<u16, u32> {
        let mut map = HashMap::with_capacity(self.sheets.len());
        for (index, name) in self.sheets.iter().enumerate() {
            if self.gone[index] {
                continue;
            }
            if let Some(id) = self.workbook.engine().sheet_id(name) {
                map.insert(id, index as u32);
            }
        }
        map
    }

    /// Every formula's cell with its text. Costs as much as the formulas there are.
    fn formulas(&self) -> Vec<(CellRef, String)> {
        let indexes = self.sheet_indexes();
        let mut out = Vec::new();
        for cell in self.workbook.engine().formula_cells() {
            let Some(&sheet) = indexes.get(&cell.sheet_id) else {
                continue;
            };
            let at = CellRef {
                sheet,
                row: cell.coord.row() + 1,
                col: cell.coord.col() + 1,
            };
            if let Some(text) = self.formula_text(at) {
                out.push((at, text));
            }
        }
        out
    }

    /// Moves what this keeps by position.
    fn move_anchors(&mut self, sheet: u32, edit: &cgs_refs::AxisEdit) {
        let moved = |at: &mut CellRef| -> bool {
            if at.sheet != sheet {
                return true;
            }
            let place = match edit.axis {
                cgs_refs::Axis::Row => &mut at.row,
                cgs_refs::Axis::Col => &mut at.col,
            };
            match edit.point(*place) {
                Some(next) => {
                    *place = next;
                    true
                }
                None => false,
            }
        };
        let mut anchors = std::mem::take(&mut self.array_anchors);
        anchors.retain_mut(|(at, _, shape)| {
            if !moved(at) {
                return false;
            }
            if at.sheet == sheet {
                *shape = moved_shape(*shape, edit);
            }
            true
        });
        self.array_anchors = anchors;
        let linked = std::mem::take(&mut self.linked);
        self.linked = linked
            .into_iter()
            .filter_map(|(mut at, text)| moved(&mut at).then_some((at, text)))
            .collect();
        let name = self.sheets[sheet as usize].clone();
        let this_row = std::mem::take(&mut self.this_row);
        self.this_row = this_row
            .into_iter()
            .filter_map(|(mut at, text)| {
                let home = self.sheets[at.sheet as usize].clone();
                let text = cgs_refs::edit_formula(&text, &home, &name, edit).into_owned();
                moved(&mut at).then_some((at, text))
            })
            .collect();
        let extent = &mut self.extents[sheet as usize];
        let reach = match edit.axis {
            cgs_refs::Axis::Row => &mut extent.0,
            cgs_refs::Axis::Col => &mut extent.1,
        };
        if *reach > 0 {
            *reach = if edit.delete {
                edit.span((1, *reach)).map_or(0, |span| span.1)
            } else if *reach >= edit.at {
                (*reach + edit.count).min(edit.axis.max())
            } else {
                *reach
            };
        }
    }

    /// Takes down every array on a sheet before its rows or columns move, as an array's spill
    /// doesn't move with them. `put_back_arrays` makes them again where they land.
    fn take_down_arrays(&mut self, sheet: u32) -> Result<Vec<(CellRef, String)>> {
        let name = self.sheet_name(sheet)?.to_string();
        let mut taken = Vec::new();
        let mut anchors: Vec<CellRef> = self
            .array_anchors
            .iter()
            .filter(|(at, _, _)| at.sheet == sheet)
            .map(|(at, _, _)| *at)
            .collect();
        for area in self.spill_areas() {
            if area.sheet == sheet {
                let at = CellRef {
                    sheet,
                    row: area.rect.top,
                    col: area.rect.left,
                };
                if !anchors.contains(&at) {
                    anchors.push(at);
                }
            }
        }
        for at in anchors {
            let Some(text) = self.formula_text(at) else {
                continue;
            };
            self.workbook
                .set_value(&name, at.row, at.col, LiteralValue::Empty)
                .map_err(err)?;
            taken.push((at, text));
        }
        Ok(taken)
    }

    /// Makes arrays again after their rows or columns moved. `move_anchors` has already moved
    /// their shapes.
    fn put_back_arrays(
        &mut self,
        sheet: u32,
        edit: &cgs_refs::AxisEdit,
        taken: Vec<(CellRef, String)>,
    ) -> Result<()> {
        let name = self.sheet_name(sheet)?.to_string();
        for (at, text) in taken {
            let (row, col) = match edit.axis {
                cgs_refs::Axis::Row => (edit.point(at.row), Some(at.col)),
                cgs_refs::Axis::Col => (Some(at.row), edit.point(at.col)),
            };
            let (Some(row), Some(col)) = (row, col) else {
                continue;
            };
            let moved = CellRef { sheet, row, col };
            let staged = format!("={}", edit_formula(&text, &name, &name, edit));
            self.workbook
                .set_formula(&name, row, col, &staged)
                .map_err(err)?;
            let mut shape = FormulaShape::Normal;
            if let Some(entry) = self
                .array_anchors
                .iter_mut()
                .find(|(cell, _, _)| *cell == moved)
            {
                entry.1 = staged;
                shape = entry.2;
            }
            self.declare_array(&name, moved, shape);
        }
        Ok(())
    }

    fn table_range(&mut self, table: &crate::TableInfo) -> Result<RangeRef> {
        let sheet = self.sheet_name(table.sheet)?.to_string();
        let id = self
            .engine()
            .sheet_id(&sheet)
            .ok_or_else(|| err("no sheet id"))?;
        let start = FzCell::new(
            id,
            Coord::from_excel(table.range.top, table.range.left, true, true),
        );
        let end = FzCell::new(
            id,
            Coord::from_excel(table.range.bottom, table.range.right, true, true),
        );
        Ok(RangeRef::new(start, end))
    }

    /// Tells the engine about one table, as it now stands.
    fn sync_table(&mut self, table: &crate::TableInfo) -> Result<()> {
        let range = self.table_range(table)?;
        let engine = self.engine();
        if engine
            .update_table(
                &table.name,
                range,
                table.header_row,
                table.columns.clone(),
                table.totals_row,
            )
            .is_err()
        {
            engine
                .define_table(
                    &table.name,
                    range,
                    table.header_row,
                    table.columns.clone(),
                    table.totals_row,
                )
                .map_err(err)?;
        }
        Ok(())
    }

    /// Moves tables with an edit: returns the header cells of new columns, and what has gone.
    fn move_tables(&mut self, sheet: u32, edit: &cgs_refs::AxisEdit) -> Result<MovedTables> {
        let mut headers = Vec::new();
        let mut gone = Vec::new();
        let mut tables = std::mem::take(&mut self.tables);
        let mut kept = Vec::with_capacity(tables.len());
        for mut table in tables.drain(..) {
            if table.sheet != sheet {
                kept.push(table);
                continue;
            }
            let area = (
                table.range.top,
                table.range.left,
                table.range.bottom,
                table.range.right,
            );
            let Some((top, left, bottom, right)) = cgs_refs::edit_rect(area, edit) else {
                gone.push(table.name.clone());
                let _ = self.engine().delete_table(&table.name);
                continue;
            };
            if edit.axis == cgs_refs::Axis::Col {
                if edit.delete {
                    let first = table.range.left;
                    let mut index = 0;
                    table.columns.retain(|_| {
                        let col = first + index;
                        index += 1;
                        edit.point(col).is_some()
                    });
                } else if edit.at > table.range.left && edit.at <= table.range.right {
                    let at = (edit.at - table.range.left) as usize;
                    for step in 0..edit.count as usize {
                        let name = crate::new_column_name(&table.columns);
                        table.columns.insert(at + step, name.clone());
                        if table.header_row {
                            headers.push((
                                CellRef {
                                    sheet,
                                    row: top,
                                    col: edit.at + step as u32,
                                },
                                name,
                            ));
                        }
                    }
                }
            }
            table.range = Rect {
                top,
                left,
                bottom,
                right,
            };
            self.sync_table(&table)?;
            kept.push(table);
        }
        self.tables = kept;
        Ok((headers, gone))
    }

    /// Puts names back through the engine and into this adapter's own list.
    fn set_name(&mut self, name: &NameInfo) -> Result<()> {
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
        if self
            .engine()
            .update_name(&name.name, definition.clone(), scope)
            .is_err()
        {
            self.engine()
                .define_name(&name.name, definition, scope)
                .map_err(err)?;
        }
        if let Some(entry) = self
            .names
            .iter_mut()
            .find(|entry| entry.name == name.name && entry.local_sheet == name.local_sheet)
        {
            entry.formula = name.formula.clone();
        }
        Ok(())
    }

    /// Records the results a slice's list holds, by our sheet numbers.
    fn collect_changed(&mut self) -> Result<()> {
        let delta = self.slice.take_changed().map_err(err)?;
        if delta.changed_cells.is_empty() {
            return Ok(());
        }
        let indexes = self.sheet_indexes();
        self.changed.reserve(delta.changed_cells.len());
        for cell in &delta.changed_cells {
            if let Some(&sheet) = indexes.get(&cell.sheet_id()) {
                self.changed.push(CellRef {
                    sheet,
                    row: cell.row0() + 1,
                    col: cell.col0() + 1,
                });
            }
        }
        Ok(())
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
        for (cell, text, shape) in self.array_anchors.clone() {
            let name = self.sheets[cell.sheet as usize].clone();
            self.workbook
                .set_formula(&name, cell.row, cell.col, &text)
                .map_err(err)?;
            self.declare_array(&name, cell, shape);
        }
        Ok(())
    }

    /// A `{=...}` formula fills its area whatever size its result is, as Excel's do. If the area
    /// is blocked it spills as far as it can instead, as a dynamic array would.
    fn declare_array(&mut self, name: &str, cell: CellRef, shape: FormulaShape) {
        if let FormulaShape::Array(rect) = shape {
            let _ = self.engine().declare_fixed_array_formula(
                name,
                cell.row,
                cell.col,
                rect.bottom - rect.top + 1,
                rect.right - rect.left + 1,
            );
        }
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

/// Whether a parsed formula has nothing the parser only let through by guessing.
fn well_formed(_ast: &ASTNode) -> bool {
    true
}

/// Header cells of new columns with their names, and tables nothing is left of.
type MovedTables = (Vec<(CellRef, String)>, Vec<String>);

/// Where an array's area goes.
fn moved_shape(shape: FormulaShape, edit: &cgs_refs::AxisEdit) -> FormulaShape {
    let area = |rect: Rect| {
        cgs_refs::edit_rect((rect.top, rect.left, rect.bottom, rect.right), edit).map(
            |(top, left, bottom, right)| Rect {
                top,
                left,
                bottom,
                right,
            },
        )
    };
    match shape {
        FormulaShape::Normal => FormulaShape::Normal,
        FormulaShape::Array(rect) => area(rect).map_or(FormulaShape::Normal, FormulaShape::Array),
        FormulaShape::Dynamic(rect) => {
            area(rect).map_or(FormulaShape::Normal, FormulaShape::Dynamic)
        }
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
        self.workbook = new_workbook(book.date1904, &self.locale, &self.pivots);
        self.sheets = book.sheets.clone();
        self.gone = vec![false; book.sheets.len()];
        self.names = book.names.clone();
        self.many_names = book
            .names
            .iter()
            .filter(|name| name.formula.contains(':'))
            .map(|name| name.name.to_ascii_lowercase())
            .collect();
        self.linked_names = book
            .names
            .iter()
            .filter(|name| reads_another_workbook(&name.formula))
            .map(|name| name.name.to_ascii_lowercase())
            .collect();
        self.moving_names = MovingNames::new(&book.names);
        self.tables = book.tables.clone();
        self.loads = book.sheets.iter().map(|_| None).collect();
        self.pending = vec![Vec::new(); book.sheets.len()];
        self.extents = vec![(0, 0); book.sheets.len()];
        self.array_anchors.clear();
        self.cached_spills.clear();
        self.linked.clear();
        self.this_row.clear();
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
                table
                    .columns
                    .iter()
                    .map(|name| decoded_xml_name(name))
                    .collect(),
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
        self.array_anchors
            .retain(|(cell, _, _)| cell.sheet != sheet);
        self.cached_spills.retain(|cell| cell.sheet != sheet);
        self.linked.retain(|cell, _| cell.sheet != sheet);
        self.this_row.retain(|cell, _| cell.sheet != sheet);
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
        self.sheet_name(sheet)?;
        let extent = &mut self.extents[sheet as usize];
        extent.0 = extent.0.max(row);
        extent.1 = extent.1.max(col);
        let linked_names = &self.linked_names;
        if reads_another_workbook(text)
            || (!linked_names.is_empty()
                && crate::intersect::mentions_name(text, &|word| {
                    linked_names.contains(&word.to_ascii_lowercase())
                }))
        {
            self.linked
                .insert(CellRef { sheet, row, col }, text.to_string());
            return Ok(());
        }
        if uses_this_row(text) {
            self.this_row
                .insert(CellRef { sheet, row, col }, text.to_string());
        }
        let rewritten = self.file_formula(text, sheet, row, col);
        let text = rewritten.as_deref().unwrap_or(text);
        let staged = match shape {
            FormulaShape::Normal => {
                let many = &self.many_names;
                let staged = if many.is_empty() {
                    crate::intersect::mark(text, &|_| false)
                } else {
                    crate::intersect::mark(text, &|name| many.contains(&name.to_ascii_lowercase()))
                };
                staged.unwrap_or_else(|| format!("={text}"))
            }
            FormulaShape::Array(_) | FormulaShape::Dynamic(_) => {
                let staged = format!("={text}");
                self.array_anchors
                    .push((CellRef { sheet, row, col }, staged.clone(), shape));
                staged
            }
        };
        // The name is lent to the engine rather than copied, as this runs for every formula.
        let name = std::mem::take(&mut self.sheets[sheet as usize]);
        self.engine().stage_formula_text(&name, row, col, staged);
        self.sheets[sheet as usize] = name;
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
            previous.push(CellEdit {
                at: edit.at,
                input: self.cell_input(edit.at),
            });
            self.linked.remove(&edit.at);
            self.this_row.remove(&edit.at);
            self.array_anchors.retain(|(at, _, _)| *at != edit.at);
            let extent = &mut self.extents[edit.at.sheet as usize];
            extent.0 = extent.0.max(row);
            extent.1 = extent.1.max(col);
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
                CellInput::Formula { text, shape, .. } => {
                    let staged = format!("={text}");
                    if uses_this_row(text) {
                        self.this_row.insert(edit.at, text.clone());
                    }
                    // An array that was here takes what it spilled with it first.
                    if *shape != FormulaShape::Normal {
                        self.workbook
                            .set_value(&name, row, col, LiteralValue::Empty)
                            .map_err(err)?;
                    }
                    self.workbook
                        .set_formula(&name, row, col, &staged)
                        .map_err(err)?;
                    if *shape != FormulaShape::Normal {
                        self.array_anchors.push((edit.at, staged, *shape));
                        self.declare_array(&name, edit.at, *shape);
                    }
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
        let sheet = op.sheet();
        let edit = op.edit();
        let inverse = StructuralOp::new(sheet, edit.inverse());
        let name = self.sheet_name(sheet)?.to_string();
        // A delete takes cells with it and shrinks ranges, which only their pre-images can undo.
        // Everything else comes back when the rows or columns do.
        let mut removed = Vec::new();
        let mut names = Vec::new();
        let tables_before = self.tables.clone();
        // Formulas that read a table this leaves nothing of.
        let lost: Vec<String> = self
            .tables
            .iter()
            .filter(|table| {
                table.sheet == sheet
                    && cgs_refs::edit_rect(
                        (
                            table.range.top,
                            table.range.left,
                            table.range.bottom,
                            table.range.right,
                        ),
                        &edit,
                    )
                    .is_none()
            })
            .map(|table| table.name.clone())
            .collect();
        // Columns it takes from tables that stay.
        let lost_columns: Vec<(String, Vec<String>)> = if edit.delete
            && edit.axis == cgs_refs::Axis::Col
        {
            self.tables
                .iter()
                .filter(|table| table.sheet == sheet && !lost.contains(&table.name))
                .filter_map(|table| {
                    let names: Vec<String> = table
                        .columns
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| edit.point(table.range.left + *index as u32).is_none())
                        .map(|(_, name)| name.clone())
                        .collect();
                    (!names.is_empty()).then(|| (table.name.clone(), names))
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut table_rewrites = Vec::new();
        if edit.delete {
            let band = |at: &CellRef| {
                let place = match edit.axis {
                    cgs_refs::Axis::Row => at.row,
                    cgs_refs::Axis::Col => at.col,
                };
                at.sheet == sheet && place >= edit.at && place - edit.at < edit.count
            };
            for (at, text) in self.formulas() {
                let home = self.sheets[at.sheet as usize].as_str();
                let without = lost_columns.iter().fold(
                    std::borrow::Cow::Borrowed(text.as_str()),
                    |current, (table, names)| match cgs_refs::delete_table_columns(
                        &current, table, names,
                    ) {
                        std::borrow::Cow::Owned(next) => std::borrow::Cow::Owned(next),
                        std::borrow::Cow::Borrowed(_) => current,
                    },
                );
                let reads_lost = reads_tables(&text, &lost) || without != text;
                if band(&at) || reads_lost || touches_band(&text, home, &name, &edit) {
                    removed.push(CellEdit {
                        at,
                        input: self.cell_input(at),
                    });
                }
                if reads_lost && !band(&at) {
                    let place = match edit.axis {
                        cgs_refs::Axis::Row => CellRef {
                            row: edit.point(at.row).unwrap_or(at.row),
                            ..at
                        },
                        cgs_refs::Axis::Col => CellRef {
                            col: edit.point(at.col).unwrap_or(at.col),
                            ..at
                        },
                    };
                    let place = if at.sheet == sheet { place } else { at };
                    let moved = edit_formula(&without, home, &name, &edit);
                    table_rewrites.push(CellEdit {
                        at: place,
                        input: CellInput::Formula {
                            text: delete_tables_in_formula(&moved, &lost).into_owned(),
                            shape: self.formula_shape(at),
                            cached: None,
                        },
                    });
                }
            }
            let spills = self.spill_areas();
            let extent = self.extents[sheet as usize];
            let rect = match edit.axis {
                cgs_refs::Axis::Row => Rect {
                    top: edit.at,
                    left: 1,
                    bottom: (edit.at + edit.count - 1).min(extent.0.max(1)),
                    right: extent.1.max(1),
                },
                cgs_refs::Axis::Col => Rect {
                    top: 1,
                    left: edit.at,
                    bottom: extent.0.max(1),
                    right: (edit.at + edit.count - 1).min(extent.1.max(1)),
                },
            };
            if rect.top <= rect.bottom && rect.left <= rect.right {
                let mut values = Collected::default();
                self.read_block(SheetRect { sheet, rect }, &mut values);
                for (row, col, value) in values.0 {
                    let at = CellRef { sheet, row, col };
                    let spilled = spills.iter().any(|area| {
                        area.sheet == sheet
                            && area.rect.contains(row, col)
                            && (area.rect.top, area.rect.left) != (row, col)
                    });
                    if !spilled && self.formula_text(at).is_none() {
                        removed.push(CellEdit {
                            at,
                            input: CellInput::Value(value),
                        });
                    }
                }
            }
            names = self
                .names
                .iter()
                .filter(|info| touches_band(&info.formula, "", &name, &edit))
                .cloned()
                .collect();
        }
        for info in &mut self.names {
            if let std::borrow::Cow::Owned(next) = edit_formula(&info.formula, "", &name, &edit) {
                info.formula = next;
            }
        }
        let arrays = self.take_down_arrays(sheet)?;
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
        self.move_anchors(sheet, &edit);
        let (new_headers, gone_tables) = self.move_tables(sheet, &edit)?;
        self.put_back_arrays(sheet, &edit, arrays)?;
        if !table_rewrites.is_empty() {
            self.set_cells(&table_rewrites)?;
        }
        Ok(StructuralUndo {
            inverse,
            removed,
            names,
            tables: tables_before,
            new_headers,
            gone_tables,
            gone_columns: lost_columns,
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
            self.workbook
                .engine_mut()
                .evaluate_cells_tracked(&mut self.slice, &targets)
                .map_err(err)?;
            // Those cells are clean now, so a pass planned before them is out of date.
            self.slice.reset();
        }
        let result = self
            .workbook
            .engine_mut()
            .evaluate_slice(&mut self.slice, budget)
            .map_err(err)?;
        self.collect_changed()?;
        if result.finished {
            self.idle = true;
            return Ok(CalcProgress::Done);
        }
        Ok(CalcProgress::More { remaining: None })
    }

    fn calc_all_changes(&mut self) -> Result<Vec<CellRef>> {
        while self.calc_slice(60_000, &[])? != CalcProgress::Done {}
        Ok(self.take_changed())
    }

    fn take_changed(&mut self) -> Vec<CellRef> {
        std::mem::take(&mut self.changed)
    }

    fn add_sheet(&mut self, name: &str) -> Result<u32> {
        self.adopt()?;
        self.workbook.add_sheet(name).map_err(err)?;
        self.sheets.push(name.to_string());
        self.gone.push(false);
        self.loads.push(None);
        self.pending.push(Vec::new());
        self.extents.push((0, 0));
        self.slice.reset();
        self.idle = false;
        Ok(self.sheets.len() as u32 - 1)
    }

    fn delete_sheet(&mut self, sheet: u32) -> Result<SheetUndo> {
        self.adopt()?;
        self.slice.reset();
        self.idle = false;
        let name = self.sheet_name(sheet)?.to_string();
        // The sheet's cells and what reads it, as they were.
        let (mut cells, mut readers) = (Vec::new(), Vec::new());
        let mut rewrites = Vec::new();
        let tables_before = self.tables.clone();
        let gone_tables: Vec<String> = self
            .tables
            .iter()
            .filter(|table| table.sheet == sheet)
            .map(|table| table.name.clone())
            .collect();
        for (at, text) in self.formulas() {
            let edit = CellEdit {
                at,
                input: self.cell_input(at),
            };
            if at.sheet == sheet {
                cells.push(edit);
            } else if reads_sheet(&text, "", &name) || reads_tables(&text, &gone_tables) {
                readers.push(edit);
                let without_sheet = delete_sheet_in_formula(&text, &name);
                rewrites.push(CellEdit {
                    at,
                    input: CellInput::Formula {
                        text: delete_tables_in_formula(&without_sheet, &gone_tables).into_owned(),
                        shape: self.formula_shape(at),
                        cached: None,
                    },
                });
            }
        }
        for table in &gone_tables {
            let _ = self.engine().delete_table(table);
        }
        self.tables.retain(|table| table.sheet != sheet);
        let spills = self.spill_areas();
        let extent = self.extents[sheet as usize];
        let mut values = Collected::default();
        self.read_block(
            SheetRect {
                sheet,
                rect: Rect {
                    top: 1,
                    left: 1,
                    bottom: extent.0.max(1),
                    right: extent.1.max(1),
                },
            },
            &mut values,
        );
        for (row, col, value) in values.0 {
            let at = CellRef { sheet, row, col };
            let spilled = spills.iter().any(|area| {
                area.sheet == sheet
                    && area.rect.contains(row, col)
                    && (area.rect.top, area.rect.left) != (row, col)
            });
            if !spilled && self.formula_text(at).is_none() {
                cells.push(CellEdit {
                    at,
                    input: CellInput::Value(value),
                });
            }
        }
        let names: Vec<NameInfo> = self
            .names
            .iter()
            .filter(|info| reads_sheet(&info.formula, "", &name))
            .cloned()
            .collect();
        for info in &mut self.names {
            if let std::borrow::Cow::Owned(next) = delete_sheet_in_formula(&info.formula, &name) {
                info.formula = next;
            }
        }
        self.array_anchors.retain(|(at, _, _)| at.sheet != sheet);
        self.linked.retain(|at, _| at.sheet != sheet);
        self.this_row.retain(|at, _| at.sheet != sheet);
        self.workbook.delete_sheet(&name).map_err(err)?;
        self.gone[sheet as usize] = true;
        self.set_cells(&rewrites)?;
        Ok(SheetUndo {
            cells,
            readers,
            names,
            tables: tables_before,
            gone_tables,
        })
    }

    fn restore_sheet(&mut self, sheet: u32) -> Result<()> {
        let name = self
            .sheets
            .get(sheet as usize)
            .cloned()
            .ok_or_else(|| err(format!("no sheet {sheet}")))?;
        self.workbook.add_sheet(&name).map_err(err)?;
        self.gone[sheet as usize] = false;
        self.extents[sheet as usize] = (0, 0);
        self.slice.reset();
        self.idle = false;
        Ok(())
    }

    fn rename_sheet(&mut self, sheet: u32, name: &str) -> Result<String> {
        self.adopt()?;
        self.slice.reset();
        self.idle = false;
        let old = self.sheet_name(sheet)?.to_string();
        // Read before the engine knows, as it stops being able to tell afterwards.
        let mut rewrites = Vec::new();
        for (at, text) in self.formulas() {
            if let std::borrow::Cow::Owned(next) = rename_in_formula(&text, &old, name) {
                rewrites.push(CellEdit {
                    at,
                    input: CellInput::Formula {
                        text: next,
                        shape: self.formula_shape(at),
                        cached: None,
                    },
                });
            }
        }
        self.workbook.rename_sheet(&old, name).map_err(err)?;
        self.sheets[sheet as usize] = name.to_string();
        for info in &mut self.names {
            if let std::borrow::Cow::Owned(next) = rename_in_formula(&info.formula, &old, name) {
                info.formula = next;
            }
        }
        self.set_cells(&rewrites)?;
        Ok(old)
    }

    fn names(&self) -> Vec<NameInfo> {
        self.names.clone()
    }

    fn restore_names(&mut self, names: &[NameInfo]) -> Result<()> {
        for name in names {
            self.set_name(name)?;
        }
        Ok(())
    }

    fn replace_names(&mut self, names: &[NameInfo]) -> Result<()> {
        self.adopt()?;
        self.slice.reset();
        self.idle = false;
        let gone: Vec<NameInfo> = self
            .names
            .iter()
            .filter(|old| {
                !names.iter().any(|name| {
                    name.name.eq_ignore_ascii_case(&old.name) && name.local_sheet == old.local_sheet
                })
            })
            .cloned()
            .collect();
        for name in gone {
            let scope = match name.local_sheet {
                Some(index) => {
                    let sheet = self.sheet_name(index)?.to_string();
                    match self.engine().sheet_id(&sheet) {
                        Some(id) => NameScope::Sheet(id),
                        None => continue,
                    }
                }
                None => NameScope::Workbook,
            };
            let _ = self.engine().delete_name(&name.name, scope);
        }
        // New ones join the list first, so that setting them finds an entry to keep in step.
        for name in names {
            if !self.names.iter().any(|old| {
                old.name.eq_ignore_ascii_case(&name.name) && old.local_sheet == name.local_sheet
            }) {
                self.names.push(name.clone());
            }
        }
        for name in names {
            self.set_name(name)?;
        }
        self.names = names.to_vec();
        self.many_names = names
            .iter()
            .filter(|name| name.formula.contains(':'))
            .map(|name| name.name.to_ascii_lowercase())
            .collect();
        self.moving_names = MovingNames::new(names);
        Ok(())
    }

    fn tables(&self) -> &[crate::TableInfo] {
        &self.tables
    }

    fn restore_tables(&mut self, tables: &[crate::TableInfo]) -> Result<()> {
        // Tables that aren't in the list go, so that a name can be used again.
        let gone: Vec<String> = self
            .tables
            .iter()
            .filter(|old| {
                !tables
                    .iter()
                    .any(|table| table.name.eq_ignore_ascii_case(&old.name))
            })
            .map(|old| old.name.clone())
            .collect();
        for name in gone {
            let _ = self.engine().delete_table(&name);
        }
        for table in tables {
            self.sync_table(table)?;
        }
        self.tables = tables.to_vec();
        Ok(())
    }

    fn cell_input(&self, at: CellRef) -> CellInput {
        let Ok(name) = self.sheet_name(at.sheet) else {
            return CellInput::Clear;
        };
        let value = self
            .workbook
            .get_value(name, at.row, at.col)
            .map_or(CalcValue::Empty, |value| from_literal(&value));
        match self.formula_text(at) {
            Some(text) => CellInput::Formula {
                text,
                shape: self.formula_shape(at),
                cached: Some(value),
            },
            None if value == CalcValue::Empty => CellInput::Clear,
            None => CellInput::Value(value),
        }
    }

    fn spill_areas(&self) -> Vec<SheetRect> {
        let indexes = self.sheet_indexes();
        self.workbook
            .engine()
            .spill_areas()
            .into_iter()
            .filter_map(|(anchor, rows, cols)| {
                let sheet = *indexes.get(&anchor.sheet_id)?;
                let (top, left) = (anchor.coord.row() + 1, anchor.coord.col() + 1);
                Some(SheetRect {
                    sheet,
                    rect: Rect {
                        top,
                        left,
                        bottom: top + rows - 1,
                        right: left + cols - 1,
                    },
                })
            })
            .collect()
    }

    fn formula_shape(&self, at: CellRef) -> FormulaShape {
        self.array_anchors
            .iter()
            .find(|(cell, _, _)| *cell == at)
            .map_or(FormulaShape::Normal, |(_, _, shape)| *shape)
    }

    fn is_calculating(&self) -> bool {
        !self.idle
    }

    fn all_formulas(&mut self) -> Result<Vec<(CellRef, String)>> {
        self.adopt()?;
        // Formulas set since the last calculation wait to be built, and the graph is what lists them.
        if self.engine().has_staged_formulas() {
            self.engine().build_graph_all().map_err(err)?;
        }
        Ok(self.formulas())
    }

    fn check_formula(&self, text: &str) -> Result<String> {
        let trimmed = text.trim();
        let mut depth = 0i32;
        let mut quote = None;
        for ch in trimmed.chars() {
            match quote {
                Some(open) => {
                    if ch == open {
                        quote = None;
                    }
                }
                None => match ch {
                    '"' | '\'' => quote = Some(ch),
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                },
            }
        }
        let mut fixed = trimmed.to_string();
        if let Some(open) = quote {
            if open == '"' {
                fixed.push('"');
            }
        }
        for _ in 0..depth.max(0) {
            fixed.push(')');
        }
        let problem = || {
            CalcError(
                "There's a problem with this formula. Check its brackets, commas and quotes."
                    .to_string(),
            )
        };
        if depth < 0 || fixed.starts_with('=') {
            return Err(problem());
        }
        let ast = parse(format!("={fixed}")).map_err(|_| problem())?;
        if !well_formed(&ast) {
            return Err(problem());
        }
        Ok(fixed)
    }

    fn start_volatile(&mut self) -> Result<()> {
        self.adopt()?;
        self.slice.reset();
        self.engine().mark_volatile_formulas_dirty();
        self.idle = false;
        Ok(())
    }

    fn read_block(&self, r: SheetRect, out: &mut dyn CellSink) {
        let Ok(name) = self.sheet_name(r.sheet) else {
            return;
        };
        self.workbook.engine().for_each_range_value(
            name,
            r.rect.top,
            r.rect.left,
            r.rect.bottom,
            r.rect.right,
            |row_offset, col_offset, value| {
                if !matches!(value, LiteralValue::Empty) {
                    out.take(
                        r.rect.top + row_offset as u32,
                        r.rect.left + col_offset as u32,
                        from_literal(&value),
                    );
                }
            },
        );
    }

    fn value_bounds(&self, sheet: u32) -> Option<(u32, u32)> {
        use formualizer_eval::traits::EvaluationContext;
        let name = self.sheet_name(sheet).ok()?;
        let engine = self.workbook.engine();
        let (_, last_row) = engine.used_rows_for_columns(name, 1, 16_384)?;
        let (_, last_col) = engine.used_cols_for_rows(name, 1, 1_048_576)?;
        Some((last_row, last_col))
    }

    fn formula_text(&self, at: CellRef) -> Option<String> {
        if let Some(text) = self.linked.get(&at) {
            return Some(text.clone());
        }
        let name = self.sheet_name(at.sheet).ok()?;
        if let Some(text) = self.this_row.get(&at) {
            // Only while the engine still holds a formula there.
            return self
                .workbook
                .get_formula(name, at.row, at.col)
                .map(|_| text.clone());
        }
        self.workbook
            .get_formula(name, at.row, at.col)
            .map(|text| text.trim_start_matches('=').to_string())
    }
}

impl FormualizerEngine {
    /// Works out a formula written for `anchor` as if it stood at `row` and `col`, as conditional
    /// formats and validation do, without putting it in any cell. A formula that won't parse is
    /// `#NAME?`.
    pub fn evaluate_at(
        &self,
        sheet: &str,
        anchor: (u32, u32),
        row: u32,
        col: u32,
        text: &str,
    ) -> CalcValue {
        let ast = {
            let mut parsed = self.parsed.borrow_mut();
            if parsed.len() > 10_000 {
                parsed.clear();
            }
            parsed
                .entry(text.to_string())
                .or_insert_with(|| {
                    parse(format!("={}", text.trim_start_matches('=')))
                        .ok()
                        .map(std::rc::Rc::new)
                })
                .clone()
        };
        match ast {
            Some(ast) => from_literal(
                &self
                    .workbook
                    .engine()
                    .evaluate_formula_at(sheet, anchor, row, col, &ast),
            ),
            None => CalcValue::Error("#NAME?".to_string()),
        }
    }
}

/// Whether a formula reads its own row of a table, which the engine turns into cells.
fn uses_this_row(text: &str) -> bool {
    // Most formulas have neither mark, and this runs for every formula a file holds.
    let bytes = text.as_bytes();
    if !bytes.iter().any(|&b| b == b'[' || b == b'#') {
        return false;
    }
    text.contains("[@") || text.to_ascii_lowercase().contains("#this row")
}

/// A block read, kept.
#[derive(Default)]
struct Collected(Vec<(u32, u32, CalcValue)>);

impl CellSink for Collected {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.push((row, col, value.clone()));
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
