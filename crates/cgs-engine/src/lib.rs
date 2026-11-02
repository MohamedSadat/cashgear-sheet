//! Ties the workbook together: opening, reading in slices, viewport tiles and what the UI
//! asks about a cell. Calculation scheduling comes in Phase 4; nothing calculates yet.

mod json;
pub mod tile;
pub mod view_block;

use std::rc::Rc;

use cgs_calc::formualizer::FormualizerEngine;
use cgs_calc::{now_ms, CalcEngine};
use cgs_model::{
    Anchor, AnchorPoint, Book, DrawingKind, SheetInfo, SheetKind, SheetState, TILE_COLS, TILE_ROWS,
};
use cgs_numfmt::{Locale, Options};
use cgs_xlsx::{book_info, ByteSource, Loader, ReadError};

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
    loader: Option<Loader>,
    cache: tile::TileCache,
    phase: LoadPhase,
    last_progress: (u64, u64),
}

/// The engine a worker holds.
pub struct Engine {
    locale: Locale,
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
    /// (row, pixels), sorted.
    pub rows: Vec<(u32, u32)>,
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

impl Engine {
    pub fn new() -> Self {
        Self {
            locale: Locale::en_us(),
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
            loader: Some(loader),
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
        let more = session
            .calc
            .prepare_step()
            .map_err(|error| ReadError::Engine(error.0))?;
        if !more {
            session.phase = LoadPhase::Ready;
        }
        Ok(more)
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
            rows: info
                .rows
                .rows
                .iter()
                .map(|&(row, _)| (row, info.rows.px(row)))
                .collect(),
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
        for (row, px) in &layout.rows {
            out.extend_from_slice(&[*row, *px]);
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
            out.push('}');
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
                ",\"x\":{},\"y\":{},\"width\":{},\"height\":{}}}",
                drawing.x, drawing.y, drawing.width, drawing.height
            ));
        }
        out.push(']');
        out
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
        };
        tile::edit_text(&input, &mut session.cache, sheet, row, col)
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
