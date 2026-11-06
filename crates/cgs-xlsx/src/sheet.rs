//! A worksheet part, read a little at a time: values and formulas go to the formula engine
//! as they pass, and everything else to the sheet's model. The state lives here between
//! slices, so the engine worker can answer other messages in between.

use std::io::BufReader;

use cgs_calc::{wants_sparse, CalcEngine, FormulaShape, LoadEntry, LoadValue, Rect, SheetShape};
use cgs_model::style::{base_col_width_to_px, col_width_to_px, points_to_px};
use cgs_model::{Freeze, LineInfo, SheetInfo, SheetView};
use cgs_refs::SharedFormula;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use rustc_hash::FxHashMap;

use crate::package::PartStream;
use crate::strings::SharedStrings;
use crate::xml::{
    attr, attr_bool, attr_f64, attr_u32, each_attr, parse_bool, parse_cell, parse_rect, parse_u32,
    push_text, unescape_x,
};
use crate::ReadError;

/// Rows sent to the engine in one call.
const BATCH_CELLS: usize = 8 * 1024;
/// Dense storage reserves this many cells before any file is too sparse for it.
const MAX_DENSE_AREA: u64 = 256_000_000;
/// More text than one sheet can hold in WASM's memory, which a crafted file can ask for by
/// pointing many cells at one huge shared string.
const MAX_SHEET_TEXT: u64 = 1_500_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellType {
    Number,
    Shared,
    Text,
    Inline,
    Bool,
    Error,
    Date,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormulaType {
    Normal,
    Shared,
    Array,
    DataTable,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Pending {
    Number(f64),
    Bool(bool),
    /// A shared string's index.
    Shared(u32),
    /// A byte range of `heap`.
    Text(u32, u32),
    Error(u32, u32),
}

/// What became of a slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    More,
    Done,
    /// The file broke the assumed shape: start this sheet again with this one.
    Restart(SheetShape),
}

/// Set in a format's height when it wraps or turns its text, which has to be measured.
pub const WRAPS: u32 = 1 << 31;

/// What a loader needs each time it runs.
pub struct LoadContext<'a> {
    pub engine: &'a mut dyn CalcEngine,
    pub strings: &'a SharedStrings,
    /// The widest digit of the Normal font, in pixels at 100%.
    pub digit: f64,
    /// How tall a row must be for each cell format's font, in pixels at 100%.
    pub xf_heights: &'a [u32],
    /// The Normal style's font, for rich runs that only set a size.
    pub normal_font: &'a cgs_model::Font,
}

pub struct SheetLoader {
    /// A formula here has no saved value.
    pub formula_without_value: bool,
    sheet: u32,
    reader: Reader<BufReader<PartStream>>,
    buf: Vec<u8>,
    /// The part's uncompressed size, for guessing how many cells it holds.
    part_size: u64,
    shape: Option<SheetShape>,
    forced_sparse: bool,
    min_cols: u32,

    in_sheet_data: bool,
    finished: bool,
    last_row: u32,
    row: u32,
    col: u32,
    in_cell: bool,
    cell_style: u32,
    cell_type: CellType,
    cell_dynamic: bool,
    has_value: bool,
    in_value: bool,
    value: String,
    has_inline: bool,
    in_inline_text: bool,
    in_phonetic: bool,
    inline: String,
    has_formula: bool,
    in_formula: bool,
    formula_text: String,
    formula_type: FormulaType,
    formula_ref: String,
    formula_si: Option<u32>,

    /// Master formulas by shared index, with where they are stored.
    shared: FxHashMap<u32, (SharedFormula, u32, u32)>,
    /// Followers that came before their master: (row, column, shared index).
    orphans: Vec<(u32, u32, u32)>,
    /// Arrays whose area hasn't ended yet.
    arrays: Vec<Rect>,
    scratch: String,

    pending: Vec<(u32, u32, Pending)>,
    heap: String,
    row_ended: bool,
    /// The row has a stored height, so its fonts don't decide it.
    row_has_height: bool,
    row_tallest: u32,
    text_bytes: u64,
    hidden: Vec<u32>,
    pub drawing_rel: Option<String>,
    /// An element being kept whole: its name, how deep inside it this is, and its XML so far.
    capture: Option<(Vec<u8>, u32, quick_xml::Writer<Vec<u8>>)>,
}

/// Worksheet elements kept whole as they're read, for what reads them as a whole.
const CAPTURED: &[&[u8]] = &[
    b"autoFilter",
    b"conditionalFormatting",
    b"dataValidations",
    b"hyperlinks",
    b"extLst",
];

fn entry<'a>(
    row: u32,
    col: u32,
    pending: Pending,
    heap: &'a str,
    strings: &'a SharedStrings,
) -> Option<LoadEntry<'a>> {
    let value = match pending {
        Pending::Number(number) => LoadValue::Number(number),
        Pending::Bool(flag) => LoadValue::Bool(flag),
        Pending::Shared(index) => LoadValue::Text(strings.get(index as usize)?),
        Pending::Text(from, to) => LoadValue::Text(heap.get(from as usize..to as usize)?),
        Pending::Error(from, to) => LoadValue::Error(heap.get(from as usize..to as usize)?),
    };
    Some(LoadEntry { row, col, value })
}

/// An ISO 8601 date (`t="d"`) as a serial, in the 1900 system.
fn iso_serial(text: &str) -> Option<f64> {
    if !text.contains('-') {
        return iso_time(text);
    }
    let (date, time) = text.split_once('T').unwrap_or((text, ""));
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days since 1970, from Howard Hinnant's civil calendar algorithm.
    let shifted = if month <= 2 { year - 1 } else { year };
    let era = shifted.div_euclid(400);
    let of_era = shifted - era * 400;
    let of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    let days = era * 146_097 + of_cycle - 719_468;
    let mut serial = (days + 25_569) as f64;
    if serial < 61.0 {
        serial -= 1.0;
    }
    if !time.is_empty() {
        serial += iso_time(time)?;
    }
    Some(serial)
}

/// `10:30:15.5` as a share of a day.
fn iso_time(text: &str) -> Option<f64> {
    let mut clock = text.trim_end_matches('Z').split(':');
    let hours: f64 = clock.next()?.parse().ok()?;
    let minutes: f64 = clock.next().unwrap_or("0").parse().ok()?;
    let seconds: f64 = clock.next().unwrap_or("0").parse().ok()?;
    Some((hours + minutes / 60.0 + seconds / 3600.0) / 24.0)
}

impl SheetLoader {
    pub fn new(sheet: u32, stream: PartStream, part_size: u64) -> Self {
        Self::with_shape(sheet, stream, part_size, false, 0)
    }

    fn with_shape(
        sheet: u32,
        stream: PartStream,
        part_size: u64,
        forced_sparse: bool,
        min_cols: u32,
    ) -> Self {
        let mut reader = Reader::from_reader(BufReader::with_capacity(128 * 1024, stream));
        reader.config_mut().check_end_names = false;
        Self {
            sheet,
            reader,
            buf: Vec::with_capacity(4096),
            part_size,
            shape: None,
            forced_sparse,
            min_cols,
            in_sheet_data: false,
            finished: false,
            last_row: 0,
            row: 0,
            col: 0,
            in_cell: false,
            cell_style: 0,
            cell_type: CellType::Number,
            cell_dynamic: false,
            formula_without_value: false,
            has_value: false,
            in_value: false,
            value: String::new(),
            has_inline: false,
            in_inline_text: false,
            in_phonetic: false,
            inline: String::new(),
            has_formula: false,
            in_formula: false,
            formula_text: String::new(),
            formula_type: FormulaType::Normal,
            formula_ref: String::new(),
            formula_si: None,
            shared: FxHashMap::default(),
            orphans: Vec::new(),
            arrays: Vec::new(),
            scratch: String::new(),
            pending: Vec::with_capacity(BATCH_CELLS),
            heap: String::new(),
            row_ended: false,
            row_has_height: false,
            row_tallest: 0,
            text_bytes: 0,
            hidden: Vec::new(),
            drawing_rel: None,
            capture: None,
        }
    }

    /// The same sheet from the start, in a shape that fits what the last attempt found.
    pub fn restarted(sheet: u32, stream: PartStream, part_size: u64, shape: SheetShape) -> Self {
        Self::with_shape(sheet, stream, part_size, shape.sparse, shape.cols)
    }

    /// Compressed bytes taken from the file so far.
    pub fn consumed(&self) -> u64 {
        self.reader.get_ref().get_ref().consumed()
    }

    fn estimated_cells(&self) -> u64 {
        // A typical `<c r="A1" s="1"><v>123</v></c>` runs 30 to 40 bytes.
        self.part_size / 32
    }

    fn begin_cells(
        &mut self,
        context: &mut LoadContext<'_>,
        info: &mut SheetInfo,
    ) -> Result<(), ReadError> {
        let (rows, cols) = info.used;
        let rows = rows.max(1);
        let cols = cols.max(self.min_cols).max(1);
        let sparse = self.forced_sparse || wants_sparse(self.estimated_cells(), rows, cols);
        let shape = SheetShape { rows, cols, sparse };
        context
            .engine
            .begin_sheet(self.sheet, shape)
            .map_err(|error| ReadError::Engine(error.0))?;
        self.shape = Some(shape);
        info.used = (0, 0);
        Ok(())
    }

    pub fn sheet_index(&self) -> u32 {
        self.sheet
    }

    /// Runs until the budget says stop. `keep_going` is asked after each batch of cells.
    pub fn step(
        &mut self,
        context: &mut LoadContext<'_>,
        info: &mut SheetInfo,
        keep_going: &mut dyn FnMut() -> bool,
    ) -> Result<Step, ReadError> {
        if self.finished {
            return Ok(Step::Done);
        }
        let mut buf = std::mem::take(&mut self.buf);
        let result = self.run(&mut buf, context, info, keep_going);
        buf.clear();
        self.buf = buf;
        result
    }

    fn run(
        &mut self,
        buf: &mut Vec<u8>,
        context: &mut LoadContext<'_>,
        info: &mut SheetInfo,
        keep_going: &mut dyn FnMut() -> bool,
    ) -> Result<Step, ReadError> {
        loop {
            buf.clear();
            // A worksheet damaged part way keeps what it read, as Excel's repair does.
            let event = self.reader.read_event_into(buf).unwrap_or(Event::Eof);
            let mut restart = None;
            // Inside an element kept whole, everything goes into its XML.
            if let Some((_, depth, writer)) = self.capture.as_mut() {
                let _ = writer.write_event(event.borrow());
                match &event {
                    Event::Start(_) => *depth += 1,
                    Event::End(_) => {
                        *depth -= 1;
                        if *depth == 0 {
                            let (name, _, writer) = self.capture.take().expect("capturing");
                            keep_captured(name, writer.into_inner(), info);
                        }
                    }
                    Event::Eof => {
                        self.capture = None;
                        self.finish(context, info)?;
                        return Ok(Step::Done);
                    }
                    _ => {}
                }
                continue;
            }
            if !self.in_sheet_data {
                if let Event::Start(element) | Event::Empty(element) = &event {
                    let local = element.local_name().as_ref().to_vec();
                    if CAPTURED.contains(&local.as_slice()) {
                        let mut writer = quick_xml::Writer::new(Vec::new());
                        let _ = writer.write_event(event.borrow());
                        if matches!(event, Event::Start(_)) {
                            self.capture = Some((local, 1, writer));
                        } else {
                            keep_captured(local, writer.into_inner(), info);
                        }
                        continue;
                    }
                }
            }
            match event {
                Event::Eof => {
                    self.finish(context, info)?;
                    return Ok(Step::Done);
                }
                Event::Start(ref element) => {
                    let name = element.local_name();
                    if self.in_sheet_data {
                        restart = self.start_data_element(name.as_ref(), element, info, false);
                    } else if name.as_ref() == b"sheetData" {
                        self.in_sheet_data = true;
                        self.begin_cells(context, info)?;
                    } else {
                        read_head_element(name.as_ref(), element, info, context.digit);
                        read_tail_element(name.as_ref(), element, info, &mut self.drawing_rel);
                    }
                }
                Event::Empty(ref element) => {
                    let name = element.local_name();
                    if self.in_sheet_data {
                        restart = self.start_data_element(name.as_ref(), element, info, true);
                    } else if name.as_ref() != b"sheetData" {
                        read_head_element(name.as_ref(), element, info, context.digit);
                        read_tail_element(name.as_ref(), element, info, &mut self.drawing_rel);
                    }
                }
                Event::End(ref element) => {
                    if self.in_sheet_data {
                        match element.local_name().as_ref() {
                            // A damaged file can close a cell it never opened.
                            b"c" if self.in_cell => {
                                let needs = context.xf_heights.get(self.cell_style as usize);
                                self.row_tallest =
                                    self.row_tallest.max(needs.copied().unwrap_or(0) & !WRAPS);
                                if self.cell_type == CellType::Shared && self.has_value {
                                    if let Ok(index) = self.value.trim().parse::<u32>() {
                                        if let Some(runs) = context.strings.runs(index) {
                                            info.rich.insert((self.row, self.col), index);
                                            // A run of a bigger font makes the row taller, as in Excel.
                                            self.row_tallest = self.row_tallest.max(
                                                cgs_model::style::rich_row_px(
                                                    runs,
                                                    context.normal_font,
                                                ),
                                            );
                                        }
                                    }
                                }
                                // Only the bit a wrapping format sets, so most cells pay nothing.
                                if !self.row_has_height
                                    && needs.is_some_and(|px| px & WRAPS != 0)
                                    && info.unfitted_rows.last() != Some(&self.row)
                                {
                                    info.unfitted_rows.push(self.row);
                                }
                                self.finish_cell(context)?;
                                self.in_cell = false;
                            }
                            b"f" => self.in_formula = false,
                            b"v" => self.in_value = false,
                            b"t" => self.in_inline_text = false,
                            b"rPh" => self.in_phonetic = false,
                            b"row" => {
                                self.row_ended = true;
                                self.fit_row(info);
                            }
                            b"sheetData" => {
                                self.in_sheet_data = false;
                                self.flush(context)?;
                            }
                            _ => {}
                        }
                    }
                }
                ref other => {
                    if self.in_cell {
                        if self.in_value {
                            push_text(other, &mut self.value);
                        } else if self.in_formula {
                            push_text(other, &mut self.formula_text);
                        } else if self.in_inline_text && !self.in_phonetic {
                            push_text(other, &mut self.inline);
                        }
                    }
                }
            }
            if let Some(shape) = restart {
                return Ok(Step::Restart(shape));
            }
            if self.row_ended {
                self.row_ended = false;
                if self.pending.len() >= BATCH_CELLS {
                    self.flush(context)?;
                    if !keep_going() {
                        return Ok(Step::More);
                    }
                }
            }
        }
    }

    /// Handles a `<row>`, `<c>` or something inside a cell. May ask for a restart.
    fn start_data_element(
        &mut self,
        name: &[u8],
        element: &BytesStart<'_>,
        info: &mut SheetInfo,
        empty: bool,
    ) -> Option<SheetShape> {
        match name {
            b"row" => return self.start_row(element, info),
            b"c" => return self.start_cell(element, info, empty),
            b"f" if self.in_cell => {
                self.has_formula = true;
                self.in_formula = !empty;
                self.formula_type = FormulaType::Normal;
                self.formula_ref.clear();
                self.formula_si = None;
                each_attr(element, |key, value| match key {
                    b"t" => {
                        self.formula_type = match value {
                            b"shared" => FormulaType::Shared,
                            b"array" => FormulaType::Array,
                            b"dataTable" => FormulaType::DataTable,
                            _ => FormulaType::Normal,
                        };
                    }
                    b"ref" => self.formula_ref.push_str(&String::from_utf8_lossy(value)),
                    b"si" => self.formula_si = parse_u32(value),
                    _ => {}
                });
            }
            b"v" if self.in_cell => {
                self.has_value = true;
                self.in_value = !empty;
            }
            b"is" if self.in_cell => self.has_inline = true,
            b"t" if self.has_inline => self.in_inline_text = !empty,
            b"rPh" if self.has_inline => self.in_phonetic = !empty,
            _ => {}
        }
        None
    }

    /// A row with no stored height is as tall as its largest font, as Excel fits it.
    fn fit_row(&mut self, info: &mut SheetInfo) {
        if self.row_has_height || self.row_tallest <= info.rows.default_px {
            return;
        }
        let row = self.row;
        match info.rows.rows.last_mut() {
            Some((last, line)) if *last == row => line.size = Some(self.row_tallest),
            _ => info.rows.rows.push((
                row,
                LineInfo {
                    size: Some(self.row_tallest),
                    ..LineInfo::default()
                },
            )),
        }
    }

    fn start_row(&mut self, element: &BytesStart<'_>, info: &mut SheetInfo) -> Option<SheetShape> {
        let mut number = None;
        let mut height = None;
        let mut hidden = false;
        let mut style = None;
        let mut custom_format = false;
        let mut custom_height = false;
        each_attr(element, |key, value| match key {
            b"r" => number = parse_u32(value),
            b"customHeight" => custom_height = parse_bool(value),
            b"ht" => {
                height = std::str::from_utf8(value)
                    .ok()
                    .and_then(|text| text.parse::<f64>().ok());
            }
            b"hidden" => hidden = parse_bool(value),
            b"s" => style = parse_u32(value),
            b"customFormat" => custom_format = parse_bool(value),
            _ => {}
        });
        let row = number.unwrap_or(self.row + 1);
        self.col = 0;
        self.row_has_height = height.is_some();
        self.row_tallest = 0;
        // Rows out of order can't go into the row-by-row builder, so the sheet starts
        // over and goes in cell by cell, which takes them in any order.
        if row < self.last_row {
            if let Some(shape) = self.shape.filter(|shape| !shape.sparse) {
                return Some(SheetShape {
                    sparse: true,
                    ..shape
                });
            }
        }
        self.row = row;
        self.last_row = self.last_row.max(row);
        if hidden {
            self.hidden.push(row);
        }
        if let Some(shape) = self.shape.filter(|shape| !shape.sparse) {
            // A dense sheet that keeps growing past what its size can justify.
            let area = u64::from(row) * u64::from(shape.cols);
            if area > MAX_DENSE_AREA && wants_sparse(self.estimated_cells(), row, shape.cols) {
                return Some(SheetShape {
                    sparse: true,
                    rows: row,
                    ..shape
                });
            }
        }
        let size = height.map(points_to_px);
        let style = style.filter(|_| custom_format);
        // A fixed height stays known even at the default, as a later edit mustn't refit that row.
        if hidden
            || style.is_some()
            || custom_height
            || size.is_some_and(|size| size != info.rows.default_px)
        {
            info.rows.rows.push((
                row,
                LineInfo {
                    size,
                    hidden,
                    style,
                    custom: custom_height,
                },
            ));
        }
        None
    }

    fn start_cell(
        &mut self,
        element: &BytesStart<'_>,
        info: &mut SheetInfo,
        empty: bool,
    ) -> Option<SheetShape> {
        let mut named = false;
        let mut at = None;
        let mut style = 0;
        let mut cell_type = CellType::Number;
        let mut dynamic = false;
        each_attr(element, |key, value| match key {
            b"r" => {
                named = true;
                at = parse_cell(value);
            }
            b"s" => style = parse_u32(value).unwrap_or(0),
            b"t" => {
                cell_type = match value {
                    b"s" => CellType::Shared,
                    b"str" => CellType::Text,
                    b"inlineStr" => CellType::Inline,
                    b"b" => CellType::Bool,
                    b"e" => CellType::Error,
                    b"d" => CellType::Date,
                    _ => CellType::Number,
                };
            }
            b"cm" => dynamic = true,
            _ => {}
        });
        match at {
            Some((row, col)) => {
                self.row = row;
                self.col = col;
            }
            None => self.col += 1,
        }
        if self.row == 0 {
            self.row = 1;
        }
        // An address off the sheet: skip the cell, as there's nowhere to put it.
        let off_sheet = self.col > cgs_model::MAX_COLS || self.row > cgs_model::MAX_ROWS;
        if off_sheet || (named && at.is_none()) {
            self.in_cell = false;
            return None;
        }
        if let Some(shape) = self.shape.filter(|shape| !shape.sparse) {
            if self.col > shape.cols {
                // The file's dimension was wrong. Grow generously, as there may be more.
                let cols = self.col.max(shape.cols * 2).min(cgs_model::MAX_COLS);
                return Some(SheetShape {
                    cols,
                    sparse: wants_sparse(
                        self.estimated_cells(),
                        self.last_row.max(shape.rows),
                        cols,
                    ),
                    ..shape
                });
            }
            if self.row < self.last_row {
                return Some(SheetShape {
                    sparse: true,
                    ..shape
                });
            }
        }
        self.last_row = self.last_row.max(self.row);
        info.set_style(self.row, self.col, style);
        info.used.0 = info.used.0.max(self.row);
        info.used.1 = info.used.1.max(self.col);
        self.in_cell = !empty;
        self.cell_style = style;
        self.cell_type = cell_type;
        self.cell_dynamic = dynamic;
        self.has_value = false;
        self.has_inline = false;
        self.has_formula = false;
        self.in_value = false;
        self.in_formula = false;
        self.in_inline_text = false;
        self.value.clear();
        self.inline.clear();
        self.formula_text.clear();
        None
    }

    fn push_heap(&mut self, text: &str) -> (u32, u32) {
        let from = self.heap.len() as u32;
        self.heap.push_str(text);
        (from, self.heap.len() as u32)
    }

    fn stored_value(&mut self) -> Option<Pending> {
        if self.has_inline {
            let text = std::mem::take(&mut self.inline);
            let (from, to) = self.push_heap(&unescape_x(&text));
            self.inline = text;
            return Some(Pending::Text(from, to));
        }
        if !self.has_value {
            // Excel stores an empty string result as `t="str"` with no text.
            if self.cell_type == CellType::Text && self.has_formula {
                let (from, to) = self.push_heap("");
                return Some(Pending::Text(from, to));
            }
            return None;
        }
        let value = std::mem::take(&mut self.value);
        let stored = match self.cell_type {
            CellType::Number => value.trim().parse().ok().map(Pending::Number),
            CellType::Shared => value.trim().parse().ok().map(Pending::Shared),
            CellType::Bool => {
                let text = value.trim();
                Some(Pending::Bool(
                    text == "1" || text.eq_ignore_ascii_case("true"),
                ))
            }
            CellType::Error => {
                let (from, to) = self.push_heap(value.trim());
                Some(Pending::Error(from, to))
            }
            CellType::Date => iso_serial(value.trim()).map(Pending::Number),
            CellType::Text | CellType::Inline => {
                let (from, to) = self.push_heap(&unescape_x(&value));
                Some(Pending::Text(from, to))
            }
        };
        self.value = value;
        stored
    }

    fn finish_cell(&mut self, context: &mut LoadContext<'_>) -> Result<(), ReadError> {
        let stored = self.stored_value();
        let (row, col) = (self.row, self.col);
        if let Some(pending) = stored {
            self.pending.push((row, col, pending));
        }
        if self.has_formula {
            if stored.is_none() && self.formula_type != FormulaType::DataTable {
                self.formula_without_value = true;
            }
            self.stage_formula(context)?;
        } else if !self.arrays.is_empty() && stored.is_some() {
            // The other cells of an array are the array's output, not constants.
            self.arrays.retain(|area| area.bottom >= row);
            if self.arrays.iter().any(|area| area.contains(row, col)) {
                context
                    .engine
                    .mark_spill(self.sheet, row, col)
                    .map_err(|error| ReadError::Engine(error.0))?;
            }
        }
        Ok(())
    }

    fn stage_formula(&mut self, context: &mut LoadContext<'_>) -> Result<(), ReadError> {
        let (row, col) = (self.row, self.col);
        let sheet = self.sheet;
        let engine_error = |error: cgs_calc::CalcError| ReadError::Engine(error.0);
        // A formula with no text, other than a shared follower, has nothing to stage.
        if self.formula_text.is_empty() && self.formula_type != FormulaType::Shared {
            return Ok(());
        }
        match self.formula_type {
            // What-if cells: their value stands, and nothing can recalculate them.
            FormulaType::DataTable => Ok(()),
            FormulaType::Array => {
                let shape = match parse_rect(&self.formula_ref) {
                    Some(area) => {
                        let rect = Rect {
                            top: area.top,
                            left: area.left,
                            bottom: area.bottom,
                            right: area.right,
                        };
                        // The anchor itself holds the formula, so it is never a spill.
                        self.arrays.push(rect);
                        if self.cell_dynamic {
                            FormulaShape::Dynamic(rect)
                        } else {
                            FormulaShape::Array(rect)
                        }
                    }
                    None => FormulaShape::Normal,
                };
                context
                    .engine
                    .load_formula(sheet, row, col, &self.formula_text, shape)
                    .map_err(engine_error)
            }
            FormulaType::Shared => {
                let Some(index) = self.formula_si else {
                    return Ok(());
                };
                if !self.formula_text.is_empty() {
                    context
                        .engine
                        .load_formula(sheet, row, col, &self.formula_text, FormulaShape::Normal)
                        .map_err(engine_error)?;
                    self.shared
                        .insert(index, (SharedFormula::new(&self.formula_text), row, col));
                    return Ok(());
                }
                let Some((master, master_row, master_col)) = self.shared.get(&index) else {
                    self.orphans.push((row, col, index));
                    return Ok(());
                };
                self.scratch.clear();
                master.render(
                    i64::from(row) - i64::from(*master_row),
                    i64::from(col) - i64::from(*master_col),
                    &mut self.scratch,
                );
                context
                    .engine
                    .load_formula(sheet, row, col, &self.scratch, FormulaShape::Normal)
                    .map_err(engine_error)
            }
            FormulaType::Normal => context
                .engine
                .load_formula(sheet, row, col, &self.formula_text, FormulaShape::Normal)
                .map_err(engine_error),
        }
    }

    fn flush(&mut self, context: &mut LoadContext<'_>) -> Result<(), ReadError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let heap = std::mem::take(&mut self.heap);
        let entries: Vec<LoadEntry<'_>> = self
            .pending
            .iter()
            .filter_map(|&(row, col, value)| entry(row, col, value, &heap, context.strings))
            .collect();
        for entry in &entries {
            if let LoadValue::Text(text) = entry.value {
                self.text_bytes += text.len() as u64;
            }
        }
        if self.text_bytes > MAX_SHEET_TEXT {
            return Err(ReadError::TooLarge);
        }
        let result = context
            .engine
            .load_rows(self.sheet, &entries)
            .map_err(|error| ReadError::Engine(error.0));
        drop(entries);
        self.pending.clear();
        self.heap = heap;
        self.heap.clear();
        result
    }

    fn finish(
        &mut self,
        context: &mut LoadContext<'_>,
        info: &mut SheetInfo,
    ) -> Result<(), ReadError> {
        if self.shape.is_none() {
            self.begin_cells(context, info)?;
        }
        self.flush(context)?;
        // Conditional formats, now that all of their elements are in.
        let classic: Vec<&[u8]> = info
            .captured
            .iter()
            .filter(|(name, _)| name == "conditionalFormatting")
            .map(|(_, xml)| xml.as_slice())
            .collect();
        let ext = info
            .captured
            .iter()
            .find(|(name, _)| name == "extLst")
            .map(|(_, xml)| xml.as_slice());
        info.cf = crate::cf::read_cf(&classic, ext);
        for (row, col, index) in std::mem::take(&mut self.orphans) {
            if let Some((master, master_row, master_col)) = self.shared.get(&index) {
                self.scratch.clear();
                master.render(
                    i64::from(row) - i64::from(*master_row),
                    i64::from(col) - i64::from(*master_col),
                    &mut self.scratch,
                );
                context
                    .engine
                    .load_formula(self.sheet, row, col, &self.scratch, FormulaShape::Normal)
                    .map_err(|error| ReadError::Engine(error.0))?;
            }
        }
        context
            .engine
            .finish_sheet(self.sheet, &self.hidden)
            .map_err(|error| ReadError::Engine(error.0))?;
        info.rows.rows.sort_by_key(|&(row, _)| row);
        info.rows.rows.dedup_by_key(|&mut (row, _)| row);
        info.index_merges();
        info.loaded = true;
        self.finished = true;
        self.shared = FxHashMap::default();
        Ok(())
    }
}

fn read_head_element(name: &[u8], element: &BytesStart<'_>, info: &mut SheetInfo, digit: f64) {
    match name {
        b"dimension" => {
            if let Some(area) = attr(element, b"ref").and_then(|text| parse_rect(&text)) {
                info.used = (area.bottom, area.right);
            }
        }
        b"sheetView" => {
            info.view = SheetView {
                zoom: attr_u32(element, b"zoomScale")
                    .filter(|&zoom| zoom > 0)
                    .unwrap_or(100)
                    .clamp(10, 400) as u16,
                show_grid: attr_bool(element, b"showGridLines").unwrap_or(true),
                show_headers: attr_bool(element, b"showRowColHeaders").unwrap_or(true),
                right_to_left: attr_bool(element, b"rightToLeft").unwrap_or(false),
                freeze: None,
                top_left: attr(element, b"topLeftCell")
                    .and_then(|text| parse_cell(text.as_bytes()))
                    .unwrap_or((1, 1)),
                active_cell: (1, 1),
            };
        }
        b"pane" => {
            let state = attr(element, b"state");
            if matches!(state.as_deref(), Some("frozen" | "frozenSplit")) {
                let (top, left) = info.view.top_left;
                let rows = attr_f64(element, b"ySplit").unwrap_or(0.0) as u32;
                let cols = attr_f64(element, b"xSplit").unwrap_or(0.0) as u32;
                info.view.freeze = Some(Freeze {
                    rows,
                    cols,
                    top,
                    left,
                });
                info.view.top_left = attr(element, b"topLeftCell")
                    .and_then(|text| parse_cell(text.as_bytes()))
                    .unwrap_or((top + rows, left + cols));
            }
        }
        b"selection" => {
            let cell = attr(element, b"activeCell").and_then(|text| parse_cell(text.as_bytes()));
            if let Some(cell) = cell {
                info.view.active_cell = cell;
            }
        }
        b"sheetFormatPr" => {
            let height = attr_f64(element, b"defaultRowHeight").unwrap_or(15.0);
            let hidden = attr_bool(element, b"zeroHeight").unwrap_or(false);
            info.rows.default_px = if hidden {
                0
            } else {
                points_to_px(height).max(1)
            };
            info.cols.default_px = match attr_f64(element, b"defaultColWidth") {
                Some(width) => col_width_to_px(width, digit),
                None => {
                    base_col_width_to_px(attr_f64(element, b"baseColWidth").unwrap_or(8.0), digit)
                }
            };
        }
        b"col" => {
            let Some(first) = attr_u32(element, b"min") else {
                return;
            };
            let last = attr_u32(element, b"max").unwrap_or(first);
            let width = attr_f64(element, b"width");
            let hidden = attr_bool(element, b"hidden").unwrap_or(false)
                || width.is_some_and(|width| width == 0.0);
            info.cols.ranges.push((
                first,
                last.min(cgs_model::MAX_COLS),
                LineInfo {
                    size: width.map(|width| col_width_to_px(width, digit)),
                    hidden,
                    style: attr_u32(element, b"style"),
                    custom: attr_bool(element, b"customWidth").unwrap_or(false),
                },
            ));
        }
        _ => {}
    }
}

/// What a kept element is for: an autofilter is read now, and the rest wait for their readers.
fn keep_captured(name: Vec<u8>, xml: Vec<u8>, info: &mut SheetInfo) {
    if name == b"autoFilter" {
        if info.filter.is_none() {
            info.filter = crate::filter::read_auto_filter(&xml);
        }
        return;
    }
    info.captured
        .push((String::from_utf8_lossy(&name).into_owned(), xml));
}

fn read_tail_element(
    name: &[u8],
    element: &BytesStart<'_>,
    info: &mut SheetInfo,
    drawing: &mut Option<String>,
) {
    match name {
        b"mergeCell" => {
            if let Some(area) = attr(element, b"ref").and_then(|text| parse_rect(&text)) {
                if area.top != area.bottom || area.left != area.right {
                    info.merges.push(area);
                }
            }
        }
        b"drawing" => *drawing = attr(element, b"id"),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_iso_dates_in_the_1900_system() {
        assert_eq!(iso_serial("2024-01-15"), Some(45306.0));
        assert_eq!(iso_serial("1900-03-01"), Some(61.0));
        assert_eq!(iso_serial("2024-01-15T12:00:00Z"), Some(45306.5));
        assert_eq!(iso_serial("12:00:00"), Some(0.5));
        assert_eq!(iso_serial("nonsense"), None);
    }
}
