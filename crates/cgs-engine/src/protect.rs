//! Sheet and workbook protection: passwords hashed as Excel 2016 hashes them, and what a protected
//! sheet or workbook refuses, as Excel 2016 measured over COM refuses it.

use base64::Engine as _;
use cgs_model::{PasswordHash, SheetProtection, WorkbookProtection};
use serde::Deserialize;
use sha2::Digest;

use crate::command::{Applied, Area, AxisName, Command, Op};
use crate::Session;

/// Excel 2016's words when a change would touch a locked cell, or anything else protection keeps.
pub const PROTECTED: &str = "The cell or chart you're trying to change is on a protected sheet. \
To make a change, unprotect the sheet. You might be requested to enter a password.";
/// And when the workbook's structure is locked.
pub const STRUCTURE: &str = "Workbook is protected and cannot be changed.";
/// And when a password doesn't match.
pub const WRONG_PASSWORD: &str = "The password you supplied is not correct. Verify that the CAPS \
LOCK key is off and be sure to use the correct capitalization.";

/// Excel's spin count, from Excel 2010 on.
const SPIN: u32 = 100_000;

/// What Protect Sheet lets people do on the protected sheet, as its list says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AllowSpec {
    pub select_locked: bool,
    pub select_unlocked: bool,
    pub format_cells: bool,
    pub format_columns: bool,
    pub format_rows: bool,
    pub insert_columns: bool,
    pub insert_rows: bool,
    pub insert_hyperlinks: bool,
    pub delete_columns: bool,
    pub delete_rows: bool,
    pub sort: bool,
    pub auto_filter: bool,
    pub pivot_tables: bool,
    pub edit_objects: bool,
    pub edit_scenarios: bool,
}

impl Default for AllowSpec {
    /// The dialog's own ticks: selecting cells, and nothing else.
    fn default() -> Self {
        Self {
            select_locked: true,
            select_unlocked: true,
            format_cells: false,
            format_columns: false,
            format_rows: false,
            insert_columns: false,
            insert_rows: false,
            insert_hyperlinks: false,
            delete_columns: false,
            delete_rows: false,
            sort: false,
            auto_filter: false,
            pivot_tables: false,
            edit_objects: false,
            edit_scenarios: false,
        }
    }
}

impl AllowSpec {
    pub fn of(protection: &SheetProtection) -> Self {
        Self {
            select_locked: protection.select_locked,
            select_unlocked: protection.select_unlocked,
            format_cells: protection.format_cells,
            format_columns: protection.format_columns,
            format_rows: protection.format_rows,
            insert_columns: protection.insert_columns,
            insert_rows: protection.insert_rows,
            insert_hyperlinks: protection.insert_hyperlinks,
            delete_columns: protection.delete_columns,
            delete_rows: protection.delete_rows,
            sort: protection.sort,
            auto_filter: protection.auto_filter,
            pivot_tables: protection.pivot_tables,
            edit_objects: !protection.objects,
            edit_scenarios: !protection.scenarios,
        }
    }

    fn protection(self, password: Option<PasswordHash>) -> SheetProtection {
        SheetProtection {
            sheet: true,
            // Excel doesn't let selecting locked cells stay on with unlocked ones off.
            select_locked: self.select_locked && self.select_unlocked,
            select_unlocked: self.select_unlocked,
            format_cells: self.format_cells,
            format_columns: self.format_columns,
            format_rows: self.format_rows,
            insert_columns: self.insert_columns,
            insert_rows: self.insert_rows,
            insert_hyperlinks: self.insert_hyperlinks,
            delete_columns: self.delete_columns,
            delete_rows: self.delete_rows,
            sort: self.sort,
            auto_filter: self.auto_filter,
            pivot_tables: self.pivot_tables,
            objects: !self.edit_objects,
            scenarios: !self.edit_scenarios,
            password,
        }
    }
}

/// Excel's hash from Excel 2010 on: SHA-512 of the salt and the password as UTF-16, then again
/// with each round's number, `spin` times.
pub fn hash_password(algorithm: &str, password: &str, salt: &[u8], spin: u32) -> Option<Vec<u8>> {
    fn run<D: Digest>(password: &str, salt: &[u8], spin: u32) -> Vec<u8> {
        let units: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut hash = D::new().chain_update(salt).chain_update(&units).finalize();
        for round in 0..spin {
            hash = D::new()
                .chain_update(&hash)
                .chain_update(round.to_le_bytes())
                .finalize();
        }
        hash.to_vec()
    }
    match algorithm {
        "SHA-512" => Some(run::<sha2::Sha512>(password, salt, spin)),
        "SHA-384" => Some(run::<sha2::Sha384>(password, salt, spin)),
        "SHA-256" => Some(run::<sha2::Sha256>(password, salt, spin)),
        _ => None,
    }
}

/// The 16-bit hash older Excel wrote, of each character's low byte.
pub fn legacy_hash(password: &str) -> u16 {
    let bytes: Vec<u16> = password
        .encode_utf16()
        .map(|unit| unit & 0xFF)
        .take(15)
        .collect();
    let mut hash: u16 = 0;
    for &byte in bytes.iter().rev() {
        hash = ((hash >> 14) & 1) | ((hash << 1) & 0x7FFF);
        hash ^= byte;
    }
    hash = ((hash >> 14) & 1) | ((hash << 1) & 0x7FFF);
    hash ^= bytes.len() as u16;
    hash ^ 0xCE4B
}

/// The hash a new password is kept as, with the salt the shell picked at random.
pub fn new_hash(password: &str, salt: &str) -> Result<Option<PasswordHash>, String> {
    if password.is_empty() {
        return Ok(None);
    }
    if password.chars().count() > 255 {
        return Err("A password can be at most 255 characters long.".to_string());
    }
    let base64 = base64::engine::general_purpose::STANDARD;
    let salt_bytes = base64
        .decode(salt)
        .ok()
        .filter(|bytes| !bytes.is_empty())
        .ok_or_else(|| "A password needs a salt.".to_string())?;
    let hash = hash_password("SHA-512", password, &salt_bytes, SPIN)
        .ok_or_else(|| "That hash isn't known.".to_string())?;
    Ok(Some(PasswordHash::Hashed {
        algorithm: "SHA-512".to_string(),
        hash: base64.encode(hash),
        salt: base64.encode(&salt_bytes),
        spin: SPIN,
    }))
}

/// Whether a password opens what this hash keeps. No hash takes any password, as Excel does.
pub fn matches(stored: Option<&PasswordHash>, password: &str) -> bool {
    match stored {
        None => true,
        Some(PasswordHash::Legacy(hash)) => legacy_hash(password) == *hash,
        Some(PasswordHash::Hashed {
            algorithm,
            hash,
            salt,
            spin,
        }) => {
            let base64 = base64::engine::general_purpose::STANDARD;
            let (Ok(salt), Ok(hash)) = (base64.decode(salt), base64.decode(hash)) else {
                return false;
            };
            hash_password(algorithm, password, &salt, *spin).is_some_and(|made| made == hash)
        }
    }
}

impl Session {
    /// The sheet's protection, when it is protected.
    pub(crate) fn sheet_protection(&self, sheet: u32) -> Option<&SheetProtection> {
        self.book
            .sheets
            .get(sheet as usize)
            .and_then(|info| info.protection.as_ref())
            .filter(|protection| protection.sheet)
    }

    fn structure_locked(&self) -> bool {
        self.book
            .protection
            .as_ref()
            .is_some_and(|protection| protection.structure)
    }

    /// Whether every cell of an area is unlocked, as a protected sheet needs before they change.
    pub(crate) fn all_unlocked(&self, sheet: u32, area: Area) -> bool {
        let xfs = &self.book.styles.xfs;
        if xfs.iter().all(|xf| xf.locked) {
            return false;
        }
        let Some(info) = self.book.sheets.get(sheet as usize) else {
            return false;
        };
        let locked = |style: u32| xfs.get(style as usize).is_none_or(|xf| xf.locked);
        let right = area.right.min(cgs_model::MAX_COLS);
        (area.left..=right).all(|col| {
            info.drawn_style_spans(col, area.top, area.bottom)
                .iter()
                .all(|&(_, _, style)| !locked(style))
        })
    }

    /// Why protection refuses a command, if it does.
    pub(crate) fn protection_refusal(&self, command: &Command) -> Option<&'static str> {
        use Command as C;
        if self.structure_locked() {
            let structural = match command {
                C::InsertSheet { .. }
                | C::DeleteSheet { .. }
                | C::RenameSheet { .. }
                | C::SheetState { .. } => true,
                C::CreatePivot { to, .. } => matches!(to, crate::pivots::Destination::NewSheet),
                _ => false,
            };
            if structural {
                return Some(STRUCTURE);
            }
        }
        let cells = |sheet: u32, areas: &[Area]| -> Option<&'static str> {
            self.sheet_protection(sheet)?;
            (!areas.iter().all(|&area| self.all_unlocked(sheet, area))).then_some(PROTECTED)
        };
        let allowed = |sheet: u32, allow: fn(&SheetProtection) -> bool| -> Option<&'static str> {
            let protection = self.sheet_protection(sheet)?;
            (!allow(protection)).then_some(PROTECTED)
        };
        let protected = |sheet: u32| self.sheet_protection(sheet).map(|_| PROTECTED);
        let whole = |axis: AxisName, first: u32, last: u32| match axis {
            AxisName::Row => Area {
                top: first,
                left: 1,
                bottom: last,
                right: cgs_model::MAX_COLS,
            },
            AxisName::Col => Area {
                top: 1,
                left: first,
                bottom: cgs_model::MAX_ROWS,
                right: last,
            },
        };
        match command {
            C::Input {
                sheet,
                row,
                col,
                areas,
                ..
            } => {
                let mut all = areas.clone();
                all.push(Area::cell(*row, *col));
                cells(*sheet, &all)
            }
            C::ArrayInput { sheet, area, .. } => cells(*sheet, &[*area]),
            C::RichInput {
                sheet, row, col, ..
            } => cells(*sheet, &[Area::cell(*row, *col)]),
            C::Clear { sheet, areas } => cells(*sheet, areas),
            C::Fill { sheet, to, .. } => cells(*sheet, &[*to]),
            C::Paste { sheet, to, .. } => cells(*sheet, &[*to]),
            C::Move {
                from_sheet,
                from,
                sheet,
                row,
                col,
            } => cells(*from_sheet, &[*from]).or_else(|| {
                let to = Area {
                    top: *row,
                    left: *col,
                    bottom: (row + from.rows() - 1).min(cgs_model::MAX_ROWS),
                    right: (col + from.cols() - 1).min(cgs_model::MAX_COLS),
                };
                cells(*sheet, &[to])
            }),
            C::PasteExternal {
                sheet,
                row,
                col,
                text,
                ..
            } => {
                let rows = text.trim_end_matches(['\r', '\n']).lines().count().max(1) as u32;
                let cols = text
                    .lines()
                    .map(|line| line.split('\t').count())
                    .max()
                    .unwrap_or(1) as u32;
                cells(
                    *sheet,
                    &[Area {
                        top: *row,
                        left: *col,
                        bottom: (row + rows - 1).min(cgs_model::MAX_ROWS),
                        right: (col + cols - 1).min(cgs_model::MAX_COLS),
                    }],
                )
            }
            C::ReplaceOne {
                sheet, row, col, ..
            }
            | C::IncludeCells { sheet, row, col } => cells(*sheet, &[Area::cell(*row, *col)]),
            C::ConvertToNumber { sheet, areas } => cells(*sheet, areas),
            C::Format { sheet, .. } | C::CellStyle { sheet, .. } => {
                allowed(*sheet, |p| p.format_cells)
            }
            C::Lines { sheet, axis, .. } => match axis {
                AxisName::Row => allowed(*sheet, |p| p.format_rows),
                AxisName::Col => allowed(*sheet, |p| p.format_columns),
            },
            C::AutoFitRows { sheet, .. } => allowed(*sheet, |p| p.format_rows),
            C::FitCols { sheet, .. } => allowed(*sheet, |p| p.format_columns),
            C::Axis {
                sheet,
                axis,
                at,
                count,
                delete,
            } => {
                let allow: fn(&SheetProtection) -> bool = match (axis, delete) {
                    (AxisName::Row, false) => |p| p.insert_rows,
                    (AxisName::Col, false) => |p| p.insert_columns,
                    (AxisName::Row, true) => |p| p.delete_rows,
                    (AxisName::Col, true) => |p| p.delete_columns,
                };
                allowed(*sheet, allow).or_else(|| {
                    // Deleting takes cells with it, which must all be unlocked.
                    let last = at + count.max(&1) - 1;
                    if *delete {
                        cells(*sheet, &[whole(*axis, *at, last)])
                    } else {
                        None
                    }
                })
            }
            C::Sort { sheet, area, .. } => {
                allowed(*sheet, |p| p.sort).or_else(|| cells(*sheet, &[*area]))
            }
            C::SetFilter { sheet, .. }
            | C::ClearFilter { sheet, .. }
            | C::ReapplyFilter { sheet, .. } => allowed(*sheet, |p| p.auto_filter),
            C::SetLink { sheet, .. } => allowed(*sheet, |p| p.insert_hyperlinks),
            C::SetNote { sheet, .. }
            | C::NoteVisible { sheet, .. }
            | C::SetDrawingBox { sheet, .. }
            | C::DeleteDrawings { sheet, .. }
            | C::InsertChart { sheet, .. }
            | C::InsertPicture { sheet, .. }
            | C::EditChart { sheet, .. } => allowed(*sheet, |p| !p.objects),
            C::MoveDrawing { sheet, to, .. } => {
                allowed(*sheet, |p| !p.objects).or_else(|| allowed(*to, |p| !p.objects))
            }
            C::RefreshPivot { pivot } => pivot
                .as_ref()
                .and_then(|pivot| allowed(pivot.sheet, |p| p.pivot_tables)),
            C::PivotLayout { pivot, .. }
            | C::PivotOptions { pivot, .. }
            | C::PivotField { pivot, .. } => allowed(pivot.sheet, |p| p.pivot_tables),
            C::DeletePivot { pivot } => protected(pivot.sheet),
            C::CreatePivot { to, .. } => match to {
                crate::pivots::Destination::Existing { sheet, .. } => protected(*sheet),
                crate::pivots::Destination::NewSheet => None,
            },
            C::ShiftCells { sheet, .. }
            | C::Merge { sheet, .. }
            | C::Unmerge { sheet, .. }
            | C::RemoveDuplicates { sheet, .. }
            | C::SetValidation { sheet, .. }
            | C::AddCf { sheet, .. }
            | C::ClearCf { sheet, .. }
            | C::SetCf { sheet, .. }
            | C::ToggleFilter { sheet, .. }
            | C::CreateTable { sheet, .. } => protected(*sheet),
            C::TableOptions { table, .. }
            | C::TotalsFunction { table, .. }
            | C::RenameTable { table, .. }
            | C::ResizeTable { table, .. }
            | C::ConvertToRange { table } => self
                .book
                .tables
                .iter()
                .find(|found| found.name.eq_ignore_ascii_case(table))
                .and_then(|found| protected(found.sheet)),
            _ => None,
        }
    }

    /// Whether a cell may be selected, as a protected sheet's Select options say.
    pub(crate) fn selectable(&self, sheet: u32, row: u32, col: u32) -> bool {
        let Some(protection) = self.sheet_protection(sheet) else {
            return true;
        };
        let style = self.book.sheets[sheet as usize].style_at(row, col);
        let locked = self
            .book
            .styles
            .xfs
            .get(style as usize)
            .is_none_or(|xf| xf.locked);
        if locked {
            protection.select_locked
        } else {
            protection.select_unlocked
        }
    }

    /// See `Question::NextSelectable`. It looks no further than the cells the sheet holds or
    /// styles, where unlocked cells can be.
    pub(crate) fn next_selectable(
        &self,
        sheet: u32,
        row: u32,
        col: u32,
        rows: i32,
        cols: i32,
    ) -> Option<(u32, u32)> {
        if self.selectable(sheet, row, col) {
            return Some((row, col));
        }
        let protection = self.sheet_protection(sheet)?;
        if !protection.select_unlocked || (rows == 0 && cols == 0) {
            return None;
        }
        let info = self.book.sheets.get(sheet as usize)?;
        let last_row = info
            .used
            .0
            .max(info.rows.rows.last().map_or(0, |&(r, _)| r))
            .max(row)
            .min(cgs_model::MAX_ROWS);
        let last_col = info
            .used
            .1
            .max(info.styled_cols())
            .max(col)
            .min(cgs_model::MAX_COLS);
        if rows == 0 && cols > 0 {
            // Going right reads on along the rows, as Tab does in Excel, and comes round.
            let (mut r, mut c) = (row, col);
            for _ in 0..(u64::from(last_row) * u64::from(last_col)).min(4_000_000) {
                c += 1;
                if c > last_col {
                    c = 1;
                    r = if r >= last_row { 1 } else { r + 1 };
                }
                if (r, c) == (row, col) {
                    return None;
                }
                if self.selectable(sheet, r, c) {
                    return Some((r, c));
                }
            }
            return None;
        }
        let (mut r, mut c) = (i64::from(row), i64::from(col));
        loop {
            r += i64::from(rows.signum());
            c += i64::from(cols.signum());
            if r < 1 || c < 1 || r > i64::from(last_row) || c > i64::from(last_col) {
                return None;
            }
            if self.selectable(sheet, r as u32, c as u32) {
                return Some((r as u32, c as u32));
            }
        }
    }

    /// Protect Sheet, or a change to what it allows. It can't be undone and clears the undo list,
    /// as in Excel.
    pub(crate) fn protect_sheet(
        &mut self,
        sheet: u32,
        allow: AllowSpec,
        password: &str,
        salt: &str,
    ) -> Result<Applied, String> {
        let info = self
            .book
            .sheets
            .get(sheet as usize)
            .filter(|info| !info.deleted)
            .ok_or_else(|| "That sheet has been deleted.".to_string())?;
        if info.kind != cgs_model::SheetKind::Worksheet {
            return Err("Only worksheets can be protected here.".to_string());
        }
        if self.sheet_protection(sheet).is_some() {
            return Err("That sheet is already protected.".to_string());
        }
        let password = new_hash(password, salt)?;
        self.protection_step(Op::SheetProtection {
            sheet,
            protection: Some(Box::new(allow.protection(password))),
        })
    }

    pub(crate) fn unprotect_sheet(
        &mut self,
        sheet: u32,
        password: &str,
    ) -> Result<Applied, String> {
        let Some(protection) = self.sheet_protection(sheet) else {
            return Ok(Applied::default());
        };
        if !matches(protection.password.as_ref(), password) {
            return Err(WRONG_PASSWORD.to_string());
        }
        self.protection_step(Op::SheetProtection {
            sheet,
            protection: None,
        })
    }

    pub(crate) fn protect_workbook(
        &mut self,
        structure: bool,
        password: &str,
        salt: &str,
    ) -> Result<Applied, String> {
        if self
            .book
            .protection
            .as_ref()
            .is_some_and(|protection| protection.structure || protection.windows)
        {
            return Err("The workbook is already protected.".to_string());
        }
        if !structure {
            return Ok(Applied::default());
        }
        let password = new_hash(password, salt)?;
        self.protection_step(Op::BookProtection(Some(WorkbookProtection {
            structure,
            windows: false,
            password,
        })))
    }

    pub(crate) fn unprotect_workbook(&mut self, password: &str) -> Result<Applied, String> {
        let Some(protection) = self.book.protection.as_ref() else {
            return Ok(Applied::default());
        };
        if !matches(protection.password.as_ref(), password) {
            return Err(WRONG_PASSWORD.to_string());
        }
        self.protection_step(Op::BookProtection(None))
    }

    fn protection_step(&mut self, op: Op) -> Result<Applied, String> {
        self.absorb_results();
        self.apply_op(op)?;
        self.history.touch();
        self.history.forget();
        Ok(Applied::default())
    }

    pub(crate) fn apply_sheet_protection(
        &mut self,
        sheet: u32,
        protection: Option<Box<SheetProtection>>,
    ) -> Op {
        let info = &mut self.book.sheets[sheet as usize];
        let before = std::mem::replace(&mut info.protection, protection.map(|p| *p));
        self.edits.protection_sheets.insert(sheet);
        self.dirty.layouts.insert(sheet);
        Op::SheetProtection {
            sheet,
            protection: before.map(Box::new),
        }
    }

    pub(crate) fn apply_book_protection(&mut self, protection: Option<WorkbookProtection>) -> Op {
        let before = std::mem::replace(&mut self.book.protection, protection);
        self.edits.book_protection = true;
        self.dirty.book = true;
        Op::BookProtection(before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_as_excel_2016_did() {
        // Protect with "abc" in Excel 2016 over COM saved this.
        let stored = PasswordHash::Hashed {
            algorithm: "SHA-512".to_string(),
            hash: "lE+oY/K8O9RzxhHbfj+59f6OKjW/HyUpDp1tBw9LKbUXQ2NlFMdltyninyJ7dov5g+VPTaGpfgt/Z1zYKcAFyQ==".to_string(),
            salt: "mZgorKXCxX0pnb7n04x0bA==".to_string(),
            spin: 100_000,
        };
        assert!(matches(Some(&stored), "abc"));
        assert!(!matches(Some(&stored), "ABC"));
        assert!(!matches(Some(&stored), ""));
        assert_eq!(
            new_hash("abc", "mZgorKXCxX0pnb7n04x0bA==").unwrap(),
            Some(stored)
        );
        assert_eq!(new_hash("", "mZgorKXCxX0pnb7n04x0bA==").unwrap(), None);
    }

    #[test]
    fn the_old_hash_too() {
        assert_eq!(legacy_hash("test"), 0xCBEB);
        assert_eq!(legacy_hash("abc"), 0xCC1A);
        assert!(matches(Some(&PasswordHash::Legacy(0xCBEB)), "test"));
        assert!(!matches(Some(&PasswordHash::Legacy(0xCBEB)), "tests"));
    }
}
