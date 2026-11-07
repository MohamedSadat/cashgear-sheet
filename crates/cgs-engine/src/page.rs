//! Page setup as the Page Layout tab and the Page Setup dialog change it, and the print area and
//! print titles, which are the defined names `_xlnm.Print_Area` and `_xlnm.Print_Titles`.

use cgs_model::{
    DefinedName, Margins, Orientation, PageOrder, PageSetup, PrintComments, PrintErrors, Rect,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::command::{Applied, Area, Op};
use crate::Session;

pub const PRINT_AREA: &str = "_xlnm.Print_Area";
pub const PRINT_TITLES: &str = "_xlnm.Print_Titles";

/// The parts of page setup to change. Anything left out stays as it is.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PageSpec {
    pub paper: Option<u32>,
    /// `default`, `portrait` or `landscape`.
    pub orientation: Option<String>,
    pub scale: Option<u32>,
    pub fit_to_page: Option<bool>,
    pub fit_width: Option<u32>,
    pub fit_height: Option<u32>,
    /// The first page's number, with `0` for automatic.
    pub first_page: Option<i64>,
    pub first_page_auto: Option<bool>,
    /// `downThenOver` or `overThenDown`.
    pub page_order: Option<String>,
    pub black_and_white: Option<bool>,
    pub draft: Option<bool>,
    /// `none`, `atEnd` or `asDisplayed`.
    pub comments: Option<String>,
    /// `displayed`, `blank`, `dash` or `NA`.
    pub errors: Option<String>,
    pub margins: Option<MarginSpec>,
    pub center_horizontally: Option<bool>,
    pub center_vertically: Option<bool>,
    pub headings: Option<bool>,
    pub gridlines: Option<bool>,
    pub header_footer: Option<HeaderFooterSpec>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MarginSpec {
    pub left: Option<f64>,
    pub right: Option<f64>,
    pub top: Option<f64>,
    pub bottom: Option<f64>,
    pub header: Option<f64>,
    pub footer: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HeaderFooterSpec {
    pub odd_header: Option<String>,
    pub odd_footer: Option<String>,
    pub even_header: Option<String>,
    pub even_footer: Option<String>,
    pub first_header: Option<String>,
    pub first_footer: Option<String>,
    pub different_odd_even: Option<bool>,
    pub different_first: Option<bool>,
    pub scale_with_doc: Option<bool>,
    pub align_with_margins: Option<bool>,
}

/// What Insert Page Break, Remove Page Break and Reset All Page Breaks do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BreakAction {
    Insert,
    Remove,
    Reset,
}

/// What Set Print Area, Add to Print Area and Clear Print Area do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrintAreaAction {
    Set,
    Add,
    Clear,
}

fn margin_ok(value: f64) -> Result<f64, String> {
    if value.is_finite() && (0.0..=49.0).contains(&value) {
        Ok(value)
    } else {
        Err("Margins must be between 0 and 49 inches.".to_string())
    }
}

/// `spec` laid over `page`, refused where Excel refuses it.
pub fn apply_spec(page: &PageSetup, spec: &PageSpec) -> Result<PageSetup, String> {
    let mut next = page.clone();
    if let Some(paper) = spec.paper {
        if cgs_model::page::paper_size(paper).is_none() {
            return Err(format!("Paper size {paper} isn't one this app knows."));
        }
        next.paper = paper;
    }
    if let Some(orientation) = spec.orientation.as_deref() {
        next.orientation = match orientation {
            "portrait" => Orientation::Portrait,
            "landscape" => Orientation::Landscape,
            "default" => Orientation::Default,
            other => return Err(format!("{other} isn't a page orientation.")),
        };
    }
    if let Some(scale) = spec.scale {
        if !(10..=400).contains(&scale) {
            return Err("The scale must be between 10% and 400%.".to_string());
        }
        next.scale = scale;
        if spec.fit_to_page.is_none() {
            next.fit_to_page = false;
        }
    }
    if let Some(fit) = spec.fit_to_page {
        next.fit_to_page = fit;
    }
    if let Some(width) = spec.fit_width {
        if width > 32767 {
            return Err("Fit to at most 32767 pages wide.".to_string());
        }
        next.fit_width = width;
    }
    if let Some(height) = spec.fit_height {
        if height > 32767 {
            return Err("Fit to at most 32767 pages tall.".to_string());
        }
        next.fit_height = height;
    }
    if spec.first_page_auto == Some(true) {
        next.first_page = None;
    } else if let Some(first) = spec.first_page {
        next.first_page = Some(first);
    }
    if let Some(order) = spec.page_order.as_deref() {
        next.page_order = match order {
            "overThenDown" => PageOrder::OverThenDown,
            _ => PageOrder::DownThenOver,
        };
    }
    if let Some(on) = spec.black_and_white {
        next.black_and_white = on;
    }
    if let Some(on) = spec.draft {
        next.draft = on;
    }
    if let Some(comments) = spec.comments.as_deref() {
        next.comments = match comments {
            "atEnd" => PrintComments::AtEnd,
            "asDisplayed" => PrintComments::AsDisplayed,
            _ => PrintComments::None,
        };
    }
    if let Some(errors) = spec.errors.as_deref() {
        next.errors = match errors {
            "blank" => PrintErrors::Blank,
            "dash" => PrintErrors::Dash,
            "NA" => PrintErrors::Na,
            _ => PrintErrors::Displayed,
        };
    }
    if let Some(m) = &spec.margins {
        let old = next.margins;
        next.margins = Margins {
            left: margin_ok(m.left.unwrap_or(old.left))?,
            right: margin_ok(m.right.unwrap_or(old.right))?,
            top: margin_ok(m.top.unwrap_or(old.top))?,
            bottom: margin_ok(m.bottom.unwrap_or(old.bottom))?,
            header: margin_ok(m.header.unwrap_or(old.header))?,
            footer: margin_ok(m.footer.unwrap_or(old.footer))?,
        };
        let (width, height) = next.paper_points();
        let m = next.margins;
        if (m.left + m.right) * 72.0 >= width || (m.top + m.bottom) * 72.0 >= height {
            return Err("Margins don't fit the paper size.".to_string());
        }
    }
    if let Some(on) = spec.center_horizontally {
        next.center_horizontally = on;
    }
    if let Some(on) = spec.center_vertically {
        next.center_vertically = on;
    }
    if let Some(on) = spec.headings {
        next.headings = on;
    }
    if let Some(on) = spec.gridlines {
        next.gridlines = on;
    }
    if let Some(hf) = &spec.header_footer {
        let to = &mut next.header_footer;
        for (slot, text) in [
            (&mut to.odd_header, &hf.odd_header),
            (&mut to.odd_footer, &hf.odd_footer),
            (&mut to.even_header, &hf.even_header),
            (&mut to.even_footer, &hf.even_footer),
            (&mut to.first_header, &hf.first_header),
            (&mut to.first_footer, &hf.first_footer),
        ] {
            if let Some(text) = text {
                if text.chars().count() > 255 {
                    return Err("A header or footer can be at most 255 characters.".to_string());
                }
                *slot = text.clone();
            }
        }
        if let Some(on) = hf.different_odd_even {
            to.different_odd_even = on;
        }
        if let Some(on) = hf.different_first {
            to.different_first = on;
        }
        if let Some(on) = hf.scale_with_doc {
            to.scale_with_doc = on;
        }
        if let Some(on) = hf.align_with_margins {
            to.align_with_margins = on;
        }
    }
    Ok(next)
}

/// A piece of a print area or print titles name: cells, whole rows or whole columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefPart {
    Cells(Rect),
    Rows(u32, u32),
    Cols(u32, u32),
}

/// Splits a name's formula at commas that aren't in a quoted sheet name.
fn split_top(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut start = 0usize;
    for (at, ch) in text.char_indices() {
        match ch {
            '\'' => quoted = !quoted,
            ',' if !quoted => {
                parts.push(text[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim());
    parts.retain(|part| !part.is_empty());
    parts
}

/// Reads `Sheet1!$A$1:$D$20,Sheet1!$1:$2` into its parts. Parts on another sheet are left out.
pub fn read_parts(formula: &str, sheet_name: &str) -> Vec<RefPart> {
    let mut out = Vec::new();
    let formula = formula.trim().trim_start_matches('=');
    let formula = formula
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or(formula);
    for part in split_top(formula) {
        let (name, reference) = crate::query::split_sheet(part);
        if name.is_some_and(|name| !name.eq_ignore_ascii_case(sheet_name)) {
            continue;
        }
        let Some(area) = crate::query::read_area(reference) else {
            continue;
        };
        out.push(if area.left == 1 && area.right == cgs_model::MAX_COLS {
            RefPart::Rows(area.top, area.bottom)
        } else if area.top == 1 && area.bottom == cgs_model::MAX_ROWS {
            RefPart::Cols(area.left, area.right)
        } else {
            RefPart::Cells(Rect {
                top: area.top,
                left: area.left,
                bottom: area.bottom,
                right: area.right,
            })
        });
    }
    out
}

fn col(col: u32) -> String {
    cgs_refs::col_to_letters(i64::from(col))
}

/// A part as a name writes it, with its sheet.
pub fn part_text(sheet_name: &str, part: RefPart) -> String {
    let sheet = cgs_refs::rewrite::quote_sheet(sheet_name);
    match part {
        RefPart::Cells(r) if r.top == r.bottom && r.left == r.right => {
            format!("{sheet}!${}${}", col(r.left), r.top)
        }
        RefPart::Cells(r) => format!(
            "{sheet}!${}${}:${}${}",
            col(r.left),
            r.top,
            col(r.right),
            r.bottom
        ),
        RefPart::Rows(top, bottom) => format!("{sheet}!${top}:${bottom}"),
        RefPart::Cols(left, right) => format!("{sheet}!${}:${}", col(left), col(right)),
    }
}

/// What prints of a sheet, as `print_area` and `print_titles` read them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrintNames {
    pub areas: Vec<Rect>,
    pub title_rows: Option<(u32, u32)>,
    pub title_cols: Option<(u32, u32)>,
}

fn margins_json(m: &Margins) -> Value {
    json!({
        "left": m.left, "right": m.right, "top": m.top,
        "bottom": m.bottom, "header": m.header, "footer": m.footer,
    })
}

/// A sheet's page setup for the UI.
pub fn page_json(page: &PageSetup, names: &PrintNames, sheet_name: &str) -> Value {
    let hf = &page.header_footer;
    let area_text: Vec<String> = names
        .areas
        .iter()
        .map(|&r| {
            part_text(sheet_name, RefPart::Cells(r))
                .rsplit_once('!')
                .map(|(_, text)| text.to_string())
                .unwrap_or_default()
        })
        .collect();
    let rows_text = names
        .title_rows
        .map(|(top, bottom)| format!("${top}:${bottom}"));
    let cols_text = names
        .title_cols
        .map(|(left, right)| format!("${}:${}", col(left), col(right)));
    let papers: Vec<Value> = cgs_model::page::PAPERS
        .iter()
        .map(|&(code, name)| {
            let (w, h) = cgs_model::page::paper_size(code).unwrap_or((612.0, 792.0));
            json!({ "code": code, "name": name, "width": w, "height": h })
        })
        .collect();
    let (width, height) = page.paper_points();
    json!({
        "paper": page.paper,
        "paperWidth": width,
        "paperHeight": height,
        "orientation": match page.orientation {
            Orientation::Default => "default",
            Orientation::Portrait => "portrait",
            Orientation::Landscape => "landscape",
        },
        "scale": page.scale,
        "fitToPage": page.fit_to_page,
        "fitWidth": page.fit_width,
        "fitHeight": page.fit_height,
        "firstPage": page.first_page,
        "pageOrder": match page.page_order {
            PageOrder::DownThenOver => "downThenOver",
            PageOrder::OverThenDown => "overThenDown",
        },
        "blackAndWhite": page.black_and_white,
        "draft": page.draft,
        "comments": match page.comments {
            PrintComments::None => "none",
            PrintComments::AtEnd => "atEnd",
            PrintComments::AsDisplayed => "asDisplayed",
        },
        "errors": match page.errors {
            PrintErrors::Displayed => "displayed",
            PrintErrors::Blank => "blank",
            PrintErrors::Dash => "dash",
            PrintErrors::Na => "NA",
        },
        "margins": margins_json(&page.margins),
        "centerHorizontally": page.center_horizontally,
        "centerVertically": page.center_vertically,
        "headings": page.headings,
        "gridlines": page.gridlines,
        "headerFooter": {
            "oddHeader": hf.odd_header,
            "oddFooter": hf.odd_footer,
            "evenHeader": hf.even_header,
            "evenFooter": hf.even_footer,
            "firstHeader": hf.first_header,
            "firstFooter": hf.first_footer,
            "differentOddEven": hf.different_odd_even,
            "differentFirst": hf.different_first,
            "scaleWithDoc": hf.scale_with_doc,
            "alignWithMargins": hf.align_with_margins,
        },
        "rowBreaks": page.manual_row_breaks().collect::<Vec<_>>(),
        "colBreaks": page.manual_col_breaks().collect::<Vec<_>>(),
        "printArea": area_text.join(","),
        "printAreas": names.areas.iter().map(|r| json!({
            "top": r.top, "left": r.left, "bottom": r.bottom, "right": r.right
        })).collect::<Vec<_>>(),
        "titleRows": rows_text,
        "titleCols": cols_text,
        "papers": papers,
    })
}

impl Session {
    fn sheet_name(&self, sheet: u32) -> Result<String, String> {
        self.book
            .sheets
            .get(sheet as usize)
            .filter(|info| !info.deleted)
            .map(|info| info.name.clone())
            .ok_or_else(|| format!("There is no sheet {sheet}."))
    }

    fn builtin(&self, name: &str, sheet: u32) -> Option<&DefinedName> {
        self.book
            .names
            .iter()
            .find(|n| n.local_sheet == Some(sheet) && n.name.eq_ignore_ascii_case(name))
    }

    /// The sheet's print area and titles, from its names.
    pub(crate) fn print_names(&self, sheet: u32) -> PrintNames {
        let Ok(sheet_name) = self.sheet_name(sheet) else {
            return PrintNames::default();
        };
        let mut names = PrintNames::default();
        if let Some(area) = self.builtin(PRINT_AREA, sheet) {
            for part in read_parts(&area.formula, &sheet_name) {
                names.areas.push(match part {
                    RefPart::Cells(r) => r,
                    RefPart::Rows(top, bottom) => Rect {
                        top,
                        left: 1,
                        bottom,
                        right: cgs_model::MAX_COLS,
                    },
                    RefPart::Cols(left, right) => Rect {
                        top: 1,
                        left,
                        bottom: cgs_model::MAX_ROWS,
                        right,
                    },
                });
            }
        }
        if let Some(titles) = self.builtin(PRINT_TITLES, sheet) {
            for part in read_parts(&titles.formula, &sheet_name) {
                match part {
                    RefPart::Rows(top, bottom) => names.title_rows = Some((top, bottom)),
                    RefPart::Cols(left, right) => names.title_cols = Some((left, right)),
                    RefPart::Cells(_) => {}
                }
            }
        }
        names
    }

    pub(crate) fn page_setup_json(&self, sheet: u32) -> Result<Value, String> {
        let name = self.sheet_name(sheet)?;
        let info = &self.book.sheets[sheet as usize];
        Ok(page_json(&info.page, &self.print_names(sheet), &name))
    }

    fn page_op(&self, sheet: u32, page: PageSetup) -> Op {
        Op::PageSetup {
            sheet,
            page: Box::new(page),
        }
    }

    pub(crate) fn page_setup_command(
        &mut self,
        sheets: &[u32],
        spec: &PageSpec,
    ) -> Result<Applied, String> {
        let mut ops = Vec::new();
        for &sheet in sheets {
            self.sheet_name(sheet)?;
            let info = &self.book.sheets[sheet as usize];
            let next = apply_spec(&info.page, spec)?;
            if next != info.page {
                ops.push(self.page_op(sheet, next));
            }
        }
        if ops.is_empty() {
            return Ok(Applied::default());
        }
        self.perform("page setup", Op::Many(ops), None)
    }

    /// Insert Page Break above and left of a cell, as `row` and `col` say where the new page
    /// starts; Remove Page Break there; or Reset All Page Breaks.
    pub(crate) fn page_break_command(
        &mut self,
        sheet: u32,
        row: Option<u32>,
        col: Option<u32>,
        action: BreakAction,
    ) -> Result<Applied, String> {
        self.sheet_name(sheet)?;
        let mut next = self.book.sheets[sheet as usize].page.clone();
        let before = next.clone();
        let (row, col) = (
            row.map(|r| r.saturating_sub(1)),
            col.map(|c| c.saturating_sub(1)),
        );
        match action {
            BreakAction::Insert => {
                next.insert_break(row, col);
            }
            BreakAction::Remove => {
                next.remove_break(row, col);
            }
            BreakAction::Reset => {
                next.row_breaks.clear();
                next.col_breaks.clear();
            }
        }
        if next == before {
            return Ok(Applied::default());
        }
        let op = self.page_op(sheet, next);
        self.perform("page break", op, None)
    }

    fn with_builtin(&self, sheet: u32, name: &str, formula: Option<String>) -> Vec<DefinedName> {
        let mut names = self.book.names.clone();
        names.retain(|n| !(n.local_sheet == Some(sheet) && n.name.eq_ignore_ascii_case(name)));
        if let Some(formula) = formula {
            names.push(DefinedName {
                name: name.to_string(),
                formula,
                local_sheet: Some(sheet),
                hidden: false,
            });
        }
        names
    }

    pub(crate) fn print_area_command(
        &mut self,
        sheet: u32,
        areas: &[Area],
        action: PrintAreaAction,
    ) -> Result<Applied, String> {
        let sheet_name = self.sheet_name(sheet)?;
        let mut list: Vec<Rect> = match action {
            PrintAreaAction::Add => self.print_names(sheet).areas,
            _ => Vec::new(),
        };
        if action != PrintAreaAction::Clear {
            if areas.is_empty() {
                return Err("Select the cells to print first.".to_string());
            }
            for area in areas {
                let rect = Rect {
                    top: area.top,
                    left: area.left,
                    bottom: area.bottom,
                    right: area.right,
                };
                if !list.contains(&rect) {
                    list.push(rect);
                }
            }
        }
        let formula = (!list.is_empty()).then(|| {
            list.iter()
                .map(|&r| part_text(&sheet_name, RefPart::Cells(r)))
                .collect::<Vec<_>>()
                .join(",")
        });
        let names = self.with_builtin(sheet, PRINT_AREA, formula);
        if names == self.book.names {
            return Ok(Applied::default());
        }
        let op = Op::BookNames(names);
        self.perform("print area", op, None)
    }

    pub(crate) fn print_titles_command(
        &mut self,
        sheet: u32,
        rows: Option<(u32, u32)>,
        cols: Option<(u32, u32)>,
    ) -> Result<Applied, String> {
        let sheet_name = self.sheet_name(sheet)?;
        let mut parts = Vec::new();
        // Excel writes the columns first.
        if let Some((left, right)) = cols {
            if left == 0 || right < left || right > cgs_model::MAX_COLS {
                return Err("Those columns can't repeat.".to_string());
            }
            parts.push(part_text(&sheet_name, RefPart::Cols(left, right)));
        }
        if let Some((top, bottom)) = rows {
            if top == 0 || bottom < top || bottom > cgs_model::MAX_ROWS {
                return Err("Those rows can't repeat.".to_string());
            }
            parts.push(part_text(&sheet_name, RefPart::Rows(top, bottom)));
        }
        let formula = (!parts.is_empty()).then(|| parts.join(","));
        let names = self.with_builtin(sheet, PRINT_TITLES, formula);
        if names == self.book.names {
            return Ok(Applied::default());
        }
        let op = Op::BookNames(names);
        self.perform("print titles", op, None)
    }

    /// Puts a sheet's page setup in place, for undo and redo.
    pub(crate) fn apply_page(&mut self, sheet: u32, page: PageSetup) -> Result<Op, String> {
        let info = self
            .book
            .sheets
            .get_mut(sheet as usize)
            .ok_or_else(|| format!("There is no sheet {sheet}."))?;
        let before = std::mem::replace(&mut info.page, page);
        self.edits.page_sheets.insert(sheet);
        Ok(Op::PageSetup {
            sheet,
            page: Box::new(before),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_print_names() {
        let parts = read_parts("'My Sheet'!$A$1:$D$20,'My Sheet'!$F$3", "my sheet");
        assert_eq!(
            parts,
            vec![
                RefPart::Cells(Rect {
                    top: 1,
                    left: 1,
                    bottom: 20,
                    right: 4
                }),
                RefPart::Cells(Rect {
                    top: 3,
                    left: 6,
                    bottom: 3,
                    right: 6
                }),
            ]
        );
        let titles = read_parts("Sheet1!$A:$B,Sheet1!$1:$2", "Sheet1");
        assert_eq!(titles, vec![RefPart::Cols(1, 2), RefPart::Rows(1, 2)]);
        assert_eq!(
            part_text("My Sheet", RefPart::Rows(1, 3)),
            "'My Sheet'!$1:$3"
        );
        assert_eq!(part_text("S", RefPart::Cols(1, 2)), "S!$A:$B");
        assert_eq!(
            part_text(
                "S",
                RefPart::Cells(Rect {
                    top: 2,
                    left: 3,
                    bottom: 9,
                    right: 27
                })
            ),
            "S!$C$2:$AA$9"
        );
        assert!(read_parts("Other!$A$1:$B$2", "Sheet1").is_empty());
    }

    #[test]
    fn a_spec_changes_only_what_it_names() {
        let page = PageSetup::default();
        let spec: PageSpec = serde_json::from_str(
            r#"{"orientation":"landscape","margins":{"left":0.25},"headerFooter":{"oddFooter":"&CPage &P"}}"#,
        )
        .unwrap();
        let next = apply_spec(&page, &spec).unwrap();
        assert!(next.landscape());
        assert_eq!(next.margins.left, 0.25);
        assert_eq!(next.margins.right, 0.7);
        assert_eq!(next.header_footer.odd_footer, "&CPage &P");
        let scaled = apply_spec(
            &PageSetup {
                fit_to_page: true,
                ..PageSetup::default()
            },
            &serde_json::from_str(r#"{"scale":80}"#).unwrap(),
        )
        .unwrap();
        assert!(!scaled.fit_to_page);
        assert_eq!(scaled.scale, 80);
        assert!(apply_spec(&page, &serde_json::from_str(r#"{"scale":5}"#).unwrap()).is_err());
        assert!(apply_spec(
            &page,
            &serde_json::from_str(r#"{"margins":{"left":5,"right":5}}"#).unwrap()
        )
        .is_err());
    }
}
