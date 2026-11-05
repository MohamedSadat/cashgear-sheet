//! A sheet apart from its values: sizes, merges, panes, drawings, and which style each
//! cell uses. The formula engine holds the values.

use std::collections::HashMap;

use cgs_refs::{edit_rect, Axis, AxisEdit};

use crate::{Rect, MAX_ROWS, TILE_COLS, TILE_ROWS};

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

    /// Makes room for rows, which take the style of the row above them, as new rows do in Excel.
    fn insert_rows(&mut self, at: u32, count: u32) {
        for run in &mut self.runs {
            if run.0 >= at {
                run.0 = run.0.saturating_add(count);
            }
        }
        self.runs.retain(|&(start, _)| start <= MAX_ROWS);
        if self.end >= at {
            self.end = self.end.saturating_add(count).min(MAX_ROWS + 1);
        }
    }

    fn delete_rows(&mut self, at: u32, count: u32) {
        let after = at + count;
        let carried = self.get(after);
        let mut runs: Vec<(u32, u32)> = self
            .runs
            .iter()
            .copied()
            .take_while(|&(start, _)| start < at)
            .collect();
        if self.end > after && runs.last().is_none_or(|&(_, style)| style != carried) {
            runs.push((at, carried));
        }
        for &(start, style) in self.runs.iter().filter(|&&(start, _)| start > after) {
            runs.push((start - count, style));
        }
        self.runs = runs;
        if self.end > after {
            self.end -= count;
        } else if self.end > at {
            self.end = at;
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

    /// New rows take the height and style of the row above them, but stay visible.
    fn apply(&mut self, edit: &AxisEdit) {
        let above = (!edit.delete && edit.at > 1)
            .then(|| self.info(edit.at - 1).copied())
            .flatten()
            .filter(|info| info.size.is_some() || info.style.is_some());
        self.rows = std::mem::take(&mut self.rows)
            .into_iter()
            .filter_map(|(row, info)| edit.point(row).map(|row| (row, info)))
            .collect();
        if let Some(above) = above {
            let split = self.rows.partition_point(|&(row, _)| row < edit.at);
            let tail = self.rows.split_off(split);
            self.rows.extend((0..edit.count).map(|step| {
                (
                    edit.at + step,
                    LineInfo {
                        hidden: false,
                        ..above
                    },
                )
            }));
            self.rows.extend(tail);
        }
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

    /// New columns join the range of the column to their left, as they take its width and style.
    fn apply(&mut self, edit: &AxisEdit) {
        self.ranges = std::mem::take(&mut self.ranges)
            .into_iter()
            .filter_map(|(first, last, info)| {
                let reaches = !edit.delete && last + 1 == edit.at;
                let (first, last) = edit.span((first, last))?;
                let last = if reaches {
                    (last + edit.count).min(crate::MAX_COLS)
                } else {
                    last
                };
                Some((first, last, info))
            })
            .collect();
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

impl AnchorPoint {
    fn apply(&mut self, edit: &AxisEdit) {
        let (place, offset) = match edit.axis {
            Axis::Row => (&mut self.row, &mut self.row_off),
            Axis::Col => (&mut self.col, &mut self.col_off),
        };
        match edit.point(*place) {
            Some(moved) => *place = moved,
            None => {
                *place = edit.clamp(*place);
                *offset = 0;
            }
        }
    }
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

impl Anchor {
    /// Follows the cells it is tied to.
    pub fn apply(&mut self, edit: &AxisEdit) {
        match self {
            Self::TwoCell { from, to } => {
                from.apply(edit);
                to.apply(edit);
            }
            Self::OneCell { from, .. } => from.apply(edit),
            Self::Absolute { .. } => {}
        }
    }
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

/// A cell's new place, or the nearest place left for one that was deleted.
fn moved_cell((row, col): (u32, u32), edit: &AxisEdit) -> (u32, u32) {
    match edit.axis {
        Axis::Row => (edit.clamp(row), col),
        Axis::Col => (row, edit.clamp(col)),
    }
}

/// How far cells reach after an edit, given how far they reached before it.
fn moved_extent(extent: u32, edit: &AxisEdit) -> u32 {
    if extent == 0 {
        return 0;
    }
    if edit.delete {
        edit.span((1, extent)).map_or(0, |span| span.1)
    } else if extent >= edit.at {
        (extent + edit.count).min(edit.axis.max())
    } else {
        extent
    }
}

/// What `SheetInfo::apply_axis` changes.
#[derive(Debug, Clone)]
pub struct SheetShape {
    rows: Vec<(u32, LineInfo)>,
    cols: Vec<(u32, u32, LineInfo)>,
    view: SheetView,
    merges: Vec<Rect>,
    drawings: Vec<Drawing>,
    used: (u32, u32),
    styles: Vec<ColumnRuns>,
}

impl SheetShape {
    /// Roughly what this holds, for the undo stack's limit.
    pub fn bytes(&self) -> usize {
        self.rows.len() * 24
            + self.cols.len() * 32
            + self.merges.len() * 16
            + self.drawings.len() * 96
            + self
                .styles
                .iter()
                .map(|runs| runs.runs.len() * 8 + 32)
                .sum::<usize>()
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
    /// Deleted. It keeps its index, so that sheets can be told apart by index and an undo can bring
    /// it back.
    pub deleted: bool,
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

    /// The cells of a row that have a style of their own, as (column, style).
    pub fn own_styles_in_row(&self, row: u32) -> Vec<(u32, u32)> {
        self.styles
            .iter()
            .enumerate()
            .filter_map(|(slot, runs)| {
                let style = runs.get(row);
                (style != NO_CELL).then_some((slot as u32 + 1, style))
            })
            .collect()
    }

    /// The cells of a column that have a style of their own, as (row, style).
    pub fn own_styles_in_col(&self, col: u32) -> Vec<(u32, u32)> {
        let Some(runs) = self.styles.get((col - 1) as usize) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (at, &(start, style)) in runs.runs.iter().enumerate() {
            if style == NO_CELL {
                continue;
            }
            let end = runs
                .runs
                .get(at + 1)
                .map_or(runs.end, |&(next, _)| next)
                .min(runs.end);
            out.extend((start..end).map(|row| (row, style)));
        }
        out
    }

    /// Everything an edit to rows or columns changes here, for putting back.
    pub fn shape(&self) -> SheetShape {
        SheetShape {
            rows: self.rows.rows.clone(),
            cols: self.cols.ranges.clone(),
            view: self.view,
            merges: self.merges.clone(),
            drawings: self.drawings.clone(),
            used: self.used,
            styles: self.styles.clone(),
        }
    }

    pub fn restore_shape(&mut self, shape: SheetShape) {
        self.rows.rows = shape.rows;
        self.cols.ranges = shape.cols;
        self.view = shape.view;
        self.merges = shape.merges;
        self.drawings = shape.drawings;
        self.used = shape.used;
        self.styles = shape.styles;
        self.index_merges();
    }

    /// Moves everything on the sheet that an edit to rows or columns moves, as Excel does: a merge
    /// grows or shrinks with its range, and one that comes down to a single cell stops being one.
    pub fn apply_axis(&mut self, edit: &AxisEdit) {
        match edit.axis {
            Axis::Row => {
                self.rows.apply(edit);
                for runs in &mut self.styles {
                    if edit.delete {
                        runs.delete_rows(edit.at, edit.count);
                    } else {
                        runs.insert_rows(edit.at, edit.count);
                    }
                }
                self.used.0 = moved_extent(self.used.0, edit);
            }
            Axis::Col => {
                self.cols.apply(edit);
                let slot = (edit.at - 1) as usize;
                if edit.delete {
                    let end = (slot + edit.count as usize).min(self.styles.len());
                    if slot < end {
                        self.styles.drain(slot..end);
                    }
                } else if slot <= self.styles.len() {
                    // New columns take the style of the column to their left.
                    let left = slot
                        .checked_sub(1)
                        .and_then(|left| self.styles.get(left))
                        .cloned()
                        .unwrap_or_default();
                    let copies = std::iter::repeat_n(left, edit.count as usize);
                    self.styles.splice(slot..slot, copies);
                    self.styles.truncate(crate::MAX_COLS as usize);
                }
                self.used.1 = moved_extent(self.used.1, edit);
            }
        }
        self.merges = self
            .merges
            .iter()
            .filter_map(|merge| {
                let (top, left, bottom, right) =
                    edit_rect((merge.top, merge.left, merge.bottom, merge.right), edit)?;
                ((top, left) != (bottom, right)).then_some(Rect {
                    top,
                    left,
                    bottom,
                    right,
                })
            })
            .collect();
        self.index_merges();
        for drawing in &mut self.drawings {
            drawing.anchor.apply(edit);
        }
        self.view.active_cell = moved_cell(self.view.active_cell, edit);
        self.view.top_left = moved_cell(self.view.top_left, edit);
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
    fn inserted_rows_take_the_style_of_the_row_above() {
        let mut sheet = SheetInfo::default();
        sheet.set_style(1, 1, 3);
        sheet.set_style(2, 1, 4);
        sheet.set_style(3, 1, 5);
        sheet.apply_axis(&AxisEdit::insert(Axis::Row, 2, 2));
        assert_eq!(sheet.style_at(1, 1), 3);
        assert_eq!(sheet.style_at(2, 1), 3);
        assert_eq!(sheet.style_at(3, 1), 3);
        assert_eq!(sheet.style_at(4, 1), 4);
        assert_eq!(sheet.style_at(5, 1), 5);
        assert_eq!(sheet.style_at(6, 1), 0);
        sheet.apply_axis(&AxisEdit::insert(Axis::Row, 1, 1));
        assert_eq!(sheet.style_at(1, 1), 0);
        assert_eq!(sheet.style_at(2, 1), 3);
    }

    #[test]
    fn deleted_rows_take_their_styles_with_them() {
        let mut sheet = SheetInfo::default();
        for (row, style) in [(1, 3), (2, 4), (3, 5), (4, 6)] {
            sheet.set_style(row, 1, style);
        }
        sheet.apply_axis(&AxisEdit::delete(Axis::Row, 2, 2));
        assert_eq!(sheet.style_at(1, 1), 3);
        assert_eq!(sheet.style_at(2, 1), 6);
        assert_eq!(sheet.style_at(3, 1), 0);
        sheet.apply_axis(&AxisEdit::delete(Axis::Row, 2, 1));
        assert_eq!(sheet.style_at(1, 1), 3);
        assert_eq!(sheet.style_at(2, 1), 0);
    }

    #[test]
    fn a_run_that_spans_a_delete_keeps_going() {
        let mut sheet = SheetInfo::default();
        for row in 1..=10 {
            sheet.set_style(row, 1, 3);
        }
        sheet.apply_axis(&AxisEdit::delete(Axis::Row, 3, 2));
        assert_eq!(sheet.style_at(8, 1), 3);
        assert_eq!(sheet.style_at(9, 1), 0);
        assert_eq!(sheet.own_styles_in_col(1).len(), 8);
    }

    #[test]
    fn inserted_columns_take_the_style_and_width_of_the_column_to_the_left() {
        let mut sheet = SheetInfo::default();
        sheet.set_style(1, 2, 7);
        sheet.cols.default_px = 64;
        sheet.cols.ranges.push((
            2,
            2,
            LineInfo {
                size: Some(100),
                ..LineInfo::default()
            },
        ));
        sheet.apply_axis(&AxisEdit::insert(Axis::Col, 3, 2));
        assert_eq!(sheet.style_at(1, 4), 7);
        assert_eq!(sheet.cols.px(4), 100);
        assert_eq!(sheet.cols.px(5), 64);
        sheet.apply_axis(&AxisEdit::delete(Axis::Col, 2, 2));
        assert_eq!(sheet.style_at(1, 2), 7);
        assert_eq!(sheet.cols.px(2), 100);
        assert_eq!(sheet.cols.px(3), 64);
    }

    #[test]
    fn merges_follow_and_a_merge_of_one_cell_goes() {
        let mut sheet = SheetInfo::default();
        sheet.merges.push(Rect {
            top: 2,
            left: 1,
            bottom: 3,
            right: 1,
        });
        sheet.index_merges();
        sheet.apply_axis(&AxisEdit::insert(Axis::Row, 3, 1));
        assert_eq!(sheet.merges[0].bottom, 4);
        assert!(sheet.merge_at(4, 1).is_some());
        sheet.apply_axis(&AxisEdit::delete(Axis::Row, 3, 2));
        assert!(sheet.merges.is_empty());
    }

    #[test]
    fn a_shape_puts_everything_back() {
        let mut sheet = SheetInfo::default();
        sheet.set_style(5, 2, 9);
        sheet.used = (5, 2);
        let shape = sheet.shape();
        sheet.apply_axis(&AxisEdit::delete(Axis::Row, 1, 5));
        assert_eq!(sheet.style_at(5, 2), 0);
        assert_eq!(sheet.used.0, 0);
        sheet.restore_shape(shape);
        assert_eq!(sheet.style_at(5, 2), 9);
        assert_eq!(sheet.used, (5, 2));
    }

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
