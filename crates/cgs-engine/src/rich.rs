//! Rich text made in the cell: runs of characters each with a font of their own, as Excel's cell
//! editor makes them, kept as a new shared string when saved.

use cgs_calc::CalcValue;
use cgs_model::{Color, ColorKind, RichRun, RunFont, Underline, VertAlign};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::command::{Applied, Area, Op};
use crate::edit::{EditCell, EditInput};
use crate::Session;

/// Rich strings made here are kept from this key up, apart from the file's own indexes.
pub const NEW_RICH: u32 = 0x8000_0000;

/// A run's font as the cell editor gives it. Anything left out is the cell's.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RunFontSpec {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    /// `single`, `double`, `singleAccounting` or `doubleAccounting`.
    pub underline: Option<String>,
    pub size: Option<f32>,
    pub name: Option<String>,
    /// `RRGGBB`.
    pub color: Option<String>,
    /// `superscript` or `subscript`.
    pub vert_align: Option<String>,
}

/// A run from a character, counted from 0.
#[derive(Debug, Clone, Deserialize)]
pub struct RunSpec {
    pub start: u32,
    #[serde(default)]
    pub font: RunFontSpec,
}

impl RunFontSpec {
    fn font(&self) -> RunFont {
        RunFont {
            name: self.name.clone().filter(|name| !name.is_empty()),
            size: self.size,
            bold: self.bold,
            italic: self.italic,
            strike: self.strike,
            underline: self.underline.as_deref().map(|name| match name {
                "double" => Underline::Double,
                "singleAccounting" => Underline::SingleAccounting,
                "doubleAccounting" => Underline::DoubleAccounting,
                "none" => Underline::None,
                _ => Underline::Single,
            }),
            color: self
                .color
                .as_deref()
                .and_then(|hex| u32::from_str_radix(hex.trim_start_matches('#'), 16).ok())
                .map(Color::rgb),
            vert_align: self
                .vert_align
                .as_deref()
                .map_or(VertAlign::Baseline, VertAlign::parse),
        }
    }
}

fn font_json(font: &RunFont) -> Value {
    json!({
        "bold": font.bold,
        "italic": font.italic,
        "strike": font.strike,
        "underline": font.underline.map(|u| match u {
            Underline::None => "none",
            Underline::Single => "single",
            Underline::Double => "double",
            Underline::SingleAccounting => "singleAccounting",
            Underline::DoubleAccounting => "doubleAccounting",
        }),
        "size": font.size,
        "name": font.name,
        "color": font.color.and_then(|c| match c.kind {
            ColorKind::Rgb(rgb) => Some(format!("{rgb:06X}")),
            _ => None,
        }),
        "vertAlign": font.vert_align.name(),
    })
}

impl Session {
    /// A cell's rich runs, from characters counted from 0, for the cell editor; nothing for a
    /// cell that isn't rich.
    pub(crate) fn rich_json(&self, sheet: u32, row: u32, col: u32) -> Value {
        let Some(info) = self.book.sheets.get(sheet as usize) else {
            return Value::Null;
        };
        let Some(runs) = info
            .rich
            .get(&(row, col))
            .and_then(|index| self.book.rich_strings.get(index))
        else {
            return Value::Null;
        };
        let text = match self.value_of(cgs_calc::CellRef { sheet, row, col }) {
            CalcValue::Text(text) => text,
            _ => return Value::Null,
        };
        Value::Array(
            runs.iter()
                .map(|run| {
                    let start = (run.start as usize).min(text.len());
                    let chars = text.get(..start).map_or(0, |head| head.chars().count());
                    json!({
                        "start": chars,
                        "font": run.font.as_ref().map(font_json),
                    })
                })
                .collect(),
        )
    }

    /// Text with runs typed into a cell: its value, and a rich string of its own.
    pub(crate) fn rich_input(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        text: String,
        runs: &[RunSpec],
    ) -> Result<Applied, String> {
        self.sheet_for_edit(sheet)?;
        // Characters to bytes, as runs keep them.
        let bytes: Vec<usize> = text
            .char_indices()
            .map(|(at, _)| at)
            .chain(std::iter::once(text.len()))
            .collect();
        let mut rich: Vec<RichRun> = runs
            .iter()
            .map(|run| RichRun {
                start: bytes[(run.start as usize).min(bytes.len() - 1)] as u32,
                font: (run.font != RunFontSpec::default()).then(|| run.font.font()),
            })
            .collect();
        rich.sort_by_key(|run| run.start);
        rich.dedup_by_key(|run| run.start);
        let cell = EditCell {
            sheet,
            row,
            col,
            input: Some(EditInput::Value(CalcValue::Text(text))),
            style: None,
        };
        if rich.iter().all(|run| run.font.is_none()) {
            return self.perform(
                "typing",
                Op::Cells(vec![cell]),
                Some((sheet, Area::cell(row, col))),
            );
        }
        let key = NEW_RICH
            + self
                .book
                .rich_strings
                .keys()
                .filter(|&&k| k >= NEW_RICH)
                .count() as u32;
        let key = (key..)
            .find(|k| !self.book.rich_strings.contains_key(k))
            .unwrap_or(key);
        self.book.rich_strings.insert(key, rich);
        self.perform(
            "typing",
            Op::Many(vec![
                Op::Cells(vec![cell]),
                Op::RichCopy {
                    sheet,
                    cells: vec![((row, col), key)],
                },
            ]),
            Some((sheet, Area::cell(row, col))),
        )
    }
}
