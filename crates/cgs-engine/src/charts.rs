//! Charts as the sheet draws them. Each series reads the cells its `c:f` names, live, and the
//! chart's colours, fonts and axis scales are worked out here as Excel works them out, so the UI
//! only draws. Dark mode's swaps happen on the way out and never touch the model.

use cgs_calc::{CalcEngine, CalcValue, CellSink, Rect as CalcRect, SheetRect};
use cgs_model::chart::*;
use cgs_model::color::{to_hls, ThemePalette};
use cgs_model::{Book, Rgb};
use cgs_numfmt::{NumFmt, Options, Value};
use serde::Serialize;

use crate::tile::Screen;

/// Points in a CSS pixel.
const PX_PER_PT: f64 = 96.0 / 72.0;
/// The most points a series draws, as Excel's own limit for a 2-D series is 32,000.
const MAX_POINTS: usize = 32_000;

// ---------------------------------------------------------------------------------------------
// Colours

/// Turns a chart's colours into screen colours, with dark mode's swaps.
pub struct Painter<'a> {
    pub theme: &'a ThemePalette,
    pub screen: Option<Screen>,
}

fn scheme_index(name: &str) -> Option<usize> {
    Some(match name {
        "lt1" | "bg1" => 0,
        "dk1" | "tx1" => 1,
        "lt2" | "bg2" => 2,
        "dk2" | "tx2" => 3,
        "accent1" => 4,
        "accent2" => 5,
        "accent3" => 6,
        "accent4" => 7,
        "accent5" => 8,
        "accent6" => 9,
        "hlink" => 10,
        "folHlink" => 11,
        _ => return None,
    })
}

/// HSL to RGB, rounding each channel, as DrawingML's luminance changes come out in Excel.
fn from_hls(h: f64, l: f64, s: f64) -> Rgb {
    let channel = |value: f64| ((value * 255.0).round() as u32).min(255);
    if s == 0.0 {
        let v = channel(l);
        return (v << 16) | (v << 8) | v;
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |t: f64| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (channel(hue(h + 1.0 / 3.0)) << 16) | (channel(hue(h)) << 8) | channel(hue(h - 1.0 / 3.0))
}

fn apply_mods(rgb: Rgb, mods: &[ColorMod]) -> (Rgb, f64) {
    if mods.is_empty() {
        return (rgb, 1.0);
    }
    let (mut h, mut l, mut s) = to_hls(rgb);
    let mut alpha = 1.0;
    for change in mods {
        let value = f64::from(change.value) / 100_000.0;
        match change.kind {
            ColorModKind::LumMod => l *= value,
            ColorModKind::LumOff => l += value,
            ColorModKind::Tint => l = l * value + (1.0 - value),
            ColorModKind::Shade => l *= value,
            ColorModKind::SatMod => s *= value,
            ColorModKind::Alpha => alpha = value,
        }
        l = l.clamp(0.0, 1.0);
        s = s.clamp(0.0, 1.0);
    }
    h = h.rem_euclid(1.0);
    (from_hls(h, l, s), alpha.clamp(0.0, 1.0))
}

fn css(rgb: Rgb, alpha: f64) -> String {
    if alpha >= 0.999 {
        format!("#{rgb:06X}")
    } else {
        format!(
            "rgba({},{},{},{:.3})",
            (rgb >> 16) & 0xFF,
            (rgb >> 8) & 0xFF,
            rgb & 0xFF,
            alpha
        )
    }
}

fn mix(from: Rgb, to: Rgb, share: f64) -> Rgb {
    let channel = |shift: u32| {
        let a = f64::from((from >> shift) & 0xFF);
        let b = f64::from((to >> shift) & 0xFF);
        ((a + (b - a) * share).round() as u32).min(255) << shift
    };
    channel(16) | channel(8) | channel(0)
}

impl Painter<'_> {
    /// A colour on screen. In dark mode, colours made from the text and background colours keep
    /// their lightness as a share of the way from the screen's text to its background, so a light
    /// grey gridline stays a quiet line on a dark chart.
    pub fn color(&self, color: &DmlColor) -> String {
        let (base, neutral) = match &color.base {
            DmlBase::Rgb(value) => (*value, false),
            DmlBase::Scheme(name) => {
                let index = scheme_index(name).unwrap_or(4);
                (self.theme.0[index], index <= 1)
            }
            DmlBase::System(value) => (*value, true),
            DmlBase::Preset(value) => (*value, false),
        };
        let (rgb, alpha) = apply_mods(base, &color.mods);
        match self.screen.filter(|screen| screen.dark) {
            Some(screen) if neutral => {
                let (_, light, _) = to_hls(rgb);
                css(mix(screen.text, screen.background, light), alpha)
            }
            _ => css(rgb, alpha),
        }
    }

    fn fill(&self, fill: &ChartFill) -> Option<String> {
        match fill {
            ChartFill::None => None,
            ChartFill::Solid(color) | ChartFill::Gradient(color) => Some(self.color(color)),
            ChartFill::Other(color) => color.as_ref().map(|color| self.color(color)),
        }
    }
}

/// The text colour Excel uses for a chart whose file says none: `tx1` at 65% for charts in the
/// look of Excel 2013 and later, which state their colours anyway, and black for older ones.
fn default_text() -> DmlColor {
    DmlColor::scheme("tx1")
}

fn default_line() -> DmlColor {
    // Excel 2007's style 2 draws axes and gridlines in tx1 at 50%.
    DmlColor::scheme("tx1")
        .with(ColorModKind::LumMod, 50_000)
        .with(ColorModKind::LumOff, 50_000)
}

/// Excel's colours for series that don't state one, by chart style: the six accents, then the
/// same six darker and lighter, as `colors1.xml`'s variations go.
pub fn auto_color(style: u8, index: usize, count: usize) -> DmlColor {
    const VARIATIONS: [(i32, i32); 9] = [
        (100_000, 0),
        (60_000, 0),
        (80_000, 20_000),
        (80_000, 0),
        (60_000, 40_000),
        (50_000, 0),
        (70_000, 30_000),
        (70_000, 0),
        (50_000, 50_000),
    ];
    let family = (style.max(1) - 1) % 8;
    match family {
        // Greys.
        0 => {
            let share = if count <= 1 {
                0.5
            } else {
                index as f64 / (count - 1) as f64
            };
            let off = (15_000.0 + share * 60_000.0) as i32;
            DmlColor::scheme("tx1")
                .with(ColorModKind::LumMod, 100_000 - off)
                .with(ColorModKind::LumOff, off)
        }
        // One accent, from dark to light.
        2..=7 => {
            let accent = format!("accent{}", family - 1);
            let share = if count <= 1 {
                0.5
            } else {
                index as f64 / (count - 1) as f64
            };
            if share < 0.5 {
                DmlColor::scheme(&accent)
                    .with(ColorModKind::Shade, (50_000.0 + share * 100_000.0) as i32)
            } else {
                DmlColor::scheme(&accent).with(
                    ColorModKind::Tint,
                    (100_000.0 - (share - 0.5) * 110_000.0) as i32,
                )
            }
        }
        _ => {
            let accent = format!("accent{}", index % 6 + 1);
            let (lum_mod, lum_off) = VARIATIONS[(index / 6) % VARIATIONS.len()];
            let mut color = DmlColor::scheme(&accent);
            if lum_mod != 100_000 {
                color = color.with(ColorModKind::LumMod, lum_mod);
            }
            if lum_off != 0 {
                color = color.with(ColorModKind::LumOff, lum_off);
            }
            color
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Axis scaling, as Excel 2016 does it. `bench/run-chart-axes.mjs` measured 920 charts: this
// matches the bounds of all of them and the major unit of 917.

/// The smallest of 1, 2 and 5 times a power of ten that is at least `x`.
fn nice_unit(x: f64) -> f64 {
    if !(x.is_finite() && x > 0.0) {
        return 1.0;
    }
    let power = 10f64.powf(x.log10().floor());
    let mantissa = x / power;
    let step = if mantissa <= 1.0 + 1e-9 {
        1.0
    } else if mantissa <= 2.0 + 1e-9 {
        2.0
    } else if mantissa <= 5.0 + 1e-9 {
        5.0
    } else {
        10.0
    };
    step * power
}

/// How many intervals the major unit aims at, for an axis this long in points.
fn interval_target(length_pt: f64, vertical: bool) -> f64 {
    let count = if vertical {
        ((length_pt - 8.0) / 11.1).floor()
    } else {
        ((length_pt + 34.0) / 37.0).floor()
    };
    count.clamp(1.0, 10.0)
}

/// The axis's raw ends before they snap to the unit. `scatter` value axes pad by 5% where others
/// leave half the data's spread below it.
fn raw_bounds(low: f64, high: f64, scatter: bool) -> (f64, f64) {
    if low >= 0.0 && high > 0.0 {
        let min = if low < high * 5.0 / 6.0 || low == high {
            0.0
        } else if scatter {
            low - 0.05 * (high - low)
        } else {
            low - (high - low) / 2.0
        };
        (min, high + 0.05 * (high - min))
    } else if high <= 0.0 && low < 0.0 {
        let max = if high > low * 5.0 / 6.0 || low == high {
            0.0
        } else if scatter {
            high + 0.05 * (high - low)
        } else {
            high + (high - low) / 2.0
        };
        (low - 0.05 * (max - low), max)
    } else if low == 0.0 && high == 0.0 {
        (0.0, 1.0)
    } else {
        let spread = high - low;
        (low - 0.05 * spread, high + 0.05 * spread)
    }
}

fn snap(value: f64, unit: f64, up: bool) -> f64 {
    let steps = value / unit;
    let steps = if up {
        (steps - 1e-9).ceil()
    } else {
        (steps + 1e-9).floor()
    };
    // Rounded to the unit's digits, so 0.1 * 3 shows as 0.3.
    let digits = (-unit.log10().floor()).max(0.0) as i32 + 1;
    let factor = 10f64.powi(digits.min(15));
    (steps * unit * factor).round() / factor
}

/// An axis's scale: where it starts and ends and the step between its labels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pub min: f64,
    pub max: f64,
    pub unit: f64,
}

/// Excel's automatic scale for data from `low` to `high`, with any end the file fixes.
pub fn auto_scale(
    low: f64,
    high: f64,
    length_pt: f64,
    vertical: bool,
    scatter: bool,
    fixed: (Option<f64>, Option<f64>, Option<f64>),
) -> Scale {
    let (low, high) = if low.is_finite() && high.is_finite() && low <= high {
        (low, high)
    } else {
        (0.0, 0.0)
    };
    let (mut min, mut max) = raw_bounds(low, high, scatter);
    if let Some(fixed) = fixed.0 {
        min = fixed;
    }
    if let Some(fixed) = fixed.1 {
        max = fixed;
    }
    if max <= min {
        max = min + 1.0;
    }
    let unit = fixed
        .2
        .filter(|unit| *unit > 0.0)
        .unwrap_or_else(|| nice_unit((max - min) / interval_target(length_pt, vertical)));
    Scale {
        min: if fixed.0.is_some() {
            min
        } else {
            snap(min, unit, false)
        },
        max: if fixed.1.is_some() {
            max
        } else {
            snap(max, unit, true)
        },
        unit,
    }
}

// ---------------------------------------------------------------------------------------------
// Reading the cells a chart names

/// One cell a series reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub value: CalcValue,
    pub text: String,
    /// Its number format's code.
    pub format: String,
}

/// A reference's areas, as sheets and rectangles. `None` if it names something not in the book,
/// such as another workbook.
pub fn resolve(book: &Book, formula: &str, depth: u8) -> Option<Vec<(u32, CalcRect)>> {
    let text = formula.trim().trim_start_matches('=');
    let text = text
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or(text);
    let mut out = Vec::new();
    for part in split_areas(text) {
        let part = part.trim();
        let (sheet, reference) = match part.rfind('!') {
            Some(at) => (Some(unquote(&part[..at])), &part[at + 1..]),
            None => (None, part),
        };
        if sheet.as_deref().is_some_and(|name| name.starts_with('[')) {
            return None;
        }
        let index = match &sheet {
            Some(name) => Some(
                book.sheets
                    .iter()
                    .position(|info| !info.deleted && info.name.eq_ignore_ascii_case(name))?
                    as u32,
            ),
            None => None,
        };
        if let Some(((top, left), (bottom, right))) = cgs_refs::parse_range(reference) {
            out.push((
                index?,
                CalcRect {
                    top: top.min(bottom),
                    left: left.min(right),
                    bottom: top.max(bottom),
                    right: left.max(right),
                },
            ));
            continue;
        }
        // A defined name, the sheet's own first.
        if depth > 3 {
            return None;
        }
        let name = book
            .names
            .iter()
            .find(|name| name.name.eq_ignore_ascii_case(reference) && name.local_sheet == index)
            .or_else(|| {
                book.names.iter().find(|name| {
                    name.name.eq_ignore_ascii_case(reference) && name.local_sheet.is_none()
                })
            })?;
        out.extend(resolve(book, &name.formula, depth + 1)?);
    }
    (!out.is_empty()).then_some(out)
}

fn unquote(name: &str) -> String {
    name.strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .map_or_else(|| name.to_string(), |inner| inner.replace("''", "'"))
}

fn split_areas(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (at, ch) in text.char_indices() {
        match ch {
            '\'' => quoted = !quoted,
            ',' if !quoted => {
                out.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

struct Collect {
    top: u32,
    left: u32,
    width: u32,
    values: Vec<CalcValue>,
}

impl CellSink for Collect {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        let at = ((row - self.top) * self.width + (col - self.left)) as usize;
        if let Some(slot) = self.values.get_mut(at) {
            *slot = value.clone();
        }
    }
}

/// Reads a chart's cells, with what they show.
pub struct Reader<'a> {
    pub book: &'a Book,
    pub calc: &'a dyn CalcEngine,
    pub options: &'a Options<'a>,
}

impl Reader<'_> {
    fn format_code(&self, sheet: u32, row: u32, col: u32) -> String {
        let info = &self.book.sheets[sheet as usize];
        let table = &self.book.styles;
        let id = table.xf(info.style_at(row, col)).num_fmt;
        table
            .num_fmts
            .get(&id)
            .cloned()
            .or_else(|| cgs_numfmt::builtin_code(id).map(str::to_string))
            .unwrap_or_else(|| "General".to_string())
    }

    /// The cells of a reference, row by row, with hidden rows and columns left out when the chart
    /// plots visible cells only. `None` if the reference isn't in this book.
    pub fn read(&self, formula: &str, visible_only: bool) -> Option<Vec<Point>> {
        let areas = resolve(self.book, formula, 0)?;
        let mut out = Vec::new();
        for (sheet, rect) in areas {
            let info = self.book.sheets.get(sheet as usize)?;
            let bottom = rect.bottom.min(rect.top + MAX_POINTS as u32);
            let right = rect.right.min(rect.left + MAX_POINTS as u32);
            let width = right - rect.left + 1;
            let height = bottom - rect.top + 1;
            if (width as usize) * (height as usize) > MAX_POINTS * 4 {
                // A whole column: only the used part matters.
                let used_bottom = info.used.0.max(rect.top).min(bottom);
                return self.read_rect(
                    sheet,
                    CalcRect {
                        bottom: used_bottom,
                        right,
                        ..rect
                    },
                    visible_only,
                    out,
                );
            }
            out = self.read_rect(
                sheet,
                CalcRect {
                    bottom,
                    right,
                    ..rect
                },
                visible_only,
                out,
            )?;
        }
        out.truncate(MAX_POINTS);
        Some(out)
    }

    fn read_rect(
        &self,
        sheet: u32,
        rect: CalcRect,
        visible_only: bool,
        mut out: Vec<Point>,
    ) -> Option<Vec<Point>> {
        let info = &self.book.sheets[sheet as usize];
        let width = rect.right - rect.left + 1;
        let height = rect.bottom - rect.top + 1;
        let mut collect = Collect {
            top: rect.top,
            left: rect.left,
            width,
            values: vec![CalcValue::Empty; (width * height) as usize],
        };
        self.calc
            .read_block(SheetRect { sheet, rect }, &mut collect);
        for row in rect.top..=rect.bottom {
            if visible_only && info.rows.info(row).is_some_and(|line| line.hidden) {
                continue;
            }
            for col in rect.left..=rect.right {
                if visible_only && info.cols.info(col).is_some_and(|line| line.hidden) {
                    continue;
                }
                let value =
                    collect.values[((row - rect.top) * width + (col - rect.left)) as usize].clone();
                let format = self.format_code(sheet, row, col);
                let mut text = String::new();
                match &value {
                    CalcValue::Empty => {}
                    CalcValue::Number(number) => {
                        NumFmt::parse(&format).format(
                            Value::Number(*number),
                            self.options,
                            &mut text,
                        );
                    }
                    CalcValue::Text(inner) => text.push_str(inner),
                    CalcValue::Bool(flag) => text.push_str(if *flag { "TRUE" } else { "FALSE" }),
                    CalcValue::Error(error) => text.push_str(error),
                }
                out.push(Point {
                    value,
                    text,
                    format,
                });
                if out.len() >= MAX_POINTS {
                    return Some(out);
                }
            }
        }
        Some(out)
    }

    /// A source's points: from its cells, or from what the file saved when they can't be read.
    pub fn source(&self, source: &DataSource, visible_only: bool) -> Vec<Point> {
        if let Some(points) = source
            .formula
            .as_deref()
            .and_then(|formula| self.read(formula, visible_only))
        {
            return points;
        }
        let format = source
            .format
            .clone()
            .unwrap_or_else(|| "General".to_string());
        let mut out: Vec<Point> = (0..source.count.min(MAX_POINTS as u32))
            .map(|_| Point {
                value: CalcValue::Empty,
                text: String::new(),
                format: format.clone(),
            })
            .collect();
        for point in &source.cache {
            if let Some(slot) = out.get_mut(point.index as usize) {
                slot.value = match (source.kind, point.value.trim().parse::<f64>()) {
                    (SourceKind::Number, Ok(number)) => CalcValue::Number(number),
                    _ => CalcValue::Text(point.value.clone()),
                };
                slot.text = match slot.value {
                    CalcValue::Number(number) => {
                        let mut text = String::new();
                        NumFmt::parse(&format).format(
                            Value::Number(number),
                            self.options,
                            &mut text,
                        );
                        text
                    }
                    _ => point.value.clone(),
                };
            }
        }
        out
    }

    /// A title's or a series name's text.
    pub fn text(&self, text: &ChartText) -> String {
        match text {
            ChartText::Rich(text) => text.clone(),
            ChartText::Cell { formula, cache } => self
                .read(formula, false)
                .map(|points| {
                    points
                        .iter()
                        .map(|point| point.text.as_str())
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_else(|| cache.clone()),
        }
    }

    pub fn format(&self, code: &str, value: f64) -> String {
        let mut out = String::new();
        NumFmt::parse(code).format(Value::Number(value), self.options, &mut out);
        out
    }
}

/// What a value cell plots as. Text and `TRUE` plot as zero, as in Excel, and `#N/A` as nothing.
fn plotted(value: &CalcValue) -> Option<f64> {
    match value {
        CalcValue::Number(number) if number.is_finite() => Some(*number),
        CalcValue::Empty => None,
        CalcValue::Error(error) if error == "#N/A" => None,
        _ => Some(0.0),
    }
}

// ---------------------------------------------------------------------------------------------
// The view the UI draws

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LineView {
    pub color: String,
    /// In CSS pixels.
    pub width: f64,
    pub dash: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextView {
    pub text: String,
    /// In CSS pixels.
    pub size: f64,
    pub color: String,
    pub bold: bool,
    pub italic: bool,
    /// Degrees.
    pub rotation: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TickView {
    pub value: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AxisView {
    /// The axis's id in the file.
    pub id: u32,
    pub visible: bool,
    /// `left`, `right`, `top` or `bottom`.
    pub position: &'static str,
    pub reversed: bool,
    pub title: Option<TextView>,
    pub labels: Option<TextView>,
    pub line: Option<LineView>,
    pub gridlines: Option<LineView>,
    pub minor_gridlines: Option<LineView>,
    pub major_tick: &'static str,
    pub minor_tick: &'static str,
    /// Value axes: their scale, and each label.
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub unit: Option<f64>,
    pub ticks: Vec<TickView>,
    /// Category axes: categories sit between tick marks, not on them.
    pub between: bool,
    pub label_skip: u32,
    /// Where the other axis crosses this one: a value, or `None` for zero.
    pub crosses_at: Option<f64>,
    pub crosses: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarkerView {
    pub symbol: &'static str,
    /// In CSS pixels.
    pub size: f64,
    pub fill: Option<String>,
    pub line: Option<LineView>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LabelsView {
    pub texts: Vec<String>,
    pub position: &'static str,
    pub look: TextView,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SeriesView {
    pub name: String,
    /// Which group it belongs to.
    pub group: usize,
    pub values: Vec<Option<f64>>,
    /// Scatter charts: each point's x.
    pub x: Option<Vec<Option<f64>>>,
    pub fill: Option<String>,
    pub line: Option<LineView>,
    pub marker: Option<MarkerView>,
    pub smooth: bool,
    /// Pie slices and points with their own colour.
    pub point_fills: Vec<Option<String>>,
    pub point_lines: Vec<Option<LineView>>,
    pub labels: Option<LabelsView>,
    /// Its index in the file, for selecting and editing it.
    pub index: u32,
    pub explosion: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    /// `bar`, `line`, `area`, `pie`, `doughnut` or `scatter`.
    pub kind: &'static str,
    /// As the file says, before drawing it flat or as its nearest kind.
    pub file_kind: &'static str,
    pub horizontal: bool,
    /// `none`, `stacked` or `percent`.
    pub stacking: &'static str,
    pub gap_width: u16,
    pub overlap: i16,
    pub hole: u8,
    pub first_slice: u16,
    pub vary_colors: bool,
    /// 0 for the primary axes, 1 for the secondary.
    pub axis: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LegendView {
    /// `right`, `left`, `top`, `bottom` or `topRight`.
    pub position: &'static str,
    pub overlay: bool,
    pub look: TextView,
    pub hidden: Vec<u32>,
    pub fill: Option<String>,
    pub border: Option<LineView>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChartView {
    /// `classic`, `extended` or `unreadable`.
    pub kind: &'static str,
    /// A `cx:` chart's layout, such as `waterfall`.
    pub extended: Option<String>,
    pub editable: bool,
    pub font: String,
    pub background: Option<String>,
    pub border: Option<LineView>,
    pub rounded: bool,
    pub plot_background: Option<String>,
    pub plot_border: Option<LineView>,
    pub plot_layout: Option<[f64; 4]>,
    pub title: Option<TextView>,
    pub legend: Option<LegendView>,
    pub categories: Vec<String>,
    pub category_axis: Option<AxisView>,
    pub value_axes: Vec<AxisView>,
    pub groups: Vec<GroupView>,
    pub series: Vec<SeriesView>,
    /// Drawn as the nearest flat kind: a 3-D, radar, stock or other chart.
    pub approximate: bool,
    pub blanks: &'static str,
}

fn dash_name(dash: Dash) -> &'static str {
    match dash {
        Dash::Solid => "solid",
        Dash::Dot | Dash::SysDot => "dot",
        Dash::DashDot
        | Dash::LongDashDot
        | Dash::LongDashDotDot
        | Dash::SysDashDot
        | Dash::SysDashDotDot => "dashDot",
        Dash::Dash | Dash::LongDash | Dash::SysDash => "dash",
    }
}

fn pt(hundredths: u32) -> f64 {
    f64::from(hundredths) / 100.0 * PX_PER_PT
}

/// Builds what the UI draws for one chart.
pub struct Builder<'a> {
    pub reader: Reader<'a>,
    pub painter: Painter<'a>,
    /// The chart's size in CSS pixels at 100%.
    pub width: f64,
    pub height: f64,
}

impl Builder<'_> {
    fn line(&self, line: Option<&ChartLine>, default: Option<(DmlColor, f64)>) -> Option<LineView> {
        let fill = line.and_then(|line| line.fill.as_ref());
        let color = match fill {
            Some(fill) => self.painter.fill(fill)?,
            None => self.painter.color(&default.as_ref()?.0),
        };
        let width = line
            .and_then(|line| line.width)
            .map(|emu| emu as f64 / 12700.0 * PX_PER_PT)
            .or(default.as_ref().map(|(_, width)| *width))
            .unwrap_or(1.0);
        Some(LineView {
            color,
            width,
            dash: dash_name(line.and_then(|line| line.dash).unwrap_or_default()),
        })
    }

    fn text(
        &self,
        text: String,
        look: &TextLook,
        base: &TextLook,
        size: u32,
        bold: bool,
    ) -> TextView {
        let color = look
            .color
            .as_ref()
            .or(base.color.as_ref())
            .cloned()
            .unwrap_or_else(default_text);
        TextView {
            text,
            size: pt(look.size.or(base.size).unwrap_or(size)),
            color: self.painter.color(&color),
            bold: look.bold.or(base.bold).unwrap_or(bold),
            italic: look.italic.or(base.italic).unwrap_or(false),
            rotation: look.rotation.map_or(0.0, |rot| f64::from(rot) / 60_000.0),
        }
    }

    fn base_size(chart: &Chart) -> u32 {
        chart.look.size.unwrap_or(1000)
    }

    pub fn build(&self, chart: &Chart) -> ChartView {
        let base = &chart.look;
        let base_size = Self::base_size(chart);
        let visible_only = chart.plot_visible_only;
        let mut series_views = Vec::new();
        let mut groups = Vec::new();
        let mut categories: Option<Vec<String>> = None;
        let mut primary_value: Option<u32> = None;
        let total_series = chart.series_count();
        let mut series_number = 0usize;
        let mut approximate = chart.three_d;
        // The data range of each value axis: (low, high).
        let mut ranges: Vec<(u32, f64, f64)> = Vec::new();
        let mut x_range: Option<(f64, f64)> = None;
        let mut first_value_format: Option<String> = None;
        let mut first_category_format: Option<String> = None;

        for (group_index, group) in chart.groups.iter().enumerate() {
            let (kind, horizontal, stacking, gap_width, overlap, hole, first_slice) =
                match &group.kind {
                    GroupKind::Bar {
                        horizontal,
                        grouping,
                        gap_width,
                        overlap,
                        ..
                    } => ("bar", *horizontal, *grouping, *gap_width, *overlap, 0, 0),
                    GroupKind::Line { grouping, .. } => ("line", false, *grouping, 150, 0, 0, 0),
                    GroupKind::Area { grouping, .. } => ("area", false, *grouping, 150, 0, 0, 0),
                    GroupKind::Pie { first_slice, .. } => {
                        ("pie", false, Grouping::Standard, 0, 0, 0, *first_slice)
                    }
                    GroupKind::Doughnut { first_slice, hole } => (
                        "doughnut",
                        false,
                        Grouping::Standard,
                        0,
                        0,
                        *hole,
                        *first_slice,
                    ),
                    GroupKind::OfPie => ("pie", false, Grouping::Standard, 0, 0, 0, 0),
                    GroupKind::Scatter { .. } | GroupKind::Bubble => {
                        ("scatter", false, Grouping::Standard, 0, 0, 0, 0)
                    }
                    GroupKind::Radar | GroupKind::Stock => {
                        ("line", false, Grouping::Standard, 150, 0, 0, 0)
                    }
                    GroupKind::Surface => ("area", false, Grouping::Standard, 150, 0, 0, 0),
                };
            if !group.kind.editable() {
                approximate = true;
            }
            let value_axis = group.axes.get(1).copied().unwrap_or(0);
            let axis = match primary_value {
                None => {
                    primary_value = Some(value_axis);
                    0
                }
                Some(primary) if primary == value_axis => 0,
                Some(_) => 1,
            };
            let stacking_name = match stacking {
                Grouping::Stacked => "stacked",
                Grouping::PercentStacked => "percent",
                _ => "none",
            };
            groups.push(GroupView {
                kind,
                file_kind: group.kind.tag(),
                horizontal,
                stacking: stacking_name,
                gap_width,
                overlap,
                hole,
                first_slice,
                vary_colors: group.vary_colors,
                axis,
            });
            let round = kind == "pie" || kind == "doughnut";
            let mut sums: Vec<(f64, f64)> = Vec::new();
            let mut ordered: Vec<&Series> = group.series.iter().collect();
            ordered.sort_by_key(|series| series.order);
            for series in ordered {
                let values = series
                    .values
                    .as_ref()
                    .map(|source| self.reader.source(source, visible_only))
                    .unwrap_or_default();
                let cats = series
                    .categories
                    .as_ref()
                    .map(|source| self.reader.source(source, visible_only));
                if first_value_format.is_none() {
                    first_value_format = values.first().map(|point| point.format.clone());
                }
                let numbers: Vec<Option<f64>> =
                    values.iter().map(|point| plotted(&point.value)).collect();
                let x = (kind == "scatter").then(|| {
                    let text_x = cats.as_ref().is_some_and(|cats| {
                        cats.iter()
                            .any(|point| matches!(point.value, CalcValue::Text(_)))
                    });
                    match &cats {
                        Some(cats) if !text_x => {
                            cats.iter().map(|point| plotted(&point.value)).collect()
                        }
                        _ => (1..=numbers.len())
                            .map(|n| Some(n as f64))
                            .collect::<Vec<_>>(),
                    }
                });
                if categories.is_none() && kind != "scatter" {
                    if let Some(cats) = &cats {
                        first_category_format = cats.first().map(|point| point.format.clone());
                        categories = Some(cats.iter().map(|point| point.text.clone()).collect());
                    }
                }
                // The data ranges the axes scale to.
                if !round {
                    if stacking.stacked() {
                        if sums.len() < numbers.len() {
                            sums.resize(numbers.len(), (0.0, 0.0));
                        }
                        for (slot, value) in sums.iter_mut().zip(&numbers) {
                            if let Some(value) = value {
                                if *value >= 0.0 {
                                    slot.0 += value;
                                } else {
                                    slot.1 += value;
                                }
                            }
                        }
                    } else {
                        let entry = range_entry(&mut ranges, value_axis);
                        for value in numbers.iter().flatten() {
                            entry.1 = entry.1.min(*value);
                            entry.2 = entry.2.max(*value);
                        }
                    }
                    if let Some(x) = &x {
                        for value in x.iter().flatten() {
                            let range = x_range.get_or_insert((*value, *value));
                            range.0 = range.0.min(*value);
                            range.1 = range.1.max(*value);
                        }
                    }
                }
                let name = series
                    .name
                    .as_ref()
                    .map(|text| self.reader.text(text))
                    .unwrap_or_else(|| format!("Series{}", series.index + 1));
                series_views.push(self.series_view(
                    chart,
                    group,
                    kind,
                    series,
                    series_number,
                    total_series,
                    group_index,
                    numbers,
                    x,
                    name,
                    &values,
                    cats.as_deref(),
                ));
                series_number += 1;
            }
            if stacking.stacked() && !round {
                let entry = range_entry(&mut ranges, value_axis);
                if stacking == Grouping::PercentStacked {
                    let negative = sums.iter().any(|(_, negative)| *negative < 0.0);
                    let positive = sums.iter().any(|(positive, _)| *positive > 0.0);
                    entry.1 = entry.1.min(if negative { -1.0 } else { 0.0 });
                    entry.2 = entry.2.max(if positive { 1.0 } else { 0.0 });
                } else {
                    for (positive, negative) in &sums {
                        entry.1 = entry.1.min(*negative).min(*positive);
                        entry.2 = entry.2.max(*positive).max(*negative);
                    }
                }
            }
        }

        let round_only = !groups.is_empty()
            && groups
                .iter()
                .all(|group| group.kind == "pie" || group.kind == "doughnut");
        let count = series_views
            .iter()
            .map(|series| series.values.len())
            .max()
            .unwrap_or(0);
        let categories = categories.unwrap_or_else(|| (1..=count).map(|n| n.to_string()).collect());

        // Title: the file's, or the only series' name when the file leaves it automatic.
        let title = match &chart.title {
            Some(title) => {
                let text = match &title.text {
                    Some(text) => self.reader.text(text),
                    None if series_views.len() == 1 => series_views[0].name.clone(),
                    None => "Chart Title".to_string(),
                };
                Some(self.text(text, &title.look, base, base_size * 18 / 10, true))
            }
            None if !chart.auto_title_deleted && series_views.len() == 1 => Some(self.text(
                series_views[0].name.clone(),
                &TextLook::default(),
                base,
                base_size * 18 / 10,
                true,
            )),
            None => None,
        };
        let legend = chart.legend.as_ref().map(|legend| LegendView {
            position: match legend.position {
                LegendPosition::Right => "right",
                LegendPosition::Left => "left",
                LegendPosition::Top => "top",
                LegendPosition::Bottom => "bottom",
                LegendPosition::TopRight => "topRight",
            },
            overlay: legend.overlay,
            look: self.text(String::new(), &legend.look, base, base_size, false),
            hidden: legend.hidden.clone(),
            fill: legend
                .shape
                .fill
                .as_ref()
                .and_then(|fill| self.painter.fill(fill)),
            border: self.line(legend.shape.line.as_ref(), None),
        });

        // How long the axes are, roughly as Excel lays the chart out, in points.
        let width_pt = self.width / PX_PER_PT;
        let height_pt = self.height / PX_PER_PT;
        let label_pt = f64::from(base_size) / 100.0 * 1.25;
        let title_pt = title
            .as_ref()
            .map_or(0.0, |title| title.size / PX_PER_PT * 1.2 + 8.0);
        let (legend_v, legend_h) = match &legend {
            Some(legend) if !legend.overlay => match legend.position {
                "top" | "bottom" => (label_pt + 9.0, 0.0),
                _ => (0.0, 60.0),
            },
            _ => (0.0, 0.0),
        };
        let plot_height = height_pt - 25.0 - title_pt - legend_v - label_pt;
        let plot_width = width_pt - 30.0 - legend_h - 30.0;

        let mut category_axis = None;
        let mut value_axes = Vec::new();
        if !round_only {
            let scatter = groups.first().is_some_and(|group| group.kind == "scatter");
            let horizontal = groups.first().is_some_and(|group| group.horizontal);
            let first = chart.groups.first();
            let cat_id = first
                .and_then(|group| group.axes.first().copied())
                .unwrap_or(0);
            let mut value_ids: Vec<u32> = Vec::new();
            for group in &chart.groups {
                if let Some(&id) = group.axes.get(1) {
                    if !value_ids.contains(&id) {
                        value_ids.push(id);
                    }
                }
            }
            let percent = groups.iter().any(|group| group.stacking == "percent");
            if let Some(axis) = chart.axis(cat_id) {
                if scatter {
                    let (low, high) = x_range.unwrap_or((0.0, 0.0));
                    let scale = auto_scale(
                        low,
                        high,
                        plot_width,
                        false,
                        true,
                        (axis.min, axis.max, axis.major_unit),
                    );
                    let format = self.axis_format(axis, first_category_format.as_deref());
                    category_axis = Some(self.value_axis(axis, scale, &format, base, base_size));
                } else {
                    category_axis = Some(self.category_axis(axis, base, base_size));
                }
            }
            for id in value_ids {
                let Some(axis) = chart.axis(id) else { continue };
                let (_, low, high) = ranges
                    .iter()
                    .find(|(axis_id, _, _)| *axis_id == id)
                    .copied()
                    .unwrap_or((id, 0.0, 0.0));
                let (low, high) = if low > high { (0.0, 0.0) } else { (low, high) };
                let vertical = !horizontal;
                let length = if vertical { plot_height } else { plot_width };
                let mut scale = auto_scale(
                    low,
                    high,
                    length,
                    vertical,
                    scatter,
                    (axis.min, axis.max, axis.major_unit),
                );
                if percent && axis.max.is_none() && high <= 1.0 {
                    scale.max = scale.max.min(1.0);
                }
                let format = if percent && axis.format.as_ref().is_none_or(|(_, linked)| *linked) {
                    "0%".to_string()
                } else {
                    self.axis_format(axis, first_value_format.as_deref())
                };
                value_axes.push(self.value_axis(axis, scale, &format, base, base_size));
            }
        }

        let background = match &chart.shape.fill {
            Some(fill) => self.painter.fill(fill),
            None => Some(self.painter.color(&DmlColor::scheme("bg1"))),
        };
        ChartView {
            kind: "classic",
            extended: None,
            editable: chart.editable(),
            font: base.font.clone().unwrap_or_else(|| "Calibri".to_string()),
            background,
            border: self.line(
                chart.shape.line.as_ref(),
                Some((
                    DmlColor::scheme("tx1")
                        .with(ColorModKind::LumMod, 15_000)
                        .with(ColorModKind::LumOff, 85_000),
                    1.0,
                )),
            ),
            rounded: chart.rounded_corners,
            plot_background: chart
                .plot_shape
                .fill
                .as_ref()
                .and_then(|fill| self.painter.fill(fill)),
            plot_border: self.line(chart.plot_shape.line.as_ref(), None),
            plot_layout: chart.plot_layout,
            title,
            legend,
            categories,
            category_axis,
            value_axes,
            groups,
            series: series_views,
            approximate,
            blanks: match chart.blanks {
                Blanks::Gap => "gap",
                Blanks::Zero => "zero",
                Blanks::Span => "span",
            },
        }
    }

    fn axis_format(&self, axis: &ChartAxis, linked: Option<&str>) -> String {
        match &axis.format {
            Some((code, false)) => code.clone(),
            _ => linked.unwrap_or("General").to_string(),
        }
    }

    fn axis_common(&self, axis: &ChartAxis, base: &TextLook, base_size: u32) -> AxisView {
        let labels = (axis.labels != LabelPlace::None && !axis.deleted)
            .then(|| self.text(String::new(), &axis.look, base, base_size, false));
        AxisView {
            id: axis.id,
            visible: !axis.deleted,
            position: match axis.position {
                AxisPosition::Bottom => "bottom",
                AxisPosition::Left => "left",
                AxisPosition::Right => "right",
                AxisPosition::Top => "top",
            },
            reversed: axis.reversed,
            title: axis.title.as_ref().map(|title| {
                let text = title
                    .text
                    .as_ref()
                    .map(|text| self.reader.text(text))
                    .unwrap_or_else(|| "Axis Title".to_string());
                self.text(text, &title.look, base, base_size, true)
            }),
            labels,
            line: if axis.deleted {
                None
            } else {
                self.line(axis.shape.line.as_ref(), Some((default_line(), 1.0)))
            },
            gridlines: axis.major_gridlines.as_ref().and_then(|lines| {
                self.line(lines.shape.line.as_ref(), Some((default_line(), 1.0)))
            }),
            minor_gridlines: axis.minor_gridlines.as_ref().and_then(|lines| {
                self.line(
                    lines.shape.line.as_ref(),
                    Some((
                        DmlColor::scheme("tx1")
                            .with(ColorModKind::LumMod, 25_000)
                            .with(ColorModKind::LumOff, 75_000),
                        1.0,
                    )),
                )
            }),
            major_tick: axis.major_tick.code(),
            minor_tick: axis.minor_tick.code(),
            min: None,
            max: None,
            unit: None,
            ticks: Vec::new(),
            between: true,
            label_skip: axis.label_skip.unwrap_or(1).max(1),
            crosses_at: match axis.crosses {
                Crosses::At(value) => Some(value),
                _ => None,
            },
            crosses: match axis.crosses {
                Crosses::AutoZero => "autoZero",
                Crosses::Min => "min",
                Crosses::Max => "max",
                Crosses::At(_) => "at",
            },
        }
    }

    fn category_axis(&self, axis: &ChartAxis, base: &TextLook, base_size: u32) -> AxisView {
        let mut view = self.axis_common(axis, base, base_size);
        view.gridlines = axis
            .major_gridlines
            .as_ref()
            .and_then(|lines| self.line(lines.shape.line.as_ref(), Some((default_line(), 1.0))));
        view
    }

    fn value_axis(
        &self,
        axis: &ChartAxis,
        scale: Scale,
        format: &str,
        base: &TextLook,
        base_size: u32,
    ) -> AxisView {
        let mut view = self.axis_common(axis, base, base_size);
        view.min = Some(scale.min);
        view.max = Some(scale.max);
        view.unit = Some(scale.unit);
        let steps = ((scale.max - scale.min) / scale.unit)
            .round()
            .clamp(0.0, 1000.0) as u32;
        for step in 0..=steps {
            let value = scale.min + f64::from(step) * scale.unit;
            let digits = (-scale.unit.log10().floor()).max(0.0) as i32 + 2;
            let factor = 10f64.powi(digits.min(15));
            let value = (value * factor).round() / factor;
            view.ticks.push(TickView {
                value,
                text: self.reader.format(format, value),
            });
        }
        view
    }

    #[allow(clippy::too_many_arguments)]
    fn series_view(
        &self,
        chart: &Chart,
        group: &ChartGroup,
        kind: &'static str,
        series: &Series,
        number: usize,
        total: usize,
        group_index: usize,
        values: Vec<Option<f64>>,
        x: Option<Vec<Option<f64>>>,
        name: String,
        points: &[Point],
        categories: Option<&[Point]>,
    ) -> SeriesView {
        let round = kind == "pie" || kind == "doughnut";
        let auto = auto_color(chart.style, number, total);
        let shows_line = matches!(kind, "line")
            || matches!(
                group.kind,
                GroupKind::Scatter {
                    style: ScatterStyle::Line
                        | ScatterStyle::LineMarker
                        | ScatterStyle::Smooth
                        | ScatterStyle::SmoothMarker
                }
            );
        let fill = match &series.shape.fill {
            Some(fill) => self.painter.fill(fill),
            None if matches!(kind, "bar" | "area") || round => Some(self.painter.color(&auto)),
            None => None,
        };
        let line_default = if shows_line {
            Some((auto.clone(), 2.25 * PX_PER_PT))
        } else if round {
            Some((DmlColor::scheme("bg1"), 1.0))
        } else {
            None
        };
        let mut line = self.line(series.shape.line.as_ref(), line_default);
        if kind == "scatter" && !shows_line && series.shape.line.is_none() {
            line = None;
        }
        // Markers: the series' own, or the group's automatic ones.
        let group_markers = match &group.kind {
            GroupKind::Line { markers, .. } => *markers,
            GroupKind::Scatter { style } => matches!(
                style,
                ScatterStyle::LineMarker | ScatterStyle::Marker | ScatterStyle::SmoothMarker
            ),
            _ => false,
        };
        const AUTO_SYMBOLS: [&str; 7] = [
            "diamond", "square", "triangle", "x", "star", "circle", "plus",
        ];
        let marker = if matches!(kind, "line" | "scatter") {
            let symbol = series
                .marker
                .as_ref()
                .map_or(MarkerSymbol::Auto, |marker| marker.symbol);
            let shown = symbol != MarkerSymbol::None
                && (group_markers
                    || series
                        .marker
                        .as_ref()
                        .is_some_and(|m| m.symbol != MarkerSymbol::Auto));
            shown.then(|| {
                let marker = series.marker.clone().unwrap_or_default();
                MarkerView {
                    symbol: if symbol == MarkerSymbol::Auto {
                        AUTO_SYMBOLS[number % AUTO_SYMBOLS.len()]
                    } else {
                        symbol.code()
                    },
                    size: f64::from(marker.size.unwrap_or(7)) * PX_PER_PT,
                    fill: match &marker.shape.fill {
                        Some(fill) => self.painter.fill(fill),
                        None => Some(self.painter.color(&auto)),
                    },
                    line: self.line(marker.shape.line.as_ref(), Some((auto.clone(), 1.0))),
                }
            })
        } else {
            None
        };
        // Points with their own colour, and every point of a pie that varies its colours.
        let count = values.len();
        let mut point_fills = Vec::new();
        let mut point_lines = Vec::new();
        if group.vary_colors && (round || total == 1) {
            point_fills = (0..count)
                .map(|at| Some(self.painter.color(&auto_color(chart.style, at, count))))
                .collect();
        }
        for point in &series.points {
            let at = point.index as usize;
            if at >= count {
                continue;
            }
            if point_fills.len() < count {
                point_fills.resize(count, None);
            }
            if let Some(fill) = &point.shape.fill {
                point_fills[at] = self.painter.fill(fill);
            }
            if point.shape.line.is_some() {
                if point_lines.len() < count {
                    point_lines.resize(count, None);
                }
                point_lines[at] = self.line(point.shape.line.as_ref(), None);
            }
        }
        let labels = series
            .labels
            .as_ref()
            .or(group.labels.as_ref())
            .filter(|labels| labels.shows_anything())
            .map(|labels| self.labels(chart, labels, round, &name, &values, points, categories));
        SeriesView {
            name,
            group: group_index,
            values,
            x,
            fill,
            line,
            marker,
            smooth: series.smooth.unwrap_or(false),
            point_fills,
            point_lines,
            labels,
            index: series.index,
            explosion: f64::from(series.explosion.unwrap_or(0)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn labels(
        &self,
        chart: &Chart,
        labels: &DataLabels,
        round: bool,
        name: &str,
        values: &[Option<f64>],
        points: &[Point],
        categories: Option<&[Point]>,
    ) -> LabelsView {
        let total: f64 = values.iter().flatten().map(|value| value.abs()).sum();
        let separator = labels.separator.clone().unwrap_or_else(|| ", ".to_string());
        let texts = values
            .iter()
            .enumerate()
            .map(|(at, value)| {
                let Some(value) = value else {
                    return String::new();
                };
                let mut parts = Vec::new();
                if labels.series {
                    parts.push(name.to_string());
                }
                if labels.category {
                    parts.push(
                        categories
                            .and_then(|cats| cats.get(at))
                            .map_or_else(|| (at + 1).to_string(), |point| point.text.clone()),
                    );
                }
                if labels.value {
                    parts.push(match &labels.format {
                        Some(code) => self.reader.format(code, *value),
                        None => points
                            .get(at)
                            .map_or_else(|| value.to_string(), |point| point.text.clone()),
                    });
                }
                if labels.percent && total > 0.0 {
                    parts.push(self.reader.format("0%", value.abs() / total));
                }
                parts.join(&separator)
            })
            .collect();
        let base_size = Self::base_size(chart);
        LabelsView {
            texts,
            position: match labels.position {
                LabelPosition::Auto => {
                    if round {
                        "bestFit"
                    } else {
                        "outsideEnd"
                    }
                }
                LabelPosition::BestFit => "bestFit",
                LabelPosition::Bottom => "below",
                LabelPosition::Center => "center",
                LabelPosition::InsideBase => "insideBase",
                LabelPosition::InsideEnd => "insideEnd",
                LabelPosition::Left => "left",
                LabelPosition::OutsideEnd => "outsideEnd",
                LabelPosition::Right => "right",
                LabelPosition::Top => "above",
            },
            look: self.text(
                String::new(),
                &labels.look,
                &chart.look,
                base_size * 9 / 10,
                false,
            ),
        }
    }
}

fn range_entry(ranges: &mut Vec<(u32, f64, f64)>, axis: u32) -> &mut (u32, f64, f64) {
    let at = match ranges.iter().position(|(id, _, _)| *id == axis) {
        Some(at) => at,
        None => {
            ranges.push((axis, f64::INFINITY, f64::NEG_INFINITY));
            ranges.len() - 1
        }
    };
    &mut ranges[at]
}

/// The view of a chart part that isn't a classic chart: drawn as a labelled placeholder.
pub fn placeholder_view(body: &ChartBody) -> ChartView {
    ChartView {
        kind: match body {
            ChartBody::Extended(_) => "extended",
            _ => "unreadable",
        },
        extended: match body {
            ChartBody::Extended(kind) => Some(kind.clone()),
            _ => None,
        },
        editable: false,
        font: "Calibri".to_string(),
        background: None,
        border: None,
        rounded: false,
        plot_background: None,
        plot_border: None,
        plot_layout: None,
        title: None,
        legend: None,
        categories: Vec::new(),
        category_axis: None,
        value_axes: Vec::new(),
        groups: Vec::new(),
        series: Vec::new(),
        approximate: false,
        blanks: "gap",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cases measured in Excel 2016 by `bench/run-chart-axes.mjs`: data low and high, the plot's
    /// length in points, whether it runs up the chart, and Excel's min, max and unit.
    /// Low, high, length, vertical, scatter, and Excel's min, max and unit.
    type Measured = (f64, f64, f64, bool, bool, f64, f64, f64);
    const MEASURED: &[Measured] = &[
        (0.0, 10.0, 134.0, true, false, 0.0, 12.0, 2.0),
        (1.0, 9.0, 134.0, true, false, 0.0, 10.0, 1.0),
        (3.0, 7.0, 134.0, true, false, 0.0, 8.0, 1.0),
        (95.0, 100.0, 134.0, true, false, 92.0, 101.0, 1.0),
        (-5.0, 5.0, 134.0, true, false, -6.0, 6.0, 2.0),
        (-100.0, -20.0, 134.0, true, false, -120.0, 0.0, 20.0),
        (0.001, 0.009, 134.0, true, false, 0.0, 0.01, 0.001),
        (1234.0, 5678.0, 134.0, true, false, 0.0, 6000.0, 1000.0),
        (50.0, 51.0, 134.0, true, false, 49.4, 51.2, 0.2),
        (1e6, 3e6, 134.0, true, false, 0.0, 3.5e6, 5e5),
        (5.0, 5.0, 134.0, true, false, 0.0, 6.0, 1.0),
        (-7.0, -7.0, 134.0, true, false, -8.0, 0.0, 1.0),
        (84.0, 100.0, 134.0, true, false, 75.0, 105.0, 5.0),
        (83.0, 100.0, 134.0, true, false, 0.0, 120.0, 20.0),
        (0.0, 10.0, 38.0, true, false, 0.0, 20.0, 10.0),
        (95.0, 100.0, 38.0, true, false, 90.0, 105.0, 5.0),
        (0.0, 10.0, 165.0, false, false, 0.0, 15.0, 5.0),
        (1.0, 9.0, 325.0, false, false, 0.0, 10.0, 2.0),
        (1.0, 9.0, 565.0, false, false, 0.0, 10.0, 1.0),
        (95.0, 100.0, 157.0, true, true, 94.0, 101.0, 1.0),
        (84.0, 100.0, 157.0, true, true, 82.0, 102.0, 2.0),
        (-4819.0, -4435.0, 157.0, true, true, -4850.0, -4400.0, 50.0),
    ];

    #[test]
    fn scales_axes_as_excel_does() {
        for &(low, high, length, vertical, scatter, min, max, unit) in MEASURED {
            let scale = auto_scale(low, high, length, vertical, scatter, (None, None, None));
            let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * b.abs().max(1.0);
            assert!(
                close(scale.min, min) && close(scale.max, max) && close(scale.unit, unit),
                "{low}..{high} at {length}: got {scale:?}, Excel {min} {max} {unit}"
            );
        }
    }

    #[test]
    fn fixed_ends_stay() {
        let scale = auto_scale(3.0, 7.0, 134.0, true, false, (Some(2.0), None, Some(0.5)));
        assert_eq!(
            scale,
            Scale {
                min: 2.0,
                max: 7.5,
                unit: 0.5
            }
        );
    }

    #[test]
    fn dark_mode_turns_greys_around() {
        let theme = ThemePalette::default();
        let screen = Screen {
            dark: true,
            background: 0x1E1E1E,
            text: 0xE6E6E6,
        };
        let light = Painter {
            theme: &theme,
            screen: None,
        };
        let dark = Painter {
            theme: &theme,
            screen: Some(screen),
        };
        let gridline = DmlColor::scheme("tx1")
            .with(ColorModKind::LumMod, 15_000)
            .with(ColorModKind::LumOff, 85_000);
        assert_eq!(light.color(&gridline), "#D9D9D9");
        // A quiet line on the dark background, not a bright one.
        let shown = dark.color(&gridline);
        let value = u32::from_str_radix(&shown[1..], 16).unwrap();
        assert!(value >> 16 < 0x50, "{shown}");
        assert_eq!(dark.color(&DmlColor::scheme("bg1")), "#1E1E1E");
        assert_eq!(dark.color(&DmlColor::scheme("tx1")), "#E6E6E6");
        // Accents stay as stored.
        assert_eq!(dark.color(&DmlColor::scheme("accent1")), "#4472C4");
    }

    #[test]
    fn automatic_colours_follow_the_accents() {
        assert_eq!(auto_color(2, 0, 3), DmlColor::scheme("accent1"));
        assert_eq!(auto_color(2, 5, 8), DmlColor::scheme("accent6"));
        assert_eq!(
            auto_color(2, 6, 8),
            DmlColor::scheme("accent1").with(ColorModKind::LumMod, 60_000)
        );
    }
}
