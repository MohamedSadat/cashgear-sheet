//! Ties the workbook together: opening, reading in slices, viewport tiles, what the UI asks
//! about a cell, commands and undo, calculation in slices, and saving. Opening calculates nothing
//! that a saved value covers.

mod ask;
pub mod cell_styles;
pub mod cf;
pub mod cf_edit;
pub mod chart_edit;
pub mod charts;
pub mod clip;
pub mod command;
pub mod duplicates;
pub mod dv;
pub mod edit;
mod fill;
pub mod filter;
pub mod find;
pub mod format;
pub mod input;
mod json;
pub mod names;
pub mod notes;
pub mod page;
pub mod pivot_style;
pub mod pivots;
pub mod query;
mod shift;
pub mod sort;
pub mod table_style;
pub mod tables;
pub mod tile;
pub mod view_block;

use std::rc::Rc;

use cgs_calc::formualizer::FormualizerEngine;
use cgs_calc::{now_ms, CalcEngine};
use cgs_model::{
    Anchor, AnchorPoint, Book, DrawingKind, SheetInfo, SheetKind, SheetState, TILE_COLS, TILE_ROWS,
};
use cgs_numfmt::{Locale, Options};
use cgs_xlsx::save::{SavePlan, WorkbookEdits};
use cgs_xlsx::{book_info, ByteSource, Loader, Package, ReadError};

pub use command::{Applied, Area, Command, Dirty, UndoState};
pub use edit::{report_json, EditCell, EditInput, NewStyle};
pub use find::FindQuery;
pub use format::{BorderChange, ColorSpec, FillSpec, FormatChange, LineSpec};
pub use tile::Screen;
pub use view_block::ViewBlock;

/// EMU in one CSS pixel at 100%.
const EMU_PER_PX: i64 = 9525;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPhase {
    /// Sheets are still coming in.
    Reading,
    /// All sheets are readable; the first edit's work can start when things are quiet.
    Preparing,
    /// Nothing left to do.
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadStatus {
    pub phase: LoadPhase,
    pub done: u64,
    pub total: u64,
    /// Sheets that became readable in this step.
    pub sheets_ready: Vec<u32>,
}

struct Session {
    book: Book,
    calc: FormualizerEngine,
    /// The file saves copy from, which an overwrite or a working copy replaces.
    package: Rc<Package>,
    /// What differs from that file.
    edits: WorkbookEdits,
    history: command::History,
    /// What the UI hasn't been told about yet.
    dirty: command::Dirty,
    /// New cell formats by what they were made from, so that typing adds each one once.
    format_styles: std::collections::HashMap<(u32, input::Format), u32>,
    /// Rows a command changed, to see whether they need another height.
    touched_rows: std::collections::BTreeSet<(u32, u32)>,
    /// Rows waiting to be measured, because they wrap or turn their text.
    pending_fits: std::collections::BTreeSet<(u32, u32)>,
    /// Today's date serial, for dates typed with no year.
    today: f64,
    /// The workbook calculates only when asked.
    manual: bool,
    /// Asked to, though it is manual.
    force_calc: bool,
    /// What the last save should have written, until it has been checked and used.
    pending_save: Option<SavePlan>,
    /// Why this workbook's formulas can't be made ready for an edit, if they can't.
    cannot_edit: Option<String>,
    loader: Option<Loader>,
    /// Sheets showing Circle Invalid Data.
    circled: std::collections::HashSet<u32>,
    /// Each pivot cache's records, once a pivot table has needed them.
    pivot_records: Vec<Option<Rc<cgs_model::pivot::CacheRecords>>>,
    /// What pivot tables call things the file doesn't name, in the UI's language.
    pivot_captions: cgs_pivot::Captions,
    cache: tile::TileCache,
    phase: LoadPhase,
    last_progress: (u64, u64),
}

/// The engine a worker holds.
pub struct Engine {
    locale: Locale,
    pivot_captions: cgs_pivot::Captions,
    today: f64,
    session: Option<Session>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// A sheet's shape, as the grid needs it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SheetLayout {
    pub default_row_px: u32,
    pub default_col_px: u32,
    pub used_rows: u32,
    pub used_cols: u32,
    /// (first, last, pixels) for rows that aren't the default size, sorted. A filter that hides
    /// most of a million rows is a few runs, not a million rows.
    pub rows: Vec<(u32, u32, u32)>,
    /// (first, last, pixels), sorted.
    pub cols: Vec<(u32, u32, u32)>,
    /// (top, left, bottom, right).
    pub merges: Vec<(u32, u32, u32, u32)>,
    pub zoom: u32,
    pub show_grid: bool,
    pub show_headers: bool,
    pub frozen_rows: u32,
    pub frozen_cols: u32,
    pub first_row: u32,
    pub first_col: u32,
    pub top_row: u32,
    pub left_col: u32,
    pub active_row: u32,
    pub active_col: u32,
    pub drawings: Vec<DrawingBox>,
}

/// Where a drawing sits on the sheet, in CSS pixels at 100%.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingBox {
    pub kind: DrawingKind,
    pub name: String,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

fn point_position(info: &SheetInfo, point: AnchorPoint) -> (i64, i64) {
    (
        tile::sheet_x(info, point.col) + point.col_off / EMU_PER_PX,
        tile::sheet_y(info, point.row) + point.row_off / EMU_PER_PX,
    )
}

fn drawing_box(info: &SheetInfo, anchor: Anchor) -> (i64, i64, i64, i64) {
    match anchor {
        Anchor::TwoCell { from, to } => {
            let (x, y) = point_position(info, from);
            let (right, bottom) = point_position(info, to);
            (x, y, right - x, bottom - y)
        }
        Anchor::OneCell {
            from,
            width,
            height,
        } => {
            let (x, y) = point_position(info, from);
            (x, y, width / EMU_PER_PX, height / EMU_PER_PX)
        }
        Anchor::Absolute {
            x,
            y,
            width,
            height,
        } => (
            x / EMU_PER_PX,
            y / EMU_PER_PX,
            width / EMU_PER_PX,
            height / EMU_PER_PX,
        ),
    }
}

impl Session {
    /// Once the workbook is ready for edits, works out what a saved value doesn't cover.
    fn after_preparing(&mut self) {
        if self.manual || self.cannot_edit.is_some() {
            return;
        }
        // A file that asks to be calculated, or holds formulas with no saved value, gets all of
        // them calculated. Other files only have their volatile functions to bring up to date.
        let result = if self.book.full_calc_on_load || self.book.formulas_without_values {
            self.calc.recalc_all()
        } else {
            self.calc.start_volatile()
        };
        if let Err(error) = result {
            self.cannot_edit = Some(error.0);
        }
    }

    /// Puts in cells that are part of what opened, such as a CSV file's, with no undo step.
    fn load_cells(&mut self, edits: Vec<EditCell>) -> Result<(), String> {
        let _ = self.apply_loaded(edits)?;
        // Not an edit undo would take back, but these differ from the file, so a save writes them.
        self.history.touch();
        Ok(())
    }
}

impl Engine {
    pub fn new() -> Self {
        Self {
            locale: Locale::en_us(),
            pivot_captions: cgs_pivot::Captions::default(),
            today: 45_000.0,
            session: None,
        }
    }

    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    /// Opens a workbook, replacing any open one. Its sheets are read by `load_step`.
    pub fn open(&mut self, source: Rc<dyn ByteSource>) -> Result<(), ReadError> {
        self.session = None;
        let (loader, book) = Loader::open(source)?;
        let mut calc = FormualizerEngine::new();
        calc.set_locale(self.locale.clone());
        calc.begin_load(&book_info(&book))
            .map_err(|error| ReadError::Engine(error.0))?;
        self.session = Some(Session {
            book,
            calc,
            package: loader.package(),
            edits: WorkbookEdits::default(),
            history: command::History::default(),
            dirty: command::Dirty::default(),
            format_styles: std::collections::HashMap::new(),
            touched_rows: Default::default(),
            pending_fits: Default::default(),
            today: self.today,
            manual: false,
            force_calc: false,
            pending_save: None,
            cannot_edit: None,
            loader: Some(loader),
            circled: Default::default(),
            pivot_records: Vec::new(),
            pivot_captions: self.pivot_captions.clone(),
            cache: tile::TileCache::default(),
            phase: LoadPhase::Reading,
            last_progress: (0, 0),
        });
        Ok(())
    }

    pub fn close(&mut self) {
        self.session = None;
    }

    pub fn book(&self) -> Option<&Book> {
        self.session.as_ref().map(|session| &session.book)
    }

    /// Reads sheets for about `budget_ms`.
    pub fn load_step(&mut self, budget_ms: f64) -> Result<LoadStatus, ReadError> {
        let Some(session) = self.session.as_mut() else {
            return Ok(LoadStatus {
                phase: LoadPhase::Ready,
                done: 0,
                total: 0,
                sheets_ready: Vec::new(),
            });
        };
        let mut sheets_ready = Vec::new();
        if let Some(loader) = session.loader.as_mut() {
            let was_loaded: Vec<bool> = session.book.sheets.iter().map(|s| s.loaded).collect();
            let deadline = now_ms() + budget_ms;
            let finished = loader.step(&mut session.book, &mut session.calc, &mut || {
                now_ms() < deadline
            })?;
            let progress = loader.progress();
            session.last_progress = (progress.done, progress.total);
            for (index, sheet) in session.book.sheets.iter().enumerate() {
                if sheet.loaded && !was_loaded[index] {
                    sheets_ready.push(index as u32);
                }
            }
            if finished {
                session
                    .calc
                    .finish_load()
                    .map_err(|error| ReadError::Engine(error.0))?;
                session.loader = None;
                session.manual = session.book.manual_calc;
                session.phase = LoadPhase::Preparing;
            }
        }
        Ok(LoadStatus {
            phase: session.phase,
            done: session.last_progress.0,
            total: session.last_progress.1,
            sheets_ready,
        })
    }

    /// Does one sheet of the first edit's work. Returns whether more remains.
    pub fn prepare_step(&mut self) -> Result<bool, ReadError> {
        let Some(session) = self.session.as_mut() else {
            return Ok(false);
        };
        if session.phase != LoadPhase::Preparing {
            return Ok(false);
        }
        // This only readies the first edit, and the workbook is already showing, so a failure here
        // takes away editing and not the workbook. `edit_cells` says why.
        match session.calc.prepare_step() {
            Ok(more) => {
                if !more {
                    session.phase = LoadPhase::Ready;
                    session.after_preparing();
                }
                Ok(more)
            }
            Err(error) => {
                session.cannot_edit = Some(error.0);
                session.phase = LoadPhase::Ready;
                Ok(false)
            }
        }
    }

    /// Reads this sheet next, if it isn't read yet.
    pub fn prioritize(&mut self, sheet: u32) {
        if let Some(loader) = self.session.as_mut().and_then(|s| s.loader.as_mut()) {
            loader.prioritize(sheet);
        }
    }

    /// The tile whose top-left cell is (`first_row`, `first_col`), counted from 0.
    pub fn view_block(
        &mut self,
        sheet: u32,
        first_row: u32,
        first_col: u32,
        screen: Screen,
    ) -> ViewBlock {
        let first_row = first_row / TILE_ROWS * TILE_ROWS;
        let first_col = first_col / TILE_COLS * TILE_COLS;
        let options = Options {
            locale: &self.locale,
            date1904: self.session.as_ref().is_some_and(|s| s.book.date1904),
        };
        match self.session.as_mut() {
            Some(session) if (sheet as usize) < session.book.sheets.len() => {
                let input = tile::TileInput {
                    book: &session.book,
                    calc: &session.calc,
                    options: &options,
                    today: session.today,
                    invalid: session
                        .circled
                        .contains(&sheet)
                        .then(|| session.invalid_cells(sheet)),
                };
                tile::build_tile(
                    &input,
                    &mut session.cache,
                    sheet,
                    first_row,
                    first_col,
                    screen,
                )
            }
            _ => blank_tile(first_row, first_col),
        }
    }

    /// A sheet's sizes, merges, view and drawings. `None` if there's no such sheet.
    pub fn layout(&self, sheet: u32) -> Option<SheetLayout> {
        let info = self.book()?.sheets.get(sheet as usize)?;
        let view = info.view;
        let freeze = view.freeze.unwrap_or_default();
        Some(SheetLayout {
            default_row_px: info.rows.default_px,
            default_col_px: info.cols.default_px,
            used_rows: info.used.0,
            used_cols: info.used.1,
            rows: row_runs(&info.rows),
            cols: info
                .cols
                .ranges
                .iter()
                .map(|&(first, last, _)| (first, last, info.cols.px(first)))
                .collect(),
            merges: info
                .merges
                .iter()
                .map(|merge| (merge.top, merge.left, merge.bottom, merge.right))
                .collect(),
            zoom: u32::from(view.zoom),
            show_grid: view.show_grid,
            show_headers: view.show_headers,
            frozen_rows: freeze.rows,
            frozen_cols: freeze.cols,
            first_row: freeze.top.max(1),
            first_col: freeze.left.max(1),
            top_row: view.top_left.0,
            left_col: view.top_left.1,
            active_row: view.active_cell.0,
            active_col: view.active_cell.1,
            drawings: info
                .drawings
                .iter()
                .map(|drawing| {
                    let (x, y, width, height) = drawing_box(info, drawing.anchor);
                    DrawingBox {
                        kind: drawing.kind,
                        name: drawing.name.clone(),
                        x,
                        y,
                        width,
                        height,
                    }
                })
                .collect(),
        })
    }

    /// The layout's numbers as one array, which crosses into JS without being parsed:
    /// see `decodeLayout` in `packages/engine-client/src/layout.ts`.
    pub fn layout_numbers(&self, sheet: u32) -> Vec<u32> {
        let Some(layout) = self.layout(sheet) else {
            return Vec::new();
        };
        let mut out = vec![
            layout.default_row_px,
            layout.default_col_px,
            layout.used_rows,
            layout.used_cols,
            layout.zoom,
            u32::from(layout.show_grid),
            u32::from(layout.show_headers),
            layout.frozen_rows,
            layout.frozen_cols,
            layout.first_row,
            layout.first_col,
            layout.top_row,
            layout.left_col,
            layout.active_row,
            layout.active_col,
            layout.rows.len() as u32,
            layout.cols.len() as u32,
            layout.merges.len() as u32,
        ];
        for (first, last, px) in &layout.rows {
            out.extend_from_slice(&[*first, *last, *px]);
        }
        for (first, last, px) in &layout.cols {
            out.extend_from_slice(&[*first, *last, *px]);
        }
        for (top, left, bottom, right) in &layout.merges {
            out.extend_from_slice(&[*top, *left, *bottom, *right]);
        }
        out
    }

    /// The workbook's sheets as JSON, for the shell.
    pub fn book_json(&self) -> String {
        let Some(book) = self.book() else {
            return "null".to_string();
        };
        let mut out = String::from("{\"active\":");
        out.push_str(&book.active_sheet.to_string());
        out.push_str(",\"sheets\":[");
        for (index, sheet) in book.sheets.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            json::push_string(&mut out, &sheet.name);
            out.push_str(",\"state\":\"");
            out.push_str(match sheet.state {
                SheetState::Visible => "visible",
                SheetState::Hidden => "hidden",
                SheetState::VeryHidden => "veryHidden",
            });
            out.push_str("\",\"kind\":\"");
            out.push_str(match sheet.kind {
                SheetKind::Worksheet => "worksheet",
                SheetKind::Chart => "chart",
                SheetKind::Other => "other",
            });
            out.push_str("\",\"loaded\":");
            out.push_str(if sheet.loaded { "true" } else { "false" });
            out.push_str(",\"deleted\":");
            out.push_str(if sheet.deleted { "true" } else { "false" });
            out.push('}');
        }
        out.push_str("],\"order\":[");
        for (index, sheet) in book.tab_order().iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&sheet.to_string());
        }
        out.push_str("]}");
        out
    }

    /// A sheet's drawings as JSON, in CSS pixels at 100%.
    pub fn drawings_json(&self, sheet: u32) -> String {
        let Some(layout) = self.layout(sheet) else {
            return "[]".to_string();
        };
        let mut out = String::from("[");
        for (index, drawing) in layout.drawings.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"kind\":\"");
            out.push_str(match drawing.kind {
                DrawingKind::Chart => "chart",
                DrawingKind::Picture => "picture",
                DrawingKind::Shape => "shape",
                DrawingKind::Group => "group",
                DrawingKind::Other => "other",
            });
            out.push_str("\",\"name\":");
            json::push_string(&mut out, &drawing.name);
            out.push_str(&format!(
                ",\"index\":{index},\"x\":{},\"y\":{},\"width\":{},\"height\":{}",
                drawing.x, drawing.y, drawing.width, drawing.height
            ));
            let model = self
                .book()
                .and_then(|book| book.sheets.get(sheet as usize))
                .and_then(|info| info.drawings.get(index));
            if let Some(model) = model {
                if let Some(chart) = model.chart {
                    out.push_str(&format!(",\"chart\":{chart}"));
                }
                if let Some(image) = &model.image {
                    out.push_str(",\"image\":");
                    json::push_string(&mut out, image);
                }
                if !model.description.is_empty() {
                    out.push_str(",\"description\":");
                    json::push_string(&mut out, &model.description);
                }
            }
            out.push('}');
        }
        out.push(']');
        out
    }

    /// What a sheet's drawing shows, if it is a chart: its series' values read from the cells
    /// now, and its colours for this screen. JSON, or `null`.
    pub fn chart_json(&self, sheet: u32, drawing: u32, screen: Screen) -> String {
        let options = Options {
            locale: &self.locale,
            date1904: self.session.as_ref().is_some_and(|s| s.book.date1904),
        };
        let Some(session) = self.session.as_ref() else {
            return "null".to_string();
        };
        let book = &session.book;
        let Some(info) = book.sheets.get(sheet as usize) else {
            return "null".to_string();
        };
        let Some(found) = info.drawings.get(drawing as usize) else {
            return "null".to_string();
        };
        let Some(part) = found
            .chart
            .and_then(|chart| book.charts.get(chart as usize))
        else {
            return "null".to_string();
        };
        let (_, _, width, height) = drawing_box(info, found.anchor);
        let view = match &part.body {
            cgs_model::ChartBody::Classic(chart) => charts::Builder {
                reader: charts::Reader {
                    book,
                    calc: &session.calc,
                    options: &options,
                },
                painter: charts::Painter {
                    theme: &book.styles.colors.theme,
                    screen: Some(screen),
                },
                width: width as f64,
                height: height as f64,
            }
            .build(chart),
            other => charts::placeholder_view(other),
        };
        serde_json::to_string(&view).unwrap_or_else(|_| "null".to_string())
    }

    /// Takes a picture's bytes for `Command::InsertPicture`, and names the part they will be
    /// saved as. `None` if they aren't a picture the app can show.
    pub fn add_media(&mut self, bytes: Vec<u8>) -> Option<String> {
        let extension = media_extension(&bytes)?;
        let session = self.session.as_mut()?;
        let taken: std::collections::HashSet<String> = session
            .package
            .parts()
            .iter()
            .map(|part| part.name.to_ascii_lowercase())
            .chain(
                session
                    .edits
                    .media
                    .keys()
                    .map(|name| name.to_ascii_lowercase()),
            )
            .collect();
        let name = (1..)
            .map(|n| format!("xl/media/image{n}.{extension}"))
            .find(|name| !taken.contains(name))?;
        session.edits.media.insert(name.clone(), bytes);
        Some(name)
    }

    /// A part's bytes, such as a picture's: from the file, or added since.
    pub fn media(&self, part: &str) -> Vec<u8> {
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        if let Some(bytes) = session.edits.media.get(part) {
            return bytes.clone();
        }
        session.package.read(part).unwrap_or_default()
    }

    /// What a cell shows.
    pub fn display_text(&mut self, sheet: u32, row: u32, col: u32) -> String {
        let options = Options {
            locale: &self.locale,
            date1904: self.session.as_ref().is_some_and(|s| s.book.date1904),
        };
        let Some(session) = self.session.as_mut() else {
            return String::new();
        };
        if sheet as usize >= session.book.sheets.len() {
            return String::new();
        }
        let input = tile::TileInput {
            book: &session.book,
            calc: &session.calc,
            options: &options,
            today: session.today,
            invalid: None,
        };
        tile::display_text(&input, &mut session.cache, sheet, row, col)
    }

    /// What the formula bar shows for a cell.
    pub fn edit_text(&mut self, sheet: u32, row: u32, col: u32) -> String {
        let options = Options {
            locale: &self.locale,
            date1904: self.session.as_ref().is_some_and(|s| s.book.date1904),
        };
        let Some(session) = self.session.as_mut() else {
            return String::new();
        };
        if sheet as usize >= session.book.sheets.len() {
            return String::new();
        }
        let input = tile::TileInput {
            book: &session.book,
            calc: &session.calc,
            options: &options,
            today: session.today,
            invalid: None,
        };
        tile::edit_text(&input, &mut session.cache, sheet, row, col)
    }
}

/// A picture's file extension, from its first bytes.
fn media_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else {
        // Excel 2016 shows no SVG without a picture beside it, so the UI turns one into a PNG.
        None
    }
}

/// A tile for when no workbook is open: default sizes and nothing else.
fn blank_tile(first_row: u32, first_col: u32) -> ViewBlock {
    let mut writer = view_block::TileWriter::new(
        first_row,
        first_col,
        vec![20; TILE_ROWS as usize],
        vec![64; TILE_COLS as usize],
    );
    writer.set_flags(view_block::TILE_NOT_READY);
    for _ in 0..TILE_ROWS * TILE_COLS {
        writer.cell("", view_block::CellRecord::default());
    }
    writer.finish()
}

/// Rows that differ from the default size, as runs of the same size.
fn row_runs(rows: &cgs_model::RowAxis) -> Vec<(u32, u32, u32)> {
    let mut out: Vec<(u32, u32, u32)> = Vec::new();
    for &(row, line) in &rows.rows {
        let px = if line.hidden {
            0
        } else {
            line.size.unwrap_or(rows.default_px)
        };
        if px == rows.default_px {
            continue;
        }
        match out.last_mut() {
            Some((_, last, size)) if *last + 1 == row && *size == px => *last = row,
            _ => out.push((row, row, px)),
        }
    }
    out
}
