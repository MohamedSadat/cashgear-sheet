//! Formatting: what a change from the ribbon does to cell styles, sizes, merges and panes. Styles only
//! append, so every index in the file stays valid. A format over a whole sheet costs runs, not cells:
//! it changes what `SheetInfo` holds, and the save reads each cell's style back from there.

use std::collections::{BTreeMap, HashMap};

use cgs_calc::{CalcEngine, CalcValue, CellRef, Rect as CalcRect, SheetRect};
use cgs_model::{
    ColumnRuns, Freeze, HAlign, LineInfo, ReadingOrder, Rect, SheetView, VAlign, NO_CELL,
};
use cgs_numfmt::NumFmt;
use cgs_xlsx::patch_parts::{BorderEdits, FontEdits, StylesXml, XfChanges};
use serde::Deserialize;

use crate::command::{Area, AxisName, Op};
use crate::edit::{EditCell, EditInput};
use crate::{json, Session};

/// A colour as the ribbon gives it. Theme colours stay references to the theme.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ColorSpec {
    Auto,
    Rgb {
        /// `RRGGBB`.
        rgb: String,
    },
    Theme {
        theme: u8,
        #[serde(default)]
        tint: f64,
    },
}

impl ColorSpec {
    /// `<color>`'s attributes, starting with a space, or nothing for automatic.
    fn attributes(&self) -> Option<String> {
        match self {
            Self::Auto => None,
            Self::Rgb { rgb } => Some(format!(" rgb=\"FF{}\"", rgb.to_uppercase())),
            Self::Theme { theme, tint } if *tint == 0.0 => Some(format!(" theme=\"{theme}\"")),
            // Excel holds a tint in 32767ths, and writes what that comes to.
            Self::Theme { theme, tint } => {
                let tint = (tint * 32767.0).trunc() / 32767.0;
                Some(format!(" theme=\"{theme}\" tint=\"{tint}\""))
            }
        }
    }
}

/// A border line: Excel's name for its style, with `none` for no line.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LineSpec {
    pub style: String,
    #[serde(default)]
    pub color: Option<ColorSpec>,
}

impl LineSpec {
    fn edit(&self) -> Option<(String, String)> {
        if self.style == "none" {
            return None;
        }
        let color = self
            .color
            .as_ref()
            .and_then(ColorSpec::attributes)
            .unwrap_or_else(|| " auto=\"1\"".to_string());
        Some((self.style.clone(), color))
    }
}

/// Lines for a selection, by where a cell's edge falls in it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BorderChange {
    pub top: Option<LineSpec>,
    pub bottom: Option<LineSpec>,
    pub left: Option<LineSpec>,
    pub right: Option<LineSpec>,
    pub inside_horizontal: Option<LineSpec>,
    pub inside_vertical: Option<LineSpec>,
    pub diagonal: Option<LineSpec>,
    pub diagonal_up: Option<bool>,
    pub diagonal_down: Option<bool>,
    /// Every cell gets the top, bottom, left and right lines, as a cell style gives them,
    /// rather than the selection's outside.
    pub each: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FillSpec {
    /// `none`, `solid`, or one of Excel's pattern names.
    pub pattern: String,
    #[serde(default)]
    pub color: Option<ColorSpec>,
    #[serde(default, rename = "patternColor")]
    pub pattern_color: Option<ColorSpec>,
}

/// What a format does. Anything left out stays as it is in each cell.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FormatChange {
    /// Back to the workbook's default format.
    pub clear: bool,
    pub font_name: Option<String>,
    pub font_size: Option<f32>,
    /// Up or down Excel's list of sizes, by this many places.
    pub font_step: Option<i32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub strike: Option<bool>,
    /// `none`, `single`, `double`, `singleAccounting` or `doubleAccounting`.
    pub underline: Option<String>,
    pub font_color: Option<ColorSpec>,
    pub fill: Option<FillSpec>,
    pub number_format: Option<String>,
    pub number_format_id: Option<u32>,
    /// More or fewer decimal places, by this many.
    pub decimals: Option<i32>,
    /// `general`, `left`, `center`, `right`, `fill`, `justify`, `centerContinuous`, `distributed`.
    pub horizontal: Option<String>,
    /// `top`, `center`, `bottom`, `justify`, `distributed`.
    pub vertical: Option<String>,
    pub wrap: Option<bool>,
    pub shrink: Option<bool>,
    pub indent: Option<u8>,
    pub indent_step: Option<i32>,
    pub rotation: Option<u8>,
    /// 0 by context, 1 left to right, 2 right to left.
    pub reading_order: Option<u8>,
    pub border: Option<BorderChange>,
    /// Format Cells' Protection tab: locked cells can't change once the sheet is protected.
    pub locked: Option<bool>,
    /// Hidden cells show no formula once the sheet is protected.
    pub formula_hidden: Option<bool>,
    /// Format Cells' Superscript and Subscript: `superscript`, `subscript` or `baseline`.
    pub vert_align: Option<String>,
    /// A named style, as its place in `cellStyleXfs`: the cells take the parts it includes and
    /// a link to it.
    #[serde(skip)]
    pub named_style: Option<u32>,
}

impl FormatChange {
    /// Whether this can change how tall a row must be.
    fn changes_heights(&self) -> bool {
        self.clear
            || self.named_style.is_some()
            || self.font_name.is_some()
            || self.font_size.is_some()
            || self.font_step.is_some()
            || self.wrap.is_some()
            || self.rotation.is_some()
            || self.shrink.is_some()
    }
}

const FONT_SIZES: [f32; 16] = [
    8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0,
];

/// The next size on Excel's list.
fn stepped_size(size: f32, step: i32) -> f32 {
    let mut size = size;
    for _ in 0..step.abs() {
        size = if step > 0 {
            FONT_SIZES
                .iter()
                .copied()
                .find(|&known| known > size)
                .unwrap_or(size + 10.0)
        } else {
            FONT_SIZES
                .iter()
                .rev()
                .copied()
                .find(|&known| known < size)
                .unwrap_or((size - 1.0).max(1.0))
        };
    }
    size.min(409.0)
}

pub(crate) fn line_style_name(style: cgs_model::LineStyle) -> &'static str {
    use cgs_model::LineStyle::*;
    match style {
        None => "none",
        Hair => "hair",
        Dotted => "dotted",
        Dashed => "dashed",
        DashDot => "dashDot",
        DashDotDot => "dashDotDot",
        Thin => "thin",
        Medium => "medium",
        MediumDashed => "mediumDashed",
        MediumDashDot => "mediumDashDot",
        MediumDashDotDot => "mediumDashDotDot",
        SlantDashDot => "slantDashDot",
        Thick => "thick",
        Double => "double",
    }
}

pub(crate) fn pattern_name(pattern: cgs_model::Pattern) -> &'static str {
    match pattern {
        cgs_model::Pattern::None => "none",
        cgs_model::Pattern::Solid => "solid",
        cgs_model::Pattern::Hatch(hatch) => hatch.name(),
    }
}

pub(crate) fn horizontal_name(align: HAlign) -> &'static str {
    match align {
        HAlign::General => "general",
        HAlign::Left => "left",
        HAlign::Center => "center",
        HAlign::Right => "right",
        HAlign::Fill => "fill",
        HAlign::Justify => "justify",
        HAlign::CenterContinuous => "centerContinuous",
        HAlign::Distributed => "distributed",
    }
}

pub(crate) fn vertical_name(align: VAlign) -> &'static str {
    match align {
        VAlign::Top => "top",
        VAlign::Center => "center",
        VAlign::Bottom => "bottom",
        VAlign::Justify => "justify",
        VAlign::Distributed => "distributed",
    }
}

pub(crate) fn underline_name(underline: cgs_model::Underline) -> &'static str {
    match underline {
        cgs_model::Underline::None => "none",
        cgs_model::Underline::Single => "single",
        cgs_model::Underline::Double => "double",
        cgs_model::Underline::SingleAccounting => "singleAccounting",
        cgs_model::Underline::DoubleAccounting => "doubleAccounting",
    }
}

/// Where a cell sits in the area a border is for, as bits.
const AT_TOP: u8 = 1;
const AT_BOTTOM: u8 = 2;
const AT_LEFT: u8 = 4;
const AT_RIGHT: u8 = 8;

/// A format being applied, with each style it has made so far.
pub(crate) struct StyleEditor {
    change: FormatChange,
    made: HashMap<(u32, u8), u32>,
    /// What General becomes, for more or fewer decimals.
    general_decimals: u32,
}

impl StyleEditor {
    pub(crate) fn new(change: FormatChange) -> Self {
        Self {
            change,
            made: HashMap::new(),
            general_decimals: 0,
        }
    }

    fn by_position(&self) -> bool {
        self.change.border.is_some()
    }
}

/// What applying styles changed, kept to put it back.
#[derive(Debug, Clone, Default)]
pub(crate) struct StylesBefore {
    pub sheet: u32,
    pub columns: BTreeMap<u32, ColumnRuns>,
    pub rows: Vec<(u32, LineInfo)>,
    pub cols: Vec<(u32, u32, LineInfo)>,
    pub areas: Vec<Rect>,
    /// The sheet's default row height, if this changed it.
    pub default_row_px: Option<u32>,
}

impl StylesBefore {
    pub(crate) fn bytes(&self) -> usize {
        64 + self.columns.len() * 32
            + self.rows.len() * 24
            + self.cols.len() * 32
            + self.areas.len() * 16
    }
}

/// How a change meets cells a column doesn't hold.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Gap {
    /// They stay as they are: the row's or column's new style covers them.
    Keep,
    /// A row's own style would hide the column's, so those rows get cells.
    StyledRows,
    /// Each gets a cell of its own.
    Fill,
}

/// A row's or column's cells, past which an intersection isn't worth a cell each.
const MAX_CROSSING_CELLS: u64 = 200_000;
/// A change touching more rows than this leaves their heights to be fitted on request.
const MAX_REFIT_ROWS: usize = 100_000;

fn to_area_rect(area: Area) -> Rect {
    Rect {
        top: area.top,
        left: area.left,
        bottom: area.bottom,
        right: area.right,
    }
}

impl Session {
    pub(crate) fn styles_xml(&mut self) -> &mut StylesXml {
        if self.edits.styles.is_none() {
            let part = cgs_xlsx::workbook::styles_part(&self.package);
            let bytes = part
                .and_then(|part| self.package.read(&part))
                .unwrap_or_else(cgs_xlsx::blank::minimal_styles);
            self.edits.styles = Some(StylesXml::new(bytes));
        }
        self.edits
            .styles
            .get_or_insert_with(|| StylesXml::new(Vec::new()))
    }

    /// Adds a differential format to the workbook, for a filter, a sort or a conditional format.
    pub(crate) fn add_dxf(&mut self, dxf: cgs_model::Dxf) -> u32 {
        if let Some(at) = self.book.styles.dxfs.iter().position(|known| *known == dxf) {
            return at as u32;
        }
        let xml = cgs_xlsx::dxf::dxf_xml(&dxf);
        let index = self.styles_xml().add_dxf(&xml);
        self.refresh_styles();
        index
    }

    /// Reads the styles back after adding some.
    pub(crate) fn refresh_styles(&mut self) {
        let Some(styles) = self.edits.styles.as_ref() else {
            return;
        };
        if !styles.changed {
            return;
        }
        let mut table = cgs_xlsx::styles::read_styles(&styles.bytes);
        table.colors.theme = std::mem::take(&mut self.book.styles.colors.theme);
        self.book.styles = table;
        self.cache = crate::tile::TileCache::default();
    }

    /// The style a format gives a cell that has `base`, made once for each base and position.
    fn changed_style(&mut self, editor: &mut StyleEditor, base: u32, place: u8) -> u32 {
        if editor.change.clear {
            return 0;
        }
        if let Some(&style) = editor.made.get(&(base, place)) {
            return style;
        }
        let style = self.make_style(editor, base, place);
        editor.made.insert((base, place), style);
        style
    }

    pub(crate) fn make_style(&mut self, editor: &StyleEditor, base: u32, place: u8) -> u32 {
        let change = &editor.change;
        if let Some(index) = change.named_style {
            let changes = self.style_changes(index, base);
            return self.styles_xml().add_xf(base, changes);
        }
        let xf = *self.book.styles.xf(base);
        let font = self.book.styles.font(&xf).clone();
        let mut font_edits = FontEdits::default();
        if let Some(name) = change.font_name.as_ref().filter(|name| **name != font.name) {
            font_edits.name = Some(name.clone());
        }
        let size = change
            .font_size
            .or_else(|| change.font_step.map(|step| stepped_size(font.size, step)));
        font_edits.size = size.filter(|size| *size != font.size);
        font_edits.bold = change.bold.filter(|on| *on != font.bold);
        font_edits.italic = change.italic.filter(|on| *on != font.italic);
        font_edits.strike = change.strike.filter(|on| *on != font.strike);
        if let Some(underline) = &change.underline {
            if underline != underline_name(font.underline) {
                font_edits.underline = Some((underline != "none").then(|| underline.clone()));
            }
        }
        if let Some(color) = &change.font_color {
            font_edits.color = Some(color.attributes());
        }
        if let Some(vert) = &change.vert_align {
            let next = cgs_model::VertAlign::parse(vert);
            if next != font.vert_align {
                font_edits.vert_align = Some(next.name().map(str::to_string));
            }
        }
        let new_font = (font_edits != FontEdits::default())
            .then(|| self.styles_xml().edit_font(xf.font, &font_edits))
            .filter(|&index| index != xf.font);

        let new_fill = change.fill.as_ref().map(|fill| {
            let color = |name: &str, spec: &Option<ColorSpec>, automatic: &str| {
                let attributes = spec
                    .as_ref()
                    .and_then(ColorSpec::attributes)
                    .unwrap_or_else(|| automatic.to_string());
                format!("<{name}{attributes}/>")
            };
            let inner = match fill.pattern.as_str() {
                "none" => String::new(),
                "solid" => format!(
                    "{}<bgColor indexed=\"64\"/>",
                    color("fgColor", &fill.color, " auto=\"1\"")
                ),
                _ => format!(
                    "{}{}",
                    color("fgColor", &fill.pattern_color, " auto=\"1\""),
                    color("bgColor", &fill.color, " indexed=\"64\"")
                ),
            };
            self.styles_xml().add_pattern_fill(&fill.pattern, &inner)
        });
        let new_fill = new_fill.filter(|&index| index != xf.fill);

        let new_border = change.border.as_ref().and_then(|border| {
            let mut edits = BorderEdits::default();
            let line = |spec: &Option<LineSpec>| spec.as_ref().map(LineSpec::edit);
            let each = border.each;
            edits.top = if each || place & AT_TOP != 0 {
                line(&border.top)
            } else {
                line(&border.inside_horizontal)
            };
            edits.bottom = if each || place & AT_BOTTOM != 0 {
                line(&border.bottom)
            } else {
                line(&border.inside_horizontal)
            };
            edits.left = if each || place & AT_LEFT != 0 {
                line(&border.left)
            } else {
                line(&border.inside_vertical)
            };
            edits.right = if each || place & AT_RIGHT != 0 {
                line(&border.right)
            } else {
                line(&border.inside_vertical)
            };
            edits.diagonal = line(&border.diagonal);
            edits.diagonal_up = border.diagonal_up;
            edits.diagonal_down = border.diagonal_down;
            (edits != BorderEdits::default())
                .then(|| self.styles_xml().edit_border(xf.border, &edits))
        });
        let new_border = new_border.filter(|&index| index != xf.border);

        let code_of = |session: &Self, id: u32| -> Option<String> {
            session
                .book
                .styles
                .num_fmts
                .get(&id)
                .cloned()
                .or_else(|| cgs_numfmt::builtin_code(id).map(str::to_string))
        };
        let new_num_fmt = if let Some(id) = change.number_format_id {
            Some(id)
        } else if let Some(code) = &change.number_format {
            Some(match cgs_numfmt::builtin_id(code) {
                Some(id) => id,
                None => self.styles_xml().add_num_fmt(code),
            })
        } else if let Some(step) = change.decimals {
            code_of(self, xf.num_fmt)
                .and_then(|code| cgs_numfmt::change_decimals(&code, step, editor.general_decimals))
                .map(|code| match cgs_numfmt::builtin_id(&code) {
                    Some(id) => id,
                    None => self.styles_xml().add_num_fmt(&code),
                })
        } else {
            None
        };
        let new_num_fmt = new_num_fmt.filter(|&id| id != xf.num_fmt);

        let old = xf.alignment;
        let mut alignment_edits: Vec<(&'static str, Option<String>)> = Vec::new();
        let mut horizontal = change
            .horizontal
            .as_deref()
            .and_then(|name| (name != horizontal_name(old.horizontal)).then_some(name));
        let indent = change.indent.or_else(|| {
            change
                .indent_step
                .map(|step| (i32::from(old.indent) + step).clamp(0, 250) as u8)
        });
        if let Some(indent) = indent.filter(|&indent| indent != old.indent) {
            alignment_edits.push(("indent", (indent != 0).then(|| indent.to_string())));
            // An indent only means something from an edge, and Excel picks the left.
            if indent > 0
                && horizontal.is_none()
                && matches!(
                    old.horizontal,
                    HAlign::General | HAlign::Center | HAlign::Fill
                )
            {
                horizontal = Some("left");
            }
        }
        if let Some(name) = horizontal {
            alignment_edits.push(("horizontal", (name != "general").then(|| name.to_string())));
        }
        if let Some(name) = change.vertical.as_deref() {
            if name != vertical_name(old.vertical) {
                alignment_edits.push(("vertical", (name != "bottom").then(|| name.to_string())));
            }
        }
        let flag = |on: bool| on.then(|| "1".to_string());
        if let Some(on) = change.wrap.filter(|&on| on != old.wrap) {
            alignment_edits.push(("wrapText", flag(on)));
        }
        if let Some(on) = change.shrink.filter(|&on| on != old.shrink) {
            alignment_edits.push(("shrinkToFit", flag(on)));
        }
        if let Some(rotation) = change.rotation.filter(|&r| r != old.rotation) {
            alignment_edits.push((
                "textRotation",
                (rotation != 0).then(|| rotation.to_string()),
            ));
        }
        if let Some(order) = change.reading_order {
            let same = match old.reading_order {
                ReadingOrder::Context => 0,
                ReadingOrder::LeftToRight => 1,
                ReadingOrder::RightToLeft => 2,
            };
            if order != same {
                alignment_edits.push(("readingOrder", (order != 0).then(|| order.to_string())));
            }
        }

        let protection = (change.locked.is_some() || change.formula_hidden.is_some())
            .then(|| {
                (
                    change.locked.unwrap_or(xf.locked),
                    change.formula_hidden.unwrap_or(xf.formula_hidden),
                )
            })
            .filter(|&(locked, hidden)| locked != xf.locked || hidden != xf.formula_hidden);
        if new_font.is_none()
            && new_fill.is_none()
            && new_border.is_none()
            && new_num_fmt.is_none()
            && alignment_edits.is_empty()
            && protection.is_none()
        {
            return base;
        }
        self.styles_xml().add_xf(
            base,
            XfChanges {
                font: new_font,
                fill: new_fill,
                border: new_border,
                num_fmt: new_num_fmt,
                alignment: None,
                alignment_edits,
                protection,
                style: None,
            },
        )
    }

    /// Where a cell's stretch of rows falls in an area, cut so each piece has one place.
    fn places(
        editor: &StyleEditor,
        area: Area,
        (first, last): (u32, u32),
        col: u32,
    ) -> Vec<(u32, u32, u8)> {
        if !editor.by_position() {
            return vec![(first, last, 0)];
        }
        let side = u8::from(col == area.left) * AT_LEFT + u8::from(col == area.right) * AT_RIGHT;
        let mut out = Vec::with_capacity(3);
        let mut at = first;
        for stop in [area.top, area.bottom.saturating_sub(1), area.bottom] {
            if at > last {
                break;
            }
            if stop < at {
                continue;
            }
            let end = stop.min(last);
            let row_place =
                u8::from(at == area.top) * AT_TOP + u8::from(at == area.bottom) * AT_BOTTOM;
            out.push((at, end, row_place + side));
            at = end + 1;
        }
        if at <= last {
            out.push((at, last, side));
        }
        out
    }

    /// Changes the styles of one column between two rows.
    fn format_column(
        &mut self,
        sheet: u32,
        col: u32,
        area: Area,
        gap: Gap,
        editor: &mut StyleEditor,
        before: &mut StylesBefore,
    ) {
        let info = &self.book.sheets[sheet as usize];
        if let std::collections::btree_map::Entry::Vacant(entry) = before.columns.entry(col) {
            entry.insert(info.style_column(col));
        }
        let info = &self.book.sheets[sheet as usize];
        let spans = info.style_spans(col, area.top, area.bottom);
        let col_style = info.cols.info(col).and_then(|line| line.style).unwrap_or(0);
        let styled_rows: Vec<(u32, u32)> = if gap == Gap::Keep {
            Vec::new()
        } else {
            let list = &info.rows.rows;
            let at = list.partition_point(|&(row, _)| row < area.top);
            list[at..]
                .iter()
                .take_while(|&&(row, _)| row <= area.bottom)
                .filter_map(|&(row, line)| line.style.map(|style| (row, style)))
                .collect()
        };
        let mut out: Vec<(u32, u32, u32)> = Vec::with_capacity(spans.len());
        for (first, last, style) in spans {
            if style != NO_CELL {
                for (from, to, place) in Self::places(editor, area, (first, last), col) {
                    out.push((from, to, self.changed_style(editor, style, place)));
                }
                continue;
            }
            let mut at = first;
            let from = styled_rows.partition_point(|&(row, _)| row < first);
            for &(row, row_style) in styled_rows[from..].iter().take_while(|&&(r, _)| r <= last) {
                if row > at {
                    self.format_gap(editor, area, col, (at, row - 1), gap, col_style, &mut out);
                }
                for (a, b, place) in Self::places(editor, area, (row, row), col) {
                    out.push((a, b, self.changed_style(editor, row_style, place)));
                }
                at = row + 1;
            }
            if at <= last {
                self.format_gap(editor, area, col, (at, last), gap, col_style, &mut out);
            }
        }
        self.book.sheets[sheet as usize].replace_styles(col, area.top, area.bottom, &out);
    }

    #[allow(clippy::too_many_arguments)]
    fn format_gap(
        &mut self,
        editor: &mut StyleEditor,
        area: Area,
        col: u32,
        (first, last): (u32, u32),
        gap: Gap,
        col_style: u32,
        out: &mut Vec<(u32, u32, u32)>,
    ) {
        if gap != Gap::Fill {
            out.push((first, last, NO_CELL));
            return;
        }
        for (a, b, place) in Self::places(editor, area, (first, last), col) {
            out.push((a, b, self.changed_style(editor, col_style, place)));
        }
    }

    /// Applies a format to one area.
    fn format_area(
        &mut self,
        sheet: u32,
        area: Area,
        editor: &mut StyleEditor,
        before: &mut StylesBefore,
    ) {
        let whole_cols = area.top == 1 && area.bottom == cgs_model::MAX_ROWS;
        let whole_rows = area.left == 1 && area.right == cgs_model::MAX_COLS;
        let info = &self.book.sheets[sheet as usize];
        let slots = info.styled_cols();
        let rows_with_style = info
            .rows
            .rows
            .iter()
            .filter(|(_, line)| line.style.is_some())
            .count() as u64;
        let width = u64::from(area.right - area.left + 1);
        let gap = if whole_cols && whole_rows {
            Gap::Keep
        } else if whole_cols {
            if rows_with_style * width <= MAX_CROSSING_CELLS {
                Gap::StyledRows
            } else {
                Gap::Keep
            }
        } else if whole_rows {
            Gap::Keep
        } else {
            Gap::Fill
        };
        let last_slot = if whole_cols || whole_rows {
            area.right.min(slots)
        } else {
            area.right
        };
        for col in area.left..=last_slot {
            self.format_column(sheet, col, area, gap, editor, before);
        }
        if whole_rows && !whole_cols {
            // A column's own style shows through a row's only as cells.
            let info = &self.book.sheets[sheet as usize];
            let styled_cols: Vec<(u32, u32)> = info
                .cols
                .ranges
                .iter()
                .filter(|(_, _, line)| line.style.is_some_and(|style| style != 0))
                .map(|&(first, last, _)| (first, last))
                .collect();
            let crossing: u64 = styled_cols
                .iter()
                .map(|&(first, last)| u64::from(last - first + 1) * u64::from(area.rows()))
                .sum();
            if crossing <= MAX_CROSSING_CELLS {
                for (first, last) in styled_cols {
                    for col in first..=last {
                        self.format_column(sheet, col, area, Gap::Fill, editor, before);
                    }
                }
            }
        }
        if whole_cols {
            let slices = self.book.sheets[sheet as usize]
                .cols
                .slices(area.left, area.right);
            for (first, last, line) in slices {
                let cut = editor.by_position();
                let pieces: Vec<(u32, u32)> = if cut {
                    (first..=last).map(|col| (col, col)).collect()
                } else {
                    vec![(first, last)]
                };
                for (from, to) in pieces {
                    let place = if cut {
                        u8::from(from == area.left) * AT_LEFT
                            + u8::from(to == area.right) * AT_RIGHT
                    } else {
                        0
                    };
                    let style = self.changed_style(editor, line.style.unwrap_or(0), place);
                    before.cols.push((from, to, line));
                    self.book.sheets[sheet as usize].cols.set(
                        from,
                        to,
                        LineInfo {
                            style: (style != 0).then_some(style),
                            ..line
                        },
                    );
                }
            }
        }
        if whole_rows {
            let info = &self.book.sheets[sheet as usize];
            let mut rows: Vec<(u32, LineInfo)> = Vec::new();
            if whole_cols {
                rows.extend(
                    info.rows
                        .rows
                        .iter()
                        .copied()
                        .filter(|(_, line)| line.style.is_some()),
                );
            } else {
                let lines: HashMap<u32, LineInfo> = info
                    .rows
                    .rows
                    .iter()
                    .filter(|(row, _)| *row >= area.top && *row <= area.bottom)
                    .copied()
                    .collect();
                for row in area.top..=area.bottom {
                    rows.push((row, lines.get(&row).copied().unwrap_or_default()));
                }
            }
            let mut set = Vec::with_capacity(rows.len());
            for &(row, line) in &rows {
                let place = if editor.by_position() {
                    u8::from(row == area.top) * AT_TOP + u8::from(row == area.bottom) * AT_BOTTOM
                } else {
                    0
                };
                let style = self.changed_style(editor, line.style.unwrap_or(0), place);
                set.push((
                    row,
                    LineInfo {
                        style: (style != 0).then_some(style),
                        ..line
                    },
                ));
            }
            before.rows.extend(rows);
            self.book.sheets[sheet as usize].rows.set_many(&set);
        }
        if !whole_cols && !whole_rows {
            let info = &mut self.book.sheets[sheet as usize];
            info.used.0 = info.used.0.max(area.bottom);
            info.used.1 = info.used.1.max(area.right);
        }
        before.areas.push(to_area_rect(area));
    }

    /// Notes that an area's styles changed, for repainting and saving.
    pub(crate) fn note_styled(&mut self, sheet: u32, areas: &[Rect]) {
        let list = self.edits.style_areas.entry(sheet).or_default();
        for area in areas {
            list.retain(|old: &Rect| {
                !(area.top <= old.top
                    && area.left <= old.left
                    && area.bottom >= old.bottom
                    && area.right >= old.right)
            });
            if !list.iter().any(|old| {
                old.top <= area.top
                    && old.left <= area.left
                    && old.bottom >= area.bottom
                    && old.right >= area.right
            }) {
                list.push(*area);
            }
        }
        // Many small areas cost more to check for each row than one that's too big.
        if list.len() > 256 {
            let mut all = list[0];
            for area in list.iter() {
                all.top = all.top.min(area.top);
                all.left = all.left.min(area.left);
                all.bottom = all.bottom.max(area.bottom);
                all.right = all.right.max(area.right);
            }
            *list = vec![all];
        }
        for area in areas {
            self.dirty_area(sheet, *area);
        }
    }

    fn dirty_area(&mut self, sheet: u32, area: Rect) {
        let tiles = u64::from((area.bottom - area.top) / cgs_model::TILE_ROWS + 2)
            * u64::from((area.right - area.left) / cgs_model::TILE_COLS + 2);
        if tiles > 512 {
            self.dirty.sheets.insert(sheet);
            return;
        }
        let mut row = area.top;
        loop {
            let mut col = area.left;
            loop {
                self.dirty.cell(sheet, row, col);
                if col >= area.right {
                    break;
                }
                col = (col + cgs_model::TILE_COLS).min(area.right);
            }
            if row >= area.bottom {
                break;
            }
            row = (row + cgs_model::TILE_ROWS).min(area.bottom);
        }
    }

    /// Applies a format, and gives what puts it back.
    pub(crate) fn apply_format(
        &mut self,
        sheet: u32,
        areas: &[Area],
        change: FormatChange,
        active: Option<(u32, u32)>,
    ) -> Result<Op, String> {
        if let Some(code) = &change.number_format {
            if !NumFmt::parse(code).is_valid() {
                return Err("Excel can't use that number format.".to_string());
            }
        }
        let heights = change.changes_heights();
        let mut editor = StyleEditor::new(change);
        if editor.change.decimals.is_some() {
            editor.general_decimals =
                active.map_or(0, |(row, col)| self.shown_decimals(sheet, row, col));
        }
        let mut before = StylesBefore {
            sheet,
            ..StylesBefore::default()
        };
        let border = editor.change.border.clone();
        for area in areas {
            self.format_area(sheet, *area, &mut editor, &mut before);
            if let Some(border) = &border {
                self.clear_facing_lines(sheet, *area, border, &mut before);
            }
        }
        self.refresh_styles();
        let rects = before.areas.clone();
        self.note_styled(sheet, &rects);
        // Blank rows are as tall as the fonts of whole columns need, as in Excel.
        if heights
            && rects
                .iter()
                .any(|area| area.top == 1 && area.bottom == cgs_model::MAX_ROWS)
        {
            let table = &self.book.styles;
            let info = &self.book.sheets[sheet as usize];
            let mut tallest = cgs_model::style::fitted_row_px(table.normal_font());
            for &(_, _, line) in &info.cols.ranges {
                if let Some(style) = line.style {
                    tallest =
                        tallest.max(cgs_model::style::fitted_row_px(table.font(table.xf(style))));
                }
            }
            let info = &mut self.book.sheets[sheet as usize];
            if tallest != info.rows.default_px {
                before.default_row_px = Some(info.rows.default_px);
                info.rows.default_px = tallest;
                self.edits.default_rows.insert(sheet);
                self.dirty.sheets.insert(sheet);
                self.dirty.layouts.insert(sheet);
            }
        }
        if heights {
            for area in &rects {
                let info = &self.book.sheets[sheet as usize];
                let bottom = area.bottom.min(info.used.0);
                for row in area.top..=bottom {
                    self.touch_row(sheet, row);
                }
            }
        }
        Ok(Op::Styles(Box::new(before)))
    }

    /// A line on one side of a cell and a different one on its neighbour's side can't both show, so
    /// a new edge takes the neighbour's facing line away, as Excel does.
    fn clear_facing_lines(
        &mut self,
        sheet: u32,
        area: Area,
        border: &BorderChange,
        before: &mut StylesBefore,
    ) {
        let cleared = |top: bool, bottom: bool, left: bool, right: bool| {
            let none = || {
                Some(LineSpec {
                    style: "none".to_string(),
                    color: None,
                })
            };
            StyleEditor::new(FormatChange {
                border: Some(BorderChange {
                    top: top.then(none).flatten(),
                    bottom: bottom.then(none).flatten(),
                    left: left.then(none).flatten(),
                    right: right.then(none).flatten(),
                    ..BorderChange::default()
                }),
                ..FormatChange::default()
            })
        };
        let mut neighbours: Vec<(Area, StyleEditor)> = Vec::new();
        if border.top.is_some() && area.top > 1 {
            let side = Area {
                top: area.top - 1,
                left: area.left,
                bottom: area.top - 1,
                right: area.right,
            };
            neighbours.push((side, cleared(false, true, false, false)));
        }
        if border.bottom.is_some() && area.bottom < cgs_model::MAX_ROWS {
            let side = Area {
                top: area.bottom + 1,
                left: area.left,
                bottom: area.bottom + 1,
                right: area.right,
            };
            neighbours.push((side, cleared(true, false, false, false)));
        }
        if border.left.is_some() && area.left > 1 {
            let side = Area {
                top: area.top,
                left: area.left - 1,
                bottom: area.bottom,
                right: area.left - 1,
            };
            neighbours.push((side, cleared(false, false, false, true)));
        }
        if border.right.is_some() && area.right < cgs_model::MAX_COLS {
            let side = Area {
                top: area.top,
                left: area.right + 1,
                bottom: area.bottom,
                right: area.right + 1,
            };
            neighbours.push((side, cleared(false, false, true, false)));
        }
        for (side, mut editor) in neighbours {
            // Only cells that hold such a line change: a blank neighbour stays with no cell.
            for col in side.left
                ..=side
                    .right
                    .min(self.book.sheets[sheet as usize].styled_cols())
            {
                let info = &self.book.sheets[sheet as usize];
                let bottom = side.bottom.min(info.used.0.max(side.top));
                let spans = info.style_spans(col, side.top, bottom);
                if spans.iter().all(|&(_, _, style)| style == NO_CELL) {
                    continue;
                }
                if let std::collections::btree_map::Entry::Vacant(entry) = before.columns.entry(col)
                {
                    entry.insert(info.style_column(col));
                }
                let mut out = Vec::with_capacity(spans.len());
                for (first, last, style) in spans {
                    if style == NO_CELL {
                        out.push((first, last, NO_CELL));
                    } else {
                        // Every side is "outside" for this editor: it only names what it clears.
                        let place = AT_TOP | AT_BOTTOM | AT_LEFT | AT_RIGHT;
                        out.push((first, last, self.changed_style(&mut editor, style, place)));
                    }
                }
                self.book.sheets[sheet as usize].replace_styles(col, side.top, bottom, &out);
                before.areas.push(Rect {
                    top: side.top,
                    left: col,
                    bottom,
                    right: col,
                });
            }
        }
    }

    /// How many decimals a cell's General display shows, for the first step of "more decimals".
    fn shown_decimals(&self, sheet: u32, row: u32, col: u32) -> u32 {
        let at = CellRef { sheet, row, col };
        let CalcValue::Number(number) = self.value_of(at) else {
            return 0;
        };
        let text = format!("{number}");
        text.split_once('.')
            .map_or(0, |(_, tail)| tail.len() as u32)
    }

    pub(crate) fn value_of(&self, at: CellRef) -> CalcValue {
        struct One(CalcValue);
        impl cgs_calc::CellSink for One {
            fn cell(&mut self, _: u32, _: u32, value: &CalcValue) {
                self.0 = value.clone();
            }
        }
        let mut one = One(CalcValue::Empty);
        self.calc.read_block(
            SheetRect {
                sheet: at.sheet,
                rect: CalcRect {
                    top: at.row,
                    left: at.col,
                    bottom: at.row,
                    right: at.col,
                },
            },
            &mut one,
        );
        one.0
    }

    /// Puts back what a format changed, and keeps what's there now for going forward again.
    pub(crate) fn apply_styles_before(&mut self, before: StylesBefore) -> Op {
        let sheet = before.sheet;
        let mut after = StylesBefore {
            sheet,
            areas: before.areas.clone(),
            ..StylesBefore::default()
        };
        let info = &mut self.book.sheets[sheet as usize];
        for (col, runs) in before.columns {
            after.columns.insert(col, info.set_style_column(col, runs));
        }
        // Later records of the same line came after earlier ones, so going back goes in reverse.
        for &(first, last, line) in before.cols.iter().rev() {
            after.cols.extend(info.cols.slices(first, last));
            info.cols.set(first, last, line);
        }
        // The first record of a row is what it held before any of this.
        let mut seen = std::collections::HashSet::new();
        let mut rows: Vec<(u32, LineInfo)> = before
            .rows
            .into_iter()
            .filter(|&(row, _)| seen.insert(row))
            .collect();
        rows.sort_by_key(|&(row, _)| row);
        for &(row, _) in &rows {
            after
                .rows
                .push((row, info.rows.info(row).copied().unwrap_or_default()));
        }
        info.rows.set_many(&rows);
        if let Some(px) = before.default_row_px {
            after.default_row_px = Some(info.rows.default_px);
            info.rows.default_px = px;
            self.edits.default_rows.insert(sheet);
            self.dirty.sheets.insert(sheet);
        }
        self.note_styled(sheet, &before.areas);
        self.dirty.layouts.insert(sheet);
        Op::Styles(Box::new(after))
    }

    /// Sets sizes and hiding, and gives what puts them back.
    pub(crate) fn apply_lines(
        &mut self,
        sheet: u32,
        rows: Vec<(u32, LineInfo)>,
        cols: Vec<(u32, u32, LineInfo)>,
    ) -> Op {
        let info = &mut self.book.sheets[sheet as usize];
        let mut old_rows = Vec::with_capacity(rows.len());
        // Rows come in order, so the records are walked alongside.
        let records = &info.rows.rows;
        let mut at = 0;
        let mut sorted = true;
        let mut last = 0;
        for &(row, _) in &rows {
            sorted &= row >= last;
            last = row;
            if !sorted {
                old_rows.push((row, info.rows.info(row).copied().unwrap_or_default()));
                continue;
            }
            while at < records.len() && records[at].0 < row {
                at += 1;
            }
            let line = match records.get(at) {
                Some(&(found, line)) if found == row => line,
                _ => LineInfo::default(),
            };
            old_rows.push((row, line));
        }
        info.rows.set_many(&rows);
        let mut old_cols = Vec::new();
        for &(first, last, line) in &cols {
            old_cols.extend(info.cols.slices(first, last));
            info.cols.set(first, last, line);
        }
        if !rows.is_empty() {
            // Runs of rows first, as a filter can change a million at once.
            let mut spans: Vec<(u32, u32)> = Vec::new();
            for &(row, _) in &rows {
                match spans.last_mut() {
                    Some((_, end)) if *end + 1 == row => *end = row,
                    _ => spans.push((row, row)),
                }
            }
            let list = self.edits.row_lines.entry(sheet).or_default();
            list.extend(spans);
            join_spans(list);
        }
        if !cols.is_empty() {
            let list = self.edits.col_lines.entry(sheet).or_default();
            list.extend(cols.iter().map(|&(first, last, _)| (first, last)));
            join_spans(list);
        }
        self.dirty.sheets.insert(sheet);
        self.dirty.layouts.insert(sheet);
        Op::Lines {
            sheet,
            rows: old_rows,
            cols: old_cols,
        }
    }

    /// The records of a resize or a hide, as `Op::Lines` wants them.
    pub(crate) fn lines_op(
        &mut self,
        sheet: u32,
        axis: AxisName,
        first: u32,
        last: u32,
        size: Option<u32>,
        hidden: Option<bool>,
    ) -> Op {
        let info = &self.book.sheets[sheet as usize];
        let change = |mut line: LineInfo| {
            if let Some(px) = size {
                line.size = Some(px);
                line.custom = true;
                // A size of 0 is how Excel hides, and any other shows the line.
                line.hidden = px == 0;
            }
            if let Some(hide) = hidden {
                line.hidden = hide;
                // Showing a line that hiding took down to nothing gives it its size back.
                if !hide && line.size == Some(0) {
                    line.size = None;
                    line.custom = false;
                }
            }
            line
        };
        match axis {
            AxisName::Row => {
                let lines: HashMap<u32, LineInfo> = info
                    .rows
                    .rows
                    .iter()
                    .filter(|(row, _)| *row >= first && *row <= last)
                    .copied()
                    .collect();
                let rows = (first..=last)
                    .map(|row| (row, change(lines.get(&row).copied().unwrap_or_default())))
                    .collect();
                Op::Lines {
                    sheet,
                    rows,
                    cols: Vec::new(),
                }
            }
            AxisName::Col => {
                let cols = info
                    .cols
                    .slices(first, last)
                    .into_iter()
                    .map(|(from, to, line)| (from, to, change(line)))
                    .collect();
                Op::Lines {
                    sheet,
                    rows: Vec::new(),
                    cols,
                }
            }
        }
    }

    /// Asks for these rows to be checked for how tall they need to be, once this command is done.
    pub(crate) fn touch_row(&mut self, sheet: u32, row: u32) {
        if self.touched_rows.len() <= MAX_REFIT_ROWS {
            self.touched_rows.insert((sheet, row));
        }
    }

    /// How tall a row must be for its fonts, and which of its cells need measuring first.
    fn row_needs(&self, sheet: u32, row: u32) -> (u32, Vec<u32>) {
        let info = &self.book.sheets[sheet as usize];
        let table = &self.book.styles;
        let mut tallest = 0;
        let mut measure = Vec::new();
        let normal = table.normal_font();
        // The workbook's own font is what the default row already fits.
        let fits = |font: &cgs_model::Font| {
            if font.name == normal.name && font.size == normal.size {
                0
            } else {
                cgs_model::style::fitted_row_px(font)
            }
        };
        let line_style = info.rows.info(row).and_then(|line| line.style);
        if let Some(style) = line_style {
            tallest = fits(table.font(table.xf(style)));
        }
        for (&(_, col), index) in info.rich.iter().filter(|((at, _), _)| *at == row) {
            if let Some(runs) = self.book.rich_strings.get(index) {
                let cell = table.font(table.xf(info.style_at(row, col)));
                tallest = tallest.max(cgs_model::style::rich_row_px(runs, cell));
            }
        }
        for (col, style) in info.own_styles_in_row(row) {
            let xf = table.xf(style);
            tallest = tallest.max(fits(table.font(xf)));
            let wraps = xf.alignment.wrap || xf.alignment.rotation != 0;
            if wraps && info.merge_at(row, col).is_none() {
                let at = CellRef { sheet, row, col };
                if !matches!(self.value_of(at), CalcValue::Empty) {
                    measure.push(col);
                }
            }
        }
        (tallest, measure)
    }

    /// Fits the rows a command touched, as far as fonts say. Rows that wrap wait to be measured.
    pub(crate) fn refit_touched(&mut self) -> Option<Op> {
        let touched: Vec<(u32, u32)> = std::mem::take(&mut self.touched_rows).into_iter().collect();
        let mut by_sheet: BTreeMap<u32, Vec<(u32, LineInfo)>> = BTreeMap::new();
        for (sheet, row) in touched {
            let Some(info) = self.book.sheets.get(sheet as usize) else {
                continue;
            };
            let line = info.rows.info(row).copied().unwrap_or_default();
            if line.custom || line.hidden {
                continue;
            }
            let (tallest, measure) = self.row_needs(sheet, row);
            if !measure.is_empty() {
                self.pending_fits.insert((sheet, row));
            }
            let info = &self.book.sheets[sheet as usize];
            let px = tallest.max(info.rows.default_px);
            let size = (px != info.rows.default_px).then_some(px);
            if size != line.size {
                by_sheet
                    .entry(sheet)
                    .or_default()
                    .push((row, LineInfo { size, ..line }));
            }
        }
        let mut list = Vec::new();
        for (sheet, rows) in by_sheet {
            list.push(self.apply_lines(sheet, rows, Vec::new()));
        }
        match list.len() {
            0 => None,
            _ => Some(Op::Many(list)),
        }
    }

    /// What must be measured to fit rows that wrap or turn their text, as JSON. See
    /// `fitRowsRequest` in `packages/engine-client`.
    pub(crate) fn fit_rows_json(
        &mut self,
        rows: &[(u32, u32)],
        options: &cgs_numfmt::Options<'_>,
    ) -> String {
        let mut out = String::from("[");
        let mut first = true;
        for &(sheet, row) in rows {
            let Some(info) = self.book.sheets.get(sheet as usize) else {
                continue;
            };
            let line = info.rows.info(row).copied().unwrap_or_default();
            if line.custom || line.hidden {
                continue;
            }
            let (tallest, measure) = self.row_needs(sheet, row);
            let info = &self.book.sheets[sheet as usize];
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str(&format!(
                "{{\"sheet\":{sheet},\"row\":{row},\"base\":{},\"cells\":[",
                tallest.max(info.rows.default_px)
            ));
            for (index, &col) in measure.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                let info = &self.book.sheets[sheet as usize];
                let width = info.cols.px(col);
                let xf = *self.book.styles.xf(info.style_at(row, col));
                let font = self.book.styles.font(&xf).clone();
                let text = {
                    let input = crate::tile::TileInput {
                        book: &self.book,
                        calc: &self.calc,
                        options,
                        today: self.today,
                        invalid: None,
                        errors: 0,
                        erp: None,
                    };
                    crate::tile::display_text(&input, &mut self.cache, sheet, row, col)
                };
                push_font_cell(&mut out, &text, &font, width, xf.alignment);
            }
            out.push_str("]}");
        }
        out.push(']');
        out
    }

    /// What must be measured to fit columns, as JSON. For each font, only the longest texts of a
    /// column go, since measuring a million cells would say the same.
    pub(crate) fn fit_cols_json(
        &mut self,
        sheet: u32,
        first_col: u32,
        last_col: u32,
        keep: usize,
        options: &cgs_numfmt::Options<'_>,
    ) -> String {
        // The longest texts by font in each column, by characters, which the UI measures.
        let keep_per_font = if keep == 0 { 256 } else { keep };
        let mut out = String::from("[");
        let mut comma = false;
        let Some(info) = self.book.sheets.get(sheet as usize) else {
            return "[]".to_string();
        };
        let used = info.used;
        for col in first_col..=last_col.min(used.1.max(first_col)) {
            let mut filled = crate::edit::CollectCells::default();
            if used.0 >= 1 {
                self.calc.read_block(
                    SheetRect {
                        sheet,
                        rect: CalcRect {
                            top: 1,
                            left: col,
                            bottom: used.0,
                            right: col,
                        },
                    },
                    &mut filled,
                );
            }
            // By font, the rows holding the longest text.
            let mut best: HashMap<u32, Vec<(usize, u32)>> = HashMap::new();
            for (row, _) in filled.0 {
                let info = &self.book.sheets[sheet as usize];
                if info.merge_at(row, col).is_some() {
                    continue;
                }
                let style = info.style_at(row, col);
                let font = self.book.styles.xf(style).font;
                let text = {
                    let input = crate::tile::TileInput {
                        book: &self.book,
                        calc: &self.calc,
                        options,
                        today: self.today,
                        invalid: None,
                        errors: 0,
                        erp: None,
                    };
                    crate::tile::display_text(&input, &mut self.cache, sheet, row, col)
                };
                let list = best.entry(font).or_default();
                list.push((text.chars().count(), row));
                if list.len() >= keep_per_font * 2 {
                    list.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.0));
                    list.truncate(keep_per_font);
                }
            }
            for list in best.values_mut() {
                list.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.0));
                list.truncate(keep_per_font);
                for &(_, row) in list.iter() {
                    let info = &self.book.sheets[sheet as usize];
                    let xf = *self.book.styles.xf(info.style_at(row, col));
                    let font = self.book.styles.font(&xf).clone();
                    let text = {
                        let input = crate::tile::TileInput {
                            book: &self.book,
                            calc: &self.calc,
                            options,
                            today: self.today,
                            invalid: None,
                            errors: 0,
                            erp: None,
                        };
                        crate::tile::display_text(&input, &mut self.cache, sheet, row, col)
                    };
                    if comma {
                        out.push(',');
                    }
                    comma = true;
                    out.push_str(&format!("{{\"col\":{col},\"row\":{row},"));
                    let start = out.len();
                    push_font_cell(&mut out, &text, &font, 0, xf.alignment);
                    // The cell's own braces become part of this object.
                    out.remove(start);
                }
            }
        }
        out.push(']');
        out
    }

    /// Merges an area, as Excel does: the first thing in it stays, in its top-left cell. Gives what
    /// puts it back.
    pub(crate) fn apply_merge(
        &mut self,
        sheet: u32,
        area: Area,
        center: bool,
    ) -> Result<Op, String> {
        let touches_table = self.calc.tables().iter().any(|table| {
            table.sheet == sheet
                && table.range.top <= area.bottom
                && table.range.bottom >= area.top
                && table.range.left <= area.right
                && table.range.right >= area.left
        });
        if touches_table {
            return Err("Cells in a table can't be merged.".to_string());
        }
        let info = &self.book.sheets[sheet as usize];
        if info.merges.iter().any(|merge| {
            merge.top <= area.bottom
                && merge.bottom >= area.top
                && merge.left <= area.right
                && merge.right >= area.left
                && !(merge.top >= area.top
                    && merge.bottom <= area.bottom
                    && merge.left >= area.left
                    && merge.right <= area.right)
        }) {
            return Err("That would cut across a merged area.".to_string());
        }
        let filled = self.merge_discards(sheet, area);
        let mut undo = Vec::new();
        // What's kept: the top-left cell's, or the first thing there is.
        let corner = CellRef {
            sheet,
            row: area.top,
            col: area.left,
        };
        let mut edits = Vec::new();
        if let (CalcValue::Empty, Some(&(row, col))) = (self.value_of(corner), filled.first()) {
            let source = CellRef { sheet, row, col };
            let input = match self.calc.cell_input(source) {
                cgs_calc::CellInput::Formula { text, .. } => {
                    EditInput::Formula(cgs_refs::moved_formula(
                        &text,
                        i64::from(area.top) - i64::from(row),
                        i64::from(area.left) - i64::from(col),
                    ))
                }
                cgs_calc::CellInput::Value(value) => EditInput::Value(value),
                _ => EditInput::Clear,
            };
            edits.push(EditCell {
                sheet,
                row: area.top,
                col: area.left,
                input: Some(input),
                style: None,
            });
        }
        for &(row, col) in &filled {
            if (row, col) != (area.top, area.left) {
                edits.push(EditCell {
                    sheet,
                    row,
                    col,
                    input: Some(EditInput::Clear),
                    style: None,
                });
            }
        }
        if !edits.is_empty() {
            undo.push(self.apply_cells(edits)?);
        }
        // Each cell of it takes the top-left cell's look.
        let info = &self.book.sheets[sheet as usize];
        let mut base = info.style_at(area.top, area.left);
        if center {
            let mut editor = StyleEditor::new(FormatChange {
                horizontal: Some("center".to_string()),
                ..FormatChange::default()
            });
            base = self.changed_style(&mut editor, base, 0);
            self.refresh_styles();
        }
        let info = &self.book.sheets[sheet as usize];
        let mut before = StylesBefore {
            sheet,
            ..StylesBefore::default()
        };
        for col in area.left..=area.right {
            before.columns.insert(col, info.style_column(col));
        }
        before.areas.push(to_area_rect(area));
        let info = &mut self.book.sheets[sheet as usize];
        for col in area.left..=area.right {
            info.replace_styles(col, area.top, area.bottom, &[(area.top, area.bottom, base)]);
        }
        info.used.0 = info.used.0.max(area.bottom);
        info.used.1 = info.used.1.max(area.right);
        let rects = before.areas.clone();
        self.note_styled(sheet, &rects);
        undo.push(Op::Styles(Box::new(before)));
        let mut merges: Vec<Rect> = self.book.sheets[sheet as usize]
            .merges
            .iter()
            .filter(|merge| {
                !(merge.top >= area.top
                    && merge.bottom <= area.bottom
                    && merge.left >= area.left
                    && merge.right <= area.right)
            })
            .copied()
            .collect();
        merges.push(to_area_rect(area));
        undo.push(self.apply_op(Op::Merges { sheet, merges })?);
        undo.reverse();
        Ok(Op::Many(undo))
    }

    /// The cells of an area that hold anything, in reading order.
    pub(crate) fn merge_discards(&self, sheet: u32, area: Area) -> Vec<(u32, u32)> {
        let Some(info) = self.book.sheets.get(sheet as usize) else {
            return Vec::new();
        };
        let bottom = area.bottom.min(info.used.0.max(1));
        let right = area.right.min(info.used.1.max(1));
        if area.top > bottom || area.left > right {
            return Vec::new();
        }
        let mut found = crate::edit::CollectCells::default();
        self.calc.read_block(
            SheetRect {
                sheet,
                rect: CalcRect {
                    top: area.top,
                    left: area.left,
                    bottom,
                    right,
                },
            },
            &mut found,
        );
        found.0.sort_unstable();
        found.0
    }

    /// Freezes panes, or unfreezes them with no rows or columns.
    pub(crate) fn freeze_op(&mut self, sheet: u32, freeze: Option<Freeze>) -> Op {
        let mut view = self.book.sheets[sheet as usize].view;
        view.freeze = freeze.filter(|freeze| freeze.rows > 0 || freeze.cols > 0);
        Op::View {
            sheet,
            view,
            freeze: true,
        }
    }

    /// Puts a sheet's view in place, and gives what puts it back.
    pub(crate) fn apply_view(&mut self, sheet: u32, view: SheetView, freeze: bool) -> Op {
        let info = &mut self.book.sheets[sheet as usize];
        // Zoom isn't part of what undo puts back.
        let mut view = view;
        view.zoom = info.view.zoom;
        let before = std::mem::replace(&mut info.view, view);
        if freeze {
            self.edits.view_freeze.insert(sheet);
        }
        self.edits.view_flags.insert(sheet);
        self.dirty.layouts.insert(sheet);
        Op::View {
            sheet,
            view: before,
            freeze,
        }
    }
}

fn push_font_cell(
    out: &mut String,
    text: &str,
    font: &cgs_model::Font,
    width: u32,
    alignment: cgs_model::Alignment,
) {
    out.push_str("{\"text\":");
    json::push_string(out, text);
    out.push_str(",\"font\":");
    json::push_string(out, &font.name);
    out.push_str(&format!(
        ",\"size\":{},\"bold\":{},\"italic\":{},\"width\":{width},\"rotation\":{},\"indent\":{},\"wrap\":{}}}",
        font.size, font.bold, font.italic, alignment.rotation, alignment.indent, alignment.wrap
    ));
}

/// Sorts spans and joins those that touch.
pub(crate) fn join_spans(list: &mut Vec<(u32, u32)>) {
    list.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(list.len());
    for &(first, last) in list.iter() {
        match out.last_mut() {
            Some((_, end)) if first <= end.saturating_add(1) => *end = (*end).max(last),
            _ => out.push((first, last)),
        }
    }
    *list = out;
}
