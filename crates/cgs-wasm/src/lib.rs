//! The only JS-facing API. It runs in the engine worker, on web and on desktop.
#![cfg(target_arch = "wasm32")]

mod alloc;

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

    /// Answers a question about the workbook, given and answered as JSON. See `Question` in
    /// `crates/cgs-engine/src/ask.rs`.
    pub fn query(&self, question: &str) -> Result<String, JsError> {
        self.0.query(question).map_err(|error| JsError::new(&error))
    }

    /// Rows waiting to be measured, as JSON. See `RowFit` in `packages/engine-client`.
    pub fn take_row_fits(&mut self) -> String {
        self.0.take_row_fits()
    }

    /// Rows from another program's file that wrap and hold no height, as JSON.
    pub fn opening_row_fits(&mut self, sheet: u32) -> String {
        self.0.opening_row_fits(sheet)
    }

    /// What to measure to fit columns, as JSON: the `keep` longest texts by font in each column,
    /// or 256 when `keep` is 0.
    pub fn col_fits(&mut self, sheet: u32, first: u32, last: u32, keep: u32) -> String {
        self.0.col_fits_keeping(sheet, first, last, keep)
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

    /// Starts a save that `save_step` writes a slice at a time. Commands and calculation are
    /// turned away until it ends; tiles and reads are served between slices.
    pub fn save_begin(&mut self) -> Result<(), JsError> {
        self.0
            .save_begin()
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// Writes for about `budget_ms` through `write(chunk)`. Returns the save report as JSON once it
    /// has all been written, and an empty string until then.
    pub fn save_step(&mut self, budget_ms: f64, write: Function) -> Result<String, JsError> {
        let mut sink = JsSink {
            write,
            buffer: Vec::with_capacity(WRITE_CHUNK + 64 * 1024),
        };
        let done = self
            .0
            .save_step(budget_ms, &mut sink)
            .map_err(|error| JsError::new(&error.to_string()))?;
        sink.flush()
            .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(done.map(|report| report_json(&report)).unwrap_or_default())
    }

    /// How far a save or its check has come: done and total, in bytes of work.
    pub fn save_progress(&self) -> Vec<f64> {
        let (done, total) = self.0.save_progress();
        vec![done as f64, total as f64]
    }

    pub fn is_saving(&self) -> bool {
        self.0.is_saving()
    }

    /// Stops a save or its check.
    pub fn save_cancel(&mut self) {
        self.0.save_cancel();
    }

    /// Starts checking what the save wrote, a slice at a time.
    pub fn verify_begin(&mut self, read: Function, size: f64) -> Result<(), JsError> {
        self.0
            .verify_begin(js_source(read, size))
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// Checks for about `budget_ms`. Returns whether the check is over and passed.
    pub fn verify_step(&mut self, budget_ms: f64) -> Result<bool, JsError> {
        self.0
            .verify_step(budget_ms)
            .map_err(|error| JsError::new(&error.to_string()))
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

    /// The pivot tables on a sheet, as JSON: see `PivotBox` in `packages/engine-client`.
    pub fn pivot_list(&self, sheet: u32) -> String {
        self.0.pivot_list(sheet)
    }

    /// The pivot table at a cell, as JSON: see `PivotInfo` in `packages/engine-client`.
    pub fn pivot_info(&mut self, sheet: u32, row: u32, col: u32) -> Option<String> {
        self.0.pivot_info(sheet, row, col)
    }

    /// What a chart drawing shows, as JSON: see `ChartView` in `packages/engine-client`.
    /// Colours are `0xRRGGBB`.
    pub fn chart_json(
        &self,
        sheet: u32,
        drawing: u32,
        dark: bool,
        background: u32,
        text: u32,
    ) -> String {
        self.0.chart_json(
            sheet,
            drawing,
            Screen {
                dark,
                background,
                text,
            },
        )
    }

    /// A part's bytes, such as a picture's.
    pub fn media(&self, part: &str) -> Vec<u8> {
        self.0.media(part)
    }

    /// Takes a picture's bytes for an `insertPicture` command, and names its part.
    pub fn add_media(&mut self, bytes: Vec<u8>) -> Option<String> {
        self.0.add_media(bytes)
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

    /// Starts staging ERP record pages for a new binding, against the entity's schema as JSON.
    pub fn erp_begin(&mut self, schema: &str) -> Result<(), JsError> {
        self.0
            .erp_begin(schema)
            .map_err(|error| JsError::new(&error))
    }

    /// Starts staging a refresh of the open workbook's ERP records.
    pub fn erp_begin_refresh(&mut self) -> Result<(), JsError> {
        self.0
            .erp_begin_refresh()
            .map_err(|error| JsError::new(&error))
    }

    /// Reads one Query page from its bytes, transferred, so no record is built in JavaScript.
    /// Answers how many records it held and what comes next, as JSON.
    pub fn erp_page(&mut self, bytes: &[u8]) -> Result<String, JsError> {
        self.0.erp_page(bytes).map_err(|error| JsError::new(&error))
    }

    /// Drops whatever ERP records were staged.
    pub fn erp_cancel(&mut self) {
        self.0.erp_cancel();
    }

    /// Binds the staged records into the open, new workbook. `session` is Redeem's answer
    /// without its token. Answers the binding, as `binding_json` does.
    pub fn erp_bind(&mut self, session: &str) -> Result<String, JsError> {
        self.0
            .erp_bind(session)
            .map_err(|error| JsError::new(&error))
    }

    /// Merges the staged refresh with the bound records, three ways. Answers what it did, as
    /// JSON, with `journal`, what a recovery draft needs to replay it.
    pub fn erp_refresh(&mut self) -> Result<String, JsError> {
        self.0.erp_refresh().map_err(|error| JsError::new(&error))
    }

    /// Resolve Conflicts: `{choices: [{key, field, take}]}` or `{all}`, `take` and `all` being
    /// `mine` or `erp`. Answers `{fields, records, left}`.
    pub fn erp_resolve(&mut self, json: &str) -> Result<String, JsError> {
        self.0
            .erp_resolve(json)
            .map_err(|error| JsError::new(&error))
    }

    /// The open workbook's binding as a recovery draft's bind payload, if it has one.
    pub fn erp_draft(&self) -> Option<String> {
        self.0.erp_draft()
    }

    /// Whether the bound table holds changes, formulas or conflicts that wait on the ERP.
    pub fn erp_pending(&self) -> bool {
        self.0.erp_pending()
    }

    /// Binds a recovery draft's records into the open, new workbook. Answers the binding.
    pub fn erp_restore(&mut self, json: &str) -> Result<String, JsError> {
        self.0
            .erp_restore(json)
            .map_err(|error| JsError::new(&error))
    }

    /// Replays a refresh a recovery draft kept.
    pub fn erp_replay_refresh(&mut self, json: &str) -> Result<(), JsError> {
        self.0
            .erp_replay_refresh(json)
            .map_err(|error| JsError::new(&error))
    }

    /// Leaves nothing to undo from setting ERP data up, such as its fitted column widths.
    pub fn erp_settle(&mut self) {
        self.0.erp_settle();
    }

    /// Holds every write to the bound table while a commit is in flight, or lets them go.
    pub fn erp_freeze(&mut self, on: bool) {
        self.0.erp_freeze(on);
    }

    /// Takes in what Cash Gear ERP answered to a dry run or a commit, `{sent, errors,
    /// conflicts}`: errors on their cells, and conflicts merged three ways with the cells.
    pub fn erp_set_states(&mut self, json: &str) -> Result<String, JsError> {
        self.0
            .erp_set_states(json)
            .map_err(|error| JsError::new(&error))
    }

    /// Applies a commit's answer, `{sent, records}`, and lets writes go again.
    pub fn erp_apply_commit(&mut self, json: &str) -> Result<String, JsError> {
        self.0
            .erp_apply_commit(json)
            .map_err(|error| JsError::new(&error))
    }

    /// The open workbook's ERP binding as JSON, or `null`.
    pub fn binding_json(&self) -> String {
        self.0.binding_json()
    }

    /// What the binding keeps in memory, as JSON, for `bench/erp-speed.mjs` (plan B.6).
    pub fn erp_memory(&self) -> String {
        self.0.erp_memory_json()
    }

    /// The bound records' ERP ids, ascending.
    pub fn binding_ids(&self) -> Vec<f64> {
        self.0.binding_ids()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// The bytes of a new blank workbook, whose sheet is right to left if `right_to_left`.
#[wasm_bindgen]
pub fn blank_workbook(right_to_left: bool) -> Result<Vec<u8>, JsError> {
    cgs_xlsx::blank::blank_workbook_directed(right_to_left)
        .map_err(|error| JsError::new(&error.to_string()))
}

/// A PDF of recorded pages. `document` is the JSON `cgs_pdf::Document` describes, and `blob`
/// holds the font files and pictures it points into.
#[wasm_bindgen]
pub fn write_pdf(document: &str, blob: &[u8]) -> Result<Vec<u8>, JsError> {
    let doc: cgs_pdf::Document =
        serde_json::from_str(document).map_err(|error| JsError::new(&error.to_string()))?;
    cgs_pdf::write_pdf(&doc, blob).map_err(|error| JsError::new(&error))
}

/// The crate version this module was built from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
