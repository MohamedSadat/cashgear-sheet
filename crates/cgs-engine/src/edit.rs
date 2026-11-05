//! What differs from the file, and saving it. Commands and undo are in `command`. Cells live in the
//! formula engine, so a patch here only says which cells differ from the file, and their values
//! and formulas are read from the engine when saving.

use std::io::Write;
use std::rc::Rc;

use cgs_calc::{CalcEngine, CalcValue, CellRef, CellSink, FormulaShape, Rect, SheetRect};
use cgs_xlsx::patch_parts::XfChanges;
use cgs_xlsx::patch_sheet::{ArrayKind, CellChange, ColPatch, NewElement, RowPatch, ViewPatch};
use cgs_xlsx::save::{save_workbook, verify_directory, verify_saved, SaveError, SaveReport};
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
    /// A formula stored as an array.
    Array(String, FormulaShape),
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
    /// A built-in number format's id.
    pub number_format_id: Option<u32>,
    /// `<alignment>`'s attributes, starting with a space.
    pub alignment: Option<String>,
    /// A border's children.
    pub border: Option<String>,
}

/// Holds one cell's value from a block read.
#[derive(Default)]
struct OneCell(Option<CalcValue>);

impl CellSink for OneCell {
    fn cell(&mut self, _row: u32, _col: u32, value: &CalcValue) {
        self.0 = Some(value.clone());
    }
}

/// Which cells of a block hold anything.
#[derive(Default)]
pub(crate) struct CollectCells(pub(crate) Vec<(u32, u32)>);

impl CellSink for CollectCells {
    fn cell(&mut self, row: u32, col: u32, _value: &CalcValue) {
        self.0.push((row, col));
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

fn model_rect(rect: Rect) -> cgs_model::Rect {
    cgs_model::Rect {
        top: rect.top,
        left: rect.left,
        bottom: rect.bottom,
        right: rect.right,
    }
}

/// `mergeCells` as a sheet's merges now stand. Empty if it has none.
fn merge_element(merges: &[cgs_model::Rect]) -> String {
    if merges.is_empty() {
        return String::new();
    }
    let mut xml = format!("<mergeCells count=\"{}\">", merges.len());
    for merge in merges {
        xml.push_str(&format!(
            "<mergeCell ref=\"{}:{}\"/>",
            cgs_xlsx::patch_sheet::cell_name(merge.top, merge.left),
            cgs_xlsx::patch_sheet::cell_name(merge.bottom, merge.right),
        ));
    }
    xml.push_str("</mergeCells>");
    xml
}

impl Session {
    /// Adds a cell format copied from `base`, and returns its index.
    pub(crate) fn add_style(&mut self, base: u32, new: &NewStyle) -> u32 {
        let styles = self.styles_xml();
        let changes = XfChanges {
            font: new.font.as_deref().map(|font| styles.add_font(font)),
            fill: new.fill.as_deref().map(|fill| styles.add_solid_fill(fill)),
            border: new
                .border
                .as_deref()
                .map(|border| styles.add_border(border)),
            num_fmt: new.number_format_id.or_else(|| {
                new.number_format
                    .as_deref()
                    .map(|code| styles.add_num_fmt(code))
            }),
            alignment: new.alignment.clone(),
            ..XfChanges::default()
        };
        let index = styles.add_xf(base, changes);
        self.refresh_styles();
        index
    }

    /// Fills in what patches leave to the engine: results, formula text, and arrays' areas.
    fn edits_for_save(&self) -> cgs_xlsx::save::WorkbookEdits {
        if self.history.at_clean() {
            return Default::default();
        }
        let mut edits = self.edits.clone();
        let areas = self.calc.spill_areas();
        let spaced_columns: Vec<(String, String, String)> = self
            .calc
            .tables()
            .iter()
            .flat_map(|table| {
                table
                    .columns
                    .iter()
                    .filter(|column| column.trim() != column.as_str())
                    .map(|column| {
                        (
                            table.name.clone(),
                            column.trim().to_string(),
                            column.clone(),
                        )
                    })
            })
            .collect();
        for (&sheet, patches) in &mut edits.cells {
            let here: Vec<Rect> = areas
                .iter()
                .filter(|area| area.sheet == sheet)
                .map(|area| area.rect)
                .collect();
            for (&(row, col), patch) in patches.iter_mut() {
                let at = CellRef { sheet, row, col };
                let area = here.iter().find(|area| area.contains(row, col)).copied();
                let is_anchor = area.is_some_and(|area| (area.top, area.left) == (row, col));
                match &mut patch.change {
                    CellChange::Formula { text, value, array } => {
                        match self.calc.formula_text(at) {
                            Some(formula) => {
                                *array = match self.calc.formula_shape(at) {
                                    FormulaShape::Array(rect) => {
                                        ArrayKind::Legacy(model_rect(rect))
                                    }
                                    _ => match area.filter(|_| is_anchor) {
                                        // An index the save fills in, once it knows the metadata.
                                        Some(area)
                                            if (area.top, area.left)
                                                != (area.bottom, area.right) =>
                                        {
                                            ArrayKind::Dynamic(model_rect(area), 1)
                                        }
                                        _ => ArrayKind::None,
                                    },
                                };
                                let in_array = *array != ArrayKind::None;
                                let mut plain = cgs_refs::formula_for_people(&formula);
                                // The engine trims spaces from a column's name, and Excel doesn't.
                                for (table, trimmed, exact) in &spaced_columns {
                                    if let std::borrow::Cow::Owned(next) =
                                        cgs_refs::rename_table_column(
                                            &plain, table, trimmed, exact, false,
                                        )
                                    {
                                        plain = next;
                                    }
                                }
                                *text = cgs_refs::formula_for_file(&plain, in_array);
                                *value = value_at(self, sheet, row, col);
                            }
                            // It was overwritten by something a formula doesn't own, such as a spill.
                            None => {
                                patch.change = CellChange::Result(value_at(self, sheet, row, col));
                            }
                        }
                    }
                    CellChange::Result(_) => {
                        let value = value_at(self, sheet, row, col);
                        patch.change = match area {
                            Some(area) if is_anchor => CellChange::ArrayResult {
                                value,
                                area: model_rect(area),
                            },
                            Some(_) => CellChange::Spilled(value),
                            None => CellChange::Result(value),
                        };
                    }
                    _ => {}
                }
            }
        }
        // An array's anchor must say how far it now reaches, even if its own result didn't change.
        for area in &areas {
            let Some(patches) = edits.cells.get_mut(&area.sheet) else {
                continue;
            };
            let rect = area.rect;
            let touched = patches
                .range((rect.top, 0)..=(rect.bottom, u32::MAX))
                .any(|(&(row, col), _)| rect.contains(row, col));
            if !touched {
                continue;
            }
            let value = value_at(self, area.sheet, rect.top, rect.left);
            let entry =
                patches
                    .entry((rect.top, rect.left))
                    .or_insert(cgs_xlsx::patch_sheet::CellPatch {
                        change: CellChange::Keep,
                        style: None,
                    });
            if entry.change == CellChange::Keep {
                entry.change = CellChange::ArrayResult {
                    value,
                    area: model_rect(rect),
                };
            }
        }
        let digit = cgs_model::style::max_digit_width(self.book.styles.normal_font());
        for (&sheet, areas) in &edits.style_areas {
            if let Some(info) = self.book.sheets.get(sheet as usize).filter(|i| !i.deleted) {
                if !areas.is_empty() {
                    edits.extras.entry(sheet).or_default().styled =
                        Some((info.clone(), areas.clone()));
                }
            }
        }
        for (&sheet, spans) in &edits.row_lines {
            let Some(info) = self.book.sheets.get(sheet as usize).filter(|i| !i.deleted) else {
                continue;
            };
            let extra = edits.extras.entry(sheet).or_default();
            for &(first, last) in spans {
                for row in first..=last {
                    let line = info.rows.info(row).copied().unwrap_or_default();
                    extra.rows.insert(
                        row,
                        RowPatch {
                            size_px: line.size,
                            custom: line.custom,
                            hidden: line.hidden,
                            style: line.style,
                        },
                    );
                }
            }
        }
        for (&sheet, spans) in &edits.col_lines {
            let Some(info) = self.book.sheets.get(sheet as usize).filter(|i| !i.deleted) else {
                continue;
            };
            let extra = edits.extras.entry(sheet).or_default();
            for &(first, last) in spans {
                for (from, to, line) in info.cols.slices(first, last) {
                    extra.cols.push(ColPatch {
                        first: from,
                        last: to,
                        size_px: line.size,
                        default_px: info.cols.default_px,
                        digit,
                        custom: line.custom,
                        hidden: line.hidden,
                        style: line.style,
                    });
                }
            }
        }
        // Whole columns' styles go on `<col>` too, which a format doesn't list as lines.
        for (&sheet, areas) in &edits.style_areas {
            let Some(info) = self.book.sheets.get(sheet as usize).filter(|i| !i.deleted) else {
                continue;
            };
            let extra = edits.extras.entry(sheet).or_default();
            let mut spans: Vec<(u32, u32)> = areas
                .iter()
                .filter(|area| area.top == 1 && area.bottom == cgs_model::MAX_ROWS)
                .map(|area| (area.left, area.right))
                .collect();
            crate::format::join_spans(&mut spans);
            for (first, last) in spans {
                for (from, to, line) in info.cols.slices(first, last) {
                    let covered = extra
                        .cols
                        .iter()
                        .any(|old| old.first <= from && old.last >= to);
                    if !covered {
                        extra.cols.push(ColPatch {
                            first: from,
                            last: to,
                            size_px: line.size,
                            default_px: info.cols.default_px,
                            digit,
                            custom: line.custom,
                            hidden: line.hidden,
                            style: line.style,
                        });
                    }
                }
            }
            // And whole rows' on `<row>`.
            for area in areas
                .iter()
                .filter(|area| area.left == 1 && area.right == cgs_model::MAX_COLS)
            {
                if area.top == 1 && area.bottom == cgs_model::MAX_ROWS {
                    for &(row, line) in &info.rows.rows {
                        if line.style.is_some() {
                            extra.rows.entry(row).or_insert(RowPatch {
                                size_px: line.size,
                                custom: line.custom,
                                hidden: line.hidden,
                                style: line.style,
                            });
                        }
                    }
                    continue;
                }
                for row in area.top..=area.bottom {
                    let line = info.rows.info(row).copied().unwrap_or_default();
                    extra.rows.entry(row).or_insert(RowPatch {
                        size_px: line.size,
                        custom: line.custom,
                        hidden: line.hidden,
                        style: line.style,
                    });
                }
            }
        }
        for &sheet in &edits.default_rows.clone() {
            if let Some(info) = self.book.sheets.get(sheet as usize).filter(|i| !i.deleted) {
                edits.extras.entry(sheet).or_default().default_row_px = Some(info.rows.default_px);
            }
        }
        let viewed: std::collections::BTreeSet<u32> = edits
            .view_flags
            .iter()
            .chain(edits.view_freeze.iter())
            .copied()
            .collect();
        for sheet in viewed {
            let Some(info) = self.book.sheets.get(sheet as usize).filter(|i| !i.deleted) else {
                continue;
            };
            edits.extras.entry(sheet).or_default().view = Some(ViewPatch {
                freeze: edits
                    .view_freeze
                    .contains(&sheet)
                    .then_some(info.view.freeze),
                show_grid: Some(info.view.show_grid),
                show_headers: Some(info.view.show_headers),
                active_cell: info.view.active_cell,
            });
        }
        for &sheet in &edits.merges.clone() {
            let Some(info) = self.book.sheets.get(sheet as usize) else {
                continue;
            };
            edits.elements.entry(sheet).or_default().push(NewElement {
                name: "mergeCells".to_string(),
                xml: merge_element(&info.merges),
            });
        }
        edits
    }
}

impl Engine {
    fn editable(&mut self) -> Result<&mut Session, String> {
        let session = self.session.as_mut().ok_or_else(not_open)?;
        if session.loader.is_some() {
            return Err("This workbook is still being read.".to_string());
        }
        Ok(session)
    }

    /// Changes cells and calculates what depends on them, in one go. Each new value and result can
    /// then be saved. This is for the bench and for tests: the app uses `apply` and `calc_step`,
    /// with undo.
    pub fn edit_cells(&mut self, edits: &[EditCell]) -> Result<(), String> {
        let session = self.editable()?;
        if let Some(reason) = &session.cannot_edit {
            return Err(format!("This workbook can't be edited here: {reason}"));
        }
        session.apply_loaded(edits.to_vec())?;
        session.history.touch();
        session
            .calc
            .calc_all_changes()
            .map_err(|error| error.0)
            .map(|changed| {
                for cell in changed {
                    session.note_result(cell);
                }
            })?;
        session.cache = crate::tile::TileCache::default();
        Ok(())
    }

    /// Adds a cell format copied from `base`, and returns its index for `EditCell::style`.
    pub fn add_style(&mut self, base: u32, new: &NewStyle) -> Result<u32, String> {
        let session = self.editable()?;
        Ok(session.add_style(base, new))
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
        session.history.touch();
        Ok(())
    }

    /// Makes the next save ask Excel to rebuild every pivot table when it opens the file.
    pub fn refresh_pivots_on_load(&mut self) -> Result<(), String> {
        let session = self.editable()?;
        session.edits.refresh_pivots = true;
        session.history.touch();
        Ok(())
    }

    /// The non-empty values in an area, for comparing what a save wrote with what is held.
    pub fn read_cells(&self, area: SheetRect, out: &mut dyn CellSink) {
        if let Some(session) = &self.session {
            session.calc.read_block(area, out);
        }
    }

    /// A formula's text as the engine holds it, for tests and the bench.
    pub fn formula_text(&self, sheet: u32, row: u32, col: u32) -> Option<String> {
        self.session
            .as_ref()?
            .calc
            .formula_text(CellRef { sheet, row, col })
    }

    /// Whether the workbook differs from the file it came from.
    pub fn has_changes(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| !session.history.at_clean())
    }

    /// Writes the workbook, with its changes, to `out`. The file it came from isn't touched, and
    /// the changes stay until `commit_saved` says the result has replaced it. Whatever is still
    /// being calculated gets finished first, so that saved values are current.
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
        if !session.history.at_clean() && session.calc.is_calculating() {
            session
                .calc
                .calc_all_changes()
                .map_err(|error| SaveError::Write(error.0))
                .map(|changed| {
                    for cell in changed {
                        session.note_result(cell);
                    }
                })?;
        }
        session.absorb_results();
        let edits = session.edits_for_save();
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
        for (sheet, part) in &plan.new_parts {
            session.book.sheets[*sheet as usize].part = Some(part.clone());
        }
        if !plan.file_sheets.is_empty() {
            session.book.file_sheets = plan.file_sheets.clone();
        }
        for info in &mut session.book.sheets {
            if info.deleted {
                info.part = None;
            }
        }
        // Later formats start from the saved styles, which hold all that were added.
        let styles = session.edits.styles.take();
        session.edits = Default::default();
        session.edits.styles = styles.map(|mut styles| {
            styles.changed = false;
            styles
        });
        session.history.mark_clean();
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
    out.push_str(",\"leftExtensions\":");
    list(&mut out, &report.left_extensions);
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
