//! Excel's built-in cell styles, as Excel 2016 has them under the Office theme. Applying one gives
//! the cells the parts of it that the style includes, as Excel does. The cells keep the Normal
//! style's link, so Excel's gallery shows them as Normal: the look is the same.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::command::{Applied, Area, Op};
use crate::format::{BorderChange, ColorSpec, FillSpec, FormatChange, LineSpec};
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

impl Session {
    pub(crate) fn apply_cell_style(
        &mut self,
        sheet: u32,
        areas: Vec<Area>,
        name: &str,
    ) -> Result<Applied, String> {
        self.sheet_for_edit(sheet)?;
        let entry = styles()
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("There is no cell style {name}."))?;
        let area = areas.first().copied();
        let op = Op::Format {
            sheet,
            areas,
            change: Box::new(change_of(&entry)),
            active: None,
        };
        self.perform("cell style", op, area.map(|area| (sheet, area)))
    }
}

/// The gallery: each group's styles, with what a preview draws.
pub(crate) fn cell_styles_json() -> Value {
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
