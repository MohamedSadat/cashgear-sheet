//! Cell styles: Excel's built-in ones, as Excel 2016 has them under the Office theme, and the
//! book's own. Applying one gives the cells the parts of it that the style includes and a link
//! to it, as Excel does, adding a built-in style to the file the first time it's used.

use serde::Deserialize;
use serde_json::{json, Value};

use cgs_model::{Alignment, StyleIncludes};
use cgs_xlsx::patch_parts::XfChanges;

use crate::command::{Applied, Area, Op};
use crate::format::{BorderChange, ColorSpec, FillSpec, FormatChange, LineSpec, StyleEditor};
use crate::Session;

const DATA: &str = include_str!("../../cgs-model/data/cell-styles.json");

#[derive(Deserialize)]
struct File {
    styles: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    name: String,
    #[serde(default)]
    builtin_id: Option<u32>,
    include: Include,
    num_fmt_id: u32,
    font: Font,
    fill: Fill,
    #[serde(default)]
    border: Border,
}

#[derive(Deserialize)]
struct Include {
    number: bool,
    font: bool,
    border: bool,
    fill: bool,
    #[serde(default)]
    alignment: bool,
    #[serde(default)]
    protection: bool,
}

#[derive(Deserialize)]
struct Font {
    name: String,
    size: f32,
    #[serde(default)]
    color: Option<Color>,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
}

#[derive(Deserialize)]
struct Color {
    #[serde(default)]
    theme: Option<u8>,
    #[serde(default)]
    tint: Option<f64>,
    #[serde(default)]
    rgb: Option<String>,
}

impl Color {
    fn spec(&self) -> ColorSpec {
        match (self.theme, &self.rgb) {
            (Some(theme), _) => ColorSpec::Theme {
                theme,
                tint: self.tint.unwrap_or(0.0),
            },
            (None, Some(rgb)) => ColorSpec::Rgb {
                rgb: rgb.trim_start_matches('#').to_string(),
            },
            (None, None) => ColorSpec::Auto,
        }
    }

    fn rgb(&self) -> Option<&str> {
        self.rgb.as_deref()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fill {
    pattern: String,
    #[serde(default)]
    fg_color: Option<Color>,
}

#[derive(Deserialize, Default)]
struct Border {
    #[serde(default)]
    left: Option<Line>,
    #[serde(default)]
    right: Option<Line>,
    #[serde(default)]
    top: Option<Line>,
    #[serde(default)]
    bottom: Option<Line>,
}

#[derive(Deserialize)]
struct Line {
    style: String,
    #[serde(default)]
    color: Option<Color>,
}

fn line(line: &Option<Line>) -> Option<LineSpec> {
    Some(match line {
        Some(line) => LineSpec {
            style: line.style.clone(),
            color: line.color.as_ref().map(Color::spec),
        },
        None => LineSpec {
            style: "none".to_string(),
            color: None,
        },
    })
}

fn styles() -> Vec<Entry> {
    serde_json::from_str::<File>(DATA)
        .map(|file| file.styles)
        .unwrap_or_default()
}

/// The gallery's groups, in Excel 2016's order.
const GROUPS: &[(&str, &[&str])] = &[
    (
        "Good, Bad and Neutral",
        &["Normal", "Bad", "Good", "Neutral"],
    ),
    (
        "Data and Model",
        &[
            "Calculation",
            "Check Cell",
            "Explanatory Text",
            "Input",
            "Linked Cell",
            "Note",
            "Output",
            "Warning Text",
        ],
    ),
    (
        "Titles and Headings",
        &[
            "Heading 1",
            "Heading 2",
            "Heading 3",
            "Heading 4",
            "Title",
            "Total",
        ],
    ),
    (
        "Themed Cell Styles",
        &[
            "20% - Accent1",
            "20% - Accent2",
            "20% - Accent3",
            "20% - Accent4",
            "20% - Accent5",
            "20% - Accent6",
            "40% - Accent1",
            "40% - Accent2",
            "40% - Accent3",
            "40% - Accent4",
            "40% - Accent5",
            "40% - Accent6",
            "60% - Accent1",
            "60% - Accent2",
            "60% - Accent3",
            "60% - Accent4",
            "60% - Accent5",
            "60% - Accent6",
            "Accent1",
            "Accent2",
            "Accent3",
            "Accent4",
            "Accent5",
            "Accent6",
        ],
    ),
    (
        "Number Format",
        &["Comma", "Comma [0]", "Currency", "Currency [0]", "Percent"],
    ),
];

/// The change a style makes to a cell: the parts it includes, and nothing else.
fn change_of(entry: &Entry) -> FormatChange {
    if entry.name == "Normal" {
        return FormatChange {
            clear: true,
            ..FormatChange::default()
        };
    }
    let mut change = FormatChange::default();
    if entry.include.number {
        change.number_format_id = Some(entry.num_fmt_id);
    }
    if entry.include.font {
        change.font_name = Some(entry.font.name.clone());
        change.font_size = Some(entry.font.size);
        change.bold = Some(entry.font.bold);
        change.italic = Some(entry.font.italic);
        change.strike = Some(false);
        change.underline = Some("none".to_string());
        change.font_color = Some(
            entry
                .font
                .color
                .as_ref()
                .map_or(ColorSpec::Auto, Color::spec),
        );
    }
    if entry.include.fill {
        change.fill = Some(FillSpec {
            pattern: if entry.fill.pattern == "solid" {
                "solid".to_string()
            } else {
                "none".to_string()
            },
            color: entry.fill.fg_color.as_ref().map(Color::spec),
            pattern_color: None,
        });
    }
    if entry.include.border {
        change.border = Some(BorderChange {
            top: line(&entry.border.top),
            bottom: line(&entry.border.bottom),
            left: line(&entry.border.left),
            right: line(&entry.border.right),
            each: true,
            ..BorderChange::default()
        });
    }
    change
}

/// What a style includes, as New Cell Style and Modify send it: Style Includes in Excel's dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct IncludesSpec {
    pub number: bool,
    pub alignment: bool,
    pub font: bool,
    pub border: bool,
    pub fill: bool,
    pub protection: bool,
}

impl From<IncludesSpec> for StyleIncludes {
    fn from(spec: IncludesSpec) -> Self {
        Self {
            number: spec.number,
            alignment: spec.alignment,
            font: spec.font,
            border: spec.border,
            fill: spec.fill,
            protection: spec.protection,
        }
    }
}

fn includes_of(include: &Include) -> StyleIncludes {
    StyleIncludes {
        number: include.number,
        alignment: include.alignment,
        font: include.font,
        border: include.border,
        fill: include.fill,
        protection: include.protection,
    }
}

/// `<alignment>`'s attributes in a format's XML, starting with a space, if it has one.
fn alignment_attributes(xml: &str) -> Option<String> {
    let at = xml.find("alignment ").or_else(|| xml.find("alignment/"))? + "alignment".len();
    let tail = &xml[at..];
    let end = tail.find('>')?;
    Some(tail[..end].trim_end_matches('/').trim_end().to_string())
}

const NO_STYLES: &str = "The workbook's styles can't be read.";

impl Session {
    pub(crate) fn apply_cell_style(
        &mut self,
        sheet: u32,
        areas: Vec<Area>,
        name: &str,
    ) -> Result<Applied, String> {
        self.sheet_for_edit(sheet)?;
        let change = if name.eq_ignore_ascii_case("Normal") {
            FormatChange {
                clear: true,
                ..FormatChange::default()
            }
        } else {
            FormatChange {
                named_style: Some(self.named_style(name)?),
                ..FormatChange::default()
            }
        };
        let area = areas.first().copied();
        let op = Op::Format {
            sheet,
            areas,
            change: Box::new(change),
            active: None,
        };
        self.perform("cell style", op, area.map(|area| (sheet, area)))
    }

    /// A named style's place in `cellStyleXfs`. One of Excel's built-in styles that the book
    /// doesn't have yet is added, as Excel adds it when it's first used.
    fn named_style(&mut self, name: &str) -> Result<u32, String> {
        let known = self
            .book
            .styles
            .named
            .iter()
            .find(|style| style.name.eq_ignore_ascii_case(name))
            .map(|style| style.xf);
        if let Some(index) = known {
            if self
                .styles_xml()
                .entry_xml("cellStyleXfs", "xf", index)
                .is_some()
            {
                return Ok(index);
            }
        }
        let entry = styles()
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("There is no cell style {name}."))?;
        self.styles_xml().ensure_normal_style();
        if entry.name == "Normal" {
            self.refresh_styles();
            return Ok(0);
        }
        // The style's look on a cell format from Normal's, and then that as the style.
        let editor = StyleEditor::new(change_of(&entry));
        let cell = self.make_style(&editor, 0, 0);
        let styles = self.styles_xml();
        let xml = styles
            .style_xf_from(cell, includes_of(&entry.include))
            .ok_or_else(|| NO_STYLES.to_string())?;
        let index = styles.add_style_xf(&xml);
        styles.add_cell_style(&entry.name, index, entry.builtin_id);
        self.refresh_styles();
        Ok(index)
    }

    /// What a cell format starting from `base` takes from a named style: the parts it includes,
    /// and the link.
    pub(crate) fn style_changes(&mut self, index: u32, base: u32) -> XfChanges {
        let link = XfChanges {
            style: Some(index),
            ..XfChanges::default()
        };
        let Some(style) = self.book.styles.style_xfs.get(index as usize).copied() else {
            return link;
        };
        let includes = style.includes;
        let own = style.xf;
        let base_alignment = self.book.styles.xf(base).alignment;
        let alignment = if includes.alignment {
            self.styles_xml()
                .entry_xml("cellStyleXfs", "xf", index)
                .and_then(|xml| alignment_attributes(&xml))
                .or_else(|| (base_alignment != Alignment::default()).then(String::new))
        } else {
            None
        };
        XfChanges {
            font: includes.font.then_some(own.font),
            fill: includes.fill.then_some(own.fill),
            border: includes.border.then_some(own.border),
            num_fmt: includes.number.then_some(own.num_fmt),
            alignment,
            protection: includes
                .protection
                .then_some((own.locked, own.formula_hidden)),
            ..link
        }
    }

    /// New Cell Style: a style with the look of the cell at `row`, `col`, by example as Excel
    /// makes one. The cell itself stays as it is.
    pub(crate) fn new_cell_style(
        &mut self,
        name: &str,
        sheet: u32,
        row: u32,
        col: u32,
        includes: StyleIncludes,
    ) -> Result<Applied, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A style needs a name.".to_string());
        }
        if name.chars().count() > 255 {
            return Err("A style's name can't be longer than 255 characters.".to_string());
        }
        let taken = self
            .book
            .styles
            .named
            .iter()
            .any(|style| style.name.eq_ignore_ascii_case(name))
            || styles()
                .iter()
                .any(|entry| entry.name.eq_ignore_ascii_case(name));
        if taken {
            return Err(format!("There is already a cell style named {name}."));
        }
        let info = self.sheet_for_edit(sheet)?;
        let base = match info.merge_at(row, col) {
            Some(merge) => info.style_at(merge.top, merge.left),
            None => info.style_at(row, col),
        };
        let styles = self.styles_xml();
        styles.ensure_normal_style();
        let xml = styles
            .style_xf_from(base, includes)
            .ok_or_else(|| NO_STYLES.to_string())?;
        let index = styles.add_style_xf(&xml);
        styles.add_cell_style(name, index, None);
        self.refresh_styles();
        self.history.touch();
        self.history.forget();
        Ok(Applied::default())
    }

    /// Modify Cell Style: the style takes this change, and the cells linked to it follow in each
    /// part they still had from it, as in Excel. The undo list goes, as in Excel.
    pub(crate) fn modify_cell_style(
        &mut self,
        name: &str,
        change: FormatChange,
        includes: Option<StyleIncludes>,
    ) -> Result<Applied, String> {
        let index = self.named_style(name)?;
        let old = self
            .book
            .styles
            .style_xfs
            .get(index as usize)
            .copied()
            .unwrap_or_default();
        let includes = includes.unwrap_or(old.includes);
        let probe = self
            .styles_xml()
            .cell_xf_from_style(index)
            .ok_or_else(|| NO_STYLES.to_string())?;
        self.refresh_styles();
        let editor = StyleEditor::new(FormatChange {
            named_style: None,
            ..change
        });
        let cell = self.make_style(&editor, probe, 0);
        let styles = self.styles_xml();
        let xml = styles
            .style_xf_from(cell, includes)
            .ok_or_else(|| NO_STYLES.to_string())?;
        styles.replace_entry("cellStyleXfs", "xf", index, &xml);
        self.refresh_styles();
        let new = self.book.styles.style_xfs[index as usize];
        let alignment = self
            .styles_xml()
            .entry_xml("cellStyleXfs", "xf", index)
            .and_then(|xml| alignment_attributes(&xml))
            .unwrap_or_default();
        let linked: Vec<(u32, cgs_model::Xf)> = self
            .book
            .styles
            .xfs
            .iter()
            .enumerate()
            .filter(|(_, xf)| xf.style == index)
            .map(|(at, xf)| (at as u32, *xf))
            .collect();
        let (before, after) = (old.xf, new.xf);
        for (at, xf) in linked {
            let follow = |on: bool, old: u32, new: u32, has: u32| {
                (on && old != new && has == old).then_some(new)
            };
            let mut changes = XfChanges {
                font: follow(includes.font, before.font, after.font, xf.font),
                fill: follow(includes.fill, before.fill, after.fill, xf.fill),
                border: follow(includes.border, before.border, after.border, xf.border),
                num_fmt: follow(includes.number, before.num_fmt, after.num_fmt, xf.num_fmt),
                style: Some(index),
                ..XfChanges::default()
            };
            if includes.alignment
                && before.alignment != after.alignment
                && xf.alignment == before.alignment
            {
                changes.alignment = Some(alignment.clone());
            }
            let protection = |xf: &cgs_model::Xf| (xf.locked, xf.formula_hidden);
            if includes.protection
                && protection(&before) != protection(&after)
                && protection(&xf) == protection(&before)
            {
                changes.protection = Some(protection(&after));
            }
            let unchanged = XfChanges {
                style: Some(index),
                ..XfChanges::default()
            };
            if changes != unchanged {
                self.styles_xml().edit_xf(at, &changes);
            }
        }
        self.refresh_styles();
        for sheet in 0..self.book.sheets.len() as u32 {
            self.dirty.sheets.insert(sheet);
        }
        self.history.touch();
        self.history.forget();
        Ok(Applied::default())
    }

    /// A named style's format, as Format Cells reads a cell's, and what it includes. Null for
    /// a built-in style the book doesn't have yet.
    pub(crate) fn style_format_json(&self, name: &str) -> Value {
        let table = &self.book.styles;
        let Some(style) = table
            .named
            .iter()
            .find(|style| style.name.eq_ignore_ascii_case(name))
            .and_then(|named| table.style_xfs.get(named.xf as usize))
        else {
            return Value::Null;
        };
        let format = crate::query::format_json(&self.book, &style.xf, false);
        let includes = style.includes;
        json!({
            "format": serde_json::from_str::<Value>(&format).unwrap_or(Value::Null),
            "includes": {
                "number": includes.number,
                "alignment": includes.alignment,
                "font": includes.font,
                "border": includes.border,
                "fill": includes.fill,
                "protection": includes.protection,
            },
        })
    }

    /// The gallery: the book's own styles first, then each group of Excel's, with what a
    /// preview draws.
    pub(crate) fn cell_styles_json(&self) -> Value {
        let table = &self.book.styles;
        let hex = |color: cgs_model::Color| {
            table
                .colors
                .resolve(color, None)
                .map(|rgb| format!("#{rgb:06X}"))
        };
        let custom: Vec<Value> = table
            .named
            .iter()
            .filter(|named| named.builtin.is_none())
            .filter_map(|named| {
                let xf = table.style_xfs.get(named.xf as usize)?.xf;
                let font = table.font(&xf);
                let fill = table.fill(&xf);
                let border = table.border(&xf);
                let bottom = (border.bottom.style != cgs_model::LineStyle::None)
                    .then(|| crate::format::line_style_name(border.bottom.style));
                Some(json!({
                    "name": named.name,
                    "font": font.name,
                    "size": font.size,
                    "bold": font.bold,
                    "italic": font.italic,
                    "color": hex(font.color),
                    "fill": (fill.pattern == cgs_model::Pattern::Solid).then(|| hex(fill.fg)).flatten(),
                    "boxed": border.left.style != cgs_model::LineStyle::None,
                    "bottom": bottom,
                    "bottomColor": bottom.and_then(|_| hex(border.bottom.color)),
                }))
            })
            .collect();
        let mut groups = Vec::new();
        if !custom.is_empty() {
            groups.push(json!({ "group": "Custom", "styles": custom }));
        }
        if let Value::Array(builtin) = builtin_styles_json() {
            groups.extend(builtin);
        }
        Value::Array(groups)
    }
}

/// Excel's own styles, as the gallery shows them.
fn builtin_styles_json() -> Value {
    let all = styles();
    let groups: Vec<Value> = GROUPS
        .iter()
        .map(|(group, names)| {
            let styles: Vec<Value> = names
                .iter()
                .filter_map(|name| all.iter().find(|entry| entry.name == *name))
                .map(|entry| {
                    let fill = (entry.fill.pattern == "solid")
                        .then(|| entry.fill.fg_color.as_ref().and_then(Color::rgb))
                        .flatten();
                    let bottom =
                        entry.border.bottom.as_ref().map(|line| {
                            (line.style.clone(), line.color.as_ref().and_then(Color::rgb))
                        });
                    json!({
                        "name": entry.name,
                        "font": entry.font.name,
                        "size": entry.font.size,
                        "bold": entry.font.bold,
                        "italic": entry.font.italic,
                        "color": entry.font.color.as_ref().and_then(Color::rgb),
                        "fill": fill,
                        "boxed": entry.border.left.is_some(),
                        "bottom": bottom.as_ref().map(|(style, _)| style),
                        "bottomColor": bottom.and_then(|(_, color)| color.map(str::to_string)),
                    })
                })
                .collect();
            json!({ "group": group, "styles": styles })
        })
        .collect();
    Value::Array(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_reads_and_every_gallery_name_is_one() {
        let all = styles();
        assert_eq!(all.len(), 47);
        for (_, names) in GROUPS {
            for name in *names {
                assert!(all.iter().any(|entry| entry.name == *name), "{name}");
            }
        }
        let good = all.iter().find(|entry| entry.name == "Good").unwrap();
        let change = change_of(good);
        assert_eq!(
            change.font_color,
            Some(ColorSpec::Rgb {
                rgb: "006100".to_string()
            })
        );
        assert!(change.border.is_none() && change.number_format_id.is_none());
    }
}
