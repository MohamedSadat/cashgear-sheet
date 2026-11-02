//! One workbook through one engine: load, read cached values, calculate in slices,
//! score against the stored values, random edits, then a top-row insert.

use std::collections::{BTreeMap, HashMap};

use cgs_calc::{
    now_ms, BookInfo, CalcEngine, CalcProgress, CalcValue, CellEdit, CellInput, CellRef, CellSink,
    FormulaShape, NameInfo, Rect, SheetCells, SheetRect, StructuralOp, TableInfo,
};
use serde::Serialize;

use crate::book::{read_book, Book, Content, FormulaKind, Value};
use crate::formula::{parse_range, referenced_cells, scan};

/// The plan's slice length.
const SLICE_MS: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    Formualizer,
}

pub fn engine_by_name(name: &str) -> Option<EngineKind> {
    match name {
        "formualizer" => Some(EngineKind::Formualizer),
        _ => None,
    }
}

fn new_engine(kind: EngineKind) -> Box<dyn CalcEngine> {
    match kind {
        EngineKind::Formualizer => Box::new(cgs_calc::formualizer::FormualizerEngine::new()),
    }
}

pub fn new_engine_by_name(name: &str) -> Option<Box<dyn CalcEngine>> {
    engine_by_name(name).map(new_engine)
}

/// Loads a workbook and calculates it fully, for the comparison with Excel.
pub fn load_for_structural(
    engine: &mut dyn CalcEngine,
    book: &Book,
) -> Result<(), cgs_calc::CalcError> {
    let mut score = Score::default();
    let (_, batches) = prepare(book, &mut score);
    engine.begin_load(&book_info(book))?;
    for (index, cells) in batches.iter().enumerate() {
        engine.load_batch(&SheetCells {
            sheet: index as u32,
            cells,
        })?;
    }
    engine.finish_load()?;
    engine.recalc_all()?;
    settle_engine(engine)
}

pub fn settle_engine(engine: &mut dyn CalcEngine) -> Result<(), cgs_calc::CalcError> {
    while engine.calc_slice(SLICE_MS, &[])? != CalcProgress::Done {}
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct RunOptions {
    pub edits: usize,
    pub top_row_insert: bool,
    /// Keep example cells for mismatches. Off for private workbooks.
    pub examples: bool,
    /// Load and read only, which is the cached-value test.
    pub skip_calc: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            edits: 100,
            top_row_insert: true,
            examples: true,
            skip_calc: false,
        }
    }
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub application: String,
    pub app_version: String,
    /// `fileVersion` says Excel, and the file doesn't ask to be recalculated on load.
    pub excel_saved: bool,
    pub manual_calc: bool,
    pub full_calc_on_load: bool,
    pub iterate: bool,
    pub date1904: bool,
    pub external_links: bool,
    pub sheets: usize,
    pub names: usize,
    pub tables: usize,
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FunctionScore {
    pub cells: u64,
    pub matched: u64,
    /// The engine returned `#NAME?` or a "not implemented" error, where Excel didn't.
    pub unsupported: u64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Mismatch {
    pub sheet: String,
    pub cell: String,
    pub formula: String,
    pub stored: String,
    pub got: String,
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub scored: u64,
    pub matched: u64,
    /// Formula cells loaded as their stored value: volatile, external or data table.
    pub seeded: u64,
    /// Formula cells with no stored value, which can't be scored.
    pub no_stored_value: u64,
    pub wrong_value: u64,
    pub wrong_type: u64,
    pub near_zero_only: u64,
    pub unsupported: u64,
    pub by_function: BTreeMap<String, FunctionScore>,
    pub examples: Vec<Mismatch>,
}

#[derive(Serialize, Default, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    pub count: usize,
    pub total_ms: f64,
    pub p50_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

impl Timings {
    pub fn of(mut samples: Vec<f64>) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        samples.sort_by(f64::total_cmp);
        // Nearest rank: the smallest sample with at least q of the samples at or below it.
        let at = |q: f64| samples[((samples.len() as f64 * q).ceil() as usize).saturating_sub(1)];
        Self {
            count: samples.len(),
            total_ms: samples.iter().sum(),
            p50_ms: at(0.5),
            p99_ms: at(0.99),
            max_ms: samples[samples.len() - 1],
        }
    }
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub ok: bool,
    pub error: Option<String>,
    pub provenance: Provenance,
    pub cells: usize,
    pub formulas: usize,
    pub parse_ms: f64,
    pub load_ms: f64,
    /// Formula cells whose loaded value differs from what was saved, before any
    /// calculation. Zero is what "loads cached values without calculating" needs.
    pub cached_differs: u64,
    pub cached_read_ms: f64,
    /// The first edit after a cached load, with nothing calculated before it: the
    /// time until it has settled, and the longest slice on the way.
    pub first_edit_ms: f64,
    pub first_edit_max_slice_ms: f64,
    pub first_calc_ms: f64,
    /// Durations of the `calc_slice` calls of the first calculation.
    pub first_calc_slices: Timings,
    pub score: Score,
    pub edit: Timings,
    /// Durations of the `calc_slice` calls after edits.
    pub edit_slices: Timings,
    /// Every slice, for pooling: first calculation and edits.
    pub slice_samples: Vec<f32>,
    pub top_row_insert_ms: f64,
    pub top_row_settle_ms: f64,
    /// Cells that a top-row insert should have kept but didn't.
    pub top_row_value_changes: u64,
    pub top_row_examples: Vec<String>,
    pub memory_after_load: u64,
    pub memory_after_calc: u64,
    pub memory_after_edits: u64,
}

#[cfg(target_arch = "wasm32")]
fn memory_bytes() -> u64 {
    core::arch::wasm32::memory_size(0) as u64 * 65536
}

#[cfg(not(target_arch = "wasm32"))]
fn memory_bytes() -> u64 {
    0
}

fn calc_value(value: &Value) -> CalcValue {
    match value {
        Value::Empty => CalcValue::Empty,
        Value::Number(number) => CalcValue::Number(*number),
        Value::Bool(flag) => CalcValue::Bool(*flag),
        Value::Text(text) => CalcValue::Text(text.clone()),
        Value::Error(text) => CalcValue::Error(text.clone()),
    }
}

fn rect_of(range: ((u32, u32), (u32, u32))) -> Rect {
    let ((top, left), (bottom, right)) = range;
    Rect {
        top,
        left,
        bottom,
        right,
    }
}

/// What the formula engine gets from a table: range and flags, column names.
fn book_info(book: &Book) -> BookInfo {
    BookInfo {
        sheets: book.sheets.iter().map(|sheet| sheet.name.clone()).collect(),
        names: book
            .names
            .iter()
            // Built-in names such as print areas never feed a formula, and
            // some are not valid for the engines.
            .filter(|name| !name.name.starts_with("_xlnm.") && !name.formula.contains("#REF!"))
            .map(|name| NameInfo {
                name: name.name.clone(),
                formula: name.formula.clone(),
                local_sheet: name.local_sheet,
            })
            .collect(),
        tables: book
            .tables
            .iter()
            .filter_map(|table| {
                Some(TableInfo {
                    name: table.display_name.clone(),
                    sheet: table.sheet,
                    range: rect_of(parse_range(&table.reference)?),
                    header_row: table.header_rows > 0,
                    totals_row: table.totals_rows > 0,
                    columns: table.columns.clone(),
                })
            })
            .collect(),
        hidden_rows: book
            .sheets
            .iter()
            .enumerate()
            .flat_map(|(index, sheet)| {
                sheet
                    .hidden_rows
                    .iter()
                    .map(move |&row| (index as u32, row))
            })
            .collect(),
        date1904: book.date1904,
    }
}

#[derive(Debug, Clone)]
enum Plan {
    Scored {
        stored: Value,
        functions: Vec<String>,
        text: String,
    },
    NotScored,
}

struct Loaded {
    /// Sheet index, row, column to what scoring must do with the cell.
    plans: Vec<HashMap<(u32, u32), Plan>>,
    constants: Vec<CellRef>,
    extents: Vec<(u32, u32)>,
    referenced: Vec<CellRef>,
}

type Batch = Vec<(u32, u32, CellInput)>;

/// Builds the load batches. Cells whose stored value can't be reproduced go in as
/// that value, so their dependents still see what Excel saw.
fn prepare(book: &Book, score: &mut Score) -> (Loaded, Vec<Batch>) {
    let mut plans = vec![HashMap::new(); book.sheets.len()];
    let mut batches = Vec::with_capacity(book.sheets.len());
    let mut constants = Vec::new();
    let mut extents = vec![(1u32, 1u32); book.sheets.len()];
    let mut referenced = Vec::new();
    for (index, sheet) in book.sheets.iter().enumerate() {
        let mut batch = Vec::with_capacity(sheet.cells.len() + sheet.spill_values.len());
        let extent = &mut extents[index];
        for cell in &sheet.cells {
            extent.0 = extent.0.max(cell.row);
            extent.1 = extent.1.max(cell.col);
            let input = match &cell.content {
                Content::Value(value) => {
                    if matches!(value, Value::Number(_)) {
                        constants.push(CellRef {
                            sheet: index as u32,
                            row: cell.row,
                            col: cell.col,
                        });
                    }
                    CellInput::Value(calc_value(value))
                }
                Content::Formula { text, kind, cached } => {
                    let found = scan(text);
                    let unscorable = found.external
                        || found.outside_data()
                        || found.non_deterministic()
                        || matches!(kind, FormulaKind::DataTable)
                        || text.is_empty();
                    if unscorable {
                        score.seeded += 1;
                        plans[index].insert((cell.row, cell.col), Plan::NotScored);
                        CellInput::Value(cached.as_ref().map_or(CalcValue::Empty, calc_value))
                    } else {
                        match cached {
                            Some(stored) => {
                                plans[index].insert(
                                    (cell.row, cell.col),
                                    Plan::Scored {
                                        stored: stored.clone(),
                                        functions: found.functions.clone(),
                                        text: text.clone(),
                                    },
                                );
                            }
                            None => {
                                score.no_stored_value += 1;
                                plans[index].insert((cell.row, cell.col), Plan::NotScored);
                            }
                        }
                        for (row, col) in referenced_cells(text).into_iter().take(4) {
                            referenced.push(CellRef {
                                sheet: index as u32,
                                row,
                                col,
                            });
                        }
                        let shape = match kind {
                            FormulaKind::Array(range) => FormulaShape::Array(rect_of(*range)),
                            FormulaKind::Dynamic(range) => FormulaShape::Dynamic(rect_of(*range)),
                            _ => FormulaShape::Normal,
                        };
                        if let FormulaKind::Array(range) | FormulaKind::Dynamic(range) = kind {
                            let area = rect_of(*range);
                            extent.0 = extent.0.max(area.bottom);
                            extent.1 = extent.1.max(area.right);
                            for (&(row, col), value) in &sheet.spill_values {
                                if area.contains(row, col) {
                                    plans[index].insert(
                                        (row, col),
                                        Plan::Scored {
                                            stored: value.clone(),
                                            functions: found.functions.clone(),
                                            text: text.clone(),
                                        },
                                    );
                                    batch.push((
                                        row,
                                        col,
                                        CellInput::Spill {
                                            anchor: (cell.row, cell.col),
                                            cached: calc_value(value),
                                        },
                                    ));
                                }
                            }
                        }
                        CellInput::Formula {
                            text: text.clone(),
                            shape,
                            cached: cached.as_ref().map(calc_value),
                        }
                    }
                }
            };
            batch.push((cell.row, cell.col, input));
        }
        batches.push(batch);
    }
    (
        Loaded {
            plans,
            constants,
            extents,
            referenced,
        },
        batches,
    )
}

#[derive(Default)]
struct Collect(HashMap<(u32, u32), CalcValue>);

impl CellSink for Collect {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.insert((row, col), value.clone());
    }
}

fn read_sheet(engine: &dyn CalcEngine, sheet: u32, (rows, cols): (u32, u32)) -> Collect {
    let mut out = Collect::default();
    engine.read_block(
        SheetRect {
            sheet,
            rect: Rect {
                top: 1,
                left: 1,
                bottom: rows,
                right: cols,
            },
        },
        &mut out,
    );
    out
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum Verdict {
    Match,
    WrongValue,
    NearZero,
    WrongType,
    Unsupported,
}

/// Excel's cell boundary turns an empty result into 0, or "" for text.
pub fn compare(stored: &Value, got: Option<&CalcValue>) -> Verdict {
    let got = got.unwrap_or(&CalcValue::Empty);
    if let CalcValue::Error(error) = got {
        let unsupported = matches!(error.as_str(), "#NAME?" | "#N/IMPL!" | "#ERROR!");
        if let Value::Error(expected) = stored {
            if expected.eq_ignore_ascii_case(error) {
                return Verdict::Match;
            }
        }
        return if unsupported {
            Verdict::Unsupported
        } else {
            Verdict::WrongType
        };
    }
    match (stored, got) {
        (Value::Empty, CalcValue::Empty) => Verdict::Match,
        (Value::Number(a), CalcValue::Number(b)) => {
            let difference = (a - b).abs();
            if difference <= 1e-9 * a.abs().max(b.abs()) {
                Verdict::Match
            } else if a.abs() < 1e-9 && b.abs() < 1e-9 {
                Verdict::NearZero
            } else {
                Verdict::WrongValue
            }
        }
        (Value::Number(a), CalcValue::Empty) if *a == 0.0 => Verdict::Match,
        (Value::Text(a), CalcValue::Empty) if a.is_empty() => Verdict::Match,
        (Value::Text(a), CalcValue::Text(b)) if a == b => Verdict::Match,
        (Value::Text(_), CalcValue::Text(_)) => Verdict::WrongValue,
        (Value::Bool(a), CalcValue::Bool(b)) if a == b => Verdict::Match,
        (Value::Bool(_), CalcValue::Bool(_)) => Verdict::WrongValue,
        _ => Verdict::WrongType,
    }
}

fn describe(value: &CalcValue) -> String {
    match value {
        CalcValue::Empty => "(empty)".into(),
        CalcValue::Number(number) => number.to_string(),
        CalcValue::Bool(flag) => flag.to_string(),
        CalcValue::Text(text) => format!("{text:?}"),
        CalcValue::Error(error) => error.clone(),
    }
}

fn score_all(
    book: &Book,
    engine: &dyn CalcEngine,
    loaded: &Loaded,
    score: &mut Score,
    examples: bool,
) {
    let read = |sheet: u32, extent: (u32, u32)| read_sheet(engine, sheet, extent).0;
    score_sheets(book, &read, loaded, score, examples);
}

fn score_sheets(
    book: &Book,
    read: &dyn Fn(u32, (u32, u32)) -> HashMap<(u32, u32), CalcValue>,
    loaded: &Loaded,
    score: &mut Score,
    examples: bool,
) {
    const NO_FUNCTION: &str = "(no function)";
    for (index, sheet) in book.sheets.iter().enumerate() {
        let plans = &loaded.plans[index];
        if plans.is_empty() {
            continue;
        }
        let got = read(index as u32, loaded.extents[index]);
        for (&(row, col), plan) in plans {
            let Plan::Scored {
                stored,
                functions,
                text,
            } = plan
            else {
                continue;
            };
            let verdict = compare(stored, got.get(&(row, col)));
            score.scored += 1;
            match verdict {
                Verdict::Match => score.matched += 1,
                Verdict::WrongValue => score.wrong_value += 1,
                Verdict::NearZero => score.near_zero_only += 1,
                Verdict::WrongType => score.wrong_type += 1,
                Verdict::Unsupported => score.unsupported += 1,
            }
            let matched = verdict == Verdict::Match;
            let mut tally = |name: &str| {
                let entry = score.by_function.entry(name.to_string()).or_default();
                entry.cells += 1;
                entry.matched += u64::from(matched);
                entry.unsupported += u64::from(verdict == Verdict::Unsupported);
            };
            if functions.is_empty() {
                tally(NO_FUNCTION);
            }
            for name in functions {
                tally(name);
            }
            if !matched && examples && score.examples.len() < 40 {
                let shown = got.get(&(row, col)).unwrap_or(&CalcValue::Empty);
                score.examples.push(Mismatch {
                    sheet: sheet.name.clone(),
                    cell: format!("{}{row}", crate::formula::col_to_letters(i64::from(col))),
                    formula: text.chars().take(200).collect(),
                    stored: describe(&calc_value(stored)),
                    got: describe(shown),
                });
            }
        }
    }
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }
}

fn settle(engine: &mut dyn CalcEngine, slices: &mut Vec<f64>) -> Result<f64, String> {
    let start = now_ms();
    loop {
        let slice_start = now_ms();
        let progress = engine.calc_slice(SLICE_MS, &[]).map_err(|e| e.0)?;
        slices.push(now_ms() - slice_start);
        if progress == CalcProgress::Done {
            break;
        }
    }
    Ok(now_ms() - start)
}

pub fn run_file(kind: EngineKind, bytes: &[u8], options: RunOptions) -> Report {
    let mut report = Report::default();
    if let Err(error) = run_inner(kind, bytes, options, &mut report) {
        report.error = Some(error);
    } else {
        report.ok = true;
    }
    report
}

fn run_inner(
    kind: EngineKind,
    bytes: &[u8],
    options: RunOptions,
    report: &mut Report,
) -> Result<(), String> {
    let parse_start = now_ms();
    let book = read_book(bytes)?;
    report.parse_ms = now_ms() - parse_start;
    report.provenance = Provenance {
        application: book.application.clone(),
        app_version: book.app_version.clone(),
        excel_saved: book.file_app == "xl" && !book.rup_build.is_empty(),
        manual_calc: book.calc_mode == "manual",
        full_calc_on_load: book.full_calc_on_load,
        iterate: book.iterate,
        date1904: book.date1904,
        external_links: book.has_external_links,
        sheets: book.sheets.len(),
        names: book.names.len(),
        tables: book.tables.len(),
    };
    report.cells = book.sheets.iter().map(|sheet| sheet.cells.len()).sum();
    report.formulas = book.formula_cells();

    let mut score = Score::default();
    let (loaded, batches) = prepare(&book, &mut score);
    let mut engine = new_engine(kind);
    let info = book_info(&book);

    let load_start = now_ms();
    engine.begin_load(&info).map_err(|e| e.0)?;
    for (index, cells) in batches.iter().enumerate() {
        engine
            .load_batch(&SheetCells {
                sheet: index as u32,
                cells,
            })
            .map_err(|e| e.0)?;
    }
    engine.finish_load().map_err(|e| e.0)?;
    report.load_ms = now_ms() - load_start;
    report.memory_after_load = memory_bytes();
    drop(batches);

    // Nothing has calculated yet, so reads must give the saved values.
    let read_start = now_ms();
    for (index, plans) in loaded.plans.iter().enumerate() {
        if plans.is_empty() {
            continue;
        }
        let got = read_sheet(engine.as_ref(), index as u32, loaded.extents[index]);
        for (&at, plan) in plans {
            if let Plan::Scored { stored, .. } = plan {
                if compare(stored, got.0.get(&at)) != Verdict::Match {
                    report.cached_differs += 1;
                }
            }
        }
    }
    report.cached_read_ms = now_ms() - read_start;
    if options.skip_calc {
        report.score = score;
        return Ok(());
    }

    // Prefer constants that formulas read, so that an edit has something to do.
    let mut candidates: Vec<CellRef> = Vec::new();
    if !loaded.constants.is_empty() {
        let constants: std::collections::HashSet<CellRef> =
            loaded.constants.iter().copied().collect();
        candidates = loaded
            .referenced
            .iter()
            .copied()
            .filter(|cell| constants.contains(cell))
            .collect();
        candidates.sort_unstable();
        candidates.dedup();
        if candidates.is_empty() {
            candidates.clone_from(&loaded.constants);
        }
    }
    let mut random = Random(0x9E37_79B9_7F4A_7C15 ^ bytes.len() as u64);
    if let Some(&at) = candidates.get(random.below(candidates.len().max(1))) {
        let start = now_ms();
        let undo = engine
            .set_cells(&[CellEdit {
                at,
                input: CellInput::Value(CalcValue::Number(12345.0)),
            }])
            .map_err(|e| e.0)?;
        let mut slices = Vec::new();
        settle(engine.as_mut(), &mut slices)?;
        report.first_edit_ms = now_ms() - start;
        report.first_edit_max_slice_ms = slices.iter().copied().fold(0.0, f64::max);
        // Put the value back, so that scoring below compares like with like.
        engine.set_cells(&undo).map_err(|e| e.0)?;
        engine.take_changed();
    }

    engine.recalc_all().map_err(|e| e.0)?;
    let mut first_slices = Vec::new();
    report.first_calc_ms = settle(engine.as_mut(), &mut first_slices)?;
    report.memory_after_calc = memory_bytes();
    report.first_calc_slices = Timings::of(first_slices.clone());
    score_all(
        &book,
        engine.as_ref(),
        &loaded,
        &mut score,
        options.examples,
    );
    report.score = score;
    engine.take_changed();

    let mut slices = first_slices;
    let mut edit_slices = Vec::new();
    let mut edits = Vec::new();
    if !candidates.is_empty() {
        for _ in 0..options.edits {
            let at = candidates[random.below(candidates.len())];
            let value = (random.next() % 100_000) as f64 / 8.0;
            let start = now_ms();
            engine
                .set_cells(&[CellEdit {
                    at,
                    input: CellInput::Value(CalcValue::Number(value)),
                }])
                .map_err(|e| e.0)?;
            let mut this_edit = Vec::new();
            settle(engine.as_mut(), &mut this_edit)?;
            edits.push(now_ms() - start);
            edit_slices.extend_from_slice(&this_edit);
        }
        engine.take_changed();
    }
    slices.extend_from_slice(&edit_slices);
    report.edit = Timings::of(edits);
    report.edit_slices = Timings::of(edit_slices);
    report.memory_after_edits = memory_bytes();
    report.slice_samples = slices.iter().map(|&ms| ms as f32).collect();

    if options.top_row_insert {
        let biggest = book
            .sheets
            .iter()
            .enumerate()
            .max_by_key(|(_, sheet)| sheet.cells.len())
            .map_or(0, |(index, _)| index as u32);
        if !book.sheets.is_empty() {
            let extent = loaded.extents[biggest as usize];
            let before = read_sheet(engine.as_ref(), biggest, extent);
            let start = now_ms();
            engine
                .structural(&StructuralOp::InsertRows {
                    sheet: biggest,
                    at: 1,
                    count: 1,
                })
                .map_err(|e| e.0)?;
            report.top_row_insert_ms = now_ms() - start;
            let mut settle_slices = Vec::new();
            report.top_row_settle_ms = settle(engine.as_mut(), &mut settle_slices)?;
            let after = read_sheet(engine.as_ref(), biggest, (extent.0 + 1, extent.1));
            for (&(row, col), value) in &before.0 {
                let moved = after.0.get(&(row + 1, col));
                let same = match (value, moved) {
                    (CalcValue::Number(a), Some(CalcValue::Number(b))) => {
                        (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
                    }
                    (a, Some(b)) => a == b,
                    (_, None) => false,
                };
                if !same {
                    report.top_row_value_changes += 1;
                    if options.examples && report.top_row_examples.len() < 5 {
                        report.top_row_examples.push(format!(
                            "{}{row}: {} -> {}",
                            crate::formula::col_to_letters(i64::from(col)),
                            describe(value),
                            moved.map_or("(gone)".to_string(), describe),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_applies_the_plans_tolerance() {
        let number = |n| Some(CalcValue::Number(n));
        assert_eq!(
            compare(&Value::Number(1.0), number(1.0 + 1e-10).as_ref()),
            Verdict::Match
        );
        assert_eq!(
            compare(&Value::Number(1.0), number(1.0 + 1e-8).as_ref()),
            Verdict::WrongValue
        );
        assert_eq!(compare(&Value::Number(0.0), None), Verdict::Match);
        assert_eq!(
            compare(&Value::Number(1e-12), number(0.0).as_ref()),
            Verdict::NearZero
        );
        assert_eq!(compare(&Value::Text(String::new()), None), Verdict::Match);
        assert_eq!(
            compare(
                &Value::Number(1.0),
                Some(&CalcValue::Error("#NAME?".into()))
            ),
            Verdict::Unsupported
        );
        assert_eq!(
            compare(
                &Value::Error("#N/A".into()),
                Some(&CalcValue::Error("#N/A".into()))
            ),
            Verdict::Match
        );
        assert_eq!(
            compare(&Value::Bool(true), number(1.0).as_ref()),
            Verdict::WrongType
        );
    }

    #[test]
    fn timings_use_nearest_rank() {
        let timings = Timings::of((1..=100).map(f64::from).collect());
        assert_eq!(timings.p99_ms, 99.0);
        assert_eq!(timings.max_ms, 100.0);
        assert_eq!(timings.count, 100);
    }
}
