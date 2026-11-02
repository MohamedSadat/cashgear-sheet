//! Everything about a workbook except its cells: styles, dimensions, merges, tables,
//! drawings and which style each cell uses. Commands and undo come in Phase 4. No I/O.

pub mod color;
pub mod sheet;
pub mod style;

pub use color::{Color, ColorKind, ColorSpace, DarkColors, Rgb, ThemePalette};
pub use sheet::{
    Anchor, AnchorPoint, ColAxis, Drawing, DrawingKind, Freeze, LineInfo, RowAxis, SheetInfo,
    SheetKind, SheetState, SheetView,
};
pub use style::{
    Alignment, Border, Fill, Font, HAlign, Line, LineStyle, Pattern, ReadingOrder, StyleTable,
    Underline, VAlign, Xf,
};

/// Excel's row limit.
pub const MAX_ROWS: u32 = 1_048_576;
/// Excel's column limit.
pub const MAX_COLS: u32 = 16_384;
/// Rows in one grid tile.
pub const TILE_ROWS: u32 = 64;
/// Columns in one grid tile.
pub const TILE_COLS: u32 = 16;

/// 1-based and inclusive on all sides.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedName {
    pub name: String,
    pub formula: String,
    pub local_sheet: Option<u32>,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub sheet: u32,
    pub range: Rect,
    pub header_rows: u32,
    pub totals_rows: u32,
    pub columns: Vec<String>,
}

/// A workbook as `cgs-xlsx` reads it.
#[derive(Debug, Clone, Default)]
pub struct Book {
    pub sheets: Vec<SheetInfo>,
    pub styles: StyleTable,
    pub names: Vec<DefinedName>,
    pub tables: Vec<Table>,
    pub date1904: bool,
    /// The sheet showing when the file was saved.
    pub active_sheet: u32,
    /// The file asks for a full calculation when it opens. Phase 2 shows saved values.
    pub full_calc_on_load: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_divides_into_whole_tiles() {
        assert_eq!(MAX_ROWS % TILE_ROWS, 0);
        assert_eq!(MAX_COLS % TILE_COLS, 0);
    }
}
