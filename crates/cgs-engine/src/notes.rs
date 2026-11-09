//! Notes and hyperlinks: adding, changing and taking them away, and what a cell has of either.

use cgs_calc::CalcEngine;
use cgs_model::{Hyperlink, Note, Rect};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::command::{Applied, Area, Op};
use crate::edit::{EditCell, EditInput};
use crate::format::{ColorSpec, FormatChange};
use crate::Session;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkSpec {
    /// A web address, file or email address.
    #[serde(default)]
    pub address: Option<String>,
    /// A place in this workbook, such as `Sheet2!A1`.
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub tooltip: Option<String>,
    /// What the cell shows, written in if the cell is empty.
    #[serde(default)]
    pub text: Option<String>,
}

impl Session {
    /// A note on a cell, or none to take it away.
    pub(crate) fn set_note_command(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        text: Option<String>,
        author: Option<String>,
    ) -> Result<Applied, String> {
        // A threaded comment's note is its stand-in for older Excel, tied to the thread.
        if !self.sheet_for_edit(sheet)?.threaded.is_empty() {
            return Err(
                "This sheet has threaded comments from a newer Excel, so its notes are kept as they are."
                    .to_string(),
            );
        }
        let mut notes = self.book.sheets[sheet as usize].notes.clone();
        match text {
            Some(text) => {
                // An edited note keeps its author, and whether and where its box shows.
                let old = notes.get(&(row, col)).cloned().unwrap_or_default();
                let author = author
                    .filter(|_| old.author.is_empty())
                    .or_else(|| (!old.author.is_empty()).then(|| old.author.clone()))
                    .unwrap_or_else(|| "Author".to_string());
                notes.insert(
                    (row, col),
                    Note {
                        author,
                        text,
                        ..old
                    },
                );
            }
            None => {
                if notes.remove(&(row, col)).is_none() {
                    return Ok(Applied::default());
                }
            }
        }
        self.perform(
            "note",
            Op::SheetNotes { sheet, notes },
            Some((sheet, Area::cell(row, col))),
        )
    }

    /// Show/Hide Note: a note's box showing all the time, or only on hover.
    pub(crate) fn note_visible_command(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        visible: bool,
    ) -> Result<Applied, String> {
        let mut notes = self.sheet_for_edit(sheet)?.notes.clone();
        let Some(note) = notes.get_mut(&(row, col)) else {
            return Ok(Applied::default());
        };
        if note.visible == visible {
            return Ok(Applied::default());
        }
        note.visible = visible;
        self.perform(
            "show or hide a note",
            Op::SheetNotes { sheet, notes },
            Some((sheet, Area::cell(row, col))),
        )
    }

    /// A link on cells, or none to take links off them. A new link's cell shows its text, in
    /// blue and underlined, as Excel's Hyperlink style draws it.
    pub(crate) fn set_link_command(
        &mut self,
        sheet: u32,
        area: Area,
        spec: Option<&LinkSpec>,
        plain: bool,
    ) -> Result<Applied, String> {
        self.sheet_for_edit(sheet)?;
        let rect = Rect {
            top: area.top,
            left: area.left,
            bottom: area.bottom,
            right: area.right,
        };
        let meets = |a: &Rect| {
            a.top <= rect.bottom
                && a.bottom >= rect.top
                && a.left <= rect.right
                && a.right >= rect.left
        };
        let mut links: Vec<Hyperlink> = self.book.sheets[sheet as usize]
            .links
            .iter()
            .filter(|link| !meets(&link.area))
            .cloned()
            .collect();
        let mut list = Vec::new();
        if let Some(spec) = spec {
            let address = spec
                .address
                .as_deref()
                .map(str::trim)
                .filter(|a| !a.is_empty());
            let location = spec
                .location
                .as_deref()
                .map(str::trim)
                .filter(|l| !l.is_empty());
            if address.is_none() && location.is_none() {
                return Err("Enter an address or a place in this document.".to_string());
            }
            // A bare email address is a mailto link, and a bare host a web one.
            let address = address.map(|a| {
                if a.contains('@') && !a.contains(':') {
                    format!("mailto:{a}")
                } else if !a.contains(':') && a.contains('.') && !a.contains('\\') {
                    format!("http://{a}")
                } else {
                    a.to_string()
                }
            });
            links.push(Hyperlink {
                area: rect,
                target: address.clone(),
                location: location.map(str::to_string),
                tooltip: spec.tooltip.clone().filter(|t| !t.is_empty()),
                display: None,
            });
            let shown = spec
                .text
                .clone()
                .filter(|t| !t.is_empty())
                .or_else(|| address.clone())
                .or_else(|| location.map(str::to_string));
            let empty = matches!(
                self.calc.cell_input(cgs_calc::CellRef {
                    sheet,
                    row: area.top,
                    col: area.left
                }),
                cgs_calc::CellInput::Clear
            );
            if let (Some(text), true) = (shown, empty || spec.text.is_some()) {
                list.push(Op::Cells(vec![EditCell {
                    sheet,
                    row: area.top,
                    col: area.left,
                    input: Some(EditInput::Value(cgs_calc::CalcValue::Text(text))),
                    style: None,
                }]));
            }
            list.push(Op::Format {
                sheet,
                areas: vec![area],
                change: Box::new(FormatChange {
                    font_color: Some(ColorSpec::Theme {
                        theme: 10,
                        tint: 0.0,
                    }),
                    underline: Some("single".to_string()),
                    ..FormatChange::default()
                }),
                active: None,
            });
        }
        if spec.is_none() && plain {
            // Remove Hyperlink takes the link's look off too, back to the Normal style's, as
            // Excel 2016 does; Clear Hyperlinks leaves it.
            list.push(Op::Format {
                sheet,
                areas: vec![area],
                change: Box::new(FormatChange {
                    font_color: Some(ColorSpec::Auto),
                    underline: Some("none".to_string()),
                    ..FormatChange::default()
                }),
                active: None,
            });
        }
        list.push(Op::SheetLinks { sheet, links });
        self.perform("hyperlink", Op::Many(list), Some((sheet, area)))
    }

    pub(crate) fn note_json(&self, sheet: u32, row: u32, col: u32) -> Value {
        let Some(info) = self.book.sheets.get(sheet as usize) else {
            return Value::Null;
        };
        match info.notes.get(&(row, col)) {
            Some(note) => json!({
                "author": note.author,
                "text": note.text,
                "visible": note.visible,
                "anchor": note.anchor.unwrap_or_else(|| Note::default_anchor(row, col)),
            }),
            None if info.threaded.contains(&(row, col)) => json!({ "threaded": true }),
            None => Value::Null,
        }
    }

    pub(crate) fn link_json(&self, sheet: u32, row: u32, col: u32) -> Value {
        let Some(link) = self
            .book
            .sheets
            .get(sheet as usize)
            .and_then(|info| info.links.iter().find(|link| link.area.contains(row, col)))
        else {
            return Value::Null;
        };
        json!({
            "address": link.target,
            "location": link.location,
            "tooltip": link.tooltip,
        })
    }

    /// Every note on a sheet, in order, for Next and Previous.
    pub(crate) fn notes_list(&self, sheet: u32) -> Value {
        let Some(info) = self.book.sheets.get(sheet as usize) else {
            return Value::Array(Vec::new());
        };
        Value::Array(
            info.notes
                .iter()
                .map(|(&(row, col), note)| {
                    json!({
                        "row": row,
                        "col": col,
                        "author": note.author,
                        "text": note.text,
                        "visible": note.visible,
                        "anchor": note.anchor.unwrap_or_else(|| Note::default_anchor(row, col)),
                    })
                })
                .collect(),
        )
    }
}
