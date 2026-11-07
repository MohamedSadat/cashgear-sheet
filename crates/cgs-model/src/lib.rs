//! Everything about a workbook except its cells: styles, dimensions, merges, tables,
//! drawings and which style each cell uses. No I/O.

pub mod cf;
pub mod chart;
pub mod color;
pub mod dv;
pub mod dxf;
pub mod filter;
pub mod notes;
pub mod page;
pub mod pivot;
pub mod sheet;
pub mod style;
pub mod table;

pub use cf::{CfBlock, CfKind, CfOperator, CfRule, CfValueKind, Cfvo, DataBar, IconSet};
pub use chart::{Chart, ChartBody, ChartPart};
pub use color::{Color, ColorKind, ColorSpace, DarkColors, Rgb, ThemePalette};
pub use dv::{DvKind, DvOp, DvStyle, Validation};
pub use dxf::{Dxf, DxfBorder, DxfFill, DxfFont, TableElement, TableStyleDef, TableStyleElement};
pub use filter::{
    AutoFilter, DateGroup, FilterColumn, FilterCriteria, FilterOp, SortBy, SortKey, SortState,
};
pub use notes::{Hyperlink, Note};
pub use page::{
    HeaderFooter, HfFields, HfPicture, HfRun, HfSections, HfStyle, HfText, Margins, Orientation,
    PageBreak, PageOrder, PageSetup, PrintComments, PrintErrors,
};
pub use sheet::{
    Anchor, AnchorPoint, ColAxis, ColumnRuns, Drawing, DrawingKind, Freeze, LineInfo, RowAxis,
    SheetInfo, SheetKind, SheetShape, SheetState, SheetView, NO_CELL,
};
pub use style::{
    Alignment, Border, Fill, Font, HAlign, Hatch, Line, LineStyle, Pattern, ReadingOrder, RichRun,
    RunFont, StyleTable, Underline, VAlign, Xf,
};
pub use table::{Table, TableColumnInfo, TableStyle, TotalsFunction};

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
    /// The file asks for a full calculation when it opens.
    pub full_calc_on_load: bool,
    /// The file is set to calculate only when asked.
    pub manual_calc: bool,
    /// Some formula has no saved value, which only files from other programs do.
    pub formulas_without_values: bool,
    /// Sheets by index in tab order, once that differs from their indexes. See `tab_order`.
    pub order: Vec<u32>,
    /// Which sheet each `<sheet>` element of the file is, once that differs from their indexes.
    pub file_sheets: Vec<u32>,
    /// Shared strings with more than one look, by their index in the file.
    pub rich_strings: std::collections::HashMap<u32, Vec<RichRun>>,
    /// Chart parts, which drawings point at by index.
    pub charts: Vec<ChartPart>,
    /// Pivot caches, which pivot tables point at by index.
    pub pivot_caches: Vec<pivot::PivotCache>,
    pub pivot_tables: Vec<pivot::PivotTable>,
}

impl Book {
    /// Sheets by index, in the order their tabs show, deleted ones left out.
    pub fn tab_order(&self) -> Vec<u32> {
        let all: Vec<u32> = if self.order.is_empty() {
            (0..self.sheets.len() as u32).collect()
        } else {
            self.order.clone()
        };
        all.into_iter()
            .filter(|&sheet| !self.sheets[sheet as usize].deleted)
            .collect()
    }

    /// Which sheet each `<sheet>` element of the file holds.
    pub fn sheets_in_file(&self) -> Vec<u32> {
        if self.file_sheets.is_empty() {
            (0..self
                .sheets
                .iter()
                .filter(|sheet| sheet.part.is_some())
                .count() as u32)
                .collect()
        } else {
            self.file_sheets.clone()
        }
    }
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
