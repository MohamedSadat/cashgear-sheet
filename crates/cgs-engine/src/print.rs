//! Where pages break, as Excel breaks them: a page holds as many whole rows and columns as fit
//! inside the margins at the page's scale, title rows and columns repeat on every page after
//! their own, a manual break ends a page, and fitting to pages picks the largest whole percent
//! that fits, ignoring manual breaks as Excel does. Print areas each start their own pages.

use cgs_model::{
    page::{HfRun, HfSections, HfText},
    Anchor, PageOrder, PageSetup, Rect,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::Session;

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
    /// The row headings' width and the column headings' height, when headings print, in sheet
    /// points at 100%.
    pub headings: Option<(f64, f64)>,
    pub center: (bool, bool),
    pub order: PageOrder,
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
    /// What it prints, at 100%, in sheet points.
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paginated {
    /// The percent pages print at.
    pub zoom: u32,
    pub pages: Vec<PageBox>,
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
    // Excel counts in device units, so a sum a hair over from rounding still fits.
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

/// Pages at one percent, with or without manual breaks.
fn pages_at(
    layout: &Layout,
    zoom: u32,
    manual: bool,
    row: &dyn Fn(u32) -> f64,
    col: &dyn Fn(u32) -> f64,
) -> (Vec<PageBox>, Vec<u32>, Vec<u32>) {
    let scale = f64::from(zoom) / 100.0;
    let (left, right, top, bottom) = layout.margins;
    let printable_w = (layout.paper.0 - left - right).max(1.0);
    let printable_h = (layout.paper.1 - top - bottom).max(1.0);
    let (head_w, head_h) = layout.headings.unwrap_or((0.0, 0.0));
    let room_w = printable_w / scale - head_w;
    let room_h = printable_h / scale - head_h;
    let none: Vec<u32> = Vec::new();
    let (manual_rows, manual_cols) = if manual {
        (&layout.manual_rows, &layout.manual_cols)
    } else {
        (&none, &none)
    };
    let title_h = title_size(layout.title_rows, row);
    let title_w = title_size(layout.title_cols, col);
    let mut pages = Vec::new();
    let mut row_breaks = Vec::new();
    let mut col_breaks = Vec::new();
    for (index, area) in layout.areas.iter().enumerate() {
        let rows = segments(
            area.top,
            area.bottom,
            room_h,
            layout.title_rows,
            title_h,
            manual_rows,
            row,
        );
        let cols = segments(
            area.left,
            area.right,
            room_w,
            layout.title_cols,
            title_w,
            manual_cols,
            col,
        );
        if index == 0 {
            row_breaks.extend(rows.iter().rev().skip(1).map(|&(_, end)| end));
            col_breaks.extend(cols.iter().rev().skip(1).map(|&(_, end)| end));
            row_breaks.sort_unstable();
            col_breaks.sort_unstable();
        }
        let mut place = |r: (u32, u32), c: (u32, u32)| {
            let title_rows = layout.title_rows.filter(|&(_, end)| end < r.0);
            let title_cols = layout.title_cols.filter(|&(_, end)| end < c.0);
            let width =
                head_w + title_cols.map_or(0.0, |(a, b)| sum(a, b, col)) + sum(c.0, c.1, col);
            let height =
                head_h + title_rows.map_or(0.0, |(a, b)| sum(a, b, row)) + sum(r.0, r.1, row);
            let x = left
                + if layout.center.0 {
                    ((printable_w - width * scale) / 2.0).max(0.0)
                } else {
                    0.0
                };
            let y = top
                + if layout.center.1 {
                    ((printable_h - height * scale) / 2.0).max(0.0)
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
    (pages, row_breaks, col_breaks)
}

/// How many pages wide and tall the first print area takes at a percent, others added in.
fn extent(
    layout: &Layout,
    zoom: u32,
    row: &dyn Fn(u32) -> f64,
    col: &dyn Fn(u32) -> f64,
) -> (u32, u32) {
    let (pages, _, _) = pages_at(layout, zoom, false, row, col);
    let mut wide = 0u32;
    let mut tall = 0u32;
    for index in 0..layout.areas.len() {
        let here: Vec<&PageBox> = pages.iter().filter(|p| p.area == index).collect();
        let mut cols: Vec<(u32, u32)> = here.iter().map(|p| p.cols).collect();
        cols.dedup();
        cols.sort_unstable();
        cols.dedup();
        let mut rows: Vec<(u32, u32)> = here.iter().map(|p| p.rows).collect();
        rows.sort_unstable();
        rows.dedup();
        wide = wide.max(cols.len() as u32);
        tall += rows.len() as u32;
    }
    (wide, tall)
}

/// Lays out the pages. `row` and `col` give each line's size in points at 100%, hidden ones 0.
pub fn paginate(layout: &Layout, row: &dyn Fn(u32) -> f64, col: &dyn Fn(u32) -> f64) -> Paginated {
    let zoom = match layout.zoom {
        Zoom::Percent(percent) => percent.clamp(10, 400),
        Zoom::Fit { wide, tall } => {
            let fits = |zoom: u32| {
                let (w, t) = extent(layout, zoom, row, col);
                (wide == 0 || w <= wide) && (tall == 0 || t <= tall)
            };
            // Pages only grow as the percent does, so the largest that fits is found by halves.
            if fits(100) {
                100
            } else if !fits(10) {
                10
            } else {
                let (mut low, mut high) = (10u32, 100u32);
                while high - low > 1 {
                    let mid = (low + high) / 2;
                    if fits(mid) {
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
                low
            }
        }
    };
    let manual = matches!(layout.zoom, Zoom::Percent(_));
    let (pages, row_breaks, col_breaks) = pages_at(layout, zoom, manual, row, col);
    Paginated {
        zoom,
        pages,
        row_breaks,
        col_breaks,
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

impl Session {
    /// The cells that print when nothing says which: from the first to the last cell holding a
    /// value, a fill or a border, with any drawing's cells.
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
        struct Seen<'a>(&'a mut dyn FnMut(u32, u32));
        impl CellSink for Seen<'_> {
            fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
                if *value != CalcValue::Empty {
                    (self.0)(row, col);
                }
            }
        }
        let (rows, cols) = info.used;
        if rows > 0 && cols > 0 {
            self.calc.read_block(
                SheetRect {
                    sheet,
                    rect: cgs_calc::Rect {
                        top: 1,
                        left: 1,
                        bottom: rows,
                        right: cols,
                    },
                },
                &mut Seen(&mut take),
            );
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
        for drawing in &info.drawings {
            let (from, to) = match drawing.anchor {
                Anchor::TwoCell { from, to } => (from, to),
                Anchor::OneCell { from, .. } => (from, from),
                Anchor::Absolute { .. } => continue,
            };
            take(from.row, from.col);
            take(to.row, to.col);
        }
        bounds
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
                (digits * digit + 2.0 * (digit / 4.0).ceil() + 1.0) * 0.75,
                f64::from(info.rows.default_px) * 0.75,
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
        })
    }

    pub(crate) fn paginate_sheet(&self, sheet: u32, request: &PrintRequest) -> Option<Paginated> {
        let info = self.book.sheets.get(sheet as usize)?;
        let layout = self.layout_for(sheet, request)?;
        let row = |r: u32| f64::from(info.rows.px(r)) * 0.75;
        let col = |c: u32| f64::from(info.cols.px(c)) * 0.75;
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

    const ROW: &dyn Fn(u32) -> f64 = &|_| 15.0;
    const COL: &dyn Fn(u32) -> f64 = &|_| 48.0;

    #[test]
    fn whole_rows_and_columns_fit_a_letter_page() {
        // 684 points tall holds 45 rows of 15, and 511.2 wide holds 10 columns of 48.
        let paged = paginate(&layout(vec![area(1, 1, 100, 25)]), ROW, COL);
        assert_eq!(paged.row_breaks, vec![45, 90]);
        assert_eq!(paged.col_breaks, vec![10, 20]);
        assert_eq!(paged.pages.len(), 9);
        assert_eq!(paged.pages[1].rows, (46, 90));
        assert_eq!(paged.pages[1].cols, (1, 10));
        assert_eq!(paged.pages[3].cols, (11, 20));
    }

    #[test]
    fn over_then_down_numbers_across_first() {
        let mut l = layout(vec![area(1, 1, 100, 25)]);
        l.order = PageOrder::OverThenDown;
        let paged = paginate(&l, ROW, COL);
        assert_eq!(paged.pages[1].rows, (1, 45));
        assert_eq!(paged.pages[1].cols, (11, 20));
    }

    #[test]
    fn titles_repeat_after_their_own_page_and_take_room() {
        let mut l = layout(vec![area(1, 1, 100, 5)]);
        l.title_rows = Some((1, 2));
        let paged = paginate(&l, ROW, COL);
        assert_eq!(paged.pages[0].rows, (1, 45));
        assert_eq!(paged.pages[0].title_rows, None);
        assert_eq!(paged.pages[1].rows, (46, 88));
        assert_eq!(paged.pages[1].title_rows, Some((1, 2)));
        assert_eq!(paged.pages[1].height, 45.0 * 15.0);
    }

    #[test]
    fn manual_breaks_end_pages_unless_fitting() {
        let mut l = layout(vec![area(1, 1, 60, 5)]);
        l.manual_rows = vec![10];
        let paged = paginate(&l, ROW, COL);
        assert_eq!(paged.row_breaks, vec![10, 55]);
        l.zoom = Zoom::Fit { wide: 1, tall: 0 };
        let fitted = paginate(&l, ROW, COL);
        assert_eq!(fitted.row_breaks, vec![45]);
        assert_eq!(fitted.zoom, 100);
    }

    #[test]
    fn fitting_picks_the_largest_percent_that_fits() {
        let mut l = layout(vec![area(1, 1, 100, 25)]);
        l.zoom = Zoom::Fit { wide: 1, tall: 0 };
        let paged = paginate(&l, ROW, COL);
        // 25 columns of 48 is 1200 points, and 511.2 / 1200 is 42.6%.
        assert_eq!(paged.zoom, 42);
        assert!(paged.col_breaks.is_empty());
        l.zoom = Zoom::Fit { wide: 1, tall: 1 };
        // 100 rows of 15 is 1500 points, and 684 / 1500 is 45.6%, but the width needs 42.
        assert_eq!(paginate(&l, ROW, COL).zoom, 42);
        assert_eq!(paginate(&l, ROW, COL).pages.len(), 1);
    }

    #[test]
    fn scaling_fits_more_and_hidden_lines_take_none() {
        let mut l = layout(vec![area(1, 1, 100, 5)]);
        l.zoom = Zoom::Percent(50);
        assert_eq!(paginate(&l, ROW, COL).row_breaks, vec![91]);
        let hidden: &dyn Fn(u32) -> f64 = &|r| if r <= 10 { 0.0 } else { 15.0 };
        l.zoom = Zoom::Percent(100);
        assert_eq!(paginate(&l, hidden, COL).row_breaks, vec![55]);
    }

    #[test]
    fn centring_and_print_areas() {
        let mut l = layout(vec![area(1, 1, 10, 2), area(20, 3, 25, 4)]);
        l.center = (true, true);
        let paged = paginate(&l, ROW, COL);
        assert_eq!(paged.pages.len(), 2);
        assert_eq!(paged.pages[1].area, 1);
        assert_eq!(paged.pages[0].x, 50.4 + (511.2 - 96.0) / 2.0);
        assert_eq!(paged.pages[0].y, 54.0 + (684.0 - 150.0) / 2.0);
    }

    #[test]
    fn a_row_taller_than_a_page_gets_one() {
        let tall: &dyn Fn(u32) -> f64 = &|r| if r == 3 { 900.0 } else { 15.0 };
        let paged = paginate(&layout(vec![area(1, 1, 5, 1)]), tall, COL);
        assert_eq!(paged.row_breaks, vec![2, 3]);
    }
}
