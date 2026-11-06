//! Remove Duplicates: rows of a selection whose chosen columns show what an earlier row shows go,
//! and the rows under them move up, inside the selection only. Excel compares what cells show,
//! whatever the case, so a date in two formats is two values.

use std::collections::{HashMap, HashSet};

use cgs_calc::{CalcEngine, CalcValue, CellSink, Rect as CalcRect, SheetRect};
use cgs_numfmt::Options;

use crate::command::{Applied, Area, Op, PasteMode};
use crate::edit::{EditCell, EditInput};
use crate::Session;

#[derive(Default)]
struct Values(HashMap<(u32, u32), CalcValue>);

impl CellSink for Values {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.take(row, col, value.clone());
    }

    fn take(&mut self, row: u32, col: u32, value: CalcValue) {
        if value != CalcValue::Empty {
            self.0.insert((row, col), value);
        }
    }
}

impl Session {
    pub(crate) fn remove_duplicates_command(
        &mut self,
        sheet: u32,
        area: Area,
        columns: &[u32],
        header: bool,
        locale: &cgs_numfmt::Locale,
    ) -> Result<Applied, String> {
        let info = self.sheet_for_edit(sheet)?;
        let area = Area {
            bottom: area.bottom.min(info.used.0.max(area.top)),
            right: area.right.min(info.used.1.max(area.left)),
            ..area
        };
        if columns.is_empty() {
            return Err("Select one or more columns.".to_string());
        }
        if columns
            .iter()
            .any(|&col| col < area.left || col > area.right)
        {
            return Err("A column to compare must be inside the selection.".to_string());
        }
        if info.merges.iter().any(|merge| {
            merge.top <= area.bottom
                && merge.bottom >= area.top
                && merge.left <= area.right
                && merge.right >= area.left
        }) {
            return Err("To do this, all the merged cells need to be the same size.".to_string());
        }
        if self.cuts_an_array(sheet, area) {
            return Err("You can't change part of an array.".to_string());
        }
        let first = area.top + u32::from(header);
        if first > area.bottom {
            return Ok(Applied::default());
        }
        let mut values = Values::default();
        for &col in columns {
            self.calc.read_block(
                SheetRect {
                    sheet,
                    rect: CalcRect {
                        top: first,
                        left: col,
                        bottom: area.bottom,
                        right: col,
                    },
                },
                &mut values,
            );
        }
        let options = Options {
            locale,
            date1904: self.book.date1904,
        };
        let mut formats = HashMap::new();
        let mut seen: HashSet<Vec<String>> = HashSet::new();
        let mut kept = Vec::new();
        for row in first..=area.bottom {
            let key: Vec<String> = columns
                .iter()
                .map(|&col| {
                    let num_fmt = {
                        let info = &self.book.sheets[sheet as usize];
                        self.book.styles.xf(info.style_at(row, col)).num_fmt
                    };
                    values.0.get(&(row, col)).map_or_else(String::new, |value| {
                        self.shown(value, num_fmt, &mut formats, &options)
                            .to_lowercase()
                    })
                })
                .collect();
            if seen.insert(key) {
                kept.push(row);
            }
        }
        let removed = (area.bottom - first + 1) as usize - kept.len();
        if removed == 0 {
            return Ok(Applied {
                selection: Some((sheet, area)),
                replaced: 0,
            });
        }
        let mut edits = Vec::new();
        for (target, &source) in (first..).zip(&kept) {
            if target == source {
                continue;
            }
            let shift = i64::from(target) - i64::from(source);
            for col in area.left..=area.right {
                let input = self.copied_input(sheet, source, col, shift, 0, PasteMode::All);
                edits.push(EditCell {
                    sheet,
                    row: target,
                    col,
                    input,
                    style: Some(self.book.sheets[sheet as usize].style_at(source, col)),
                });
            }
        }
        // What's left at the bottom is empty, as when Excel moves cells up.
        for row in first + kept.len() as u32..=area.bottom {
            for col in area.left..=area.right {
                edits.push(EditCell {
                    sheet,
                    row,
                    col,
                    input: Some(EditInput::Clear),
                    style: Some(0),
                });
            }
        }
        let mut applied =
            self.perform("remove duplicates", Op::Cells(edits), Some((sheet, area)))?;
        applied.replaced = removed as u32;
        Ok(applied)
    }
}
