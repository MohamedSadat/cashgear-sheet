//! The only JS-facing API. It runs in the engine worker, on web and on desktop.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use cgs_engine::{LoadPhase, Screen};
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
        let source = JsSource {
            read,
            size: size as u64,
            window: RefCell::new((0, Vec::new())),
        };
        self.0
            .open(Rc::new(source))
            .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(self.0.book_json())
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

/// The crate version this module was built from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
