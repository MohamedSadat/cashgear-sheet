//! Changes to an open workbook, and saving them. Phase 4's commands and undo sit on top of this;
//! until then only tests drive it. Cells live in the formula engine, so a patch here only says
//! which cells differ from the file, and their values are read from the engine when saving.

use std::io::Write;
use std::rc::Rc;

use cgs_calc::{
    CalcEngine, CalcValue, CellEdit, CellInput, CellRef, CellSink, FormulaShape, Rect, SheetRect,
};
use cgs_xlsx::patch_parts::{StylesXml, XfChanges};
use cgs_xlsx::patch_sheet::{CellChange, CellPatch, NewElement};
use cgs_xlsx::save::{save_workbook, verify_directory, verify_saved, SaveError, SaveReport};
use cgs_xlsx::styles::read_styles;
use cgs_xlsx::zipdir::Directory;
use cgs_xlsx::{ByteSource, Package};

use crate::{json, Engine, Session};

/// A cell to change. Rows and columns are 1-based.
#[derive(Debug, Clone, PartialEq)]
pub struct EditCell {
    pub sheet: u32,
    pub row: u32,
    pub col: u32,
    /// What it should hold. `None` leaves its contents alone.
    pub input: Option<EditInput>,
    /// A cell format's index, if that changes.
    pub style: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditInput {
    Value(CalcValue),
    /// As a file stores it: no leading `=`, English names, comma separators.
    Formula(String),
    Clear,
}

/// What a new cell format changes from its base, for `add_style`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewStyle {
    /// A font's children, such as `<b/><sz val="11"/><name val="Calibri"/>`.
    pub font: Option<String>,
    /// `RRGGBB`.
    pub fill: Option<String>,
    pub number_format: Option<String>,
}

/// Holds one cell's value from a block read.
#[derive(Default)]
struct OneCell(Option<CalcValue>);

impl CellSink for OneCell {
    fn cell(&mut self, _row: u32, _col: u32, value: &CalcValue) {
        self.0 = Some(value.clone());
    }
}

/// The value the engine holds now. A cell that's empty never reaches the sink.
fn value_at(session: &Session, sheet: u32, row: u32, col: u32) -> CalcValue {
    let mut sink = OneCell::default();
    let area = SheetRect {
        sheet,
        rect: Rect {
            top: row,
            left: col,
            bottom: row,
            right: col,
        },
    };
    session.calc.read_block(area, &mut sink);
    sink.0.unwrap_or(CalcValue::Empty)
}

fn not_open() -> String {
    "No workbook is open.".to_string()
}

impl Engine {
    fn editable(&mut self) -> Result<&mut Session, String> {
        let session = self.session.as_mut().ok_or_else(not_open)?;
        if session.loader.is_some() {
            return Err("This workbook is still being read.".to_string());
        }
        Ok(session)
    }

    /// Changes cells, calculates what depends on them, and keeps track of what differs from the
    /// file. Each new value and result can then be saved.
    pub fn edit_cells(&mut self, edits: &[EditCell]) -> Result<(), String> {
        let session = self.editable()?;
        if let Some(reason) = &session.cannot_edit {
            return Err(format!("This workbook can't be edited here: {reason}"));
        }
        for edit in edits {
            let sheet = session
                .book
                .sheets
                .get(edit.sheet as usize)
                .ok_or_else(|| format!("There is no sheet {}.", edit.sheet))?;
            if sheet.part.is_none() {
                return Err(format!("Sheet {} has no cells to edit.", sheet.name));
            }
            if !(1..=cgs_model::MAX_ROWS).contains(&edit.row)
                || !(1..=cgs_model::MAX_COLS).contains(&edit.col)
            {
                return Err("That cell is off the sheet.".to_string());
            }
        }
        let inputs: Vec<CellEdit> = edits
            .iter()
            .filter_map(|edit| {
                let input = match edit.input.as_ref()? {
                    EditInput::Value(CalcValue::Empty) | EditInput::Clear => CellInput::Clear,
                    EditInput::Value(value) => CellInput::Value(value.clone()),
                    EditInput::Formula(text) => CellInput::Formula {
                        text: text.clone(),
                        shape: FormulaShape::Normal,
                        cached: None,
                    },
                };
                Some(CellEdit {
                    at: CellRef {
                        sheet: edit.sheet,
                        row: edit.row,
                        col: edit.col,
                    },
                    input,
                })
            })
            .collect();
        if !inputs.is_empty() {
            session.calc.set_cells(&inputs).map_err(|error| error.0)?;
        }
        let results = if inputs.is_empty() {
            Vec::new()
        } else {
            session.calc.calc_all_changes().map_err(|error| error.0)?
        };
        for edit in edits {
            let patches = session.edits.cells.entry(edit.sheet).or_default();
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
                Some(EditInput::Formula(text)) => {
                    patch.change = CellChange::Formula {
                        text: text.clone(),
                        value: CalcValue::Empty,
                    };
                }
            }
            if edit.style.is_some() {
                patch.style = edit.style;
            }
            let info = &mut session.book.sheets[edit.sheet as usize];
            if let Some(style) = edit.style {
                info.set_style(edit.row, edit.col, style);
            }
            info.used.0 = info.used.0.max(edit.row);
            info.used.1 = info.used.1.max(edit.col);
        }
        // A cell an edit named keeps that change; anything else that moved is a new result.
        for cell in results {
            let patches = session.edits.cells.entry(cell.sheet).or_default();
            let patch = patches.entry((cell.row, cell.col)).or_insert(CellPatch {
                change: CellChange::Keep,
                style: None,
            });
            if patch.change == CellChange::Keep {
                patch.change = CellChange::Result(CalcValue::Empty);
            }
        }
        session.cache = crate::tile::TileCache::default();
        Ok(())
    }

    /// Adds a cell format copied from `base`, and returns its index for `EditCell::style`.
    pub fn add_style(&mut self, base: u32, new: &NewStyle) -> Result<u32, String> {
        let session = self.editable()?;
        if session.edits.styles.is_none() {
            let part = cgs_xlsx::workbook::styles_part(&session.package);
            let bytes = part
                .and_then(|part| session.package.read(&part))
                .unwrap_or_else(cgs_xlsx::blank::minimal_styles);
            session.edits.styles = Some(StylesXml::new(bytes));
        }
        let Some(styles) = session.edits.styles.as_mut() else {
            return Err("No styles.".to_string());
        };
        let changes = XfChanges {
            font: new.font.as_deref().map(|font| styles.add_font(font)),
            fill: new.fill.as_deref().map(|fill| styles.add_solid_fill(fill)),
            border: None,
            num_fmt: new
                .number_format
                .as_deref()
                .map(|code| styles.add_num_fmt(code)),
        };
        let index = styles.add_xf(base, changes);
        let mut table = read_styles(&styles.bytes);
        table.colors.theme = std::mem::take(&mut session.book.styles.colors.theme);
        session.book.styles = table;
        session.cache = crate::tile::TileCache::default();
        Ok(index)
    }

    /// Puts a new element in a worksheet, where the schema's order wants it.
    pub fn add_sheet_element(&mut self, sheet: u32, element: NewElement) -> Result<(), String> {
        let session = self.editable()?;
        session
            .edits
            .elements
            .entry(sheet)
            .or_default()
            .push(element);
        Ok(())
    }

    /// Makes the next save ask Excel to rebuild every pivot table when it opens the file.
    pub fn refresh_pivots_on_load(&mut self) -> Result<(), String> {
        self.editable()?.edits.refresh_pivots = true;
        Ok(())
    }

    /// The non-empty values in an area, for comparing what a save wrote with what is held.
    pub fn read_cells(&self, area: SheetRect, out: &mut dyn CellSink) {
        if let Some(session) = &self.session {
            session.calc.read_block(area, out);
        }
    }

    /// Whether the workbook differs from the file it came from.
    pub fn has_changes(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| !session.edits.is_empty())
    }

    /// Writes the workbook, with its changes, to `out`. The file it came from isn't touched, and
    /// the changes stay until `commit_saved` says the result has replaced it.
    pub fn save(&mut self, out: &mut dyn Write) -> Result<SaveReport, SaveError> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| SaveError::Write(not_open()))?;
        if session.loader.is_some() {
            return Err(SaveError::Write(
                "This workbook is still being read.".to_string(),
            ));
        }
        let mut edits = session.edits.clone();
        for (&sheet, patches) in &mut edits.cells {
            for (&(row, col), patch) in patches.iter_mut() {
                match &mut patch.change {
                    CellChange::Formula { value, .. } | CellChange::Result(value) => {
                        *value = value_at(session, sheet, row, col);
                    }
                    _ => {}
                }
            }
        }
        let plan = save_workbook(&session.package, &session.book, &edits, out)?;
        let report = plan.report.clone();
        session.pending_save = Some(plan);
        Ok(report)
    }

    /// Checks what `save` wrote, before it replaces anything.
    pub fn verify_saved(&self, saved: &dyn ByteSource) -> Result<(), SaveError> {
        let plan = self
            .session
            .as_ref()
            .and_then(|session| session.pending_save.as_ref())
            .ok_or_else(|| SaveError::Verify("nothing has been saved".to_string()))?;
        verify_saved(saved, plan)
    }

    /// Says the saved file now stands where it should, and is what this workbook reads from from
    /// now on, with nothing left to save. The old source can no longer be relied on.
    pub fn commit_saved(&mut self, source: Rc<dyn ByteSource>) -> Result<(), SaveError> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| SaveError::Write(not_open()))?;
        let plan = session
            .pending_save
            .take()
            .ok_or_else(|| SaveError::Verify("nothing has been saved".to_string()))?;
        verify_directory(source.as_ref(), &plan)?;
        session.package =
            Rc::new(Package::open(source).map_err(|error| SaveError::Verify(error.to_string()))?);
        session.edits = Default::default();
        Ok(())
    }

    /// Reads from a copy of the file from now on. Refuses a copy that isn't the same zip, as a
    /// file that changed since it was opened can't supply what this workbook hasn't read.
    pub fn use_source(&mut self, source: Rc<dyn ByteSource>) -> Result<(), SaveError> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| SaveError::Write(not_open()))?;
        let same = Directory::read(session.package.source().as_ref())?
            == Directory::read(source.as_ref())?;
        if !same {
            return Err(SaveError::SourceUnreadable(
                "the file has changed since it was opened".to_string(),
            ));
        }
        session.package = Rc::new(
            Package::open(source)
                .map_err(|error| SaveError::SourceUnreadable(error.to_string()))?,
        );
        Ok(())
    }
}

/// A save report as JSON, for the shell.
pub fn report_json(report: &SaveReport) -> String {
    let list = |out: &mut String, names: &[String]| {
        out.push('[');
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            json::push_string(out, name);
        }
        out.push(']');
    };
    let mut out = format!(
        "{{\"keptParts\":{},\"cellsWritten\":{},\"stringsAdded\":{},\"recalculatesOnOpen\":{},\"pivotsRefreshOnOpen\":{},\"resultsLeftToExcel\":{},\"tableHeadersFixed\":{},\"changedParts\":",
        report.kept_parts,
        report.cells_written,
        report.strings_added,
        report.recalculates_on_open,
        report.pivots_refresh_on_open,
        report.results_left_to_excel,
        report.table_headers_fixed,
    );
    list(&mut out, &report.changed_parts);
    out.push_str(",\"removedParts\":");
    list(&mut out, &report.removed_parts);
    out.push_str(",\"addedParts\":");
    list(&mut out, &report.added_parts);
    out.push_str(",\"keptContent\":[");
    for (index, (what, count)) in report.kept_content.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"what\":");
        json::push_string(&mut out, what.label());
        out.push_str(&format!(",\"parts\":{count}}}"));
    }
    out.push_str("]}");
    out
}
