//! Inserting and deleting cells that shift others along. Cells move as ordinary changes to cells, and
//! a change to the workbook's shape with a band says what follows them in the rest of the file.

use std::collections::BTreeMap;

use cgs_calc::{CalcEngine, CellRef};
use cgs_model::{Rect, SheetKind};
use cgs_refs::{edit_rect, Axis, AxisEdit};

use crate::command::{Area, Op};
use crate::edit::{EditCell, EditInput};
use crate::Session;

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
            table.sheet == sheet
                && meets(&Rect {
                    top: table.range.top,
                    left: table.range.left,
                    bottom: table.range.bottom,
                    right: table.range.right,
                })
        }) {
            return Err("Cells can't shift through a table.".to_string());
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
        let edit = self.shift_edit(sheet, area, down, delete)?;
        let name = self.book.sheets[sheet as usize].name.clone();
        let mut edits: BTreeMap<(u32, u32, u32), EditCell> = BTreeMap::new();
        // Formulas anywhere that read what moves follow it.
        for (at, text) in self.calc.all_formulas().map_err(|error| error.0)? {
            let home = self.book.sheets[at.sheet as usize].name.as_str();
            if let std::borrow::Cow::Owned(next) = cgs_refs::edit_formula(&text, home, &name, &edit)
            {
                edits.insert(
                    (at.sheet, at.row, at.col),
                    EditCell {
                        sheet: at.sheet,
                        row: at.row,
                        col: at.col,
                        input: Some(EditInput::Formula(next)),
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
                    cgs_calc::CellInput::Formula { text, .. } => Some(EditInput::Formula(text)),
                    cgs_calc::CellInput::Value(value) => Some(EditInput::Value(value)),
                    cgs_calc::CellInput::Clear => Some(EditInput::Clear),
                    cgs_calc::CellInput::Spill { .. } => None,
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
        let mut list = vec![
            Op::ShiftLog { sheet, edit },
            Op::Cells(edits.into_values().collect()),
        ];
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
