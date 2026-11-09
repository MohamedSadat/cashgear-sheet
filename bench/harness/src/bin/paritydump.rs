//! What Phase 13's Excel check compares, as the app reads a workbook: each worksheet's state,
//! zoom and protection, the workbook's protection, and asked-for cells' protection, in the shape
//! `bench/excel-parity.ps1` gives Excel's reading.
//!
//! Usage: paritydump <workbook> [--cell=Sheet!A1]...

use std::fs;
use std::rc::Rc;

use cgs_engine::{Engine, LoadPhase};
use cgs_model::{SheetKind, SheetState};
use serde_json::{json, Value};

/// `A1` as 1-based (row, column).
fn cell_at(text: &str) -> Option<(u32, u32)> {
    let split = text.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = text.split_at(split);
    let col = letters.bytes().try_fold(0u32, |col, byte| {
        byte.is_ascii_alphabetic()
            .then(|| col * 26 + u32::from(byte.to_ascii_uppercase() - b'A' + 1))
    })?;
    Some((digits.parse().ok()?, col))
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .ok_or("usage: paritydump <workbook> [--cell=Sheet!A1]...")?;
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let mut engine = Engine::new();
    engine
        .open(Rc::new(bytes))
        .map_err(|error| error.to_string())?;
    while engine
        .load_step(1000.0)
        .map_err(|error| error.to_string())?
        .phase
        == LoadPhase::Reading
    {}
    let book = engine.book().ok_or("no workbook")?.clone();
    let order = book.tab_order();
    let active = book.active_sheet;
    let sheets: Vec<Value> = order
        .iter()
        .map(|&index| &book.sheets[index as usize])
        .zip(order.iter())
        .filter(|(info, _)| info.kind == SheetKind::Worksheet)
        .map(|(info, &index)| {
            let protection = info.protection.as_ref();
            let on = |pick: fn(&cgs_model::SheetProtection) -> bool| protection.is_some_and(pick);
            // Excel's `Protection.Allow…` say true for anything on an unprotected sheet.
            let allow = |pick: fn(&cgs_model::SheetProtection) -> bool| {
                !on(|p| p.sheet) || protection.is_some_and(pick)
            };
            json!({
                "name": info.name,
                // As Excel's `Visible` says it: -1, 0 or 2.
                "visible": match info.state {
                    SheetState::Visible => -1,
                    SheetState::Hidden => 0,
                    SheetState::VeryHidden => 2,
                },
                "active": index == active,
                "protected": on(|p| p.sheet),
                "objects": on(|p| p.objects),
                "scenarios": on(|p| p.scenarios),
                // `EnableSelection`: 0 anything, 1 unlocked cells, -4142 nothing.
                "selection": match protection.filter(|p| p.sheet) {
                    Some(p) if !p.select_unlocked => -4142,
                    Some(p) if !p.select_locked => 1,
                    _ => 0,
                },
                "allow": {
                    "formatCells": allow(|p| p.format_cells),
                    "formatColumns": allow(|p| p.format_columns),
                    "formatRows": allow(|p| p.format_rows),
                    "insertColumns": allow(|p| p.insert_columns),
                    "insertRows": allow(|p| p.insert_rows),
                    "insertHyperlinks": allow(|p| p.insert_hyperlinks),
                    "deleteColumns": allow(|p| p.delete_columns),
                    "deleteRows": allow(|p| p.delete_rows),
                    "sort": allow(|p| p.sort),
                    "autoFilter": allow(|p| p.auto_filter),
                    "pivotTables": allow(|p| p.pivot_tables),
                },
                "zoom": (info.state == SheetState::Visible).then_some(info.view.zoom),
            })
        })
        .collect();
    let mut cells = Vec::new();
    for ask in args.iter().filter_map(|arg| arg.strip_prefix("--cell=")) {
        let (sheet, cell) = ask.rsplit_once('!').ok_or("a cell is Sheet!A1")?;
        let info = book
            .sheets
            .iter()
            .find(|info| info.name == sheet && !info.deleted)
            .ok_or_else(|| format!("no sheet {sheet}"))?;
        let (row, col) = cell_at(cell).ok_or_else(|| format!("not a cell: {cell}"))?;
        let style = info.style_at(row, col);
        let xf = book
            .styles
            .xfs
            .get(style as usize)
            .copied()
            .unwrap_or_default();
        let note = info.notes.get(&(row, col)).map(
            |note| json!({ "author": note.author, "text": note.text, "visible": note.visible }),
        );
        let links = info
            .links
            .iter()
            .filter(|link| link.area.contains(row, col))
            .count();
        // As Excel's `Font.Underline` says it: 2 single, -4142 none.
        let underline = match book.styles.fonts.get(xf.font as usize).map(|f| f.underline) {
            Some(cgs_model::Underline::None) | None => -4142,
            Some(cgs_model::Underline::Single) => 2,
            Some(_) => -1,
        };
        cells.push(json!({
            "sheet": sheet,
            "cell": cell,
            "locked": xf.locked,
            "hidden": xf.formula_hidden,
            "note": note,
            "links": links,
            "underline": underline,
        }));
    }
    let out = json!({
        "sheets": sheets,
        "cells": cells,
        "book": { "structure": book.protection.as_ref().is_some_and(|p| p.structure) },
    });
    println!("{out}");
    Ok(())
}
