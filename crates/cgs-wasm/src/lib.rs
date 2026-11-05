//! The only JS-facing API. It runs in the engine worker, on web and on desktop.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

use cgs_calc::{Rect, SheetRect};
use cgs_engine::query::{applied_json, dirty_numbers};
use cgs_engine::{report_json, Area, LoadPhase, Screen};
use cgs_numfmt::Locale;
use cgs_xlsx::ByteSource;
use js_sys::{Function, Uint8Array};
use wasm_bindgen::prelude::*;

/// Small reads, such as the zip directory's, come out of one window of this size.
const WINDOW_BYTES: u64 = 64 * 1024;

/// Reads the file through a JS function `(offset, length) => Uint8Array`, which is
/// `FileReaderSync` on a `File`, or a synchronous range request on the desktop.
struct JsSource {
    read: Function,
    size: u64,
    window: RefCell<(u64, Vec<u8>)>,
}

impl JsSource {
    fn fetch(&self, offset: u64, length: u64) -> io::Result<Uint8Array> {
        let result = self
            .read
            .call2(
                &JsValue::NULL,
                &JsValue::from_f64(offset as f64),
                &JsValue::from_f64(length as f64),
            )
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        Ok(Uint8Array::from(result))
    }
}

impl ByteSource for JsSource {
    fn len(&self) -> u64 {
        self.size
    }

    fn read_at(&self, offset: u64, out: &mut [u8]) -> io::Result<usize> {
        if offset >= self.size || out.is_empty() {
            return Ok(0);
        }
        if out.len() as u64 >= WINDOW_BYTES {
            let bytes = self.fetch(offset, out.len() as u64)?;
            let count = (bytes.length() as usize).min(out.len());
            bytes.slice(0, count as u32).copy_to(&mut out[..count]);
            return Ok(count);
        }
        let mut window = self.window.borrow_mut();
        let inside = offset >= window.0 && offset < window.0 + window.1.len() as u64;
        if !inside {
            let start = offset / WINDOW_BYTES * WINDOW_BYTES;
            let bytes = self.fetch(start, WINDOW_BYTES)?;
            window.0 = start;
            window.1 = bytes.to_vec();
        }
        let from = (offset - window.0) as usize;
        let count = out.len().min(window.1.len().saturating_sub(from));
        out[..count].copy_from_slice(&window.1[from..from + count]);
        Ok(count)
    }
}

/// How much a save hands to JS at a time. It leaves WASM's memory there, so a large file costs
/// no more here than its largest changed part.
const WRITE_CHUNK: usize = 1024 * 1024;

/// Passes a save's bytes to a JS function `(chunk: Uint8Array) => void`.
struct JsSink {
    write: Function,
    buffer: Vec<u8>,
}

impl JsSink {
    fn send(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let chunk = Uint8Array::from(self.buffer.as_slice());
        self.buffer.clear();
        self.write
            .call1(&JsValue::NULL, &chunk)
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        Ok(())
    }
}

impl Write for JsSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() >= WRITE_CHUNK {
            self.send()?;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.send()
    }
}

fn js_source(read: Function, size: f64) -> Rc<dyn ByteSource> {
    Rc::new(JsSource {
        read,
        size: size as u64,
        window: RefCell::new((0, Vec::new())),
    })
}

#[wasm_bindgen]
pub struct Engine(cgs_engine::Engine);

fn to_array<const N: usize>(names: Vec<String>) -> Result<[String; N], JsError> {
    names
        .try_into()
        .map_err(|_| JsError::new("A locale's list of names has the wrong length."))
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine(cgs_engine::Engine::new())
    }

    /// Sets the regional conventions built-in formats follow. See `locale.ts`.
    #[allow(clippy::too_many_arguments)]
    pub fn set_locale(
        &mut self,
        decimal: String,
        group: String,
        short_date: String,
        long_date: String,
        short_time: String,
        long_time: String,
        am: String,
        pm: String,
        months: Vec<String>,
        months_short: Vec<String>,
        days: Vec<String>,
        days_short: Vec<String>,
        currency: String,
        date_separator: String,
        time_separator: String,
    ) -> Result<(), JsError> {
        self.0.set_locale(Locale {
            decimal,
            group,
            short_date,
            long_date,
            short_time,
            long_time,
            am,
            pm,
            months: to_array(months)?,
            months_short: to_array(months_short)?,
            days: to_array(days)?,
            days_short: to_array(days_short)?,
            currency,
            date_separator,
            time_separator,
        });
        Ok(())
    }

    /// Opens a workbook, read through `read(offset, length)`. Returns its sheets as JSON.
    pub fn open(&mut self, read: Function, size: f64) -> Result<String, JsError> {
        self.0
            .open(js_source(read, size))
            .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(self.0.book_json())
    }

    /// Whether the workbook differs from the file it came from.
    pub fn has_changes(&self) -> bool {
        self.0.has_changes()
    }

    /// Runs a command, given as JSON. See `Command` in `crates/cgs-engine/src/command.rs`. Returns
    /// what to select and what undo would do next, as JSON.
    pub fn apply_command(&mut self, json: &str) -> Result<String, JsError> {
        let applied = self
            .0
            .apply_json(json)
            .map_err(|error| JsError::new(&error))?;
        Ok(applied_json(
            applied.selection,
            applied.replaced,
            &self.0.undo_state(),
        ))
    }

    /// Rows waiting to be measured, as JSON. See `RowFit` in `packages/engine-client`.
    pub fn take_row_fits(&mut self) -> String {
        self.0.take_row_fits()
    }

    /// Rows from another program's file that wrap and hold no height, as JSON.
    pub fn opening_row_fits(&mut self, sheet: u32) -> String {
        self.0.opening_row_fits(sheet)
    }

    /// What to measure to fit columns, as JSON.
    pub fn col_fits(&mut self, sheet: u32, first: u32, last: u32) -> String {
        self.0.col_fits(sheet, first, last)
    }

    /// Whether merging would throw away anything. `areas` is JSON.
    pub fn merge_discards(&self, sheet: u32, areas: &str, across: bool) -> Result<bool, JsError> {
        self.0
            .merge_discards_json(sheet, areas, across)
            .map_err(|error| JsError::new(&error))
    }

    /// How a cell is formatted, as JSON.
    pub fn cell_format(&self, sheet: u32, row: u32, col: u32) -> String {
        self.0.cell_format_json(sheet, row, col)
    }

    /// The workbook's theme colours and its own number formats, as JSON.
    pub fn format_lists(&self) -> String {
        self.0.format_lists_json()
    }

    /// What a cell looks like in a format code, or nothing if the code can't be used.
    pub fn format_sample(&mut self, sheet: u32, row: u32, col: u32, code: &str) -> Option<String> {
        self.0.format_sample(sheet, row, col, code)
    }

    /// The next match after a cell, as JSON. `query` is JSON too. See `FindQuery`.
    pub fn find_next(
        &mut self,
        query: &str,
        row: u32,
        col: u32,
        back: bool,
    ) -> Result<String, JsError> {
        self.0
            .find_next_json(query, row, col, back)
            .map_err(|error| JsError::new(&error))
    }

    /// Every match, as JSON.
    pub fn find_all(&mut self, query: &str) -> Result<String, JsError> {
        self.0
            .find_all_json(query)
            .map_err(|error| JsError::new(&error))
    }

    /// What the UI hasn't been told about yet: see `dirty_numbers`.
    pub fn take_dirty(&mut self) -> Vec<u32> {
        dirty_numbers(&self.0.take_dirty())
    }

    /// Calculates for about `budget_ms`, with an area first if `sheet` isn't negative. Returns
    /// whether more remains.
    #[allow(clippy::too_many_arguments)]
    pub fn calc_step(
        &mut self,
        budget_ms: u32,
        sheet: i32,
        top: u32,
        left: u32,
        bottom: u32,
        right: u32,
    ) -> Result<bool, JsError> {
        let first: Vec<SheetRect> = if sheet >= 0 {
            vec![SheetRect {
                sheet: sheet as u32,
                rect: Rect {
                    top,
                    left,
                    bottom,
                    right,
                },
            }]
        } else {
            Vec::new()
        };
        self.0
            .calc_step(budget_ms, &first)
            .map_err(|error| JsError::new(&error))
    }

    pub fn is_calculating(&self) -> bool {
        self.0.is_calculating()
    }

    pub fn is_manual(&self) -> bool {
        self.0.is_manual()
    }

    pub fn set_manual(&mut self, manual: bool) {
        self.0.set_manual(manual);
    }

    /// F9, or Ctrl+Alt+F9 with `all`.
    pub fn calculate_now(&mut self, all: bool) -> Result<(), JsError> {
        self.0
            .calculate_now(all)
            .map_err(|error| JsError::new(&error))
    }

    /// Shift+F9.
    pub fn calculate_sheet(&mut self, sheet: u32) -> Result<(), JsError> {
        self.0
            .calculate_sheet(sheet)
            .map_err(|error| JsError::new(&error))
    }

    /// Today's date serial, for dates typed with no year.
    pub fn set_today(&mut self, serial: f64) {
        self.0.set_today(serial);
    }

    /// The clipboard's text and HTML for an area, as two strings.
    pub fn copy_text(
        &mut self,
        sheet: u32,
        top: u32,
        left: u32,
        bottom: u32,
        right: u32,
    ) -> Vec<String> {
        let (text, html) = self.0.copy_text(
            sheet,
            Area {
                top,
                left,
                bottom,
                right,
            },
        );
        vec![text, html]
    }

    /// A new workbook's cells from CSV text.
    pub fn load_csv(&mut self, text: &str) -> Result<(), JsError> {
        self.0.load_csv(text).map_err(|error| JsError::new(&error))
    }

    /// A sheet as CSV text.
    pub fn csv_text(&mut self, sheet: u32) -> String {
        self.0.csv_text(sheet)
    }

    /// Where Ctrl+arrow goes, as a row and a column.
    pub fn jump_target(&self, sheet: u32, row: u32, col: u32, rows: i32, cols: i32) -> Vec<u32> {
        let (row, col) = self.0.jump_target(sheet, row, col, rows, cols);
        vec![row, col]
    }

    /// What the name box's text refers to: sheet, top, left, bottom, right. Empty if nothing.
    pub fn resolve_name(&self, text: &str, current: u32) -> Vec<u32> {
        match self.0.resolve_name(text, current) {
            Some((sheet, area)) => vec![sheet, area.top, area.left, area.bottom, area.right],
            None => Vec::new(),
        }
    }

    pub fn function_names(&self) -> Vec<String> {
        self.0.function_names()
    }

    pub fn defined_names(&self) -> Vec<String> {
        self.0.defined_names()
    }

    /// Writes the workbook through `write(chunk)`, and returns its save report as JSON.
    pub fn save(&mut self, write: Function) -> Result<String, JsError> {
        let mut sink = JsSink {
            write,
            buffer: Vec::with_capacity(WRITE_CHUNK + 64 * 1024),
        };
        let report = self
            .0
            .save(&mut sink)
            .map_err(|error| JsError::new(&error.to_string()))?;
        sink.flush()
            .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(report_json(&report))
    }

    /// Checks what the last save wrote, before it replaces anything.
    pub fn verify_saved(&self, read: Function, size: f64) -> Result<(), JsError> {
        self.0
            .verify_saved(js_source(read, size).as_ref())
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// Reads from the saved file from now on, with nothing left to save.
    pub fn commit_saved(&mut self, read: Function, size: f64) -> Result<(), JsError> {
        self.0
            .commit_saved(js_source(read, size))
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// Reads from a copy of the file from now on.
    pub fn use_source(&mut self, read: Function, size: f64) -> Result<(), JsError> {
        self.0
            .use_source(js_source(read, size))
            .map_err(|error| JsError::new(&error.to_string()))
    }

    pub fn close(&mut self) {
        self.0.close();
    }

    /// Reads sheets for about `budget_ms`, and returns how far it has come as JSON.
    pub fn load_step(&mut self, budget_ms: f64) -> Result<String, JsError> {
        let status = self
            .0
            .load_step(budget_ms)
            .map_err(|error| JsError::new(&error.to_string()))?;
        let phase = match status.phase {
            LoadPhase::Reading => "reading",
            LoadPhase::Preparing => "preparing",
            LoadPhase::Ready => "ready",
        };
        let ready: Vec<String> = status.sheets_ready.iter().map(u32::to_string).collect();
        Ok(format!(
            "{{\"phase\":\"{phase}\",\"done\":{},\"total\":{},\"ready\":[{}]}}",
            status.done,
            status.total,
            ready.join(",")
        ))
    }

    /// Does one sheet of the first edit's work. Returns whether more remains.
    pub fn prepare_step(&mut self) -> Result<bool, JsError> {
        self.0
            .prepare_step()
            .map_err(|error| JsError::new(&error.to_string()))
    }

    pub fn prioritize(&mut self, sheet: u32) {
        self.0.prioritize(sheet);
    }

    pub fn book_json(&self) -> String {
        self.0.book_json()
    }

    pub fn layout_numbers(&self, sheet: u32) -> Vec<u32> {
        self.0.layout_numbers(sheet)
    }

    pub fn drawings_json(&self, sheet: u32) -> String {
        self.0.drawings_json(sheet)
    }

    /// The encoded `ViewBlock` tile whose top-left cell is (`first_row`, `first_col`),
    /// counted from 0. Colours are `0xRRGGBB`.
    pub fn view_block(
        &mut self,
        sheet: u32,
        first_row: u32,
        first_col: u32,
        dark: bool,
        background: u32,
        text: u32,
    ) -> Vec<u8> {
        self.0
            .view_block(
                sheet,
                first_row,
                first_col,
                Screen {
                    dark,
                    background,
                    text,
                },
            )
            .into_bytes()
    }

    /// What a cell shows. `row` and `col` are 1-based.
    pub fn display_text(&mut self, sheet: u32, row: u32, col: u32) -> String {
        self.0.display_text(sheet, row, col)
    }

    /// What the formula bar shows for a cell. `row` and `col` are 1-based.
    pub fn edit_text(&mut self, sheet: u32, row: u32, col: u32) -> String {
        self.0.edit_text(sheet, row, col)
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// The bytes of a new blank workbook.
#[wasm_bindgen]
pub fn blank_workbook() -> Result<Vec<u8>, JsError> {
    cgs_xlsx::blank::blank_workbook().map_err(|error| JsError::new(&error.to_string()))
}

/// The crate version this module was built from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
