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

/// What undoing a structural edit needs: the inverse operation, plus the cells a
/// delete removed. Formulas that turned to `#REF!` elsewhere are restored by Phase 4's
/// command stack, which holds their pre-images.
#[derive(Debug, Clone, PartialEq)]
pub struct StructuralUndo {
    pub inverse: StructuralOp,
    pub removed: Vec<CellEdit>,
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
    fn load_batch(&mut self, cells: &SheetCells<'_>) -> Result<()>;
    /// Ends the load. Nothing is calculated, and cached values are readable after it.
    fn finish_load(&mut self) -> Result<()>;
    /// Applies edits and returns the pre-images, for undo.
    fn set_cells(&mut self, edits: &[CellEdit]) -> Result<Vec<CellEdit>>;
    fn structural(&mut self, op: &StructuralOp) -> Result<StructuralUndo>;
    /// Marks every formula for recalculation, as Ctrl+Alt+F9 does in Excel.
    fn recalc_all(&mut self) -> Result<()>;
    /// Calculates for about `ms` milliseconds, cells in `first` before others. A single
    /// engine call can't be interrupted, so a slice can run over; the bake-off measures by how much.
    fn calc_slice(&mut self, ms: u32, first: &[SheetRect]) -> Result<CalcProgress>;
    fn take_changed(&mut self) -> Vec<SheetRect>;
    fn read_block(&self, r: SheetRect, out: &mut dyn CellSink);
    fn formula_text(&self, at: CellRef) -> Option<String>;
}
