//! Inserting and deleting cells that shift others along. Cells move as ordinary changes to cells, and
//! a change to the workbook's shape with a band says what follows them in the rest of the file.

use std::collections::BTreeMap;

use cgs_calc::{CalcEngine, CellInput, CellRef, FormulaShape, TableInfo};
use cgs_model::{Rect, SheetKind};
use cgs_refs::{edit_rect, Axis, AxisEdit};

use crate::command::{Area, Op};
use crate::edit::{EditCell, EditInput};
use crate::Session;

/// Excel's reason, in its words.
const TABLE_REFUSED: &str =
    "This operation is not allowed. The operation is attempting to shift cells in a table on your worksheet.";

/// The data rows of a table, between its header and its totals.
fn data_rows(table: &TableInfo) -> (u32, u32) {
    (
        table.range.top + u32::from(table.header_row),
        table.range.bottom - u32::from(table.totals_row),
    )
}

/// Whether a shift that reaches a table is one Excel makes, once `widened` has made it whole rows
/// or columns of the table. Read off Excel 2016 over COM: the table either moves whole, or loses or
/// gains rows or columns of its own, and anything that would cut it is refused.
fn table_takes(table: &TableInfo, area: Area, down: bool, delete: bool) -> bool {
    let t = table.range;
    let (first, last) = data_rows(table);
    if down {
        let across = area.left <= t.left && area.right >= t.right;
        let whole = area.left == t.left && area.right == t.right;
        if delete {
            // Moving up from below, or losing data rows.
            (across && area.bottom < t.top)
                || (whole && area.top >= first && area.bottom <= last && first <= last)
        } else {
            (across && area.top <= t.top) || (whole && area.top >= first && area.top <= last)
        }
    } else {
        let across = area.top <= t.top && area.bottom >= t.bottom;
        let whole = area.top == t.top && area.bottom == t.bottom;
        if delete {
            (across && area.right < t.left)
                || (whole
                    && area.left >= t.left
                    && area.right <= t.right
                    && (area.left > t.left || area.right < t.right))
        } else {
            (across && area.left <= t.left) || (whole && area.left > t.left && area.left <= t.right)
        }
    }
}

/// A deletion inside a table takes whole rows or columns of it, as in Excel. Returns the area to
/// shift, and a row of the table to empty instead, since deleting every data row leaves one.
fn widened(
    tables: &[TableInfo],
    sheet: u32,
    area: Area,
    down: bool,
    delete: bool,
) -> (Area, Option<Area>) {
    if !delete {
        return (area, None);
    }
    let Some(table) = tables.iter().find(|table| {
        let t = table.range;
        table.sheet == sheet
            && area.top >= t.top
            && area.bottom <= t.bottom
            && area.left >= t.left
            && area.right <= t.right
    }) else {
        return (area, None);
    };
    let t = table.range;
    if !down {
        return (
            Area {
                top: t.top,
                bottom: t.bottom,
                ..area
            },
            None,
        );
    }
    let (first, last) = data_rows(table);
    if area.top < first || area.bottom > last {
        return (area, None);
    }
    let rows = Area {
        left: t.left,
        right: t.right,
        ..area
    };
    if area.top == first && area.bottom == last {
        let kept = Area {
            top: first,
            bottom: first,
            ..rows
        };
        return (
            Area {
                top: first + 1,
                ..rows
            },
            Some(kept),
        );
    }
    (rows, None)
}

impl Session {
    /// What a shift would move, checked. `down` moves cells along columns, or else along rows.
    fn shift_edit(
        &self,
        sheet: u32,
        area: Area,
        down: bool,
        delete: bool,
    ) -> Result<AxisEdit, String> {
        let info = self
            .book
            .sheets
            .get(sheet as usize)
            .filter(|info| !info.deleted && info.kind == SheetKind::Worksheet && info.loaded)
            .ok_or_else(|| "That sheet has no cells to change.".to_string())?;
        let (axis, at, count, band) = if down {
            (Axis::Row, area.top, area.rows(), (area.left, area.right))
        } else {
            (Axis::Col, area.left, area.cols(), (area.top, area.bottom))
        };
        let region = if down {
            Rect {
                top: area.top,
                left: area.left,
                bottom: cgs_model::MAX_ROWS,
                right: area.right,
            }
        } else {
            Rect {
                top: area.top,
                left: area.left,
                bottom: area.bottom,
                right: cgs_model::MAX_COLS,
            }
        };
        let meets = |other: &Rect| {
            other.top <= region.bottom
                && other.bottom >= region.top
                && other.left <= region.right
                && other.right >= region.left
        };
        let inside = |other: &Rect| {
            other.top >= region.top
                && other.bottom <= region.bottom
                && other.left >= region.left
                && other.right <= region.right
        };
        if self.calc.tables().iter().any(|table| {
            let rect = Rect {
                top: table.range.top,
                left: table.range.left,
                bottom: table.range.bottom,
                right: table.range.right,
            };
            table.sheet == sheet && meets(&rect) && !table_takes(table, area, down, delete)
        }) {
            return Err(TABLE_REFUSED.to_string());
        }
        if info
            .merges
            .iter()
            .any(|merge| meets(merge) && !inside(merge))
        {
            return Err("Cells can't shift through part of a merged area.".to_string());
        }
        if delete {
            let gone = Rect {
                top: area.top,
                left: area.left,
                bottom: area.bottom,
                right: area.right,
            };
            let cuts = |merge: &Rect| {
                merge.top <= gone.bottom
                    && merge.bottom >= gone.top
                    && merge.left <= gone.right
                    && merge.right >= gone.left
                    && !(merge.top >= gone.top
                        && merge.bottom <= gone.bottom
                        && merge.left >= gone.left
                        && merge.right <= gone.right)
            };
            if info.merges.iter().any(cuts) {
                return Err("Cells can't shift through part of a merged area.".to_string());
            }
        }
        if self.calc.spill_areas().iter().any(|spill| {
            spill.sheet == sheet
                && meets(&Rect {
                    top: spill.rect.top,
                    left: spill.rect.left,
                    bottom: spill.rect.bottom,
                    right: spill.rect.right,
                })
        }) {
            return Err("You can't change part of an array.".to_string());
        }
        if !delete {
            // What's at the far edge would have nowhere to go.
            let (filled, limit) = if down {
                (
                    Area {
                        top: cgs_model::MAX_ROWS - count + 1,
                        left: area.left,
                        bottom: cgs_model::MAX_ROWS,
                        right: area.right,
                    },
                    info.used.0,
                )
            } else {
                (
                    Area {
                        top: area.top,
                        left: cgs_model::MAX_COLS - count + 1,
                        bottom: area.bottom,
                        right: cgs_model::MAX_COLS,
                    },
                    info.used.1,
                )
            };
            let far = if down { filled.top } else { filled.left };
            if limit >= far && !self.merge_discards(sheet, filled).is_empty() {
                return Err("That would push cells off the edge of the sheet.".to_string());
            }
        }
        Ok(AxisEdit {
            axis,
            at,
            count,
            delete,
            band: Some(band),
        })
    }

    /// The steps of a shift, to apply and undo as one.
    pub(crate) fn shift_op(
        &mut self,
        sheet: u32,
        area: Area,
        down: bool,
        delete: bool,
    ) -> Result<Op, String> {
        let tables_before = self.calc.tables().to_vec();
        let (area, emptied) = widened(&tables_before, sheet, area, down, delete);
        let empty_row = |sheet: u32, kept: Area| {
            let mut cells = Vec::new();
            for col in kept.left..=kept.right {
                cells.push(EditCell {
                    sheet,
                    row: kept.top,
                    col,
                    input: Some(EditInput::Clear),
                    style: None,
                });
            }
            cells
        };
        if area.top > area.bottom {
            // A table's only data row stays, emptied.
            let kept = emptied.ok_or_else(|| TABLE_REFUSED.to_string())?;
            return Ok(Op::Cells(empty_row(sheet, kept)));
        }
        let edit = self.shift_edit(sheet, area, down, delete)?;
        let name = self.book.sheets[sheet as usize].name.clone();
        // Tables in the way move, or lose or gain rows or columns.
        let mut tables = tables_before.clone();
        let mut lost_columns: Vec<(String, Vec<String>)> = Vec::new();
        let mut headers: Vec<(u32, u32, String)> = Vec::new();
        // Tables that lose or gain columns, whose parts must say so.
        let mut reshaped: Vec<String> = Vec::new();
        for table in tables.iter_mut().filter(|table| table.sheet == sheet) {
            let t = table.range;
            let Some((top, left, bottom, right)) =
                edit_rect((t.top, t.left, t.bottom, t.right), &edit)
            else {
                continue;
            };
            if edit.axis == Axis::Col && edit.covers((t.top, t.bottom)) {
                if edit.delete {
                    let mut lost = Vec::new();
                    let mut col = t.left;
                    table.columns.retain(|column| {
                        let keep = edit.point(col).is_some();
                        col += 1;
                        if !keep {
                            lost.push(column.clone());
                        }
                        keep
                    });
                    if !lost.is_empty() {
                        reshaped.push(table.name.clone());
                        lost_columns.push((table.name.clone(), lost));
                    }
                } else if edit.at > t.left && edit.at <= t.right {
                    let at = (edit.at - t.left) as usize;
                    reshaped.push(table.name.clone());
                    for step in 0..edit.count as usize {
                        let column = cgs_calc::new_column_name(&table.columns);
                        table.columns.insert(at + step, column.clone());
                        if table.header_row {
                            headers.push((top, edit.at + step as u32, column));
                        }
                    }
                }
            }
            table.range = cgs_calc::Rect {
                top,
                left,
                bottom,
                right,
            };
        }
        let mut edits: BTreeMap<(u32, u32, u32), EditCell> = BTreeMap::new();
        // Formulas anywhere that read what moves follow it.
        for (at, text) in self.calc.all_formulas().map_err(|error| error.0)? {
            let home = self.book.sheets[at.sheet as usize].name.as_str();
            let mut next = cgs_refs::edit_formula(&text, home, &name, &edit);
            for (table, lost) in &lost_columns {
                if let std::borrow::Cow::Owned(gone) =
                    cgs_refs::delete_table_columns(&next, table, lost)
                {
                    next = std::borrow::Cow::Owned(gone);
                }
            }
            if let std::borrow::Cow::Owned(next) = next {
                let input = self.rewritten(at, next);
                edits.insert(
                    (at.sheet, at.row, at.col),
                    EditCell {
                        sheet: at.sheet,
                        row: at.row,
                        col: at.col,
                        input: Some(input),
                        style: None,
                    },
                );
            }
        }
        // What's in the way, as read after those changes.
        let info = &self.book.sheets[sheet as usize];
        let (region, step_rows, step_cols): (Area, i64, i64) = if down {
            let count = i64::from(edit.count);
            (
                Area {
                    top: area.top,
                    left: area.left,
                    bottom: info.used.0.max(area.bottom),
                    right: area.right,
                },
                if delete { -count } else { count },
                0,
            )
        } else {
            let count = i64::from(edit.count);
            (
                Area {
                    top: area.top,
                    left: area.left,
                    bottom: area.bottom,
                    right: info.used.1.max(area.right),
                },
                0,
                if delete { -count } else { count },
            )
        };
        let mut moving: Vec<(u32, u32)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for at in self.merge_discards(sheet, region) {
            if seen.insert(at) {
                moving.push(at);
            }
        }
        for col in region.left..=region.right.min(info.styled_cols()) {
            for (first, last, style) in info.style_spans(col, region.top, region.bottom) {
                if style != cgs_model::NO_CELL {
                    for row in first..=last {
                        if seen.insert((row, col)) {
                            moving.push((row, col));
                        }
                    }
                }
            }
        }
        let inherited = |info: &cgs_model::SheetInfo, row: u32, col: u32| {
            info.rows
                .info(row)
                .and_then(|line| line.style)
                .or_else(|| info.cols.info(col).and_then(|line| line.style))
                .unwrap_or(0)
        };
        let mut taken = Vec::with_capacity(moving.len());
        for &(row, col) in &moving {
            let source = CellRef { sheet, row, col };
            let input = edits
                .get(&(sheet, row, col))
                .and_then(|edit| edit.input.clone())
                .or_else(|| match self.calc.cell_input(source) {
                    CellInput::Formula {
                        text,
                        shape: FormulaShape::Normal,
                        ..
                    } => Some(EditInput::Formula(text)),
                    CellInput::Formula { text, shape, .. } => Some(EditInput::Array(text, shape)),
                    CellInput::Value(value) => Some(EditInput::Value(value)),
                    CellInput::Clear => Some(EditInput::Clear),
                    CellInput::Spill { .. } => None,
                })
                .unwrap_or(EditInput::Clear);
            taken.push((row, col, input, info.style_at(row, col)));
        }
        for &(row, col) in &moving {
            let style = inherited(info, row, col);
            edits.insert(
                (sheet, row, col),
                EditCell {
                    sheet,
                    row,
                    col,
                    input: Some(EditInput::Clear),
                    style: Some(style),
                },
            );
        }
        let mut filled: Vec<(u32, u32, cgs_calc::Rect)> = Vec::new();
        for (row, col, input, style) in taken {
            let gone = delete
                && row >= area.top
                && row <= area.bottom
                && col >= area.left
                && col <= area.right;
            if gone {
                continue;
            }
            let (new_row, new_col) = (
                (i64::from(row) + step_rows) as u32,
                (i64::from(col) + step_cols) as u32,
            );
            // An array that moves whole takes its area along, and what it fills isn't cleared.
            let input = match input {
                EditInput::Array(text, shape) => {
                    let moved = |rect: cgs_calc::Rect| cgs_calc::Rect {
                        top: (i64::from(rect.top) + step_rows) as u32,
                        left: (i64::from(rect.left) + step_cols) as u32,
                        bottom: (i64::from(rect.bottom) + step_rows) as u32,
                        right: (i64::from(rect.right) + step_cols) as u32,
                    };
                    let shape = match shape {
                        FormulaShape::Array(rect) => FormulaShape::Array(moved(rect)),
                        FormulaShape::Dynamic(rect) => FormulaShape::Dynamic(moved(rect)),
                        FormulaShape::Normal => FormulaShape::Normal,
                    };
                    if let FormulaShape::Array(rect) | FormulaShape::Dynamic(rect) = shape {
                        filled.push((new_row, new_col, rect));
                    }
                    EditInput::Array(text, shape)
                }
                input => input,
            };
            edits.insert(
                (sheet, new_row, new_col),
                EditCell {
                    sheet,
                    row: new_row,
                    col: new_col,
                    input: Some(input),
                    style: Some(style),
                },
            );
        }
        for (row, col, rect) in filled {
            edits.retain(|&(at_sheet, r, c), edit| {
                at_sheet != sheet
                    || (r, c) == (row, col)
                    || !rect.contains(r, c)
                    || !matches!(edit.input, Some(EditInput::Clear))
            });
        }
        for (row, col, column) in headers {
            edits.insert(
                (sheet, row, col),
                EditCell {
                    sheet,
                    row,
                    col,
                    input: Some(EditInput::Value(cgs_calc::CalcValue::Text(column))),
                    style: None,
                },
            );
        }
        if let Some(kept) = emptied {
            for cell in empty_row(sheet, kept) {
                edits.insert((sheet, cell.row, cell.col), cell);
            }
        }
        let mut list = vec![
            Op::ShiftLog { sheet, edit },
            Op::Cells(edits.into_values().collect()),
        ];
        if tables != tables_before {
            list.push(Op::Tables(tables));
        }
        if !reshaped.is_empty() {
            let mut book_tables = self.book.tables.clone();
            for table in &mut book_tables {
                if reshaped
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&table.name))
                {
                    table.edited = true;
                    // A calculated column's formula follows the edit, as its cells do.
                    for extra in &mut table.column_info {
                        if let Some(text) = &extra.calculated {
                            let mut next = cgs_refs::edit_formula(text, &name, &name, &edit);
                            for (owner, lost) in &lost_columns {
                                if let std::borrow::Cow::Owned(gone) =
                                    cgs_refs::delete_table_columns(&next, owner, lost)
                                {
                                    next = std::borrow::Cow::Owned(gone);
                                }
                            }
                            if let std::borrow::Cow::Owned(next) = next {
                                extra.calculated = Some(next);
                            }
                        }
                    }
                }
            }
            list.push(Op::BookTables(book_tables));
        }
        // Conditional formats, notes, links and validation move with the cells too.
        let info = &self.book.sheets[sheet as usize];
        let mut moved = cgs_model::SheetInfo::new(info.name.clone(), info.state, info.kind, None);
        moved.cf = info.cf.clone();
        moved.notes = info.notes.clone();
        moved.links = info.links.clone();
        moved.validations = info.validations.clone();
        moved.move_annotations(&edit);
        if moved.cf != info.cf {
            list.push(Op::SheetCf {
                sheet,
                blocks: moved.cf,
            });
        }
        if moved.notes != info.notes {
            list.push(Op::SheetNotes {
                sheet,
                notes: moved.notes,
            });
        }
        if moved.links != info.links {
            list.push(Op::SheetLinks {
                sheet,
                links: moved.links,
            });
        }
        if moved.validations != info.validations {
            list.push(Op::SheetDv {
                sheet,
                list: moved.validations,
            });
        }
        // Merged areas in the way move along.
        let info = &self.book.sheets[sheet as usize];
        if !info.merges.is_empty() {
            let merges: Vec<Rect> = info
                .merges
                .iter()
                .filter_map(|merge| {
                    let (top, left, bottom, right) =
                        edit_rect((merge.top, merge.left, merge.bottom, merge.right), &edit)?;
                    Some(Rect {
                        top,
                        left,
                        bottom,
                        right,
                    })
                })
                .collect();
            if merges != info.merges {
                list.push(Op::Merges { sheet, merges });
            }
        }
        // And what names read.
        let mut names = self.calc.names();
        let mut changed = false;
        for entry in &mut names {
            let home = entry
                .local_sheet
                .map_or("", |index| self.book.sheets[index as usize].name.as_str());
            if let std::borrow::Cow::Owned(next) =
                cgs_refs::edit_formula(&entry.formula, home, &name, &edit)
            {
                entry.formula = next;
                changed = true;
            }
        }
        if changed {
            let before = self.calc.names();
            list.push(Op::NamesTo { names, before });
        }
        Ok(Op::Many(list))
    }
}
