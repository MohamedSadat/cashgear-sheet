//! Where pages break, as Excel breaks them: a page holds as many whole rows and columns as fit
//! inside the margins at the page's scale, title rows and columns repeat on every page after
//! their own, a manual break ends a page, and fitting to pages picks the largest whole percent
//! that fits, ignoring manual breaks as Excel does. Print areas each start their own pages.
//!
//! Excel lays a page out in its printer's dots, which `Units` holds as measured over COM with
//! Microsoft Print to PDF at 600 dpi: a row is `floor(pixels × 6.0567)` dots, rounded again at
//! the page's scale, and a column `round(pixels × 6.75)` dots times the scale. So printed rows are
//! about 3% shorter than on screen and columns 8% wider, while text keeps its size. The room for
//! rows is 8 dots short of the margins.

use cgs_model::{
    page::{HfRun, HfSections, HfText},
    Anchor, PageOrder, PageSetup, Rect,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::Session;

/// Excel's printer units for a sheet whose normal font is Calibri 11, measured on the reference
/// printer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Units {
    /// Dots per inch.
    pub dpi: f64,
    /// Dots per screen pixel of a row's height and a column's width, at 100%.
    pub row: f64,
    pub col: f64,
    /// Dots the room for rows falls short of the margins by.
    pub row_slack: f64,
}

/// Excel's dots per screen pixel of rows and columns for workbooks of other normal fonts,
/// measured over COM with Microsoft Print to PDF (`bench/excel-page-breaks.ps1`'s probe):
/// printed sizes follow the printer's font metrics, which no formula here reproduces. Where a
/// column factor didn't fit one value, the middle of what fitted the default width is used.
const FONT_UNITS: &[(&str, f32, f64, f64)] = &[
    ("calibri", 9.0, 6.1935, 6.368),
    ("calibri", 10.0, 6.427, 6.017),
    ("calibri", 11.0, 6.0567, 6.75),
    ("calibri", 12.0, 6.195, 6.40),
    ("calibri", 14.0, 6.009, 5.923),
    ("calibri", 16.0, 6.1055, 6.15),
    ("arial", 8.0, 5.671, 6.222),
    ("arial", 9.0, 5.817, 6.017),
    ("arial", 10.0, 6.0685, 6.62),
    ("arial", 11.0, 6.009, 6.40),
    ("arial", 12.0, 6.164, 6.224),
    ("times new roman", 10.0, 6.2425, 6.2),
    ("times new roman", 11.0, 5.806, 6.24),
    ("times new roman", 12.0, 6.009, 6.24),
    ("cambria", 11.0, 6.0575, 6.40),
    ("verdana", 8.0, 6.289, 6.20),
    ("verdana", 10.0, 6.366, 6.665),
    ("segoe ui", 9.0, 7.134, 6.62),
    ("segoe ui", 10.0, 6.531, 6.62),
    ("courier new", 10.0, 5.3415, 6.24),
    ("tahoma", 8.0, 6.371, 6.222),
    ("tahoma", 10.0, 6.366, 6.40),
    ("georgia", 10.0, 6.0567, 5.72),
    ("consolas", 11.0, 5.806, 6.40),
];

impl Units {
    pub const EXCEL: Self = Self {
        dpi: 600.0,
        row: 6.0567,
        col: 6.75,
        row_slack: 8.0,
    };

    /// The units for a workbook whose normal font is this, or Calibri 11's for one not measured.
    pub fn for_font(name: &str, size: f32) -> Self {
        let name = name.to_ascii_lowercase();
        FONT_UNITS
            .iter()
            .find(|(known, known_size, _, _)| *known == name && (*known_size - size).abs() < 0.01)
            .map_or(Self::EXCEL, |&(_, _, row, col)| Self {
                row,
                col,
                ..Self::EXCEL
            })
    }

    /// A row's height in dots at a percent.
    pub fn row_dots(&self, px: u32, zoom: u32) -> f64 {
        ((f64::from(px) * self.row).floor() * f64::from(zoom) / 100.0).round()
    }

    /// A column's width in dots at a percent.
    pub fn col_dots(&self, px: u32, zoom: u32) -> f64 {
        (f64::from(px) * self.col).round() * f64::from(zoom) / 100.0
    }

    pub fn dots(&self, points: f64) -> f64 {
        points * self.dpi / 72.0
    }

    pub fn points(&self, dots: f64) -> f64 {
        dots * 72.0 / self.dpi
    }
}

/// How a page is scaled: a percent, or fitted to so many pages wide and tall (0 is any).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zoom {
    Percent(u32),
    Fit { wide: u32, tall: u32 },
}

/// Everything pagination needs to know, in points.
#[derive(Debug, Clone)]
pub struct Layout {
    /// The paper, turned as the page is.
    pub paper: (f64, f64),
    /// Left, right, top and bottom, in points.
    pub margins: (f64, f64, f64, f64),
    pub zoom: Zoom,
    pub areas: Vec<Rect>,
    pub title_rows: Option<(u32, u32)>,
    pub title_cols: Option<(u32, u32)>,
    /// The last row or column before each manual break, sorted.
    pub manual_rows: Vec<u32>,
    pub manual_cols: Vec<u32>,
    /// The row headings' width and the column headings' height, when headings print, in screen
    /// pixels at 100%.
    pub headings: Option<(u32, u32)>,
    pub center: (bool, bool),
    pub order: PageOrder,
    /// Excel's printer units for the workbook's normal font.
    pub units: Units,
}

/// One page: which cells it shows and where they go on the paper.
#[derive(Debug, Clone, PartialEq)]
pub struct PageBox {
    /// Which print area it is from.
    pub area: usize,
    pub rows: (u32, u32),
    pub cols: (u32, u32),
    /// The title rows and columns repeated on it.
    pub title_rows: Option<(u32, u32)>,
    pub title_cols: Option<(u32, u32)>,
    /// The top left of what it prints, headings included, in points from the paper's corner.
    pub x: f64,
    pub y: f64,
    /// What it prints, in points on the paper.
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paginated {
    /// The percent pages print at.
    pub zoom: u32,
    /// The first `PAGE_LIMIT` pages.
    pub pages: Vec<PageBox>,
    /// Every page, those past the limit too.
    pub count: usize,
    /// The last row and column of each page but the last, for the dashed lines in Normal view.
    pub row_breaks: Vec<u32>,
    pub col_breaks: Vec<u32>,
}

/// The stretches a line of rows or columns breaks into. `titles` repeat on each page that
/// starts after them, taking `title_size` from the room.
fn segments(
    first: u32,
    last: u32,
    room: f64,
    titles: Option<(u32, u32)>,
    title_size: f64,
    manual: &[u32],
    size: &dyn Fn(u32) -> f64,
) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut start = first;
    // Dots add up exactly; this only absorbs a column's fraction of a dot.
    const SLACK: f64 = 1e-6;
    while start <= last {
        let repeats = titles.is_some_and(|(_, end)| end < start);
        let room = if repeats { room - title_size } else { room };
        let mut used = 0.0;
        let mut end = start;
        let mut line = start;
        loop {
            let this = size(line);
            if line > start && used + this > room + SLACK {
                break;
            }
            used += this;
            end = line;
            if manual.binary_search(&line).is_ok() || line == last {
                break;
            }
            line += 1;
        }
        out.push((start, end));
        start = end + 1;
    }
    out
}

fn sum(first: u32, last: u32, size: &dyn Fn(u32) -> f64) -> f64 {
    (first..=last).map(size).sum()
}

fn title_size(titles: Option<(u32, u32)>, size: &dyn Fn(u32) -> f64) -> f64 {
    titles.map_or(0.0, |(a, b)| sum(a, b, size))
}

/// The most pages laid out. A sheet styled to its last cell would print millions of pages; they
/// are counted, but only these many are placed.
pub const PAGE_LIMIT: usize = 10_000;

/// The pages at one percent.
struct AtZoom {
    pages: Vec<PageBox>,
    count: usize,
    row_breaks: Vec<u32>,
    col_breaks: Vec<u32>,
    /// Each area's pages down and across.
    shape: Vec<(usize, usize)>,
}

/// Pages at one percent, with or without manual breaks, up to `limit` of them placed. `row` and
/// `col` give each line's size in screen pixels at 100%, hidden ones 0.
fn pages_at(
    layout: &Layout,
    zoom: u32,
    manual: bool,
    limit: usize,
    row: &dyn Fn(u32) -> u32,
    col: &dyn Fn(u32) -> u32,
) -> AtZoom {
    let units = layout.units;
    let (left, right, top, bottom) = layout.margins;
    let printable_w = (layout.paper.0 - left - right).max(1.0);
    let printable_h = (layout.paper.1 - top - bottom).max(1.0);
    let row_dots = |r: u32| units.row_dots(row(r), zoom);
    let col_dots = |c: u32| units.col_dots(col(c), zoom);
    let (head_w, head_h) = layout.headings.map_or((0.0, 0.0), |(w, h)| {
        (units.col_dots(w, zoom), units.row_dots(h, zoom))
    });
    let room_w = units.dots(printable_w) - head_w;
    let room_h = units.dots(printable_h).round() - units.row_slack - head_h;
    let none: Vec<u32> = Vec::new();
    // Fitting ignores manual breaks, except along a side whose pages it leaves automatic.
    let (keep_rows, keep_cols) = match layout.zoom {
        Zoom::Percent(_) => (manual, manual),
        Zoom::Fit { wide, tall } => (manual && tall == 0, manual && wide == 0),
    };
    let manual_rows = if keep_rows {
        &layout.manual_rows
    } else {
        &none
    };
    let manual_cols = if keep_cols {
        &layout.manual_cols
    } else {
        &none
    };
    let title_h = title_size(layout.title_rows, &row_dots);
    let title_w = title_size(layout.title_cols, &col_dots);
    let mut pages = Vec::new();
    let mut row_breaks = Vec::new();
    let mut col_breaks = Vec::new();
    let mut shape = Vec::new();
    for (index, area) in layout.areas.iter().enumerate() {
        let rows = segments(
            area.top,
            area.bottom,
            room_h,
            layout.title_rows,
            title_h,
            manual_rows,
            &row_dots,
        );
        let cols = segments(
            area.left,
            area.right,
            room_w,
            layout.title_cols,
            title_w,
            manual_cols,
            &col_dots,
        );
        // Every area's breaks, as Excel lists them; where one area ends isn't a break.
        row_breaks.extend(rows.iter().rev().skip(1).map(|&(_, end)| end));
        col_breaks.extend(cols.iter().rev().skip(1).map(|&(_, end)| end));
        shape.push((rows.len(), cols.len()));
        let mut place = |r: (u32, u32), c: (u32, u32)| {
            if pages.len() >= limit {
                return;
            }
            let title_rows = layout.title_rows.filter(|&(_, end)| end < r.0);
            let title_cols = layout.title_cols.filter(|&(_, end)| end < c.0);
            let width = units.points(
                head_w
                    + title_cols.map_or(0.0, |(a, b)| sum(a, b, &col_dots))
                    + sum(c.0, c.1, &col_dots),
            );
            let height = units.points(
                head_h
                    + title_rows.map_or(0.0, |(a, b)| sum(a, b, &row_dots))
                    + sum(r.0, r.1, &row_dots),
            );
            let x = left
                + if layout.center.0 {
                    ((printable_w - width) / 2.0).max(0.0)
                } else {
                    0.0
                };
            let y = top
                + if layout.center.1 {
                    ((printable_h - height) / 2.0).max(0.0)
                } else {
                    0.0
                };
            pages.push(PageBox {
                area: index,
                rows: r,
                cols: c,
                title_rows,
                title_cols,
                x,
                y,
                width,
                height,
            });
        };
        match layout.order {
            PageOrder::DownThenOver => {
                for &c in &cols {
                    for &r in &rows {
                        place(r, c);
                    }
                }
            }
            PageOrder::OverThenDown => {
                for &r in &rows {
                    for &c in &cols {
                        place(r, c);
                    }
                }
            }
        }
    }
    row_breaks.sort_unstable();
    row_breaks.dedup();
    col_breaks.sort_unstable();
    col_breaks.dedup();
    AtZoom {
        pages,
        count: shape.iter().map(|&(down, across)| down * across).sum(),
        row_breaks,
        col_breaks,
        shape,
    }
}

/// How many pages wide and tall the first print area takes at a percent, others added in.
fn extent(
    layout: &Layout,
    zoom: u32,
    row: &dyn Fn(u32) -> u32,
    col: &dyn Fn(u32) -> u32,
) -> (u32, u32) {
    let shape = pages_at(layout, zoom, false, 0, row, col).shape;
    let wide = shape.iter().map(|&(_, across)| across).max().unwrap_or(0);
    let tall = shape.iter().map(|&(down, _)| down).sum::<usize>();
    (wide as u32, tall as u32)
}

/// Lays out the pages. `row` and `col` give each line's size in screen pixels at 100%, hidden
/// ones 0.
pub fn paginate(layout: &Layout, row: &dyn Fn(u32) -> u32, col: &dyn Fn(u32) -> u32) -> Paginated {
    let zoom = match layout.zoom {
        Zoom::Percent(percent) => percent.clamp(10, 400),
        Zoom::Fit { wide, tall } => {
            let fits = |zoom: u32| {
                let (w, t) = extent(layout, zoom, row, col);
                (wide == 0 || w <= wide) && (tall == 0 || t <= tall)
            };
            // Rounding at each percent means fitting isn't strictly monotonic, so every percent
            // is tried from 100 down, as Excel finds the largest that fits.
            (10..=100).rev().find(|&zoom| fits(zoom)).unwrap_or(10)
        }
    };
    let at = pages_at(layout, zoom, true, PAGE_LIMIT, row, col);
    Paginated {
        zoom,
        pages: at.pages,
        count: at.count,
        row_breaks: at.row_breaks,
        col_breaks: at.col_breaks,
    }
}

/// What to print, as File › Print and Export › PDF ask for it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrintRequest {
    /// Print these cells instead of the print area, as Print Selection does.
    pub selection: Vec<crate::command::Area>,
    /// Print the whole used range, as Ignore Print Area does.
    pub ignore_print_area: bool,
}

fn runs_json(runs: &[HfRun]) -> Value {
    Value::Array(
        runs.iter()
            .map(|run| {
                let (field, text) = match &run.text {
                    HfText::Literal(text) => (Value::Null, Value::String(text.clone())),
                    HfText::Page(offset) => (json!("page"), json!(offset)),
                    HfText::Pages => (json!("pages"), Value::Null),
                    HfText::Date => (json!("date"), Value::Null),
                    HfText::Time => (json!("time"), Value::Null),
                    HfText::File => (json!("file"), Value::Null),
                    HfText::Path => (json!("path"), Value::Null),
                    HfText::Sheet => (json!("sheet"), Value::Null),
                    HfText::Picture => (json!("picture"), Value::Null),
                };
                let s = &run.style;
                json!({
                    "field": field,
                    "text": text,
                    "font": s.font,
                    "bold": s.bold,
                    "italic": s.italic,
                    "size": s.size,
                    "underline": s.underline,
                    "strike": s.strike,
                    "superscript": s.superscript,
                    "subscript": s.subscript,
                    "color": s.color,
                })
            })
            .collect(),
    )
}

fn sections_json(sections: &HfSections) -> Value {
    if sections.is_empty() {
        return Value::Null;
    }
    json!({
        "left": runs_json(&sections.left),
        "center": runs_json(&sections.center),
        "right": runs_json(&sections.right),
    })
}

fn rect_json(r: Rect) -> Value {
    json!({ "top": r.top, "left": r.left, "bottom": r.bottom, "right": r.right })
}

/// Calibri's advance widths for ASCII from space to `~`, in units of 2048 to the em. Other
/// characters are taken as 1000.
const CALIBRI_UNITS: [u16; 95] = [
    463, 544, 821, 1038, 1038, 1464, 1397, 452, 621, 621, 1038, 1038, 511, 627, 517, 791, 1038,
    1038, 1038, 1038, 1038, 1038, 1038, 1038, 1038, 1038, 548, 548, 1038, 1038, 1038, 948, 1823,
    1185, 1114, 1092, 1260, 1000, 941, 1292, 1276, 516, 653, 1064, 861, 1751, 1322, 1356, 1058,
    1378, 1112, 941, 998, 1314, 1162, 1822, 1063, 998, 959, 627, 799, 627, 1038, 1019, 597, 981,
    1076, 866, 1076, 1019, 625, 964, 1076, 470, 490, 931, 470, 1636, 1076, 1080, 1076, 1076, 714,
    801, 686, 1076, 925, 1464, 887, 927, 809, 648, 943, 648, 1038,
];

/// A line of text's width in Calibri units.
fn text_units(line: &str) -> u32 {
    line.chars()
        .map(|c| match c as u32 {
            code @ 32..=126 => u32::from(CALIBRI_UNITS[(code - 32) as usize]),
            _ => 1000,
        })
        .sum()
}

impl Session {
    /// The cells that print when nothing says which: from A1 to the last cell holding a value, a
    /// fill or a border, or any drawing's, and the empty cells text overflows into, as Excel
    /// prints them.
    pub(crate) fn print_range(&self, sheet: u32) -> Option<Rect> {
        use cgs_calc::{CalcEngine, CalcValue, CellSink, SheetRect};
        let info = self.book.sheets.get(sheet as usize)?;
        let mut bounds: Option<Rect> = None;
        let mut take = |row: u32, col: u32| {
            bounds = Some(match bounds {
                None => Rect {
                    top: row,
                    left: col,
                    bottom: row,
                    right: col,
                },
                Some(b) => Rect {
                    top: b.top.min(row),
                    left: b.left.min(col),
                    bottom: b.bottom.max(row),
                    right: b.right.max(col),
                },
            });
        };
        // Every cell with a value, by row, and the text that may overflow to its right.
        let mut filled: Vec<(u32, u32)> = Vec::new();
        let mut texts: Vec<(u32, u32, u32, bool)> = Vec::new();
        struct Seen<'a> {
            take: &'a mut dyn FnMut(u32, u32),
            filled: &'a mut Vec<(u32, u32)>,
            texts: &'a mut Vec<(u32, u32, u32, bool)>,
        }
        impl CellSink for Seen<'_> {
            fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
                if *value != CalcValue::Empty {
                    (self.take)(row, col);
                    self.filled.push((row, col));
                }
                if let CalcValue::Text(text) = value {
                    // Excel draws only a cell's first 1,024 characters.
                    let shown: String = text.chars().take(1024).collect();
                    let longest = shown.split('\n').map(text_units).max();
                    if let Some(units) = longest.filter(|&units| units > 0) {
                        self.texts
                            .push((row, col, units, crate::tile::starts_rtl(&shown)));
                    }
                }
            }
        }
        let (rows, cols) = info.used;
        // Styled blanks can make the used range the whole sheet; values are read only as far as
        // they go.
        let (value_rows, value_cols) = self.calc.value_bounds(sheet).unwrap_or((0, 0));
        let (read_rows, read_cols) = (rows.min(value_rows), cols.min(value_cols));
        if read_rows > 0 && read_cols > 0 {
            self.calc.read_block(
                SheetRect {
                    sheet,
                    rect: cgs_calc::Rect {
                        top: 1,
                        left: 1,
                        bottom: read_rows,
                        right: read_cols,
                    },
                },
                &mut Seen {
                    take: &mut take,
                    filled: &mut filled,
                    texts: &mut texts,
                },
            );
        }
        if rows > 0 && cols > 0 {
            let styles = &self.book.styles;
            let shows = |style: u32| {
                styles.xfs.get(style as usize).is_some_and(|xf| {
                    let fill = styles
                        .fills
                        .get(xf.fill as usize)
                        .is_some_and(|f| f.pattern != cgs_model::Pattern::None);
                    let border = styles.borders.get(xf.border as usize).is_some_and(|b| {
                        [b.left, b.right, b.top, b.bottom, b.diagonal]
                            .iter()
                            .any(|line| line.style != cgs_model::LineStyle::None)
                    });
                    fill || border
                })
            };
            for col in 1..=info.styled_cols().min(cols) {
                for (first, last, style) in info.style_spans(col, 1, rows) {
                    if style != cgs_model::NO_CELL && shows(style) {
                        take(first, col);
                        take(last, col);
                    }
                }
            }
        }
        // Text left to itself runs on through empty cells to its right, and they print too.
        // Rust has no text measure, so text is measured in Calibri's widths and scaled to the
        // cell's font by its widest digit.
        if !texts.is_empty() {
            filled.sort_unstable();
            let styles = &self.book.styles;
            let default_font = styles.fonts.first().cloned().unwrap_or_default();
            let printer = Units::for_font(&default_font.name, default_font.size);
            let (units_per_inch, col_dots) = (printer.dpi, printer.col);
            let right_to_left = info.view.right_to_left;
            for &(row, col, units, arabic) in &texts {
                let Some(xf) = styles.xfs.get(info.style_at(row, col) as usize) else {
                    continue;
                };
                let alignment = xf.alignment;
                // On a right-to-left sheet later columns lie to the left, where text aligned
                // right runs, and General text that starts in Arabic.
                let share = match alignment.horizontal {
                    cgs_model::HAlign::Center => 0.5,
                    cgs_model::HAlign::General | cgs_model::HAlign::Left if !right_to_left => 1.0,
                    cgs_model::HAlign::General if right_to_left && arabic => 1.0,
                    cgs_model::HAlign::Right if right_to_left => 1.0,
                    _ => continue,
                };
                if alignment.wrap
                    || alignment.shrink
                    || alignment.rotation != 0
                    || info.merge_at(row, col).is_some()
                {
                    continue;
                }
                let font = styles.fonts.get(xf.font as usize).unwrap_or(&default_font);
                let digit = cgs_model::style::max_digit_width(font);
                // Text prints at its size while columns print wider than on screen (`Units`),
                // so the text's width in dots is turned back into a column's screen pixels.
                let calibri = cgs_model::style::max_digit_width(&cgs_model::Font {
                    name: "Calibri".into(),
                    ..font.clone()
                });
                let dots = f64::from(units) / 2048.0 * f64::from(font.size) * units_per_inch / 72.0
                    * (digit / calibri);
                let left = crate::tile::sheet_x(info, col);
                let own = i64::from(info.cols.px(col));
                let padding = 2.0 * (digit / 4.0).ceil();
                let text = dots / col_dots + padding + f64::from(alignment.indent) * 3.0 * digit;
                let spill = ((text - own as f64) * share).ceil() as i64;
                if spill <= 0 {
                    continue;
                }
                // It stops at the next cell holding something.
                let after = filled.partition_point(|&cell| cell <= (row, col));
                let next = filled
                    .get(after)
                    .filter(|cell| cell.0 == row)
                    .map_or(cgs_model::MAX_COLS, |cell| cell.1 - 1);
                let (reach, _) = crate::chart_edit::col_at(info, left + own + spill - 1);
                take(row, reach.min(next).max(col));
            }
        }
        for drawing in info.drawings.iter().filter(|d| !d.hidden && d.prints) {
            if let Anchor::TwoCell { from, to } = drawing.anchor {
                take(from.row, from.col);
                take(to.row, to.col);
                continue;
            }
            // A picture of a fixed size prints with every cell it covers.
            let (x, y, width, height) = crate::drawing_box(info, drawing.anchor);
            let (left, _) = crate::chart_edit::col_at(info, x.max(0));
            let (top, _) = crate::chart_edit::row_at(info, y.max(0));
            let (right, _) = crate::chart_edit::col_at(info, (x + width - 1).max(0));
            let (bottom, _) = crate::chart_edit::row_at(info, (y + height - 1).max(0));
            take(top, left);
            take(bottom, right);
        }
        bounds.map(|b| Rect {
            top: 1,
            left: 1,
            bottom: b.bottom,
            right: b.right,
        })
    }

    /// The layout of a sheet's pages, or nothing to print.
    pub(crate) fn layout_for(&self, sheet: u32, request: &PrintRequest) -> Option<Layout> {
        let info = self.book.sheets.get(sheet as usize)?;
        let page: &PageSetup = &info.page;
        let names = self.print_names(sheet);
        let mut areas: Vec<Rect> = if !request.selection.is_empty() {
            request
                .selection
                .iter()
                .map(|a| Rect {
                    top: a.top,
                    left: a.left,
                    bottom: a.bottom,
                    right: a.right,
                })
                .collect()
        } else if !request.ignore_print_area && !names.areas.is_empty() {
            names.areas.clone()
        } else {
            vec![self.print_range(sheet)?]
        };
        // Whole rows or columns print only as far as the sheet holds anything.
        let used = self.print_range(sheet);
        for area in &mut areas {
            if area.right == cgs_model::MAX_COLS || area.bottom == cgs_model::MAX_ROWS {
                let Some(used) = used else { continue };
                if area.right == cgs_model::MAX_COLS {
                    area.right = used.right.max(area.left);
                }
                if area.bottom == cgs_model::MAX_ROWS {
                    area.bottom = used.bottom.max(area.top);
                }
            }
        }
        let zoom = if page.fit_to_page {
            Zoom::Fit {
                wide: page.fit_width,
                tall: page.fit_height,
            }
        } else {
            Zoom::Percent(page.scale)
        };
        let m = page.margins;
        let headings = page.headings.then(|| {
            let last = areas.iter().map(|a| a.bottom).max().unwrap_or(1);
            let digits = last.to_string().len() as f64;
            // Row numbers are as wide as their digits with a cell's padding, and column letters
            // as tall as a default row.
            let digit = cgs_model::style::max_digit_width(
                &self.book.styles.fonts.first().cloned().unwrap_or_default(),
            );
            (
                (digits * digit + 2.0 * (digit / 4.0).ceil() + 1.0) as u32,
                info.rows.default_px,
            )
        });
        Some(Layout {
            paper: page.paper_points(),
            margins: (m.left * 72.0, m.right * 72.0, m.top * 72.0, m.bottom * 72.0),
            zoom,
            areas,
            title_rows: names.title_rows,
            title_cols: names.title_cols,
            manual_rows: page.manual_row_breaks().collect(),
            manual_cols: page.manual_col_breaks().collect(),
            headings,
            center: (page.center_horizontally, page.center_vertically),
            order: page.page_order,
            units: {
                let font = self.book.styles.normal_font();
                Units::for_font(&font.name, font.size)
            },
        })
    }

    pub(crate) fn paginate_sheet(&self, sheet: u32, request: &PrintRequest) -> Option<Paginated> {
        let info = self.book.sheets.get(sheet as usize)?;
        let layout = self.layout_for(sheet, request)?;
        let row = |r: u32| info.rows.px(r);
        let col = |c: u32| info.cols.px(c);
        Some(paginate(&layout, &row, &col))
    }

    /// A sheet's pages for File › Print, Export › PDF and the dashed lines in Normal view.
    pub(crate) fn pages_json(&self, sheet: u32, request: &PrintRequest) -> Result<Value, String> {
        let info = self
            .book
            .sheets
            .get(sheet as usize)
            .filter(|i| !i.deleted)
            .ok_or_else(|| format!("There is no sheet {sheet}."))?;
        let page = &info.page;
        let (paper_w, paper_h) = page.paper_points();
        let paginated = self.paginate_sheet(sheet, request);
        let hf = &page.header_footer;
        let header_footer = json!({
            "odd": [sections_json(&cgs_model::page::parse_header_footer(&hf.odd_header)),
                    sections_json(&cgs_model::page::parse_header_footer(&hf.odd_footer))],
            "even": [sections_json(&cgs_model::page::parse_header_footer(&hf.even_header)),
                     sections_json(&cgs_model::page::parse_header_footer(&hf.even_footer))],
            "first": [sections_json(&cgs_model::page::parse_header_footer(&hf.first_header)),
                      sections_json(&cgs_model::page::parse_header_footer(&hf.first_footer))],
            "differentOddEven": hf.different_odd_even,
            "differentFirst": hf.different_first,
            "scaleWithDoc": hf.scale_with_doc,
            "alignWithMargins": hf.align_with_margins,
        });
        let font = self.book.styles.fonts.first().cloned().unwrap_or_default();
        let pages: Vec<Value> = paginated
            .as_ref()
            .map(|p| {
                p.pages
                    .iter()
                    .map(|b| {
                        json!({
                            "area": b.area,
                            "rows": [b.rows.0, b.rows.1],
                            "cols": [b.cols.0, b.cols.1],
                            "titleRows": b.title_rows.map(|t| [t.0, t.1]),
                            "titleCols": b.title_cols.map(|t| [t.0, t.1]),
                            "x": b.x,
                            "y": b.y,
                            "width": b.width,
                            "height": b.height,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let m = page.margins;
        Ok(json!({
            "sheet": sheet,
            "name": info.name,
            "paper": [paper_w, paper_h],
            "margins": { "left": m.left * 72.0, "right": m.right * 72.0, "top": m.top * 72.0,
                         "bottom": m.bottom * 72.0, "header": m.header * 72.0, "footer": m.footer * 72.0 },
            "zoom": paginated.as_ref().map_or(100, |p| p.zoom),
            "pageCount": paginated.as_ref().map_or(0, |p| p.count),
            "firstPage": page.first_page,
            "gridlines": page.gridlines,
            "headings": page.headings,
            "blackAndWhite": page.black_and_white,
            "draft": page.draft,
            "errors": match page.errors {
                cgs_model::PrintErrors::Displayed => "displayed",
                cgs_model::PrintErrors::Blank => "blank",
                cgs_model::PrintErrors::Dash => "dash",
                cgs_model::PrintErrors::Na => "NA",
            },
            "headingSize": self.layout_for(sheet, request).and_then(|l| l.headings).map(|(w, h)| [w, h]),
            "units": self.layout_for(sheet, request).map_or(
                json!({ "dpi": Units::EXCEL.dpi, "row": Units::EXCEL.row, "col": Units::EXCEL.col }),
                |l| json!({ "dpi": l.units.dpi, "row": l.units.row, "col": l.units.col }),
            ),
            "headerFooter": header_footer,
            "font": { "name": font.name, "size": font.size },
            "pages": pages,
            "rowBreaks": paginated.as_ref().map(|p| p.row_breaks.clone()).unwrap_or_default(),
            "colBreaks": paginated.as_ref().map(|p| p.col_breaks.clone()).unwrap_or_default(),
            "manualRows": page.manual_row_breaks().collect::<Vec<_>>(),
            "manualCols": page.manual_col_breaks().collect::<Vec<_>>(),
            "printAreas": self.print_names(sheet).areas.into_iter().map(rect_json).collect::<Vec<_>>(),
            "pictures": page.pictures.iter().map(|p| json!({
                "place": p.place, "part": p.part, "width": p.width, "height": p.height,
            })).collect::<Vec<_>>(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(areas: Vec<Rect>) -> Layout {
        Layout {
            paper: (612.0, 792.0),
            margins: (50.4, 50.4, 54.0, 54.0),
            zoom: Zoom::Percent(100),
            areas,
            title_rows: None,
            title_cols: None,
            manual_rows: Vec::new(),
            manual_cols: Vec::new(),
            headings: None,
            center: (false, false),
            order: PageOrder::DownThenOver,
            units: Units::EXCEL,
        }
    }

    fn area(top: u32, left: u32, bottom: u32, right: u32) -> Rect {
        Rect {
            top,
            left,
            bottom,
            right,
        }
    }

    // Excel's default row and column: 20 and 64 pixels.
    const ROW: &dyn Fn(u32) -> u32 = &|_| 20;
    const COL: &dyn Fn(u32) -> u32 = &|_| 64;

    #[test]
    fn a_letter_page_holds_what_excel_puts_on_it() {
        // Excel, measured: 47 default rows and 9 default columns to a Letter page.
        let paged = paginate(&layout(vec![area(1, 1, 100, 25)]), ROW, COL);
        assert_eq!(paged.row_breaks, vec![47, 94]);
        assert_eq!(paged.col_breaks, vec![9, 18]);
        assert_eq!(paged.pages.len(), 9);
        assert_eq!(paged.pages[1].rows, (48, 94));
        assert_eq!(paged.pages[3].cols, (10, 18));
    }

    #[test]
    fn units_round_as_excel_does() {
        let units = Units::EXCEL;
        assert_eq!(units.row_dots(20, 100), 121.0);
        assert_eq!(units.row_dots(18, 100), 109.0);
        assert_eq!(units.row_dots(20, 50), 61.0);
        assert_eq!(units.col_dots(64, 100), 432.0);
        assert_eq!(units.col_dots(64, 50), 216.0);
    }

    #[test]
    fn over_then_down_numbers_across_first() {
        let mut l = layout(vec![area(1, 1, 100, 25)]);
        l.order = PageOrder::OverThenDown;
        let paged = paginate(&l, ROW, COL);
        assert_eq!(paged.pages[1].rows, (1, 47));
        assert_eq!(paged.pages[1].cols, (10, 18));
    }

    #[test]
    fn titles_repeat_after_their_own_page_and_take_room() {
        let mut l = layout(vec![area(1, 1, 100, 5)]);
        l.title_rows = Some((1, 2));
        let paged = paginate(&l, ROW, COL);
        assert_eq!(paged.pages[0].rows, (1, 47));
        assert_eq!(paged.pages[0].title_rows, None);
        // Two title rows take 242 of the 5,692 dots, which leaves room for 45.
        assert_eq!(paged.pages[1].rows, (48, 92));
        assert_eq!(paged.pages[1].title_rows, Some((1, 2)));
    }

    #[test]
    fn manual_breaks_end_pages_unless_fitting_fixes_that_side() {
        let mut l = layout(vec![area(1, 1, 60, 5)]);
        l.manual_rows = vec![10];
        let paged = paginate(&l, ROW, COL);
        assert_eq!(paged.row_breaks, vec![10, 57]);
        // Fitting a page wide leaves the height automatic, and Excel keeps manual row breaks.
        l.zoom = Zoom::Fit { wide: 1, tall: 0 };
        let fitted = paginate(&l, ROW, COL);
        assert_eq!(fitted.row_breaks, vec![10, 57]);
        assert_eq!(fitted.zoom, 100);
        // Fitting both ways ignores them.
        l.zoom = Zoom::Fit { wide: 1, tall: 2 };
        let fitted = paginate(&l, ROW, COL);
        assert_eq!(fitted.row_breaks, vec![47]);
    }

    #[test]
    fn a_sheet_styled_to_its_last_cell_is_counted_not_laid_out() {
        let paged = paginate(&layout(vec![area(1, 1, 1_048_576, 16_384)]), ROW, COL);
        assert_eq!(paged.pages.len(), PAGE_LIMIT);
        // 47 rows a page down and 9 columns across.
        assert_eq!(
            paged.count,
            1_048_576usize.div_ceil(47) * 16_384usize.div_ceil(9)
        );
    }

    #[test]
    fn fitting_picks_the_largest_percent_that_fits() {
        let mut l = layout(vec![area(1, 1, 100, 25)]);
        l.zoom = Zoom::Fit { wide: 1, tall: 0 };
        let paged = paginate(&l, ROW, COL);
        // 25 columns of 432 dots against 4,260: 39% is the largest that fits.
        assert_eq!(paged.zoom, 39);
        assert!(paged.col_breaks.is_empty());
        l.zoom = Zoom::Fit { wide: 1, tall: 1 };
        assert_eq!(paginate(&l, ROW, COL).zoom, 39);
        assert_eq!(paginate(&l, ROW, COL).pages.len(), 1);
    }

    #[test]
    fn scaling_rounds_rows_and_hidden_lines_take_none() {
        let mut l = layout(vec![area(1, 1, 200, 5)]);
        l.zoom = Zoom::Percent(50);
        // At 50% a row is 61 dots, so 93 fit, as Excel measured.
        assert_eq!(paginate(&l, ROW, COL).row_breaks[0], 93);
        let hidden: &dyn Fn(u32) -> u32 = &|r| if r <= 10 { 0 } else { 20 };
        l.zoom = Zoom::Percent(100);
        assert_eq!(paginate(&l, hidden, COL).row_breaks[0], 57);
    }

    #[test]
    fn centring_and_print_areas() {
        let mut l = layout(vec![area(1, 1, 10, 2), area(20, 3, 25, 4)]);
        l.center = (true, true);
        let paged = paginate(&l, ROW, COL);
        assert_eq!(paged.pages.len(), 2);
        assert_eq!(paged.pages[1].area, 1);
        let width = 864.0 * 72.0 / 600.0;
        let height = 1210.0 * 72.0 / 600.0;
        assert!((paged.pages[0].x - (50.4 + (511.2 - width) / 2.0)).abs() < 1e-9);
        assert!((paged.pages[0].y - (54.0 + (684.0 - height) / 2.0)).abs() < 1e-9);
    }

    #[test]
    fn a_row_taller_than_a_page_gets_one() {
        let tall: &dyn Fn(u32) -> u32 = &|r| if r == 3 { 1200 } else { 20 };
        let paged = paginate(&layout(vec![area(1, 1, 5, 1)]), tall, COL);
        assert_eq!(paged.row_breaks, vec![2, 3]);
    }
}
