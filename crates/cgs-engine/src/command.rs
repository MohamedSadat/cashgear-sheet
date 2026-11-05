//! Everything that changes a workbook, and undoing it. Each change is an `Op`, and applying an `Op`
//! returns the `Op` that puts things back, holding pre-images of whatever it replaced. Undo applies
//! that, which gives back the `Op` for redo. Because undoing is just more changes, the same path
//! keeps track of what differs from the file, however the workbook got to where it is.

use std::collections::{BTreeMap, HashSet};

use cgs_calc::{
    edits_bytes, CalcEngine, CalcValue, CellEdit, CellInput, CellRef, FormulaShape, NameInfo,
    StructuralOp, TableInfo,
};
use cgs_model::{DefinedName, Rect, SheetInfo, SheetKind, SheetShape, SheetState, Table};
use cgs_refs::{sheet_key, Axis, AxisEdit};
use cgs_xlsx::patch_sheet::{ArrayKind, CellChange, CellPatch};
use cgs_xlsx::structure::BookOp;
use serde::Deserialize;

use crate::edit::{EditCell, EditInput};
use crate::input::{parse_input, Format, Typed};
use crate::{Engine, Session};

/// Excel's default, and how much pre-image memory undo may hold.
const UNDO_STEPS: usize = 100;
const UNDO_BYTES: usize = 256 * 1024 * 1024;
/// Past this many cells on a sheet, a repaint asks for the whole sheet's tiles again.
const DIRTY_TILE_LIMIT: usize = 4096;

/// What a sheet's tiles and shape need from the UI after a change.
#[derive(Debug, Default)]
pub struct Dirty {
    /// Tiles by sheet and top-left cell, counted from 0.
    pub tiles: HashSet<(u32, u32, u32)>,
    /// Sheets to repaint in full, because something moved.
    pub sheets: HashSet<u32>,
    /// Sheets whose sizes or merges changed.
    pub layouts: HashSet<u32>,
    /// The list of sheets itself changed.
    pub book: bool,
}

impl Dirty {
    pub(crate) fn cell(&mut self, sheet: u32, row: u32, col: u32) {
        if self.sheets.contains(&sheet) {
            return;
        }
        let tile_row = (row - 1) / cgs_model::TILE_ROWS * cgs_model::TILE_ROWS;
        let tile_col = (col - 1) / cgs_model::TILE_COLS * cgs_model::TILE_COLS;
        self.tiles.insert((sheet, tile_row, tile_col));
        // Text can overflow into the next tile either way.
        if tile_col > 0 {
            self.tiles
                .insert((sheet, tile_row, tile_col - cgs_model::TILE_COLS));
        }
        self.tiles
            .insert((sheet, tile_row, tile_col + cgs_model::TILE_COLS));
        if self.tiles.len() > DIRTY_TILE_LIMIT {
            let sheets: HashSet<u32> = self.tiles.iter().map(|&(sheet, _, _)| sheet).collect();
            self.tiles.clear();
            self.sheets.extend(sheets);
        }
    }
}

/// An inclusive area of a sheet, 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Area {
    pub top: u32,
    pub left: u32,
    pub bottom: u32,
    pub right: u32,
}

impl Area {
    pub fn cell(row: u32, col: u32) -> Self {
        Self {
            top: row,
            left: col,
            bottom: row,
            right: col,
        }
    }

    pub fn rows(&self) -> u32 {
        self.bottom - self.top + 1
    }

    pub fn cols(&self) -> u32 {
        self.right - self.left + 1
    }

    fn contains(&self, row: u32, col: u32) -> bool {
        row >= self.top && row <= self.bottom && col >= self.left && col <= self.right
    }

    fn valid(&self) -> bool {
        self.top >= 1
            && self.left >= 1
            && self.top <= self.bottom
            && self.left <= self.right
            && self.bottom <= cgs_model::MAX_ROWS
            && self.right <= cgs_model::MAX_COLS
    }
}

/// What to paste from cells in this workbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PasteMode {
    #[default]
    All,
    Values,
    Formulas,
    Formats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AxisName {
    Row,
    Col,
}

/// A change as the UI asks for it.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Command {
    /// Text typed into a cell, read as Excel reads it. `areas` fills them all with it.
    Input {
        sheet: u32,
        row: u32,
        col: u32,
        text: String,
        #[serde(default)]
        areas: Vec<Area>,
    },
    /// `{=...}`.
    ArrayInput {
        sheet: u32,
        area: Area,
        text: String,
    },
    /// Empties cells, keeping their formats.
    Clear {
        sheet: u32,
        areas: Vec<Area>,
    },
    /// Drags a selection out over `to`, which contains it.
    Fill {
        sheet: u32,
        from: Area,
        to: Area,
        #[serde(default)]
        copy: bool,
    },
    /// Pastes cells of this workbook.
    Paste {
        #[serde(rename = "fromSheet")]
        from_sheet: u32,
        from: Area,
        sheet: u32,
        to: Area,
        #[serde(default)]
        mode: PasteMode,
    },
    /// Moves cells, and what points at them follows.
    Move {
        #[serde(rename = "fromSheet")]
        from_sheet: u32,
        from: Area,
        sheet: u32,
        row: u32,
        col: u32,
    },
    /// Pastes from another program: Excel's HTML if there is any, or else tab-separated text.
    PasteExternal {
        sheet: u32,
        row: u32,
        col: u32,
        text: String,
        #[serde(default)]
        html: Option<String>,
    },
    Axis {
        sheet: u32,
        axis: AxisName,
        at: u32,
        count: u32,
        delete: bool,
    },
    InsertSheet {
        /// Before this tab, counted from 0.
        position: u32,
        #[serde(default)]
        name: Option<String>,
    },
    DeleteSheet {
        sheet: u32,
    },
    RenameSheet {
        sheet: u32,
        name: String,
    },
    Undo,
    Redo,
}

/// One change to the workbook, and what is kept to undo it.
#[derive(Debug, Clone)]
pub(crate) enum Op {
    Cells(Vec<EditCell>),
    Axis {
        sheet: u32,
        edit: AxisEdit,
    },
    /// Rows or columns coming back or going again, with what the model looked like then.
    AxisBack {
        sheet: u32,
        edit: AxisEdit,
        shape: SheetShape,
        names: Vec<DefinedName>,
        tables: Vec<Table>,
    },
    Names(Vec<NameInfo>),
    /// The engine's tables as they were.
    Tables(Vec<TableInfo>),
    Merges {
        sheet: u32,
        merges: Vec<Rect>,
    },
    AddSheet {
        name: String,
        position: u32,
    },
    RemoveSheet {
        sheet: u32,
    },
    RestoreSheet {
        sheet: u32,
        position: u32,
        cells: Vec<CellEdit>,
        readers: Vec<CellEdit>,
        names: Vec<NameInfo>,
        tables: Vec<TableInfo>,
    },
    Rename {
        sheet: u32,
        name: String,
    },
    Many(Vec<Op>),
}

impl Op {
    fn bytes(&self) -> usize {
        match self {
            Self::Cells(edits) => edits.iter().map(EditCell::bytes).sum(),
            Self::Axis { .. } | Self::RemoveSheet { .. } | Self::Rename { .. } => 64,
            Self::AxisBack {
                shape,
                names,
                tables,
                ..
            } => shape.bytes() + names.len() * 128 + tables.len() * 256,
            Self::Names(names) => names.len() * 128,
            Self::Tables(tables) => tables.len() * 256,
            Self::Merges { merges, .. } => merges.len() * 16,
            Self::AddSheet { name, .. } => 64 + name.len(),
            Self::RestoreSheet {
                cells,
                readers,
                names,
                tables,
                ..
            } => edits_bytes(cells) + edits_bytes(readers) + (names.len() + tables.len()) * 128,
            Self::Many(list) => list.iter().map(Self::bytes).sum(),
        }
    }
}

struct Step {
    label: &'static str,
    /// What gets things back to `before`, or forward to `after` once undone.
    op: Op,
    bytes: usize,
    before: u64,
    after: u64,
    selection: Option<(u32, Area)>,
    /// How long the list of shape changes was before this step, and against which file. Undoing
    /// takes this step's changes back off the list, so that the file's own rows and sheets stand.
    log_before: usize,
    log_file: u64,
}

/// Undo and redo, and which state of the workbook is the file's.
#[derive(Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    bytes: usize,
    pub(crate) state: u64,
    pub(crate) clean: u64,
    next_state: u64,
    /// The last change couldn't be kept, so there is no going back past it.
    lost: bool,
    /// Which file the list of shape changes is against, which changes with each save.
    file: u64,
}

impl History {
    pub(crate) fn touch(&mut self) {
        self.next_state += 1;
        self.state = self.next_state;
    }

    pub(crate) fn at_clean(&self) -> bool {
        self.state == self.clean
    }

    pub(crate) fn mark_clean(&mut self) {
        self.clean = self.state;
        self.file += 1;
    }

    fn push(
        &mut self,
        label: &'static str,
        op: Op,
        before: u64,
        selection: Option<(u32, Area)>,
        log_before: usize,
    ) {
        let bytes = op.bytes();
        for step in self.redo.drain(..) {
            self.bytes -= step.bytes;
        }
        self.bytes += bytes;
        self.undo.push(Step {
            label,
            op,
            bytes,
            before,
            after: self.state,
            selection,
            log_before,
            log_file: self.file,
        });
        self.lost = false;
        while self.undo.len() > UNDO_STEPS || self.bytes > UNDO_BYTES {
            let Some(oldest) = (!self.undo.is_empty()).then(|| self.undo.remove(0)) else {
                break;
            };
            self.bytes -= oldest.bytes;
            self.lost = self.undo.is_empty();
        }
    }
}

/// What a command did, for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// The cells to select afterwards.
    pub selection: Option<(u32, Area)>,
}

fn not_open() -> String {
    "No workbook is open.".to_string()
}

fn number_only(sheet: &SheetInfo) -> Result<(), String> {
    if sheet.deleted {
        return Err("That sheet has been deleted.".to_string());
    }
    if sheet.kind != SheetKind::Worksheet || !sheet.loaded {
        return Err(format!("Sheet {} has no cells to edit.", sheet.name));
    }
    Ok(())
}

/// Whether a name can be a sheet's, as Excel rules it.
fn sheet_name_problem(book: &cgs_model::Book, name: &str, except: Option<u32>) -> Option<String> {
    if name.is_empty() {
        return Some("A sheet needs a name.".to_string());
    }
    if name.chars().count() > 31 {
        return Some("A sheet's name can't be longer than 31 characters.".to_string());
    }
    if name.contains(['\\', '/', '?', '*', '[', ']', ':']) {
        return Some("A sheet's name can't contain \\ / ? * [ ] or :.".to_string());
    }
    if name.starts_with('\'') || name.ends_with('\'') {
        return Some("A sheet's name can't start or end with an apostrophe.".to_string());
    }
    if name.eq_ignore_ascii_case("History") {
        return Some("\"History\" is a name Excel keeps for itself.".to_string());
    }
    let key = sheet_key(name);
    let taken = book.sheets.iter().enumerate().any(|(index, sheet)| {
        !sheet.deleted && Some(index as u32) != except && sheet_key(&sheet.name) == key
    });
    taken.then(|| "That name is already taken.".to_string())
}

/// A sheet's next free default name.
fn next_sheet_name(book: &cgs_model::Book) -> String {
    let mut number = book.sheets.len() + 1;
    loop {
        let name = format!("Sheet{number}");
        if sheet_name_problem(book, &name, None).is_none() {
            return name;
        }
        number += 1;
    }
}

/// `input` as a command list wants it.
fn to_edit_input(input: &CellInput) -> Option<EditInput> {
    Some(match input {
        CellInput::Value(CalcValue::Empty) | CellInput::Clear => EditInput::Clear,
        CellInput::Value(value) => EditInput::Value(value.clone()),
        CellInput::Formula { text, shape, .. } => match shape {
            FormulaShape::Normal => EditInput::Formula(text.clone()),
            shape => EditInput::Array(text.clone(), *shape),
        },
        CellInput::Spill { .. } => return None,
    })
}

impl Session {
    pub(crate) fn absorb_results(&mut self) {
        for cell in self.calc.take_changed() {
            self.note_result(cell);
        }
    }

    pub(crate) fn note_result(&mut self, cell: CellRef) {
        let Some(info) = self.book.sheets.get(cell.sheet as usize) else {
            return;
        };
        if info.deleted {
            return;
        }
        self.dirty.cell(cell.sheet, cell.row, cell.col);
        let patches = self.edits.cells.entry(cell.sheet).or_default();
        let patch = patches.entry((cell.row, cell.col)).or_insert(CellPatch {
            change: CellChange::Keep,
            style: None,
        });
        if patch.change == CellChange::Keep {
            patch.change = CellChange::Result(CalcValue::Empty);
        }
    }

    /// Whether `area` holds part of a `{=...}` array, without all of it.
    fn cuts_an_array(&self, sheet: u32, area: Area) -> bool {
        self.calc.spill_areas().iter().any(|spill| {
            if spill.sheet != sheet {
                return false;
            }
            let anchor = CellRef {
                sheet,
                row: spill.rect.top,
                col: spill.rect.left,
            };
            let legacy = matches!(self.calc.formula_shape(anchor), FormulaShape::Array(_));
            let touches = spill.rect.top <= area.bottom
                && spill.rect.bottom >= area.top
                && spill.rect.left <= area.right
                && spill.rect.right >= area.left;
            let whole = area.top <= spill.rect.top
                && area.bottom >= spill.rect.bottom
                && area.left <= spill.rect.left
                && area.right >= spill.rect.right;
            legacy && touches && !whole
        })
    }

    fn apply_cells(&mut self, edits: Vec<EditCell>) -> Result<Op, String> {
        for edit in &edits {
            let sheet = self
                .book
                .sheets
                .get(edit.sheet as usize)
                .ok_or_else(|| format!("There is no sheet {}.", edit.sheet))?;
            if sheet.deleted {
                return Err("That sheet has been deleted.".to_string());
            }
            if sheet.kind != SheetKind::Worksheet {
                return Err(format!("Sheet {} has no cells to edit.", sheet.name));
            }
            if !(1..=cgs_model::MAX_ROWS).contains(&edit.row)
                || !(1..=cgs_model::MAX_COLS).contains(&edit.col)
            {
                return Err("That cell is off the sheet.".to_string());
            }
        }
        let mut inputs = Vec::with_capacity(edits.len());
        let mut before_styles = Vec::with_capacity(edits.len());
        for edit in &edits {
            let style = edit
                .style
                .map(|_| self.book.sheets[edit.sheet as usize].style_at(edit.row, edit.col));
            before_styles.push(style);
            if let Some(input) = &edit.input {
                let input = match input {
                    EditInput::Value(CalcValue::Empty) | EditInput::Clear => CellInput::Clear,
                    EditInput::Value(value) => CellInput::Value(value.clone()),
                    EditInput::Formula(text) => CellInput::Formula {
                        text: text.clone(),
                        shape: FormulaShape::Normal,
                        cached: None,
                    },
                    EditInput::Array(text, shape) => CellInput::Formula {
                        text: text.clone(),
                        shape: *shape,
                        cached: None,
                    },
                };
                inputs.push(CellEdit {
                    at: CellRef {
                        sheet: edit.sheet,
                        row: edit.row,
                        col: edit.col,
                    },
                    input,
                });
            }
        }
        let mut previous = if inputs.is_empty() {
            Vec::new()
        } else {
            self.calc.set_cells(&inputs).map_err(|error| error.0)?
        }
        .into_iter();
        let mut undo = Vec::with_capacity(edits.len());
        for (edit, style) in edits.iter().zip(before_styles) {
            let input = if edit.input.is_some() {
                previous
                    .next()
                    .and_then(|before| to_edit_input(&before.input))
            } else {
                None
            };
            undo.push(EditCell {
                sheet: edit.sheet,
                row: edit.row,
                col: edit.col,
                input,
                style,
            });
            self.note_cell_patch(edit);
        }
        undo.reverse();
        Ok(Op::Cells(undo))
    }

    fn note_cell_patch(&mut self, edit: &EditCell) {
        let patches = self.edits.cells.entry(edit.sheet).or_default();
        let patch = patches.entry((edit.row, edit.col)).or_insert(CellPatch {
            change: CellChange::Keep,
            style: None,
        });
        match &edit.input {
            None => {}
            Some(EditInput::Clear | EditInput::Value(CalcValue::Empty)) => {
                patch.change = CellChange::Clear;
            }
            Some(EditInput::Value(value)) => patch.change = CellChange::Value(value.clone()),
            // The text and the result come from the engine when saving.
            Some(EditInput::Formula(_)) => {
                patch.change = CellChange::Formula {
                    text: String::new(),
                    value: CalcValue::Empty,
                    array: ArrayKind::None,
                };
            }
            Some(EditInput::Array(_, shape)) => {
                let array = match shape {
                    FormulaShape::Array(rect) => ArrayKind::Legacy(Rect {
                        top: rect.top,
                        left: rect.left,
                        bottom: rect.bottom,
                        right: rect.right,
                    }),
                    _ => ArrayKind::None,
                };
                patch.change = CellChange::Formula {
                    text: String::new(),
                    value: CalcValue::Empty,
                    array,
                };
            }
        }
        if edit.style.is_some() {
            patch.style = edit.style;
        }
        let info = &mut self.book.sheets[edit.sheet as usize];
        if let Some(style) = edit.style {
            info.set_style(edit.row, edit.col, style);
        }
        if !matches!(
            edit.input,
            None | Some(EditInput::Clear | EditInput::Value(CalcValue::Empty))
        ) || edit.style.is_some()
        {
            info.used.0 = info.used.0.max(edit.row);
            info.used.1 = info.used.1.max(edit.col);
        }
        self.dirty.cell(edit.sheet, edit.row, edit.col);
    }

    /// Moves a sheet's patches to their new places.
    fn move_patches(&mut self, sheet: u32, edit: &AxisEdit) {
        let Some(patches) = self.edits.cells.get_mut(&sheet) else {
            return;
        };
        let moved: BTreeMap<(u32, u32), CellPatch> = std::mem::take(patches)
            .into_iter()
            .filter_map(|((row, col), patch)| {
                let at = match edit.axis {
                    Axis::Row => (edit.point(row)?, col),
                    Axis::Col => (row, edit.point(col)?),
                };
                Some((at, patch))
            })
            .collect();
        *patches = moved;
    }

    /// Everything but the engine, which `calc.structural` has already moved.
    fn move_model(&mut self, sheet: u32, edit: &AxisEdit) {
        let name = self.book.sheets[sheet as usize].name.clone();
        self.book.sheets[sheet as usize].apply_axis(edit);
        self.move_patches(sheet, edit);
        for defined in &mut self.book.names {
            let home = defined
                .local_sheet
                .map_or("", |index| self.book.sheets[index as usize].name.as_str());
            if let std::borrow::Cow::Owned(next) =
                cgs_refs::edit_formula(&defined.formula, home, &name, edit)
            {
                defined.formula = next;
            }
        }
        self.book.tables.retain_mut(|table| {
            if table.sheet != sheet {
                return true;
            }
            let area = (
                table.range.top,
                table.range.left,
                table.range.bottom,
                table.range.right,
            );
            match cgs_refs::edit_rect(area, edit) {
                Some((top, left, bottom, right)) => {
                    table.range = Rect {
                        top,
                        left,
                        bottom,
                        right,
                    };
                    true
                }
                None => false,
            }
        });
        self.edits.ops.push(BookOp::Axis {
            sheet,
            name,
            edit: *edit,
        });
        self.dirty.sheets.insert(sheet);
        self.dirty.layouts.insert(sheet);
        self.dirty.tiles.retain(|&(on, _, _)| on != sheet);
    }

    /// Cells new rows or columns inherit a format for, as blank styled cells in the file.
    fn inherit_styles(&mut self, sheet: u32, edit: &AxisEdit) {
        if edit.delete || edit.at == 1 {
            return;
        }
        let info = &self.book.sheets[sheet as usize];
        let (mut styled, axis) = match edit.axis {
            Axis::Row => (info.own_styles_in_row(edit.at), Axis::Row),
            Axis::Col => (info.own_styles_in_col(edit.at), Axis::Col),
        };
        // A column styled all the way down would take a patch per row, which isn't worth it.
        if styled.len() > 200_000 {
            styled.clear();
        }
        let patches = self.edits.cells.entry(sheet).or_default();
        for step in 0..edit.count {
            for &(other, style) in &styled {
                let at = match axis {
                    Axis::Row => (edit.at + step, other),
                    Axis::Col => (other, edit.at + step),
                };
                if style != 0 {
                    patches.entry(at).or_insert(CellPatch {
                        change: CellChange::Keep,
                        style: Some(style),
                    });
                }
            }
        }
    }

    fn apply_axis(
        &mut self,
        sheet: u32,
        edit: AxisEdit,
        back: Option<(SheetShape, Vec<DefinedName>, Vec<Table>)>,
    ) -> Result<Op, String> {
        let info = self
            .book
            .sheets
            .get(sheet as usize)
            .ok_or_else(|| format!("There is no sheet {sheet}."))?;
        number_only(info)?;
        if edit.count == 0 || edit.at == 0 || edit.at > edit.axis.max() {
            return Err("That's off the sheet.".to_string());
        }
        // Excel refuses to split a `{=...}` array this way.
        let area = match edit.axis {
            Axis::Row => Area {
                top: edit.at,
                left: 1,
                bottom: (edit.at + edit.count - 1).min(edit.axis.max()),
                right: cgs_model::MAX_COLS,
            },
            Axis::Col => Area {
                top: 1,
                left: edit.at,
                bottom: cgs_model::MAX_ROWS,
                right: (edit.at + edit.count - 1).min(edit.axis.max()),
            },
        };
        if back.is_none() && self.array_blocks(sheet, area, &edit) {
            return Err("You can't change part of an array.".to_string());
        }
        // A table can't lose its headings and keep its data.
        if back.is_none() && edit.delete && edit.axis == Axis::Row {
            let band = (edit.at, edit.at + edit.count - 1);
            let splits = self.calc.tables().iter().any(|table| {
                table.sheet == sheet
                    && table.header_row
                    && table.range.top >= band.0
                    && table.range.top <= band.1
                    && table.range.bottom > band.1
            });
            if splits {
                return Err("This would delete a table's headings and not its data.".to_string());
            }
        }
        let shape = info.shape();
        let names_before = self.book.names.clone();
        let tables_before = self.book.tables.clone();
        // Styled blanks in what a delete takes come back as patches.
        let mut styled = Vec::new();
        if edit.delete {
            let info = &self.book.sheets[sheet as usize];
            match edit.axis {
                Axis::Row => {
                    for row in edit.at..edit.at + edit.count {
                        if row > info.used.0 {
                            break;
                        }
                        for (col, style) in info.own_styles_in_row(row) {
                            styled.push((row, col, style));
                        }
                    }
                }
                Axis::Col => {
                    for col in edit.at..edit.at + edit.count {
                        if col > info.used.1 {
                            break;
                        }
                        for (row, style) in info.own_styles_in_col(col) {
                            styled.push((row, col, style));
                        }
                    }
                }
            }
        }
        let undo = self
            .calc
            .structural(&StructuralOp::new(sheet, edit))
            .map_err(|error| error.0)?;
        self.move_model(sheet, &edit);
        if !undo.gone_tables.is_empty() {
            self.edits.ops.push(BookOp::DeleteTables {
                names: undo.gone_tables.clone(),
            });
        }
        for (table, columns) in &undo.gone_columns {
            self.edits.ops.push(BookOp::DeleteTableColumns {
                table: table.clone(),
                columns: columns.clone(),
            });
        }
        // A column an insert puts inside a table gets a heading.
        let heading_edits: Vec<EditCell> = undo
            .new_headers
            .iter()
            .map(|(at, name)| EditCell {
                sheet: at.sheet,
                row: at.row,
                col: at.col,
                input: Some(EditInput::Value(CalcValue::Text(name.clone()))),
                style: None,
            })
            .collect();
        if !heading_edits.is_empty() {
            self.apply_cells(heading_edits)?;
        }
        match back {
            Some((shape, names, tables)) => {
                self.book.sheets[sheet as usize].restore_shape(shape);
                self.book.names = names;
                self.book.tables = tables;
            }
            None => self.inherit_styles(sheet, &edit),
        }
        // What was in the band, and formulas changed by more than position, as they were.
        let mut held: BTreeMap<(u32, u32, u32), EditCell> = BTreeMap::new();
        for removed in &undo.removed {
            if let Some(input) = to_edit_input(&removed.input) {
                held.insert(
                    (removed.at.sheet, removed.at.row, removed.at.col),
                    EditCell {
                        sheet: removed.at.sheet,
                        row: removed.at.row,
                        col: removed.at.col,
                        input: Some(input),
                        style: None,
                    },
                );
            }
        }
        for (row, col, style) in styled {
            let entry = held.entry((sheet, row, col)).or_insert(EditCell {
                sheet,
                row,
                col,
                input: None,
                style: None,
            });
            entry.style = Some(style);
        }
        let mut list = vec![Op::AxisBack {
            sheet,
            edit: edit.inverse(),
            shape,
            names: names_before,
            tables: tables_before,
        }];
        if !held.is_empty() {
            list.push(Op::Cells(held.into_values().collect()));
        }
        if !undo.names.is_empty() {
            list.push(Op::Names(undo.names));
        }
        list.push(Op::Tables(undo.tables));
        Ok(Op::Many(list))
    }

    /// Whether a change to rows or columns would split a `{=...}` array. Inserting exactly at its
    /// edge doesn't.
    fn array_blocks(&self, sheet: u32, area: Area, edit: &AxisEdit) -> bool {
        self.calc.spill_areas().iter().any(|spill| {
            if spill.sheet != sheet {
                return false;
            }
            let anchor = CellRef {
                sheet,
                row: spill.rect.top,
                col: spill.rect.left,
            };
            if !matches!(self.calc.formula_shape(anchor), FormulaShape::Array(_)) {
                return false;
            }
            let (first, last, from, to) = match edit.axis {
                Axis::Row => (spill.rect.top, spill.rect.bottom, area.top, area.bottom),
                Axis::Col => (spill.rect.left, spill.rect.right, area.left, area.right),
            };
            if edit.delete {
                from <= last && to >= first && !(from <= first && to >= last)
            } else {
                edit.at > first && edit.at <= last
            }
        })
    }

    fn apply_op(&mut self, op: Op) -> Result<Op, String> {
        match op {
            Op::Cells(edits) => self.apply_cells(edits),
            Op::Axis { sheet, edit } => self.apply_axis(sheet, edit, None),
            Op::AxisBack {
                sheet,
                edit,
                shape,
                names,
                tables,
            } => self.apply_axis(sheet, edit, Some((shape, names, tables))),
            Op::Names(names) => {
                self.calc.restore_names(&names).map_err(|error| error.0)?;
                // Their positions come back through the inverse of the edit that moved them.
                Ok(Op::Names(names))
            }
            Op::Tables(tables) => {
                let before = self.calc.tables().to_vec();
                self.calc.restore_tables(&tables).map_err(|error| error.0)?;
                for table in &tables {
                    if let Some(entry) = self
                        .book
                        .tables
                        .iter_mut()
                        .find(|entry| entry.name == table.name)
                    {
                        entry.columns = table.columns.clone();
                    }
                }
                Ok(Op::Tables(before))
            }
            Op::Merges { sheet, merges } => {
                let info = &mut self.book.sheets[sheet as usize];
                let before = std::mem::replace(&mut info.merges, merges);
                info.index_merges();
                self.edits.merges.insert(sheet);
                self.dirty.sheets.insert(sheet);
                self.dirty.layouts.insert(sheet);
                Ok(Op::Merges {
                    sheet,
                    merges: before,
                })
            }
            Op::AddSheet { name, position } => {
                let sheet = self.calc.add_sheet(&name).map_err(|error| error.0)?;
                let first = self.book.sheets.first();
                let mut info =
                    SheetInfo::new(name, SheetState::Visible, SheetKind::Worksheet, None);
                info.rows.default_px = first.map_or(20, |sheet| sheet.rows.default_px);
                info.cols.default_px = first.map_or(64, |sheet| sheet.cols.default_px);
                info.loaded = true;
                let mut order = self.book.tab_order();
                order.insert((position as usize).min(order.len()), sheet);
                self.book.sheets.push(info);
                self.book.order = order;
                self.edits.ops.push(BookOp::Insert { sheet });
                self.dirty.book = true;
                Ok(Op::RemoveSheet { sheet })
            }
            Op::RemoveSheet { sheet } => {
                let position = self
                    .book
                    .tab_order()
                    .iter()
                    .position(|&other| other == sheet)
                    .unwrap_or(0) as u32;
                let undo = self.calc.delete_sheet(sheet).map_err(|error| error.0)?;
                let name = self.book.sheets[sheet as usize].name.clone();
                let mut order = self.book.tab_order();
                order.retain(|&other| other != sheet);
                self.book.order = order;
                self.book.sheets[sheet as usize].deleted = true;
                self.edits.cells.remove(&sheet);
                self.edits.ops.push(BookOp::Delete { sheet, name });
                if !undo.gone_tables.is_empty() {
                    self.edits.ops.push(BookOp::DeleteTables {
                        names: undo.gone_tables.clone(),
                    });
                }
                for reader in &undo.readers {
                    let patches = self.edits.cells.entry(reader.at.sheet).or_default();
                    patches.insert(
                        (reader.at.row, reader.at.col),
                        CellPatch {
                            change: CellChange::Formula {
                                text: String::new(),
                                value: CalcValue::Empty,
                                array: ArrayKind::None,
                            },
                            style: None,
                        },
                    );
                    self.dirty
                        .cell(reader.at.sheet, reader.at.row, reader.at.col);
                }
                self.dirty.book = true;
                Ok(Op::RestoreSheet {
                    sheet,
                    position,
                    cells: undo.cells,
                    readers: undo.readers,
                    names: undo.names,
                    tables: undo.tables,
                })
            }
            Op::RestoreSheet {
                sheet,
                position,
                cells,
                readers,
                names,
                tables,
            } => {
                self.calc.restore_sheet(sheet).map_err(|error| error.0)?;
                self.book.sheets[sheet as usize].deleted = false;
                let mut order = self.book.tab_order();
                order.retain(|&other| other != sheet);
                order.insert((position as usize).min(order.len()), sheet);
                self.book.order = order;
                self.dirty.book = true;
                self.dirty.sheets.insert(sheet);
                let to_edits = |list: Vec<CellEdit>| -> Vec<EditCell> {
                    list.iter()
                        .filter_map(|edit| {
                            Some(EditCell {
                                sheet: edit.at.sheet,
                                row: edit.at.row,
                                col: edit.at.col,
                                input: Some(to_edit_input(&edit.input)?),
                                style: None,
                            })
                        })
                        .collect()
                };
                self.apply_cells(to_edits(cells))?;
                self.apply_cells(to_edits(readers))?;
                self.calc.restore_names(&names).map_err(|error| error.0)?;
                self.calc.restore_tables(&tables).map_err(|error| error.0)?;
                Ok(Op::RemoveSheet { sheet })
            }
            Op::Rename { sheet, name } => {
                let old = self
                    .calc
                    .rename_sheet(sheet, &name)
                    .map_err(|error| error.0)?;
                self.book.sheets[sheet as usize].name = name.clone();
                for defined in &mut self.book.names {
                    if let std::borrow::Cow::Owned(next) =
                        cgs_refs::rename_in_formula(&defined.formula, &old, &name)
                    {
                        defined.formula = next;
                    }
                }
                self.edits.ops.push(BookOp::Rename {
                    sheet,
                    old: old.clone(),
                    new: name,
                });
                self.dirty.book = true;
                self.dirty.sheets.extend(0..self.book.sheets.len() as u32);
                Ok(Op::Rename { sheet, name: old })
            }
            Op::Many(list) => {
                let mut undo = Vec::with_capacity(list.len());
                for op in list {
                    undo.push(self.apply_op(op)?);
                }
                undo.reverse();
                Ok(Op::Many(undo))
            }
        }
    }

    /// Applies cells that are part of what opened, with no undo step.
    pub(crate) fn apply_loaded(&mut self, edits: Vec<EditCell>) -> Result<Op, String> {
        self.apply_cells(edits)
    }

    /// Reads typed text as it lands in one cell.
    pub(crate) fn typed_edit_at(&mut self, sheet: u32, at: (u32, u32), typed: &Typed) -> EditCell {
        self.typed_edit(sheet, at.0, at.1, typed, at)
    }

    /// The table a cell is a heading of, with its column's place.
    fn heading_at(&self, sheet: u32, row: u32, col: u32) -> Option<(usize, usize)> {
        self.calc
            .tables()
            .iter()
            .enumerate()
            .find_map(|(index, table)| {
                (table.sheet == sheet
                    && table.header_row
                    && table.range.top == row
                    && col >= table.range.left
                    && col <= table.range.right)
                    .then(|| (index, (col - table.range.left) as usize))
            })
    }

    /// A table's headings are always text, and always different: this makes edits to them so, and
    /// carries a new name into the table and the formulas that use it.
    fn with_table_headings(&mut self, op: Op) -> Result<Op, String> {
        if self.calc.tables().is_empty() {
            return Ok(op);
        }
        let mut list = match op {
            Op::Many(list) => list,
            other => vec![other],
        };
        let mut tables = self.calc.tables().to_vec();
        let mut renames: Vec<(String, String, String)> = Vec::new();
        for item in &mut list {
            let Op::Cells(edits) = item else {
                continue;
            };
            for edit in edits.iter_mut() {
                let Some((index, column)) = self.heading_at(edit.sheet, edit.row, edit.col) else {
                    continue;
                };
                let Some(input) = &edit.input else {
                    continue;
                };
                let wanted = match input {
                    EditInput::Clear | EditInput::Value(CalcValue::Empty) => String::new(),
                    EditInput::Value(CalcValue::Text(text)) => text.clone(),
                    EditInput::Value(CalcValue::Number(number)) => {
                        cgs_xlsx::patch_sheet::number_text(*number)
                    }
                    EditInput::Value(CalcValue::Bool(flag)) => {
                        if *flag { "TRUE" } else { "FALSE" }.to_string()
                    }
                    EditInput::Value(CalcValue::Error(text)) => text.clone(),
                    // A heading can't be a formula, and Excel leaves what it would be as 0.
                    EditInput::Formula(_) | EditInput::Array(..) => "0".to_string(),
                };
                let table = &mut tables[index];
                let clash = |name: &str, columns: &[String]| {
                    columns
                        .iter()
                        .enumerate()
                        .any(|(at, other)| at != column && other.eq_ignore_ascii_case(name))
                };
                let name = if wanted.is_empty() {
                    let mut number = 1;
                    loop {
                        let name = format!("Column{number}");
                        if !clash(&name, &table.columns) {
                            break name;
                        }
                        number += 1;
                    }
                } else if clash(&wanted, &table.columns) {
                    let mut number = 2;
                    loop {
                        let name = format!("{wanted}{number}");
                        if !clash(&name, &table.columns) {
                            break name;
                        }
                        number += 1;
                    }
                } else {
                    wanted
                };
                edit.input = Some(EditInput::Value(CalcValue::Text(name.clone())));
                let old = std::mem::replace(&mut table.columns[column], name.clone());
                if !old.eq_ignore_ascii_case(&name) || old != name {
                    renames.push((table.name.clone(), old, name));
                }
            }
        }
        if renames.is_empty() {
            return Ok(Op::Many(list));
        }
        // Formulas follow the new names.
        let mut rewrites = Vec::new();
        let formulas = self.calc.all_formulas().map_err(|error| error.0)?;
        for (at, text) in formulas {
            let mut next = std::borrow::Cow::Borrowed(text.as_str());
            for (table_name, old, new) in &renames {
                let inside = tables.iter().any(|table| {
                    table.name == *table_name
                        && table.sheet == at.sheet
                        && table.range.contains(at.row, at.col)
                });
                if let std::borrow::Cow::Owned(changed) =
                    cgs_refs::rename_table_column(&next, table_name, old, new, inside)
                {
                    next = std::borrow::Cow::Owned(changed);
                }
            }
            if let std::borrow::Cow::Owned(changed) = next {
                rewrites.push(EditCell {
                    sheet: at.sheet,
                    row: at.row,
                    col: at.col,
                    input: Some(EditInput::Formula(changed)),
                    style: None,
                });
            }
        }
        // The table first, so that the new text finds its column.
        let mut all = vec![Op::Tables(tables)];
        all.extend(list);
        if !rewrites.is_empty() {
            all.push(Op::Cells(rewrites));
        }
        Ok(Op::Many(all))
    }

    /// Applies a change and remembers how to undo it.
    fn perform(
        &mut self,
        label: &'static str,
        op: Op,
        selection: Option<(u32, Area)>,
    ) -> Result<Applied, String> {
        self.absorb_results();
        let op = self.with_table_headings(op)?;
        let before = self.history.state;
        let log_before = self.edits.ops.len();
        let inverse = self.apply_op(op)?;
        self.history.touch();
        self.history
            .push(label, inverse, before, selection, log_before);
        Ok(Applied { selection })
    }

    fn step_back(&mut self) -> Result<Applied, String> {
        self.absorb_results();
        let Some(mut step) = self.history.undo.pop() else {
            return Ok(Applied::default());
        };
        self.history.bytes -= step.bytes;
        let op = std::mem::replace(&mut step.op, Op::Many(Vec::new()));
        let inverse = self.apply_op(op)?;
        step.bytes = inverse.bytes();
        step.op = inverse;
        self.history.bytes += step.bytes;
        self.history.state = step.before;
        let selection = step.selection;
        // Against the same file, this step's changes come off the list: what they did to the file
        // is undone by not doing it. Against a newer file, they stay as changes to that file.
        if step.log_file == self.history.file {
            self.edits.ops.truncate(step.log_before);
        }
        self.history.redo.push(step);
        self.arrive_at_state();
        Ok(Applied { selection })
    }

    fn step_forward(&mut self) -> Result<Applied, String> {
        self.absorb_results();
        let Some(mut step) = self.history.redo.pop() else {
            return Ok(Applied::default());
        };
        self.history.bytes -= step.bytes;
        let op = std::mem::replace(&mut step.op, Op::Many(Vec::new()));
        step.log_before = self.edits.ops.len();
        step.log_file = self.history.file;
        let inverse = self.apply_op(op)?;
        step.bytes = inverse.bytes();
        step.op = inverse;
        self.history.bytes += step.bytes;
        self.history.state = step.after;
        let selection = step.selection;
        self.history.undo.push(step);
        self.arrive_at_state();
        Ok(Applied { selection })
    }

    /// Back at the file's own state, nothing differs from it.
    fn arrive_at_state(&mut self) {
        if self.history.at_clean() {
            let styles = self.edits.styles.take();
            self.edits = Default::default();
            self.edits.styles = styles;
        }
    }

    /// A cell format like `base`, with this number format, made at most once.
    fn style_with_format(&mut self, base: u32, format: Format) -> u32 {
        let key = (base, format);
        if let Some(&style) = self.format_styles.get(&key) {
            return style;
        }
        let new = crate::edit::NewStyle {
            number_format: match format {
                Format::Code(code) => Some(code.to_string()),
                Format::Builtin(_) => None,
            },
            number_format_id: match format {
                Format::Builtin(id) => Some(id),
                Format::Code(_) => None,
            },
            ..Default::default()
        };
        let style = self.add_style(base, &new);
        self.format_styles.insert(key, style);
        style
    }

    /// One cell's edit from typed text. `home` is where the formula was written, for filling it
    /// into other cells. The text can be a bad formula, which only `check_formula` can tell.
    fn typed_edit(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        typed: &Typed,
        home: (u32, u32),
    ) -> EditCell {
        let (input, style) = match typed {
            Typed::Clear => (EditInput::Clear, None),
            Typed::Value(value) => (EditInput::Value(value.clone()), None),
            Typed::Formula(text) => {
                let text = if (row, col) == home {
                    text.clone()
                } else {
                    cgs_refs::moved_formula(
                        text,
                        i64::from(row) - i64::from(home.0),
                        i64::from(col) - i64::from(home.1),
                    )
                };
                (EditInput::Formula(text), None)
            }
            Typed::Formatted(value, format) => {
                // A cell with a format of its own keeps it, as in Excel.
                let base = self.book.sheets[sheet as usize].style_at(row, col);
                let general = self.book.styles.xf(base).num_fmt == 0;
                let style = general.then(|| self.style_with_format(base, *format));
                (EditInput::Value(CalcValue::Number(*value)), style)
            }
        };
        EditCell {
            sheet,
            row,
            col,
            input: Some(input),
            style,
        }
    }

    fn input_command(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        text: &str,
        areas: &[Area],
        locale: &cgs_numfmt::Locale,
    ) -> Result<Applied, String> {
        let mut typed = parse_input(text, locale, self.book.date1904, self.today);
        // A table's heading keeps what was typed as it was typed.
        if areas.is_empty()
            && self.heading_at(sheet, row, col).is_some()
            && matches!(typed, Typed::Value(_) | Typed::Formatted(..))
        {
            typed = Typed::Value(CalcValue::Text(
                text.strip_prefix('\'').unwrap_or(text).to_string(),
            ));
        }
        if let Typed::Formula(formula) = &typed {
            let fixed = self.calc.check_formula(formula).map_err(|error| error.0)?;
            typed = Typed::Formula(fixed);
        }
        let mut edits = Vec::new();
        if areas.is_empty() {
            if self.cuts_an_array(sheet, Area::cell(row, col)) {
                return Err("You can't change part of an array.".to_string());
            }
            edits.push(self.typed_edit(sheet, row, col, &typed, (row, col)));
        } else {
            for area in areas {
                if !area.valid() {
                    return Err("That's off the sheet.".to_string());
                }
                if self.cuts_an_array(sheet, *area) {
                    return Err("You can't change part of an array.".to_string());
                }
                for r in area.top..=area.bottom {
                    for c in area.left..=area.right {
                        edits.push(self.typed_edit(sheet, r, c, &typed, (row, col)));
                    }
                }
            }
        }
        let area = areas.first().copied().unwrap_or(Area::cell(row, col));
        self.perform("typing", Op::Cells(edits), Some((sheet, area)))
    }

    /// Cells that hold something in an area, which can be whole rows or columns.
    fn filled_cells(&self, sheet: u32, area: Area) -> Vec<(u32, u32)> {
        let info = &self.book.sheets[sheet as usize];
        let bottom = area.bottom.min(info.used.0.max(1));
        let right = area.right.min(info.used.1.max(1));
        if area.top > bottom || area.left > right {
            return Vec::new();
        }
        let mut found = crate::edit::CollectCells::default();
        self.calc.read_block(
            cgs_calc::SheetRect {
                sheet,
                rect: cgs_calc::Rect {
                    top: area.top,
                    left: area.left,
                    bottom,
                    right,
                },
            },
            &mut found,
        );
        found.0
    }

    fn clear_command(&mut self, sheet: u32, areas: &[Area]) -> Result<Applied, String> {
        let spills = self.calc.spill_areas();
        let mut edits = Vec::new();
        for area in areas {
            if !area.valid() {
                return Err("That's off the sheet.".to_string());
            }
            if self.cuts_an_array(sheet, *area) {
                return Err("You can't change part of an array.".to_string());
            }
            for (row, col) in self.filled_cells(sheet, *area) {
                // Clearing what an array spills does nothing; its anchor owns it.
                let spilled = spills.iter().any(|spill| {
                    spill.sheet == sheet
                        && spill.rect.contains(row, col)
                        && (spill.rect.top, spill.rect.left) != (row, col)
                });
                if !spilled {
                    edits.push(EditCell {
                        sheet,
                        row,
                        col,
                        input: Some(EditInput::Clear),
                        style: None,
                    });
                }
            }
        }
        let selection = areas.first().map(|area| (sheet, *area));
        self.perform("clearing", Op::Cells(edits), selection)
    }

    fn array_command(&mut self, sheet: u32, area: Area, text: &str) -> Result<Applied, String> {
        if !area.valid() {
            return Err("That's off the sheet.".to_string());
        }
        let text = text.strip_prefix('=').unwrap_or(text);
        let text = self.calc.check_formula(text).map_err(|error| error.0)?;
        if self.cuts_an_array(sheet, area) {
            return Err("You can't change part of an array.".to_string());
        }
        let rect = cgs_calc::Rect {
            top: area.top,
            left: area.left,
            bottom: area.bottom,
            right: area.right,
        };
        let edit = EditCell {
            sheet,
            row: area.top,
            col: area.left,
            input: Some(EditInput::Array(text, FormulaShape::Array(rect))),
            style: None,
        };
        self.perform("array formula", Op::Cells(vec![edit]), Some((sheet, area)))
    }

    /// The `{=...}` array a cell belongs to: its anchor's formula and its area.
    fn legacy_array_at(&self, sheet: u32, row: u32, col: u32) -> Option<(String, cgs_calc::Rect)> {
        let area = self.calc.spill_areas().into_iter().find(|area| {
            area.sheet == sheet
                && area.rect.contains(row, col)
                && matches!(
                    self.calc.formula_shape(CellRef {
                        sheet,
                        row: area.rect.top,
                        col: area.rect.left,
                    }),
                    FormulaShape::Array(_)
                )
        })?;
        let text = self.calc.formula_text(CellRef {
            sheet,
            row: area.rect.top,
            col: area.rect.left,
        })?;
        Some((text, area.rect))
    }

    /// What copying one cell to another place would put there.
    pub(crate) fn copied_input(
        &self,
        sheet: u32,
        row: u32,
        col: u32,
        rows: i64,
        cols: i64,
        mode: PasteMode,
    ) -> Option<EditInput> {
        let at = CellRef { sheet, row, col };
        // A cell of a `{=...}` array copies as an array of its own, as in Excel. Only the whole
        // array can stay one, which `paste_command` sees to.
        if mode != PasteMode::Values {
            if let Some((text, _)) = self.legacy_array_at(sheet, row, col) {
                let (new_row, new_col) = (
                    (i64::from(row) + rows) as u32,
                    (i64::from(col) + cols) as u32,
                );
                return Some(EditInput::Array(
                    cgs_refs::moved_formula(&text, rows, cols),
                    FormulaShape::Array(cgs_calc::Rect {
                        top: new_row,
                        left: new_col,
                        bottom: new_row,
                        right: new_col,
                    }),
                ));
            }
        }
        let input = self.calc.cell_input(at);
        Some(match input {
            CellInput::Formula { text, cached, .. } => match mode {
                PasteMode::Values => match cached {
                    None | Some(CalcValue::Empty) => EditInput::Clear,
                    Some(value) => EditInput::Value(value),
                },
                _ => EditInput::Formula(cgs_refs::moved_formula(&text, rows, cols)),
            },
            CellInput::Value(value) => EditInput::Value(value),
            CellInput::Clear => EditInput::Clear,
            CellInput::Spill { .. } => return None,
        })
    }

    /// Whether a copied cell belongs to an array the copied area holds all of.
    fn whole_array_copy(&self, sheet: u32, from: Area, row: u32, col: u32) -> Option<WholeArray> {
        let (text, rect) = self.legacy_array_at(sheet, row, col)?;
        let whole = from.top <= rect.top
            && from.left <= rect.left
            && from.bottom >= rect.bottom
            && from.right >= rect.right;
        if !whole {
            return None;
        }
        Some(if (rect.top, rect.left) == (row, col) {
            WholeArray::Anchor(text, rect)
        } else {
            WholeArray::Inside
        })
    }

    fn paste_command(
        &mut self,
        from_sheet: u32,
        from: Area,
        sheet: u32,
        to: Area,
        mode: PasteMode,
    ) -> Result<Applied, String> {
        if !from.valid() || !to.valid() {
            return Err("That's off the sheet.".to_string());
        }
        for index in [from_sheet, sheet] {
            let info = self
                .book
                .sheets
                .get(index as usize)
                .ok_or_else(|| format!("There is no sheet {index}."))?;
            number_only(info)?;
        }
        // A destination that holds the source a whole number of times gets it that many times.
        let across = (to.cols() / from.cols()).max(1);
        let down = (to.rows() / from.rows()).max(1);
        let fits = to.cols().is_multiple_of(from.cols()) && to.rows().is_multiple_of(from.rows());
        let (across, down) = if fits { (across, down) } else { (1, 1) };
        let target = Area {
            top: to.top,
            left: to.left,
            bottom: to.top + from.rows() * down - 1,
            right: to.left + from.cols() * across - 1,
        };
        if !target.valid() {
            return Err("That doesn't fit on the sheet.".to_string());
        }
        if self.cuts_an_array(sheet, target) {
            return Err("You can't change part of an array.".to_string());
        }
        let mut edits = Vec::with_capacity((from.rows() * from.cols() * across * down) as usize);
        let source = &self.book.sheets[from_sheet as usize];
        let source_merges: Vec<Rect> = source
            .merges
            .iter()
            .filter(|merge| {
                from.contains(merge.top, merge.left) && from.contains(merge.bottom, merge.right)
            })
            .copied()
            .collect();
        for repeat_down in 0..down {
            for repeat_across in 0..across {
                let top = to.top + repeat_down * from.rows();
                let left = to.left + repeat_across * from.cols();
                for row in from.top..=from.bottom {
                    for col in from.left..=from.right {
                        let rows = i64::from(top + (row - from.top)) - i64::from(row);
                        let cols = i64::from(left + (col - from.left)) - i64::from(col);
                        let new_row = (i64::from(row) + rows) as u32;
                        let new_col = (i64::from(col) + cols) as u32;
                        let input = match mode {
                            PasteMode::Formats => None,
                            // A table's heading takes what a formula comes to.
                            _ if self.heading_at(sheet, new_row, new_col).is_some() => self
                                .copied_input(from_sheet, row, col, rows, cols, PasteMode::Values),
                            _ => match self.whole_array_copy(from_sheet, from, row, col) {
                                Some(WholeArray::Anchor(text, rect))
                                    if mode != PasteMode::Values =>
                                {
                                    Some(EditInput::Array(
                                        cgs_refs::moved_formula(&text, rows, cols),
                                        FormulaShape::Array(cgs_calc::Rect {
                                            top: new_row,
                                            left: new_col,
                                            bottom: new_row + (rect.bottom - rect.top),
                                            right: new_col + (rect.right - rect.left),
                                        }),
                                    ))
                                }
                                Some(WholeArray::Inside) if mode != PasteMode::Values => None,
                                _ => self.copied_input(from_sheet, row, col, rows, cols, mode),
                            },
                        };
                        let style = match mode {
                            PasteMode::All | PasteMode::Formats => {
                                Some(self.book.sheets[from_sheet as usize].style_at(row, col))
                            }
                            _ => None,
                        };
                        edits.push(EditCell {
                            sheet,
                            row: new_row,
                            col: new_col,
                            input,
                            style,
                        });
                    }
                }
            }
        }
        let mut list = vec![Op::Cells(edits)];
        if matches!(mode, PasteMode::All | PasteMode::Formats) {
            let info = &self.book.sheets[sheet as usize];
            let mut merges: Vec<Rect> = info
                .merges
                .iter()
                .filter(|merge| {
                    !(merge.top <= target.bottom
                        && merge.bottom >= target.top
                        && merge.left <= target.right
                        && merge.right >= target.left)
                })
                .copied()
                .collect();
            let before = merges.len();
            for repeat_down in 0..down {
                for repeat_across in 0..across {
                    for merge in &source_merges {
                        let shift_rows = to.top + repeat_down * from.rows() - from.top;
                        let shift_cols = to.left + repeat_across * from.cols() - from.left;
                        merges.push(Rect {
                            top: merge.top + shift_rows,
                            left: merge.left + shift_cols,
                            bottom: merge.bottom + shift_rows,
                            right: merge.right + shift_cols,
                        });
                    }
                }
            }
            if merges.len() != before || before != info.merges.len() {
                list.push(Op::Merges { sheet, merges });
            }
        }
        self.perform("pasting", Op::Many(list), Some((sheet, target)))
    }

    fn move_command(
        &mut self,
        from_sheet: u32,
        from: Area,
        sheet: u32,
        row: u32,
        col: u32,
    ) -> Result<Applied, String> {
        if !from.valid() {
            return Err("That's off the sheet.".to_string());
        }
        let target = Area {
            top: row,
            left: col,
            bottom: row + from.rows() - 1,
            right: col + from.cols() - 1,
        };
        if !target.valid() {
            return Err("That doesn't fit on the sheet.".to_string());
        }
        if (from_sheet, from) == (sheet, target) {
            return Ok(Applied {
                selection: Some((sheet, target)),
            });
        }
        if self.cuts_an_array(from_sheet, from) || self.cuts_an_array(sheet, target) {
            return Err("You can't change part of an array.".to_string());
        }
        let mut edits: BTreeMap<(u32, u32, u32), EditCell> = BTreeMap::new();
        // Formulas anywhere that point at what moves follow it.
        let from_name = self.book.sheets[from_sheet as usize].name.clone();
        let to_name = self.book.sheets[sheet as usize].name.clone();
        let change = cgs_refs::MovedCells {
            from_sheet: &from_name,
            area: (from.top, from.left, from.bottom, from.right),
            to_sheet: &to_name,
            rows: i64::from(row) - i64::from(from.top),
            cols: i64::from(col) - i64::from(from.left),
        };
        for (at, text) in self.calc.all_formulas().map_err(|error| error.0)? {
            let home = self.book.sheets[at.sheet as usize].name.as_str();
            if let std::borrow::Cow::Owned(next) =
                cgs_refs::follow_moved_cells(&text, home, &change)
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
        // What moves, as read after those changes, emptied where it was and written where it goes.
        let mut moved = Vec::new();
        for r in from.top..=from.bottom {
            for c in from.left..=from.right {
                let source = CellRef {
                    sheet: from_sheet,
                    row: r,
                    col: c,
                };
                let input = edits
                    .get(&(from_sheet, r, c))
                    .and_then(|edit| edit.input.clone())
                    .or_else(|| to_edit_input(&self.calc.cell_input(source)))
                    .unwrap_or(EditInput::Clear);
                let style = self.book.sheets[from_sheet as usize].style_at(r, c);
                moved.push((r, c, input, style));
            }
        }
        for (r, c, _, _) in &moved {
            edits.insert(
                (from_sheet, *r, *c),
                EditCell {
                    sheet: from_sheet,
                    row: *r,
                    col: *c,
                    input: Some(EditInput::Clear),
                    style: Some(0),
                },
            );
        }
        for (r, c, input, style) in moved {
            let (new_row, new_col) = (row + (r - from.top), col + (c - from.left));
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
        self.perform(
            "moving",
            Op::Cells(edits.into_values().collect()),
            Some((sheet, target)),
        )
    }

    /// Runs a command the UI asked for.
    pub(crate) fn command(
        &mut self,
        command: Command,
        locale: &cgs_numfmt::Locale,
    ) -> Result<Applied, String> {
        if let Some(reason) = &self.cannot_edit {
            return Err(format!("This workbook can't be edited here: {reason}"));
        }
        match command {
            Command::Input {
                sheet,
                row,
                col,
                text,
                areas,
            } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                self.input_command(sheet, row, col, &text, &areas, locale)
            }
            Command::ArrayInput { sheet, area, text } => self.array_command(sheet, area, &text),
            Command::Clear { sheet, areas } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                self.clear_command(sheet, &areas)
            }
            Command::Fill {
                sheet,
                from,
                to,
                copy,
            } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                let edits = self.fill_edits(sheet, from, to, copy)?;
                self.perform("filling", Op::Cells(edits), Some((sheet, to)))
            }
            Command::Paste {
                from_sheet,
                from,
                sheet,
                to,
                mode,
            } => self.paste_command(from_sheet, from, sheet, to, mode),
            Command::Move {
                from_sheet,
                from,
                sheet,
                row,
                col,
            } => self.move_command(from_sheet, from, sheet, row, col),
            Command::PasteExternal {
                sheet,
                row,
                col,
                text,
                html,
            } => {
                let info = self.sheet_for_edit(sheet)?;
                number_only(info)?;
                let (edits, merges, area) =
                    self.external_edits(sheet, row, col, &text, html.as_deref(), locale)?;
                let mut list = vec![Op::Cells(edits)];
                if let Some(merges) = merges {
                    list.push(Op::Merges { sheet, merges });
                }
                self.perform("pasting", Op::Many(list), Some((sheet, area)))
            }
            Command::Axis {
                sheet,
                axis,
                at,
                count,
                delete,
            } => {
                let axis = match axis {
                    AxisName::Row => Axis::Row,
                    AxisName::Col => Axis::Col,
                };
                let edit = AxisEdit {
                    axis,
                    at,
                    count,
                    delete,
                };
                self.perform("rows or columns", Op::Axis { sheet, edit }, None)
            }
            Command::InsertSheet { position, name } => {
                let name = match name {
                    Some(name) => name,
                    None => next_sheet_name(&self.book),
                };
                if let Some(problem) = sheet_name_problem(&self.book, &name, None) {
                    return Err(problem);
                }
                self.perform("a new sheet", Op::AddSheet { name, position }, None)
            }
            Command::DeleteSheet { sheet } => {
                let info = self.sheet_for_edit(sheet)?;
                if info.deleted {
                    return Err("That sheet has been deleted.".to_string());
                }
                let visible = self
                    .book
                    .tab_order()
                    .iter()
                    .filter(|&&other| {
                        other != sheet
                            && self.book.sheets[other as usize].state == SheetState::Visible
                    })
                    .count();
                if visible == 0 {
                    return Err("A workbook must keep at least one visible sheet.".to_string());
                }
                self.perform("deleting a sheet", Op::RemoveSheet { sheet }, None)
            }
            Command::RenameSheet { sheet, name } => {
                let info = self.sheet_for_edit(sheet)?;
                if info.deleted {
                    return Err("That sheet has been deleted.".to_string());
                }
                if let Some(problem) = sheet_name_problem(&self.book, &name, Some(sheet)) {
                    return Err(problem);
                }
                self.perform("renaming a sheet", Op::Rename { sheet, name }, None)
            }
            Command::Undo => self.step_back(),
            Command::Redo => self.step_forward(),
        }
    }

    fn sheet_for_edit(&self, sheet: u32) -> Result<&SheetInfo, String> {
        self.book
            .sheets
            .get(sheet as usize)
            .ok_or_else(|| format!("There is no sheet {sheet}."))
    }
}

/// What a copied cell is to an array that is copied whole.
enum WholeArray {
    Anchor(String, cgs_calc::Rect),
    Inside,
}

/// Undo and redo as the menu shows them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UndoState {
    pub undo: Option<&'static str>,
    pub redo: Option<&'static str>,
    pub lost: bool,
}

impl Engine {
    fn command_session(&mut self) -> Result<&mut Session, String> {
        let session = self.session.as_mut().ok_or_else(not_open)?;
        if session.loader.is_some() {
            return Err("This workbook is still being read.".to_string());
        }
        Ok(session)
    }

    /// Runs a command, which can be told by the UI as JSON. See `Command`.
    pub fn apply(&mut self, command: Command) -> Result<Applied, String> {
        let locale = self.locale.clone();
        self.command_session()?.command(command, &locale)
    }

    pub fn apply_json(&mut self, json: &str) -> Result<Applied, String> {
        let command: Command = serde_json::from_str(json).map_err(|error| error.to_string())?;
        self.apply(command)
    }

    /// What undo and redo would do next.
    pub fn undo_state(&self) -> UndoState {
        let Some(session) = self.session.as_ref() else {
            return UndoState::default();
        };
        UndoState {
            undo: session.history.undo.last().map(|step| step.label),
            redo: session.history.redo.last().map(|step| step.label),
            lost: session.history.lost,
        }
    }

    /// Sets today's date serial, for dates typed with no year.
    pub fn set_today(&mut self, serial: f64) {
        self.today = serial;
        if let Some(session) = self.session.as_mut() {
            session.today = serial;
        }
    }

    /// What has changed for the UI since this was last asked, after folding in what calculation has
    /// finished.
    pub fn take_dirty(&mut self) -> Dirty {
        let Some(session) = self.session.as_mut() else {
            return Dirty::default();
        };
        session.absorb_results();
        std::mem::take(&mut session.dirty)
    }

    /// Calculates for about `budget_ms`, with these areas first. Returns whether more remains.
    pub fn calc_step(
        &mut self,
        budget_ms: u32,
        first: &[cgs_calc::SheetRect],
    ) -> Result<bool, String> {
        let Some(session) = self.session.as_mut() else {
            return Ok(false);
        };
        if session.manual && !session.force_calc {
            return Ok(false);
        }
        if !session.calc.is_calculating() {
            session.force_calc = false;
            return Ok(false);
        }
        let progress = session
            .calc
            .calc_slice(budget_ms, first)
            .map_err(|error| error.0)?;
        if progress == cgs_calc::CalcProgress::Done {
            session.force_calc = false;
            session.absorb_results();
            return Ok(false);
        }
        Ok(true)
    }

    /// Whether anything is waiting that `calculate_now` would do, or is being done.
    pub fn is_calculating(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.calc.is_calculating())
    }

    pub fn is_manual(&self) -> bool {
        self.session.as_ref().is_some_and(|session| session.manual)
    }

    /// Turns manual calculation on or off. Going back to automatic catches up.
    pub fn set_manual(&mut self, manual: bool) {
        if let Some(session) = self.session.as_mut() {
            if session.manual != manual {
                session.manual = manual;
                session.edits.calc_mode = Some(manual);
                session.history.touch();
            }
        }
    }

    /// F9, or Ctrl+Alt+F9 with `all`: calculates what is due, even in manual mode.
    pub fn calculate_now(&mut self, all: bool) -> Result<(), String> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        if all {
            session.calc.recalc_all().map_err(|error| error.0)?;
        }
        session.force_calc = true;
        Ok(())
    }

    /// Shift+F9: calculates what this sheet needs, and goes no further.
    pub fn calculate_sheet(&mut self, sheet: u32) -> Result<(), String> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let Some(info) = session.book.sheets.get(sheet as usize) else {
            return Ok(());
        };
        if !session.calc.is_calculating() {
            return Ok(());
        }
        let area = cgs_calc::SheetRect {
            sheet,
            rect: cgs_calc::Rect {
                top: 1,
                left: 1,
                bottom: info.used.0.max(1),
                right: info.used.1.max(1),
            },
        };
        session
            .calc
            .calc_slice(0, &[area])
            .map_err(|error| error.0)?;
        session.absorb_results();
        Ok(())
    }
}

impl EditCell {
    fn bytes(&self) -> usize {
        64 + match &self.input {
            Some(EditInput::Value(CalcValue::Text(text) | CalcValue::Error(text))) => text.len(),
            Some(EditInput::Formula(text) | EditInput::Array(text, _)) => text.len(),
            _ => 0,
        }
    }
}
