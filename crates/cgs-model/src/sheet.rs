//! A sheet apart from its values: sizes, merges, panes, drawings, and which style each
//! cell uses. The formula engine holds the values.

use std::collections::HashMap;

use crate::{Rect, TILE_COLS, TILE_ROWS};

/// No cell here: the row's or the column's style applies.
const NO_CELL: u32 = u32::MAX;

/// Which style each cell of a column uses, as runs. A column of one style costs one run.
#[derive(Debug, Clone, Default)]
struct ColumnRuns {
    /// (first row, style), ascending by row. A run lasts until the next one.
    runs: Vec<(u32, u32)>,
    /// One past the last row written while loading.
    end: u32,
}

impl ColumnRuns {
    fn push(&mut self, row: u32, style: u32) {
        if row < self.end {
            self.set(row, style);
            return;
        }
        if row > self.end {
            if self.runs.last().is_some_and(|&(_, s)| s != NO_CELL) {
                self.runs.push((self.end, NO_CELL));
            }
            self.runs.push((row, style));
        } else if self.runs.last().is_none_or(|&(_, s)| s != style) {
            self.runs.push((row, style));
        }
        self.end = row + 1;
    }

    /// A write out of order, which only an unusual file makes.
    fn set(&mut self, row: u32, style: u32) {
        let at = self.runs.partition_point(|&(start, _)| start <= row);
        let (start, old) = if at == 0 {
            (0, NO_CELL)
        } else {
            self.runs[at - 1]
        };
        if old == style {
            return;
        }
        let next_row_is_run = self.runs.get(at).is_some_and(|&(next, _)| next == row + 1);
        let mut insert = at;
        if start == row && at > 0 {
            self.runs[at - 1].1 = style;
        } else {
            self.runs.insert(insert, (row, style));
            insert += 1;
        }
        if !next_row_is_run {
            self.runs.insert(insert, (row + 1, old));
        }
    }

    fn get(&self, row: u32) -> u32 {
        if row >= self.end {
            return NO_CELL;
        }
        let at = self.runs.partition_point(|&(start, _)| start <= row);
        if at == 0 {
            NO_CELL
        } else {
            self.runs[at - 1].1
        }
    }
}

/// A row's or a column's size and formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineInfo {
    /// At 100%, in pixels. `None` means the sheet's default.
    pub size: Option<u32>,
    pub hidden: bool,
    /// The style cells without their own use, if the line has one.
    pub style: Option<u32>,
}

/// Rows: one entry per row that differs from the default, sorted.
#[derive(Debug, Clone, Default)]
pub struct RowAxis {
    pub default_px: u32,
    pub rows: Vec<(u32, LineInfo)>,
}

impl RowAxis {
    pub fn info(&self, row: u32) -> Option<&LineInfo> {
        let at = self.rows.binary_search_by_key(&row, |&(r, _)| r).ok()?;
        Some(&self.rows[at].1)
    }

    pub fn px(&self, row: u32) -> u32 {
        match self.info(row) {
            Some(info) if info.hidden => 0,
            Some(info) => info.size.unwrap_or(self.default_px),
            None => self.default_px,
        }
    }
}

/// Columns: ranges that differ from the default, sorted and not overlapping.
#[derive(Debug, Clone, Default)]
pub struct ColAxis {
    pub default_px: u32,
    pub ranges: Vec<(u32, u32, LineInfo)>,
}

impl ColAxis {
    pub fn info(&self, col: u32) -> Option<&LineInfo> {
        let at = self.ranges.partition_point(|&(first, _, _)| first <= col);
        let (_, last, info) = self.ranges.get(at.checked_sub(1)?)?;
        (col <= *last).then_some(info)
    }

    pub fn px(&self, col: u32) -> u32 {
        match self.info(col) {
            Some(info) if info.hidden => 0,
            Some(info) => info.size.unwrap_or(self.default_px),
            None => self.default_px,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SheetState {
    #[default]
    Visible,
    Hidden,
    VeryHidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SheetKind {
    #[default]
    Worksheet,
    /// A chart with no cells; shown as a placeholder until Phase 7.
    Chart,
    /// An Excel 4 macro sheet or a dialog sheet, which keeps its index and shows nothing.
    Other,
}

/// Rows and columns frozen at the top left, counted from the sheet's first visible ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Freeze {
    pub rows: u32,
    pub cols: u32,
    /// The first row and column of the frozen area, which is scrolled away when the file
    /// was saved scrolled, 1-based.
    pub top: u32,
    pub left: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawingKind {
    Chart,
    Picture,
    Shape,
    Group,
    Other,
}

/// A point on the grid: a cell, plus an offset into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnchorPoint {
    /// 1-based.
    pub row: u32,
    pub col: u32,
    /// In EMU.
    pub row_off: i64,
    pub col_off: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// Follows both corners.
    TwoCell { from: AnchorPoint, to: AnchorPoint },
    /// Follows one corner, at a fixed size in EMU.
    OneCell {
        from: AnchorPoint,
        width: i64,
        height: i64,
    },
    /// At a fixed place on the sheet, in EMU.
    Absolute {
        x: i64,
        y: i64,
        width: i64,
        height: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawing {
    pub kind: DrawingKind,
    pub name: String,
    pub anchor: Anchor,
}

/// A sheet's view when it was saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetView {
    pub zoom: u16,
    pub show_grid: bool,
    pub show_headers: bool,
    pub right_to_left: bool,
    pub freeze: Option<Freeze>,
    /// 1-based, the first visible cell of the scrolling pane.
    pub top_left: (u32, u32),
    /// 1-based.
    pub active_cell: (u32, u32),
}

impl Default for SheetView {
    fn default() -> Self {
        Self {
            zoom: 100,
            show_grid: true,
            show_headers: true,
            right_to_left: false,
            freeze: None,
            top_left: (1, 1),
            active_cell: (1, 1),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SheetInfo {
    pub name: String,
    pub state: SheetState,
    pub kind: SheetKind,
    /// The worksheet's part in the package, if it has one.
    pub part: Option<String>,
    pub rows: RowAxis,
    pub cols: ColAxis,
    pub view: SheetView,
    pub merges: Vec<Rect>,
    /// Which merges touch each tile, by the tile's top-left cell.
    merge_index: HashMap<(u32, u32), Vec<u32>>,
    pub drawings: Vec<Drawing>,
    /// The cells that hold anything, styled blanks included: (rows, columns).
    pub used: (u32, u32),
    /// All of its cells are in the formula engine.
    pub loaded: bool,
    styles: Vec<ColumnRuns>,
}

impl SheetInfo {
    pub fn new(name: String, state: SheetState, kind: SheetKind, part: Option<String>) -> Self {
        Self {
            name,
            state,
            kind,
            part,
            ..Self::default()
        }
    }

    /// Forgets what reading the worksheet part found, so it can be read again.
    pub fn reset_cells(&mut self) {
        self.rows.rows.clear();
        self.cols.ranges.clear();
        self.merges.clear();
        self.merge_index.clear();
        self.styles.clear();
        self.drawings.clear();
        self.used = (0, 0);
        self.loaded = false;
    }

    /// Records a cell's style while loading. Rows mostly arrive in order.
    pub fn set_style(&mut self, row: u32, col: u32, style: u32) {
        let slot = (col - 1) as usize;
        if slot >= self.styles.len() {
            self.styles.resize_with(slot + 1, ColumnRuns::default);
        }
        self.styles[slot].push(row, style);
    }

    /// The style a cell draws with: its own, or else its row's, or else its column's.
    pub fn style_at(&self, row: u32, col: u32) -> u32 {
        let own = self
            .styles
            .get((col - 1) as usize)
            .map_or(NO_CELL, |runs| runs.get(row));
        if own != NO_CELL {
            return own;
        }
        self.rows
            .info(row)
            .and_then(|info| info.style)
            .or_else(|| self.cols.info(col).and_then(|info| info.style))
            .unwrap_or(0)
    }

    /// Call when `merges` is complete.
    pub fn index_merges(&mut self) {
        self.merge_index.clear();
        for (at, merge) in self.merges.iter().enumerate() {
            let mut row = (merge.top - 1) / TILE_ROWS;
            while row <= (merge.bottom - 1) / TILE_ROWS {
                let mut col = (merge.left - 1) / TILE_COLS;
                while col <= (merge.right - 1) / TILE_COLS {
                    self.merge_index
                        .entry((row * TILE_ROWS + 1, col * TILE_COLS + 1))
                        .or_default()
                        .push(at as u32);
                    col += 1;
                }
                row += 1;
            }
        }
    }

    /// The merges touching the tile with this top-left cell.
    pub fn merges_in_tile(&self, first_row: u32, first_col: u32) -> impl Iterator<Item = &Rect> {
        self.merge_index
            .get(&(first_row, first_col))
            .into_iter()
            .flatten()
            .map(|&at| &self.merges[at as usize])
    }

    /// The merge a cell belongs to, if any.
    pub fn merge_at(&self, row: u32, col: u32) -> Option<&Rect> {
        let first_row = (row - 1) / TILE_ROWS * TILE_ROWS + 1;
        let first_col = (col - 1) / TILE_COLS * TILE_COLS + 1;
        self.merges_in_tile(first_row, first_col)
            .find(|merge| merge.contains(row, col))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_uses_its_own_style_then_its_rows_then_its_columns() {
        let mut sheet = SheetInfo::default();
        sheet.rows.rows.push((
            5,
            LineInfo {
                style: Some(7),
                ..LineInfo::default()
            },
        ));
        sheet.cols.ranges.push((
            2,
            3,
            LineInfo {
                style: Some(9),
                ..LineInfo::default()
            },
        ));
        sheet.set_style(5, 2, 1);
        assert_eq!(sheet.style_at(5, 2), 1);
        assert_eq!(sheet.style_at(5, 1), 7);
        assert_eq!(sheet.style_at(6, 2), 9);
        assert_eq!(sheet.style_at(6, 4), 0);
    }

    #[test]
    fn style_runs_cover_gaps_and_changes() {
        let mut sheet = SheetInfo::default();
        for row in 1..=100 {
            sheet.set_style(row, 1, 3);
        }
        sheet.set_style(200, 1, 4);
        sheet.set_style(201, 1, 4);
        sheet.set_style(202, 1, 5);
        assert_eq!(sheet.styles[0].runs.len(), 4);
        assert_eq!(sheet.style_at(100, 1), 3);
        assert_eq!(sheet.style_at(101, 1), 0);
        assert_eq!(sheet.style_at(199, 1), 0);
        assert_eq!(sheet.style_at(201, 1), 4);
        assert_eq!(sheet.style_at(202, 1), 5);
        assert_eq!(sheet.style_at(203, 1), 0);
    }

    #[test]
    fn a_blank_row_style_stays_visible_past_a_column_with_no_cells() {
        let mut sheet = SheetInfo::default();
        sheet.set_style(10, 1, 3);
        assert_eq!(sheet.style_at(9, 1), 0);
        assert_eq!(sheet.style_at(10, 1), 3);
        assert_eq!(sheet.style_at(11, 1), 0);
    }

    #[test]
    fn a_style_written_out_of_order_lands_in_place() {
        let mut sheet = SheetInfo::default();
        for row in 1..=10 {
            sheet.set_style(row, 1, 3);
        }
        sheet.set_style(5, 1, 8);
        assert_eq!(sheet.style_at(4, 1), 3);
        assert_eq!(sheet.style_at(5, 1), 8);
        assert_eq!(sheet.style_at(6, 1), 3);
        sheet.set_style(1, 1, 9);
        assert_eq!(sheet.style_at(1, 1), 9);
        assert_eq!(sheet.style_at(2, 1), 3);
    }

    #[test]
    fn merges_are_found_from_any_tile_they_touch() {
        let mut sheet = SheetInfo::default();
        let merge = Rect {
            top: 60,
            left: 15,
            bottom: 70,
            right: 18,
        };
        sheet.merges.push(merge);
        sheet.index_merges();
        assert_eq!(sheet.merges_in_tile(1, 1).count(), 1);
        assert_eq!(sheet.merges_in_tile(65, 17).count(), 1);
        assert_eq!(sheet.merges_in_tile(129, 1).count(), 0);
        assert_eq!(sheet.merge_at(70, 18), Some(&merge));
        assert_eq!(sheet.merge_at(71, 18), None);
    }

    #[test]
    fn sizes_use_the_default_unless_a_line_says_otherwise() {
        let mut sheet = SheetInfo::default();
        sheet.rows.default_px = 20;
        sheet.cols.default_px = 64;
        sheet.rows.rows.push((
            3,
            LineInfo {
                size: Some(40),
                ..LineInfo::default()
            },
        ));
        sheet.rows.rows.push((
            4,
            LineInfo {
                hidden: true,
                ..LineInfo::default()
            },
        ));
        sheet.cols.ranges.push((
            2,
            4,
            LineInfo {
                size: Some(100),
                ..LineInfo::default()
            },
        ));
        assert_eq!(sheet.rows.px(2), 20);
        assert_eq!(sheet.rows.px(3), 40);
        assert_eq!(sheet.rows.px(4), 0);
        assert_eq!(sheet.cols.px(1), 64);
        assert_eq!(sheet.cols.px(4), 100);
        assert_eq!(sheet.cols.px(5), 64);
    }
}
