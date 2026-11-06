//! Builds a tile: what each of its cells shows, in one theme's colours.

use std::collections::HashMap;

use cgs_calc::formualizer::FormualizerEngine;
use cgs_calc::{CalcEngine, CalcValue, CellSink, Rect as CalcRect, SheetRect};
use cgs_model::color::{ensure_contrast, MIN_CONTRAST};
use cgs_model::{
    Book, ColorSpace, DarkColors, Fill, HAlign, LineStyle, Pattern, ReadingOrder, Rect, Rgb,
    RichRun, SheetInfo, StyleTable, Underline, VAlign, Xf, MAX_COLS, MAX_ROWS, TILE_COLS,
    TILE_ROWS,
};
use cgs_numfmt::{FormatColor, NumFmt, Options, Value};

use crate::view_block::*;

/// The screen's own colours, which a tile's colours are worked out against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub dark: bool,
    pub background: Rgb,
    pub text: Rgb,
}

impl Screen {
    fn dark_colors(&self) -> Option<DarkColors> {
        self.dark.then_some(DarkColors {
            background: self.background,
            text: self.text,
        })
    }
}

/// How far overflowing text is looked for, in columns. One tile's width either side.
const OVERFLOW_REACH: u32 = TILE_COLS;
/// Longest text a tile carries for one cell, as Excel draws only so much anyway.
const MAX_CELL_TEXT: usize = 8192;

/// Colours and formats worked out once per theme, not once per cell.
#[derive(Debug, Default)]
pub struct TileCache {
    screen: Option<Screen>,
    fills: HashMap<u32, Option<Rgb>>,
    formats: HashMap<u32, NumFmt>,
    derived: Derived,
}

/// Where cell formats made here start, past any a file can hold.
const DERIVED: u32 = 1 << 30;
/// How many made formats are kept before starting again.
const DERIVED_LIMIT: usize = 50_000;

/// Cell formats with a table style's or a conditional format's changes laid in, made once for each
/// format and change. Their indexes start at `DERIVED`, and so do their fonts', fills' and borders'.
#[derive(Debug, Default)]
pub(crate) struct Derived {
    xfs: Vec<Xf>,
    fonts: Vec<cgs_model::Font>,
    fills: Vec<Fill>,
    borders: Vec<cgs_model::Border>,
    by_key: HashMap<(u32, bool, String), u32>,
}

/// Whether a font colour is the one text gets anyway, which a table style may replace.
fn plain_color(color: cgs_model::Color, styles: &StyleTable) -> bool {
    color.kind == cgs_model::ColorKind::Auto
        || styles.fonts.first().is_some_and(|font| font.color == color)
}

impl Derived {
    /// `base` with `dxf` laid under it (what the cell doesn't say, a table style does) or over it
    /// (a conditional format wins).
    fn derive(&mut self, styles: &StyleTable, base: u32, dxf: &cgs_model::Dxf, over: bool) -> u32 {
        let key = (base, over, format!("{dxf:?}"));
        if let Some(&index) = self.by_key.get(&key) {
            return index;
        }
        let xf = *styles.xf(base);
        let mut font = styles.font(&xf).clone();
        let mut fill = *styles.fill(&xf);
        let mut border = *styles.border(&xf);
        let d = &dxf.font;
        if over {
            font.bold = d.bold.unwrap_or(font.bold);
            font.italic = d.italic.unwrap_or(font.italic);
            font.strike = d.strike.unwrap_or(font.strike);
            font.underline = d.underline.unwrap_or(font.underline);
            font.color = d.color.unwrap_or(font.color);
        } else {
            font.bold = font.bold || d.bold == Some(true);
            font.italic = font.italic || d.italic == Some(true);
            font.strike = font.strike || d.strike == Some(true);
            if font.underline == Underline::None {
                font.underline = d.underline.unwrap_or(Underline::None);
            }
            if let Some(color) = d.color.filter(|_| plain_color(font.color, styles)) {
                font.color = color;
            }
        }
        if let Some(dxf_fill) = dxf.fill {
            if over || fill.pattern == Pattern::None {
                fill = match dxf_fill.pattern {
                    Pattern::Hatch(_) => Fill {
                        pattern: dxf_fill.pattern,
                        fg: dxf_fill.pattern_color,
                        bg: dxf_fill.color,
                    },
                    Pattern::None => Fill::default(),
                    Pattern::Solid => Fill {
                        pattern: Pattern::Solid,
                        fg: dxf_fill.color,
                        bg: cgs_model::Color::AUTO,
                    },
                };
            }
        }
        let side = |mine: &mut cgs_model::Line, theirs: Option<cgs_model::Line>| {
            if let Some(line) = theirs {
                if over || mine.style == LineStyle::None {
                    *mine = line;
                }
            }
        };
        side(&mut border.left, dxf.border.left);
        side(&mut border.right, dxf.border.right);
        side(&mut border.top, dxf.border.top);
        side(&mut border.bottom, dxf.border.bottom);
        let at = self.xfs.len() as u32;
        self.fonts.push(font);
        self.fills.push(fill);
        self.borders.push(border);
        self.xfs.push(Xf {
            font: DERIVED + at,
            fill: DERIVED + at,
            border: DERIVED + at,
            ..xf
        });
        self.by_key.insert(key, DERIVED + at);
        DERIVED + at
    }
}

/// The workbook's formats, with the ones made here alongside.
struct View<'a> {
    base: &'a StyleTable,
    derived: &'a Derived,
    colors: &'a ColorSpace,
    num_fmts: &'a HashMap<u32, String>,
}

impl View<'_> {
    fn xf(&self, index: u32) -> &Xf {
        match index.checked_sub(DERIVED) {
            Some(at) => &self.derived.xfs[at as usize],
            None => self.base.xf(index),
        }
    }

    fn font(&self, xf: &Xf) -> &cgs_model::Font {
        match xf.font.checked_sub(DERIVED) {
            Some(at) => &self.derived.fonts[at as usize],
            None => self.base.font(xf),
        }
    }

    fn fill(&self, xf: &Xf) -> &Fill {
        match xf.fill.checked_sub(DERIVED) {
            Some(at) => &self.derived.fills[at as usize],
            None => self.base.fill(xf),
        }
    }

    fn border(&self, xf: &Xf) -> &cgs_model::Border {
        match xf.border.checked_sub(DERIVED) {
            Some(at) => &self.derived.borders[at as usize],
            None => self.base.border(xf),
        }
    }
}

/// Which cells of an area take a made format, from the tables that cross it.
fn table_looks(
    book: &Book,
    info: &SheetInfo,
    sheet: u32,
    area: (u32, u32, u32, u32),
    derived: &mut Derived,
) -> HashMap<(u32, u32), u32> {
    let mut out = HashMap::new();
    let (top, left, bottom, right) = area;
    for table in book.tables.iter().filter(|table| table.sheet == sheet) {
        let t = table.range;
        if t.top > bottom || t.bottom < top || t.left > right || t.right < left {
            continue;
        }
        let Some(style) = table
            .style
            .name
            .as_deref()
            .and_then(|name| crate::table_style::find(&book.styles, name))
        else {
            continue;
        };
        for row in t.top.max(top)..=t.bottom.min(bottom) {
            for col in t.left.max(left)..=t.right.min(right) {
                let look = crate::table_style::exact_look(
                    crate::table_style::cell_look(table, style, row, col),
                    &book.styles.colors.theme,
                );
                if look.is_empty() {
                    continue;
                }
                let base = info.style_at(row, col);
                out.insert((row, col), derived.derive(&book.styles, base, &look, false));
            }
        }
    }
    out
}

fn rgba(rgb: Rgb) -> u32 {
    (rgb << 8) | 0xFF
}

fn fill_color(fill: &Fill, colors: &ColorSpace, screen: &Screen) -> Option<Rgb> {
    let dark = screen.dark_colors();
    match fill.pattern {
        Pattern::None => None,
        Pattern::Solid => Some(colors.resolve(fill.fg, dark).unwrap_or(screen.text)),
        // What the pattern is drawn on, which is the sheet's own if the file names nothing.
        Pattern::Hatch(_) => colors.resolve(fill.bg, dark),
    }
}

fn format_color(color: FormatColor, colors: &ColorSpace) -> Rgb {
    match color {
        FormatColor::Black => 0x000000,
        FormatColor::Blue => 0x0000FF,
        FormatColor::Cyan => 0x00FFFF,
        FormatColor::Green => 0x00FF00,
        FormatColor::Magenta => 0xFF00FF,
        FormatColor::Red => 0xFF0000,
        FormatColor::White => 0xFFFFFF,
        FormatColor::Yellow => 0xFFFF00,
        // [Color 1] is the palette's entry 8, the first a file can change.
        FormatColor::Indexed(number) => colors
            .indexed
            .get(usize::from(number) + 7)
            .copied()
            .unwrap_or(0),
    }
}

/// What a value shows and how, before it is placed in a cell.
#[derive(Debug)]
struct Shown {
    flags: u16,
    fill_at: u32,
    fill_char: char,
    color: Option<FormatColor>,
    /// The text's first strong character reads right to left, which is what General
    /// alignment goes by, whatever the cell's reading order says.
    starts_rtl: bool,
}

impl Default for Shown {
    fn default() -> Self {
        Self {
            flags: 0,
            fill_at: 0,
            fill_char: '\0',
            color: None,
            starts_rtl: false,
        }
    }
}

fn is_rtl_char(ch: char) -> bool {
    matches!(ch as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
}

/// Whether text reads right to left, by its first strong character.
pub fn starts_rtl(text: &str) -> bool {
    for ch in text.chars() {
        if is_rtl_char(ch) {
            return true;
        }
        if ch.is_alphabetic() {
            return false;
        }
    }
    false
}

fn truncate(text: &mut String) {
    if text.len() > MAX_CELL_TEXT {
        let mut end = MAX_CELL_TEXT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
}

fn format_for<'a>(
    formats: &'a mut HashMap<u32, NumFmt>,
    num_fmts: &HashMap<u32, String>,
    id: u32,
) -> &'a NumFmt {
    formats.entry(id).or_insert_with(|| {
        if let Some(code) = num_fmts.get(&id) {
            NumFmt::parse(code)
        } else {
            NumFmt::builtin(id).unwrap_or_else(NumFmt::general)
        }
    })
}

/// What a value shows. A `*x` fill's character goes into the text at its position, for the
/// tile worker to repeat once it can measure the column.
fn show_value(
    value: &CalcValue,
    format: &NumFmt,
    options: &Options<'_>,
    reading_order: ReadingOrder,
    out: &mut String,
) -> Shown {
    let mut shown = show_plain(value, format, options, reading_order, out);
    if shown.fill_char != '\0' {
        out.insert((shown.fill_at - 1) as usize, shown.fill_char);
    }
    shown.fill_char = '\0';
    shown
}

/// What a value shows, without its fill character.
fn show_plain(
    value: &CalcValue,
    format: &NumFmt,
    options: &Options<'_>,
    reading_order: ReadingOrder,
    out: &mut String,
) -> Shown {
    let mut shown = Shown::default();
    match value {
        CalcValue::Empty => {}
        CalcValue::Number(number) => {
            let display = format.format(Value::Number(*number), options, out);
            shown.flags = if display.general {
                KIND_GENERAL
            } else {
                KIND_NUMBER
            };
            if display.hashes {
                shown.flags |= CELL_HASHES;
            }
            shown.color = display.color;
            if let Some((ch, at)) = display.fill {
                shown.fill_at = at as u32 + 1;
                shown.fill_char = ch;
            }
        }
        CalcValue::Text(text) => {
            let display = format.format(Value::Text(text), options, out);
            shown.flags = KIND_TEXT;
            shown.color = display.color;
            if let Some((ch, at)) = display.fill {
                shown.fill_at = at as u32 + 1;
                shown.fill_char = ch;
            }
        }
        CalcValue::Bool(flag) => {
            out.push_str(if *flag { "TRUE" } else { "FALSE" });
            shown.flags = KIND_OTHER;
        }
        CalcValue::Error(text) => {
            out.push_str(text);
            shown.flags = KIND_OTHER;
        }
    }
    shown.starts_rtl = (shown.flags & KIND_MASK) == KIND_TEXT && starts_rtl(out);
    let rtl = match reading_order {
        ReadingOrder::RightToLeft => true,
        ReadingOrder::LeftToRight => false,
        ReadingOrder::Context => shown.starts_rtl,
    };
    if rtl {
        shown.flags |= CELL_RTL;
    }
    if out.is_empty() {
        shown.flags |= CELL_OCCUPIED;
    }
    truncate(out);
    shown
}

fn horizontal_code(align: HAlign, flags: u16, starts_rtl: bool) -> u8 {
    match align {
        HAlign::Left => 0,
        HAlign::Center => 1,
        HAlign::Right => 2,
        HAlign::Fill => 3,
        HAlign::Justify => 4,
        HAlign::CenterContinuous => 5,
        HAlign::Distributed => 6,
        HAlign::General => match flags & KIND_MASK {
            KIND_NUMBER | KIND_GENERAL => 2,
            KIND_OTHER => 1,
            _ if starts_rtl => 2,
            _ => 0,
        },
    }
}

fn vertical_code(align: VAlign) -> u8 {
    match align {
        VAlign::Top => 0,
        VAlign::Center => 1,
        VAlign::Bottom => 2,
        VAlign::Justify => 3,
        VAlign::Distributed => 4,
    }
}

fn underline_code(underline: Underline) -> u8 {
    match underline {
        Underline::None => 0,
        Underline::Single => 1,
        Underline::Double => 2,
        Underline::SingleAccounting => 3,
        Underline::DoubleAccounting => 4,
    }
}

fn line_code(style: LineStyle) -> u8 {
    style as u8
}

/// A style's full identity within one tile.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct StyleKey {
    font: u32,
    record: StyleRecord,
}

struct Styles<'a> {
    table: &'a View<'a>,
    screen: Screen,
    by_key: HashMap<StyleKey, u16>,
    by_name: HashMap<(String, StyleRecord), u16>,
}

impl Styles<'_> {
    fn line_color(&self, color: cgs_model::Color) -> u32 {
        match self.table.colors.resolve(color, self.screen.dark_colors()) {
            None => AUTOMATIC,
            Some(rgb) if self.screen.dark => rgba(ensure_contrast(rgb, self.screen.background)),
            Some(rgb) => rgba(rgb),
        }
    }

    /// The style a text cell needs, with its borders. Blank cells use font 0 so that
    /// blanks with the same borders share a style.
    #[allow(clippy::too_many_arguments)]
    fn index(
        &mut self,
        writer: &mut TileWriter,
        xf: &Xf,
        flags: u16,
        starts_rtl: bool,
        has_text: bool,
        right: Option<(LineStyle, cgs_model::Color)>,
        bottom: Option<(LineStyle, cgs_model::Color)>,
    ) -> u16 {
        let record = self.record(xf, flags, starts_rtl, has_text, right, bottom);
        let key = StyleKey {
            font: if has_text { xf.font } else { 0 },
            record,
        };
        if let Some(&index) = self.by_key.get(&key) {
            return index;
        }
        let name = if has_text {
            self.table.font(xf).name.as_str()
        } else {
            ""
        };
        let index = writer.style(name, record);
        self.by_key.insert(key, index);
        index
    }

    /// The style of one run of a rich text: its cell's, with the run's font.
    fn run_index(
        &mut self,
        writer: &mut TileWriter,
        xf: &Xf,
        flags: u16,
        starts_rtl: bool,
        font: &cgs_model::RunFont,
    ) -> u16 {
        let cell_font = self.table.font(xf);
        let mut record = self.record(xf, flags, starts_rtl, true, None, None);
        record.size = (f64::from(font.size.unwrap_or(cell_font.size)) * 100.0).round() as u16;
        record.font_flags = (u8::from(font.bold) * FONT_BOLD)
            | (u8::from(font.italic) * FONT_ITALIC)
            | (u8::from(font.strike) * FONT_STRIKE);
        record.underline = underline_code(font.underline.unwrap_or(Underline::None));
        let name = font.name.as_deref().unwrap_or(cell_font.name.as_str());
        if let Some(&index) = self.by_name.get(&(name.to_string(), record)) {
            return index;
        }
        let index = writer.style(name, record);
        self.by_name.insert((name.to_string(), record), index);
        index
    }

    fn record(
        &self,
        xf: &Xf,
        flags: u16,
        starts_rtl: bool,
        has_text: bool,
        right: Option<(LineStyle, cgs_model::Color)>,
        bottom: Option<(LineStyle, cgs_model::Color)>,
    ) -> StyleRecord {
        let border = self.table.border(xf);
        let mut record = StyleRecord::default();
        let fill = self.table.fill(xf);
        if let Pattern::Hatch(hatch) = fill.pattern {
            record.pattern = hatch as u8;
            let color = self
                .table
                .colors
                .resolve(fill.fg, self.screen.dark_colors())
                .unwrap_or(self.screen.text);
            record.pattern_color = rgba(color);
        }
        if has_text {
            let font = self.table.font(xf);
            record.size = (f64::from(font.size) * 100.0).round() as u16;
            record.font_flags = (u8::from(font.bold) * FONT_BOLD)
                | (u8::from(font.italic) * FONT_ITALIC)
                | (u8::from(font.strike) * FONT_STRIKE);
            record.underline = underline_code(font.underline);
            let alignment = xf.alignment;
            record.horizontal = horizontal_code(alignment.horizontal, flags, starts_rtl);
            record.vertical = vertical_code(alignment.vertical);
            record.indent = alignment.indent;
            record.rotation = alignment.rotation;
            record.layout = (u8::from(alignment.wrap) * LAYOUT_WRAP)
                | (u8::from(alignment.shrink) * LAYOUT_SHRINK);
        }
        // Text centred across a selection looks for empty cells that say the same.
        if !has_text && xf.alignment.horizontal == HAlign::CenterContinuous {
            record.horizontal = horizontal_code(HAlign::CenterContinuous, flags, false);
        }
        if let Some((style, color)) = right {
            record.right_line = line_code(style);
            record.right_color = self.line_color(color);
        }
        if let Some((style, color)) = bottom {
            record.bottom_line = line_code(style);
            record.bottom_color = self.line_color(color);
        }
        if border.diagonal.style != LineStyle::None {
            record.diagonal_line = line_code(border.diagonal.style);
            record.diagonal_color = self.line_color(border.diagonal.color);
            record.layout |= (u8::from(border.diagonal_up) * LAYOUT_DIAGONAL_UP)
                | (u8::from(border.diagonal_down) * LAYOUT_DIAGONAL_DOWN);
        }
        record
    }
}

/// The values around a tile, read once.
struct Window {
    top: u32,
    left: u32,
    width: u32,
    height: u32,
    cells: Vec<Option<CalcValue>>,
}

impl Window {
    fn new(top: u32, left: u32, width: u32, height: u32) -> Self {
        let mut cells = Vec::new();
        cells.resize_with((width * height) as usize, || None);
        Self {
            top,
            left,
            width,
            height,
            cells,
        }
    }

    fn get(&self, row: u32, col: u32) -> Option<&CalcValue> {
        if row < self.top || col < self.left {
            return None;
        }
        let (row, col) = (row - self.top, col - self.left);
        if row >= self.height || col >= self.width {
            return None;
        }
        self.cells[(row * self.width + col) as usize].as_ref()
    }
}

impl CellSink for Window {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        if row < self.top || col < self.left {
            return;
        }
        let (row, col) = (row - self.top, col - self.left);
        if row < self.height && col < self.width {
            self.cells[(row * self.width + col) as usize] = Some(value.clone());
        }
    }
}

fn pick_line(
    own: cgs_model::Line,
    other: cgs_model::Line,
) -> Option<(LineStyle, cgs_model::Color)> {
    if own.style != LineStyle::None {
        Some((own.style, own.color))
    } else if other.style != LineStyle::None {
        Some((other.style, other.color))
    } else {
        None
    }
}

/// Everything a tile is made from.
pub struct TileInput<'a> {
    pub book: &'a Book,
    pub calc: &'a FormualizerEngine,
    pub options: &'a Options<'a>,
    /// Today's date serial, for conditional formats about dates.
    pub today: f64,
}

fn row_span(info: &SheetInfo, from: u32, to: u32) -> i32 {
    // From `from` up to but not including `to`, and negative when `to` is first.
    if to < from {
        return -row_span(info, to, from);
    }
    let mut total = i64::from(to - from) * i64::from(info.rows.default_px);
    let at = info.rows.rows.partition_point(|&(row, _)| row < from);
    for &(row, line) in &info.rows.rows[at..] {
        if row >= to {
            break;
        }
        let px = if line.hidden {
            0
        } else {
            line.size.unwrap_or(info.rows.default_px)
        };
        total += i64::from(px) - i64::from(info.rows.default_px);
    }
    total.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn col_span(info: &SheetInfo, from: u32, to: u32) -> i32 {
    if to < from {
        return -col_span(info, to, from);
    }
    let mut total = i64::from(to - from) * i64::from(info.cols.default_px);
    for &(first, last, line) in &info.cols.ranges {
        let (start, end) = (first.max(from), (last + 1).min(to));
        if start >= end {
            continue;
        }
        let px = if line.hidden {
            0
        } else {
            line.size.unwrap_or(info.cols.default_px)
        };
        total += i64::from(end - start) * (i64::from(px) - i64::from(info.cols.default_px));
    }
    total.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// A rectangle's position and size, for a drawing or the app's layout.
pub fn sheet_x(info: &SheetInfo, col: u32) -> i64 {
    i64::from(col_span(info, 1, col))
}

pub fn sheet_y(info: &SheetInfo, row: u32) -> i64 {
    i64::from(row_span(info, 1, row))
}

/// Builds the tile whose top-left cell is (`first_row`, `first_col`), counted from 0.
pub fn build_tile(
    input: &TileInput<'_>,
    cache: &mut TileCache,
    sheet: u32,
    first_row: u32,
    first_col: u32,
    screen: Screen,
) -> ViewBlock {
    if cache.screen != Some(screen) {
        cache.screen = Some(screen);
        cache.fills.clear();
    }
    let info = &input.book.sheets[sheet as usize];
    let table = &input.book.styles;
    let rows = TILE_ROWS.min(MAX_ROWS.saturating_sub(first_row));
    let cols = TILE_COLS.min(MAX_COLS.saturating_sub(first_col));
    let top = first_row + 1;
    let left = first_col + 1;
    let row_px: Vec<u16> = (top..top + rows)
        .map(|row| info.rows.px(row).min(u32::from(u16::MAX)) as u16)
        .collect();
    let col_px: Vec<u16> = (left..left + cols)
        .map(|col| info.cols.px(col).min(u32::from(u16::MAX)) as u16)
        .collect();
    let mut writer = TileWriter::new(first_row, first_col, row_px, col_px);
    if !info.loaded {
        writer.set_flags(TILE_NOT_READY);
        for _ in 0..rows * cols {
            writer.cell("", CellRecord::default());
        }
        return writer.finish();
    }

    let bottom = top + rows - 1;
    let right = left + cols - 1;
    let window_left = left.saturating_sub(OVERFLOW_REACH).max(1);
    let window_right = (right + OVERFLOW_REACH).min(MAX_COLS);
    let mut window = Window::new(top, window_left, window_right - window_left + 1, rows);
    if top <= info.used.0 && window_left <= info.used.1 {
        input.calc.read_block(
            SheetRect {
                sheet,
                rect: CalcRect {
                    top,
                    left: window_left,
                    bottom: bottom.min(info.used.0),
                    right: window_right.min(info.used.1),
                },
            },
            &mut window,
        );
    }

    // Tables' styles, laid under the cells' own formats.
    if cache.derived.xfs.len() > DERIVED_LIMIT {
        cache.derived = Derived::default();
        cache.fills.retain(|&index, _| index < DERIVED);
    }
    let mut derived = std::mem::take(&mut cache.derived);
    let looks = table_looks(
        input.book,
        info,
        sheet,
        (top, window_left, (bottom + 1).min(MAX_ROWS), window_right),
        &mut derived,
    );
    // Conditional formats, laid over everything, with their bars and icons.
    let mut cf_marks: Vec<(u32, u32, MarkRecord)> = Vec::new();
    let mut hidden_values: std::collections::HashSet<(u32, u32)> = Default::default();
    let mut looks = looks;
    if !info.cf.is_empty() {
        let reader = crate::cf::CfReader::new(
            input.calc,
            table,
            sheet,
            &info.name,
            &info.cf,
            info.used,
            input.today,
        );
        let edge_bottom = (bottom + 1).min(MAX_ROWS);
        let edge_right = (right + 1).min(MAX_COLS);
        for row in top..=edge_bottom {
            for col in left..=edge_right {
                if !info.cf.iter().any(|block| block.covers(row, col)) {
                    continue;
                }
                let Some(cell) = reader.cell(row, col) else {
                    continue;
                };
                if let Some(dxf) = &cell.dxf {
                    let base = looks
                        .get(&(row, col))
                        .copied()
                        .unwrap_or_else(|| info.style_at(row, col));
                    looks.insert((row, col), derived.derive(table, base, dxf, true));
                }
                let inside = row <= bottom && col <= right;
                if inside {
                    if let Some(bar) = cell.bar {
                        let flags = (u8::from(bar.gradient) * BAR_GRADIENT)
                            | (u8::from(bar.border.is_some()) * BAR_BORDER)
                            | (u8::from(bar.axis.is_some()) * BAR_AXIS);
                        cf_marks.push((
                            row - top,
                            col - left,
                            MarkRecord {
                                kind: MARK_BAR,
                                flags,
                                icon: 0,
                                start: bar.start,
                                end: bar.end,
                                color: rgba(bar.color),
                                second: bar
                                    .border
                                    .map(rgba)
                                    .or(bar.axis.map(|_| rgba(0x000000)))
                                    .unwrap_or(0),
                            },
                        ));
                        if let Some(axis) = bar.axis {
                            if let Some(last) = cf_marks.last_mut() {
                                last.2.second = bar.border.map_or(rgba(0x000000), rgba);
                                let _ = axis;
                            }
                        }
                    }
                    if let Some((set, place)) = &cell.icon {
                        let number =
                            ICON_SETS.iter().position(|name| name == set).unwrap_or(0) as u16;
                        cf_marks.push((
                            row - top,
                            col - left,
                            MarkRecord {
                                kind: MARK_ICON,
                                icon: number * 8 + (*place as u16).min(7),
                                ..MarkRecord::default()
                            },
                        ));
                    }
                    if cell.hide_value {
                        hidden_values.insert((row, col));
                    }
                }
            }
        }
    }
    let view = View {
        base: table,
        derived: &derived,
        colors: &table.colors,
        num_fmts: &table.num_fmts,
    };
    let table = &view;
    let style_at = |row: u32, col: u32| -> u32 {
        looks
            .get(&(row, col))
            .copied()
            .unwrap_or_else(|| info.style_at(row, col))
    };
    let mut styles = Styles {
        table,
        screen,
        by_key: HashMap::new(),
        by_name: HashMap::new(),
    };
    let mut text = String::new();

    // Merged areas: which cells they cover, and the text each shows.
    let merges: Vec<Rect> = info.merges_in_tile(top, left).copied().collect();
    let merge_at = |row: u32, col: u32| merges.iter().find(|merge| merge.contains(row, col));
    let fill_of = |cache: &mut TileCache, xf_index: u32, xf: &Xf| -> Option<Rgb> {
        *cache
            .fills
            .entry(xf_index)
            .or_insert_with(|| fill_color(table.fill(xf), table.colors, &screen))
    };
    let shown_color = |base: Option<Rgb>, fill: Option<Rgb>| -> u32 {
        match (base, screen.dark, fill) {
            (None, false, _) | (None, true, None) => AUTOMATIC,
            (None, true, Some(fill)) => {
                // Automatic text on a pale fill stays the black it is everywhere else.
                if cgs_model::color::contrast(screen.text, fill) >= MIN_CONTRAST {
                    AUTOMATIC
                } else if cgs_model::color::contrast(0x000000, fill) >= MIN_CONTRAST {
                    rgba(0x000000)
                } else {
                    rgba(ensure_contrast(screen.text, fill))
                }
            }
            (Some(color), false, _) => rgba(color),
            (Some(color), true, fill) => {
                rgba(ensure_contrast(color, fill.unwrap_or(screen.background)))
            }
        }
    };
    let text_color = |xf: &Xf, shown: Option<FormatColor>, fill: Option<Rgb>| -> u32 {
        // A fill stays as stored in dark mode, so text on it can stay as stored too, where it reads.
        if let (None, true, Some(fill)) = (shown, screen.dark, fill) {
            if let Some(light) = table.colors.resolve(table.font(xf).color, None) {
                if cgs_model::color::contrast(light, fill) >= MIN_CONTRAST {
                    return rgba(light);
                }
            }
        }
        let base = match shown {
            Some(color) => Some(format_color(color, table.colors)),
            None => table
                .colors
                .resolve(table.font(xf).color, screen.dark_colors()),
        };
        shown_color(base, fill)
    };
    // The looks of a rich string, once its text is in the tile. A format that changes the text
    // leaves positions meaning nothing, so then it shows in the cell's font.
    let add_runs = |writer: &mut TileWriter,
                    styles: &mut Styles<'_>,
                    at: (u32, u32),
                    value: &CalcValue,
                    text: &str,
                    offset: u32,
                    xf: &Xf,
                    flags: u16,
                    starts_rtl: bool,
                    fill: Option<Rgb>,
                    base_color: u32| {
        let Some(runs) = info
            .rich
            .get(&at)
            .and_then(|index| input.book.rich_strings.get(index))
        else {
            return;
        };
        if !matches!(value, CalcValue::Text(stored) if stored == text) {
            return;
        }
        for (index, run) in runs.iter().enumerate() {
            let start = (run.start as usize).min(text.len());
            let end = runs.get(index + 1).map_or(text.len(), |next: &RichRun| {
                (next.start as usize).min(text.len())
            });
            if start >= end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                continue;
            }
            let (style, color) = match &run.font {
                Some(font) => {
                    let stored = font
                        .color
                        .and_then(|color| table.colors.resolve(color, screen.dark_colors()));
                    let light = font
                        .color
                        .and_then(|color| table.colors.resolve(color, None));
                    let color = match (font.color, light, fill) {
                        (Some(_), Some(light), Some(fill))
                            if screen.dark
                                && cgs_model::color::contrast(light, fill) >= MIN_CONTRAST =>
                        {
                            rgba(light)
                        }
                        (Some(_), _, _) => shown_color(stored, fill),
                        _ => base_color,
                    };
                    (styles.run_index(writer, xf, flags, starts_rtl, font), color)
                }
                None => (
                    styles.index(writer, xf, flags, starts_rtl, true, None, None),
                    base_color,
                ),
            };
            writer.run(offset, start as u32, (end - start) as u32, color, style);
        }
    };

    for row in top..=bottom {
        for col in left..=right {
            let merge = merge_at(row, col);
            let xf_index = match merge {
                Some(area) => style_at(area.top, area.left),
                None => style_at(row, col),
            };
            let xf = table.xf(xf_index);
            let fill = fill_of(cache, xf_index, xf);
            text.clear();
            let own_xf_index = if merge.is_some() {
                style_at(row, col)
            } else {
                xf_index
            };
            let own_xf = table.xf(own_xf_index);
            let mut flags = 0;
            let mut record = CellRecord {
                fill: fill.map_or(0, rgba),
                ..CellRecord::default()
            };
            let mut has_text = false;
            let mut text_starts_rtl = false;
            let mut value_here = None;
            if let Some(area) = merge {
                flags |= CELL_MERGED;
                if col < area.right {
                    flags |= CELL_NO_RIGHT_GRID;
                }
                if row < area.bottom {
                    flags |= CELL_NO_BOTTOM_GRID;
                }
            } else if let Some(value) = window
                .get(row, col)
                .filter(|_| !hidden_values.contains(&(row, col)))
            {
                let format = format_for(&mut cache.formats, table.num_fmts, xf.num_fmt);
                let shown = show_value(
                    value,
                    format,
                    input.options,
                    xf.alignment.reading_order,
                    &mut text,
                );
                flags |= shown.flags;
                let text_format = xf.num_fmt == 49
                    || table.num_fmts.get(&xf.num_fmt).map(String::as_str) == Some("@");
                if text_format && matches!(value, CalcValue::Number(_)) {
                    flags = (flags & !KIND_MASK) | KIND_TEXT;
                }
                record.fill_at = shown.fill_at;
                text_starts_rtl = shown.starts_rtl;
                has_text = !text.is_empty() || flags & CELL_HASHES != 0;
                record.color = text_color(xf, shown.color, fill);
                value_here = Some(value);
            }

            // Lines belong to the cell above and to the left of them, so each is drawn by
            // exactly one tile. Inside a merged area there are none.
            let border = table.border(own_xf);
            let right_line = if merge.is_some_and(|area| col < area.right) {
                None
            } else {
                let next = if col < MAX_COLS {
                    table.border(table.xf(style_at(row, col + 1))).left
                } else {
                    cgs_model::Line::default()
                };
                pick_line(border.right, next)
            };
            let bottom_line = if merge.is_some_and(|area| row < area.bottom) {
                None
            } else {
                let next = if row < MAX_ROWS {
                    table.border(table.xf(style_at(row + 1, col))).top
                } else {
                    cgs_model::Line::default()
                };
                pick_line(border.bottom, next)
            };
            record.flags = flags;
            record.style = styles.index(
                &mut writer,
                xf,
                flags,
                text_starts_rtl,
                has_text,
                right_line,
                bottom_line,
            );
            let offset = writer.cell(&text, record);
            if let Some(value) = value_here {
                add_runs(
                    &mut writer,
                    &mut styles,
                    (row, col),
                    value,
                    &text,
                    offset,
                    xf,
                    flags,
                    text_starts_rtl,
                    fill,
                    record.color,
                );
            }
        }
    }

    // A merged area's text, once per tile it touches.
    for area in &merges {
        let value = match window.get(area.top, area.left) {
            Some(value) => Some(value.clone()),
            None if area.top >= window.top
                && area.top < window.top + window.height
                && area.left >= window.left
                && area.left < window.left + window.width =>
            {
                None
            }
            None => {
                let mut one = Window::new(area.top, area.left, 1, 1);
                input.calc.read_block(
                    SheetRect {
                        sheet,
                        rect: CalcRect {
                            top: area.top,
                            left: area.left,
                            bottom: area.top,
                            right: area.left,
                        },
                    },
                    &mut one,
                );
                one.cells.pop().flatten()
            }
        };
        let Some(value) = value else { continue };
        let xf_index = style_at(area.top, area.left);
        let xf = table.xf(xf_index);
        let fill = fill_of(cache, xf_index, xf);
        text.clear();
        let format = format_for(&mut cache.formats, table.num_fmts, xf.num_fmt);
        let shown = show_value(
            &value,
            format,
            input.options,
            xf.alignment.reading_order,
            &mut text,
        );
        if text.is_empty() {
            continue;
        }
        let color = text_color(xf, shown.color, fill);
        let style = styles.index(
            &mut writer,
            xf,
            shown.flags,
            shown.starts_rtl,
            true,
            None,
            None,
        );
        let offset = writer.extra(
            &text,
            ExtraRecord {
                kind: EXTRA_MERGE,
                x: col_span(info, left, area.left),
                y: row_span(info, top, area.top),
                width: col_span(info, area.left, area.right + 1),
                height: row_span(info, area.top, area.bottom + 1),
                color,
                style,
                flags: shown.flags,
                fill_at: shown.fill_at,
            },
        );
        add_runs(
            &mut writer,
            &mut styles,
            (area.top, area.left),
            &value,
            &text,
            offset,
            xf,
            shown.flags,
            shown.starts_rtl,
            fill,
            color,
        );
    }

    // Text from either side that might overflow into this tile.
    let overflows_right = |h: u8| h == 0 || h == 1 || h == 5;
    let overflows_left = |h: u8| h == 2 || h == 1 || h == 5;
    let add_overflow = |row: u32,
                        col: u32,
                        value: &CalcValue,
                        going_right: bool,
                        cache: &mut TileCache,
                        writer: &mut TileWriter,
                        styles: &mut Styles<'_>,
                        text: &mut String| {
        if !matches!(value, CalcValue::Text(_)) {
            return;
        }
        let xf_index = style_at(row, col);
        let xf = table.xf(xf_index);
        let alignment = xf.alignment;
        if alignment.wrap || alignment.rotation != 0 || alignment.shrink {
            return;
        }
        text.clear();
        let format = format_for(&mut cache.formats, table.num_fmts, xf.num_fmt);
        let shown = show_value(value, format, input.options, alignment.reading_order, text);
        if text.is_empty() || shown.fill_at != 0 {
            return;
        }
        let horizontal = horizontal_code(alignment.horizontal, shown.flags, shown.starts_rtl);
        let fits = if going_right {
            overflows_right(horizontal)
        } else {
            overflows_left(horizontal)
        };
        if !fits {
            return;
        }
        let fill = fill_of(cache, xf_index, xf);
        let color = text_color(xf, shown.color, fill);
        let style = styles.index(writer, xf, shown.flags, shown.starts_rtl, true, None, None);
        let offset = writer.extra(
            text,
            ExtraRecord {
                kind: EXTRA_OVERFLOW,
                x: col_span(info, left, col),
                y: row_span(info, top, row),
                width: col_span(info, col, col + 1),
                height: i32::try_from(info.rows.px(row)).unwrap_or(0),
                color,
                style,
                flags: shown.flags,
                fill_at: 0,
            },
        );
        add_runs(
            writer,
            styles,
            (row, col),
            value,
            text,
            offset,
            xf,
            shown.flags,
            shown.starts_rtl,
            fill,
            color,
        );
    };
    for row in top..=bottom {
        if info.rows.px(row) == 0 {
            continue;
        }
        if merge_at(row, left).is_none() && window.get(row, left).is_none() {
            let mut col = left;
            while col > window_left {
                col -= 1;
                if info.merge_at(row, col).is_some() {
                    break;
                }
                if let Some(value) = window.get(row, col) {
                    add_overflow(
                        row,
                        col,
                        value,
                        true,
                        cache,
                        &mut writer,
                        &mut styles,
                        &mut text,
                    );
                    break;
                }
            }
        }
        if merge_at(row, right).is_none() && window.get(row, right).is_none() {
            let mut col = right;
            while col < window_right {
                col += 1;
                if info.merge_at(row, col).is_some() {
                    break;
                }
                if let Some(value) = window.get(row, col) {
                    add_overflow(
                        row,
                        col,
                        value,
                        false,
                        cache,
                        &mut writer,
                        &mut styles,
                        &mut text,
                    );
                    break;
                }
            }
        }
    }
    for (row, col, mark) in cf_marks {
        writer.mark(row, col, mark);
    }
    let tile = writer.finish();
    cache.derived = derived;
    tile
}

/// What one cell shows, for the formula bar and for tests against Excel.
pub fn display_text(
    input: &TileInput<'_>,
    cache: &mut TileCache,
    sheet: u32,
    row: u32,
    col: u32,
) -> String {
    let info = &input.book.sheets[sheet as usize];
    let table = &input.book.styles;
    let mut window = Window::new(row, col, 1, 1);
    input.calc.read_block(
        SheetRect {
            sheet,
            rect: CalcRect {
                top: row,
                left: col,
                bottom: row,
                right: col,
            },
        },
        &mut window,
    );
    let mut out = String::new();
    if let Some(value) = window.get(row, col) {
        let xf = table.xf(info.style_at(row, col));
        let format = format_for(&mut cache.formats, &table.num_fmts, xf.num_fmt);
        show_plain(
            value,
            format,
            input.options,
            xf.alignment.reading_order,
            &mut out,
        );
    }
    out
}

/// What the formula bar shows for a cell: its formula, or its constant in full.
/// A formula in a table, as Excel shows it there: its own columns without the table's name.
pub fn own_table_text(
    calc: &impl CalcEngine,
    sheet: u32,
    row: u32,
    col: u32,
    text: String,
) -> String {
    let text = match cgs_refs::table_refs_for_people(&text) {
        std::borrow::Cow::Owned(shown) => shown,
        std::borrow::Cow::Borrowed(_) => text,
    };
    let table = calc
        .tables()
        .iter()
        .find(|table| table.sheet == sheet && table.range.contains(row, col));
    match table.map(|table| cgs_refs::own_table_for_people(&text, &table.name)) {
        Some(std::borrow::Cow::Owned(shown)) => shown,
        _ => text,
    }
}

pub fn edit_text(
    input: &TileInput<'_>,
    cache: &mut TileCache,
    sheet: u32,
    row: u32,
    col: u32,
) -> String {
    let at = cgs_calc::CellRef { sheet, row, col };
    if let Some(formula) = input.calc.formula_text(at) {
        let formula = own_table_text(input.calc, sheet, row, col, formula);
        return format!("={}", cgs_refs::formula_for_people(&formula));
    }
    let info = &input.book.sheets[sheet as usize];
    let table = &input.book.styles;
    let mut window = Window::new(row, col, 1, 1);
    input.calc.read_block(
        SheetRect {
            sheet,
            rect: CalcRect {
                top: row,
                left: col,
                bottom: row,
                right: col,
            },
        },
        &mut window,
    );
    let mut out = String::new();
    let Some(value) = window.get(row, col) else {
        return out;
    };
    let xf = table.xf(info.style_at(row, col));
    let format = format_for(&mut cache.formats, &table.num_fmts, xf.num_fmt);
    match value {
        CalcValue::Empty => {}
        CalcValue::Number(number) => {
            cgs_numfmt::edit_text(Value::Number(*number), format, input.options, &mut out);
        }
        CalcValue::Text(text) => {
            cgs_numfmt::edit_text(Value::Text(text), format, input.options, &mut out);
        }
        CalcValue::Bool(flag) => {
            cgs_numfmt::edit_text(Value::Bool(*flag), format, input.options, &mut out);
        }
        CalcValue::Error(text) => {
            cgs_numfmt::edit_text(Value::Error(text), format, input.options, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgs_refs::formula_for_people;

    #[test]
    fn finds_direction_from_the_first_strong_character() {
        assert!(starts_rtl("مرحبا Hello"));
        assert!(starts_rtl("123 שלום"));
        assert!(!starts_rtl("Hello مرحبا"));
        assert!(!starts_rtl("123"));
        assert!(!starts_rtl(""));
    }

    #[test]
    fn newer_functions_lose_their_prefix() {
        assert_eq!(
            formula_for_people("_xlfn.CONCAT(A1,_xlfn._xlws.FILTER(B:B,C:C))"),
            "CONCAT(A1,FILTER(B:B,C:C))"
        );
    }

    #[test]
    fn a_parsed_formula_reads_as_excel_shows_it() {
        assert_eq!(formula_for_people("1 + 1"), "1+1");
        assert_eq!(
            formula_for_people(r#"IF(D1 > 100, "a b", 'My Sheet'!A1 - 1)"#),
            r#"IF(D1>100,"a b",'My Sheet'!A1-1)"#
        );
        assert_eq!(formula_for_people("A1:A3 B1:B3"), "A1:A3 B1:B3");
    }
}
