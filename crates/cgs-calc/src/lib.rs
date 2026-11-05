//! The `CalcEngine` trait and the Formualizer adapter. The engine is the only cell
//! store; `cgs-model` holds everything else.

pub mod formualizer;

mod clock;

pub use clock::now_ms;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalcError(pub String);

impl std::fmt::Display for CalcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CalcError {}

pub type Result<T> = std::result::Result<T, CalcError>;

pub(crate) fn err(error: impl std::fmt::Display) -> CalcError {
    CalcError(error.to_string())
}

/// Whether a sheet of this many cells over this area should go in cell by cell.
pub fn wants_sparse(cells: u64, rows: u32, cols: u32) -> bool {
    let area = u64::from(rows) * u64::from(cols);
    area > 4_000_000 && area > cells.saturating_mul(32)
}

/// A cell's value. Errors keep Excel's spelling, such as `#N/A`.
#[derive(Debug, Clone, PartialEq)]
pub enum CalcValue {
    Empty,
    Number(f64),
    Bool(bool),
    Text(String),
    Error(String),
}

/// Rows and columns are 1-based, as in the file format and in both engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellRef {
    pub sheet: u32,
    pub row: u32,
    pub col: u32,
}

/// Inclusive on all sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub top: u32,
    pub left: u32,
    pub bottom: u32,
    pub right: u32,
}

impl Rect {
    pub fn contains(&self, row: u32, col: u32) -> bool {
        row >= self.top && row <= self.bottom && col >= self.left && col <= self.right
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetRect {
    pub sheet: u32,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaShape {
    Normal,
    /// A legacy `{=...}` array over this range, anchored at its top left.
    Array(Rect),
    /// A dynamic array whose last spill covered this range.
    Dynamic(Rect),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CellInput {
    Value(CalcValue),
    Formula {
        /// As stored in the file: no leading `=`, English names, comma separators.
        text: String,
        shape: FormulaShape,
        /// The value last saved with the formula, shown until it recalculates.
        cached: Option<CalcValue>,
    },
    /// A cell produced by the array anchored at `anchor`, with its saved value.
    Spill {
        anchor: (u32, u32),
        cached: CalcValue,
    },
    Clear,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CellEdit {
    pub at: CellRef,
    pub input: CellInput,
}

/// Cells of one sheet, in any order. A sheet can arrive in several batches.
#[derive(Debug, Clone, Copy)]
pub struct SheetCells<'a> {
    pub sheet: u32,
    pub cells: &'a [(u32, u32, CellInput)],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameInfo {
    pub name: String,
    pub formula: String,
    pub local_sheet: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableInfo {
    pub name: String,
    pub sheet: u32,
    pub range: Rect,
    pub header_row: bool,
    pub totals_row: bool,
    pub columns: Vec<String>,
}

/// A value as the file reader hands it over, borrowing its text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoadValue<'a> {
    Number(f64),
    Bool(bool),
    Text(&'a str),
    /// As stored, such as `#N/A`.
    Error(&'a str),
}

/// A constant, or the value last saved with a formula.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadEntry<'a> {
    pub row: u32,
    pub col: u32,
    pub value: LoadValue<'a>,
}

/// The bounds a sheet's cells stay within, known before they arrive. Going past them
/// means calling `begin_sheet` again with larger ones and sending the sheet again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetShape {
    pub rows: u32,
    pub cols: u32,
    /// Few cells over a large area: stored cell by cell rather than in full columns.
    pub sparse: bool,
}

/// What the engine must know before cells arrive.
#[derive(Debug, Clone, Default)]
pub struct BookInfo {
    pub sheets: Vec<String>,
    pub names: Vec<NameInfo>,
    pub tables: Vec<TableInfo>,
    /// (sheet, row) of hidden rows, which SUBTOTAL(101..111) skips.
    pub hidden_rows: Vec<(u32, u32)>,
    pub date1904: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralOp {
    InsertRows { sheet: u32, at: u32, count: u32 },
    DeleteRows { sheet: u32, at: u32, count: u32 },
    InsertCols { sheet: u32, at: u32, count: u32 },
    DeleteCols { sheet: u32, at: u32, count: u32 },
}

impl StructuralOp {
    pub fn new(sheet: u32, edit: cgs_refs::AxisEdit) -> Self {
        let (at, count) = (edit.at, edit.count);
        match (edit.axis, edit.delete) {
            (cgs_refs::Axis::Row, false) => Self::InsertRows { sheet, at, count },
            (cgs_refs::Axis::Row, true) => Self::DeleteRows { sheet, at, count },
            (cgs_refs::Axis::Col, false) => Self::InsertCols { sheet, at, count },
            (cgs_refs::Axis::Col, true) => Self::DeleteCols { sheet, at, count },
        }
    }

    pub fn sheet(&self) -> u32 {
        match *self {
            Self::InsertRows { sheet, .. }
            | Self::DeleteRows { sheet, .. }
            | Self::InsertCols { sheet, .. }
            | Self::DeleteCols { sheet, .. } => sheet,
        }
    }

    pub fn edit(&self) -> cgs_refs::AxisEdit {
        use cgs_refs::{Axis, AxisEdit};
        match *self {
            Self::InsertRows { at, count, .. } => AxisEdit::insert(Axis::Row, at, count),
            Self::DeleteRows { at, count, .. } => AxisEdit::delete(Axis::Row, at, count),
            Self::InsertCols { at, count, .. } => AxisEdit::insert(Axis::Col, at, count),
            Self::DeleteCols { at, count, .. } => AxisEdit::delete(Axis::Col, at, count),
        }
    }
}

/// What undoing a structural edit needs: the inverse operation, then these cells as they were,
/// in the positions they had. A delete removes cells and shrinks ranges and turns references to
/// `#REF!`, and only these pre-images bring that back. Formulas that only moved come back through
/// the inverse.
#[derive(Debug, Clone, PartialEq)]
pub struct StructuralUndo {
    pub inverse: StructuralOp,
    pub removed: Vec<CellEdit>,
    /// Defined names as they were.
    pub names: Vec<NameInfo>,
    /// Tables as they were.
    pub tables: Vec<TableInfo>,
    /// Header cells of columns an insert added inside a table, and the names they got.
    pub new_headers: Vec<(CellRef, String)>,
    /// Tables this edit left nothing of.
    pub gone_tables: Vec<String>,
    /// Columns this edit took from tables that stay.
    pub gone_columns: Vec<(String, Vec<String>)>,
}

/// What putting a sheet back needs, in the order to do it.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetUndo {
    /// The sheet's own cells as they were.
    pub cells: Vec<CellEdit>,
    /// Formulas elsewhere that read it, and defined names, as they were.
    pub readers: Vec<CellEdit>,
    pub names: Vec<NameInfo>,
    /// Tables as they were.
    pub tables: Vec<TableInfo>,
    /// The tables that went with it.
    pub gone_tables: Vec<String>,
}

impl StructuralUndo {
    /// Roughly what this holds, for the undo stack's limit.
    pub fn bytes(&self) -> usize {
        edits_bytes(&self.removed) + self.names.iter().map(name_bytes).sum::<usize>()
    }
}

impl SheetUndo {
    pub fn bytes(&self) -> usize {
        edits_bytes(&self.cells)
            + edits_bytes(&self.readers)
            + self.names.iter().map(name_bytes).sum::<usize>()
    }
}

fn name_bytes(name: &NameInfo) -> usize {
    64 + name.name.len() + name.formula.len()
}

/// Roughly what a list of cell edits holds.
pub fn edits_bytes(edits: &[CellEdit]) -> usize {
    edits
        .iter()
        .map(|edit| {
            let text = |value: &CalcValue| match value {
                CalcValue::Text(text) | CalcValue::Error(text) => text.len(),
                _ => 0,
            };
            64 + match &edit.input {
                CellInput::Value(value) => text(value),
                CellInput::Formula {
                    text: formula,
                    cached,
                    ..
                } => formula.len() + cached.as_ref().map_or(0, text),
                CellInput::Spill { cached, .. } => text(cached),
                CellInput::Clear => 0,
            }
        })
        .sum()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalcProgress {
    /// No formula is waiting for calculation.
    Done,
    /// Work remains; `remaining` is an estimate in formula cells, if known.
    More { remaining: Option<usize> },
}

pub trait CellSink {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue);
}

pub trait CalcEngine {
    /// Starts a workbook. Cached values stay as they are until something dirties them.
    fn begin_load(&mut self, book: &BookInfo) -> Result<()>;
    /// Cells in any order, held until `finish_load`. The file reader uses `load_rows`
    /// instead, which holds nothing back; tests and the bench use this.
    fn load_batch(&mut self, cells: &SheetCells<'_>) -> Result<()>;
    /// Ends the load. Nothing is calculated, and cached values are readable after it.
    fn finish_load(&mut self) -> Result<()>;
    /// Starts one sheet, or starts it over.
    fn begin_sheet(&mut self, sheet: u32, shape: SheetShape) -> Result<()>;
    /// Constants and saved formula values in ascending row order, each batch ending
    /// with a complete row.
    fn load_rows(&mut self, sheet: u32, cells: &[LoadEntry<'_>]) -> Result<()>;
    /// Stages a formula for a cell whose saved value `load_rows` has or will have.
    fn load_formula(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        text: &str,
        shape: FormulaShape,
    ) -> Result<()>;
    /// Marks a cell as showing a saved spill rather than holding a constant.
    fn mark_spill(&mut self, sheet: u32, row: u32, col: u32) -> Result<()>;
    /// Makes the sheet's cells readable. `hidden_rows` are 1-based.
    fn finish_sheet(&mut self, sheet: u32, hidden_rows: &[u32]) -> Result<()>;
    /// Does one piece of the work that would otherwise fall on the first edit, and
    /// calculates nothing. Returns whether more remains. One piece can't be interrupted.
    fn prepare_step(&mut self) -> Result<bool>;
    /// Applies edits and returns the pre-images, for undo.
    fn set_cells(&mut self, edits: &[CellEdit]) -> Result<Vec<CellEdit>>;
    fn structural(&mut self, op: &StructuralOp) -> Result<StructuralUndo>;
    /// Marks every formula for recalculation, as Ctrl+Alt+F9 does in Excel.
    fn recalc_all(&mut self) -> Result<()>;
    /// Calculates for about `ms` milliseconds, cells in `first` before others. A single
    /// engine call can't be interrupted, so a slice can run over; the bake-off measures by how much.
    fn calc_slice(&mut self, ms: u32, first: &[SheetRect]) -> Result<CalcProgress>;
    /// Calculates everything that is due, in one go, and returns the cells whose values changed,
    /// along with any not yet taken. A single call can't be interrupted.
    fn calc_all_changes(&mut self) -> Result<Vec<CellRef>>;
    /// The cells whose values calculation has changed since the last call: exactly those, spills
    /// included. A formula that came out the same can be listed too.
    fn take_changed(&mut self) -> Vec<CellRef>;
    /// Adds a sheet, with the next index.
    fn add_sheet(&mut self, name: &str) -> Result<u32>;
    /// Removes a sheet, leaving `#REF!` in what read it. Its index stays taken.
    fn delete_sheet(&mut self, sheet: u32) -> Result<SheetUndo>;
    /// Brings back a sheet `delete_sheet` removed, with no cells yet.
    fn restore_sheet(&mut self, sheet: u32) -> Result<()>;
    /// Renames a sheet. What reads it follows. Returns the old name.
    fn rename_sheet(&mut self, sheet: u32, name: &str) -> Result<String>;
    /// Puts defined names back as they were.
    fn restore_names(&mut self, names: &[NameInfo]) -> Result<()>;
    /// Puts tables back as they were, making any that have gone.
    fn restore_tables(&mut self, tables: &[TableInfo]) -> Result<()>;
    /// Tables as they stand now.
    fn tables(&self) -> &[TableInfo];
    /// What a cell holds now, in a form `set_cells` can put back.
    fn cell_input(&self, at: CellRef) -> CellInput;
    /// Every array that has spilled: its anchor, and the area it covers, anchor included.
    fn spill_areas(&self) -> Vec<SheetRect>;
    /// A formula's array shape, if it was entered or stored as an array.
    fn formula_shape(&self, at: CellRef) -> FormulaShape;
    /// Whether anything is waiting to be calculated.
    fn is_calculating(&self) -> bool;
    /// Every formula with its text, as `formula_text` gives it.
    fn all_formulas(&mut self) -> Result<Vec<(CellRef, String)>>;
    /// Reads a typed formula, with no leading `=`. Excel closes brackets someone left open, so this
    /// gives back the text to use, or says what is wrong with it.
    fn check_formula(&self, text: &str) -> Result<String>;
    /// After opening, works out what is due without any edit: volatile functions.
    fn start_volatile(&mut self) -> Result<()>;
    fn read_block(&self, r: SheetRect, out: &mut dyn CellSink);
    fn formula_text(&self, at: CellRef) -> Option<String>;
}
