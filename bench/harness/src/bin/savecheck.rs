//! Saves workbooks through the shipped engine and proves what came out. Each file runs in a child
//! process, so a hang or a crash costs one file.
//!
//! For each file it saves with no edits and then again after test edits through the Rust API. It
//! compares every entry the save didn't report changing, by SHA-256 of its raw span in the file and
//! through the zip crate, which shares no code with our writer. It reads the saved file back and
//! compares every cell with what the edited engine holds.
//!
//! Usage: savecheck <out.jsonl> <folder>... [--jobs=N] [--timeout=seconds] [--private] [--keep=dir]
//!        savecheck --one <file> [--keep=<path>] [--examples]
//!        savecheck --edit <source|blank> <saved> [--set=sheet,row,col,kind:value]...
//!                  [--command=<json>]... [--refresh-pivots]
//! With `--private`, results hold no file names or cell values. With `--keep`, saved files stay
//! in that folder, numbered, for the validator and Excel.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cgs_calc::{CalcValue, CellSink, Rect, SheetRect};
use cgs_engine::{EditCell, EditInput, Engine, LoadPhase, NewStyle};
use cgs_model::SheetKind;
use cgs_xlsx::patch_sheet::NewElement;
use cgs_xlsx::zipdir::{normalize, physical_order, Directory};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

#[derive(Default, Serialize)]
struct Report {
    ok: bool,
    /// The file couldn't be opened, which is the reader's business and not a failed save.
    refused: Option<String>,
    /// Reading the file for its first edit fails in the engine, so it has no edit to save.
    no_edit_possible: Option<String>,
    /// A save refused to patch a part whose XML is damaged, which keeps the file as it is.
    refused_damage: Option<String>,
    error: Option<String>,
    /// The no-edit save gave back the source, byte for byte.
    noedit_identical: bool,
    /// The no-edit save's entries all match, though the file differs around them.
    noedit_entries_match: bool,
    edited: bool,
    entries_checked: u64,
    /// Clean entries whose raw bytes or contents differ. Anything but 0 fails the phase.
    entries_different: u64,
    changed_parts: u64,
    removed_parts: u64,
    added_parts: u64,
    cells_written: u64,
    compared: u64,
    matched: u64,
    results_left_to_excel: u64,
    categories: BTreeMap<String, u64>,
    examples: Vec<String>,
    save_ms: f64,
    source_bytes: u64,
    saved_bytes: u64,
    saved: Option<String>,
}

#[derive(Default)]
struct Block(HashMap<(u32, u32), CalcValue>);

impl CellSink for Block {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.insert((row, col), value.clone());
    }
}

fn opened(bytes: Vec<u8>) -> Result<Engine, String> {
    let mut engine = Engine::new();
    engine
        .open(Rc::new(bytes))
        .map_err(|error| error.to_string())?;
    loop {
        let status = engine
            .load_step(1000.0)
            .map_err(|error| error.to_string())?;
        if status.phase != LoadPhase::Reading {
            return Ok(engine);
        }
    }
}

/// SHA-256 of every entry's raw span, which holds its header and compressed bytes.
fn spans(bytes: &Vec<u8>) -> Result<HashMap<String, [u8; 32]>, String> {
    let directory = Directory::read(bytes).map_err(|error| error.to_string())?;
    let mut out = HashMap::new();
    for (index, end) in physical_order(&directory) {
        let entry = &directory.entries[index];
        let span = bytes
            .get(entry.local as usize..end as usize)
            .ok_or("a span is off the file")?;
        out.insert(normalize(&entry.name), Sha256::digest(span).into());
    }
    Ok(out)
}

/// SHA-256 of every entry's contents, as the zip crate reads them.
fn contents(bytes: &[u8]) -> HashMap<String, [u8; 32]> {
    let mut out = HashMap::new();
    let Ok(mut archive) = ZipArchive::new(Cursor::new(bytes)) else {
        return out;
    };
    for index in 0..archive.len() {
        let Ok(mut file) = archive.by_index(index) else {
            continue;
        };
        let Ok(name) = file.name().map(|name| name.to_string()) else {
            continue;
        };
        let mut body = Vec::new();
        if file.read_to_end(&mut body).is_ok() {
            out.insert(normalize(&name), Sha256::digest(&body).into());
        }
    }
    out
}

/// Whether a part's XML holds this text, as an element a test adds mustn't be there already.
fn sheet_mentions(bytes: &[u8], part: Option<&str>, text: &[u8]) -> bool {
    let (Some(part), Ok(mut archive)) = (part, ZipArchive::new(Cursor::new(bytes))) else {
        return false;
    };
    let Ok(mut file) = archive.by_name(part) else {
        return false;
    };
    let mut body = Vec::new();
    file.read_to_end(&mut body).is_ok() && body.windows(text.len()).any(|window| window == text)
}

/// Picks test edits for a sheet: an overwritten number and text, if it has them, then new cells
/// past what it holds, one of them a formula and one styled.
fn test_edits(engine: &mut Engine, sheet: u32, used: (u32, u32), style: u32) -> Vec<EditCell> {
    let mut edits = Vec::new();
    let value = |row, col, value| EditCell {
        sheet,
        row,
        col,
        input: Some(EditInput::Value(value)),
        style: None,
    };
    let (mut found_number, mut found_text) = (false, false);
    'scan: for row in 1..=used.0.min(64) {
        for col in 1..=used.1.min(16) {
            let text = engine.edit_text(sheet, row, col);
            if text.is_empty() || text.starts_with('=') {
                continue;
            }
            if !found_number && text.parse::<f64>().is_ok() {
                edits.push(value(row, col, CalcValue::Number(1234.5)));
                found_number = true;
            } else if !found_text && text.parse::<f64>().is_err() {
                edits.push(value(
                    row,
                    col,
                    CalcValue::Text("saved <&> عربي".to_string()),
                ));
                found_text = true;
            }
            if found_number && found_text {
                break 'scan;
            }
        }
    }
    let row = (used.0 + 2).min(cgs_model::MAX_ROWS - 2);
    edits.push(value(row, 1, CalcValue::Number(0.1 + 0.2)));
    edits.push(value(row, 2, CalcValue::Text(" spaced\nline ".to_string())));
    edits.push(value(row, 3, CalcValue::Bool(true)));
    edits.push(EditCell {
        sheet,
        row,
        col: 4,
        input: Some(EditInput::Formula("1+1".to_string())),
        style: None,
    });
    edits.push(EditCell {
        sheet,
        row: row + 1,
        col: 1,
        input: Some(EditInput::Value(CalcValue::Number(7.0))),
        style: Some(style),
    });
    edits
}

fn compare_values(ours: Option<&CalcValue>, saved: Option<&CalcValue>) -> Option<&'static str> {
    let empty = |value: Option<&CalcValue>| match value {
        None | Some(CalcValue::Empty) => true,
        Some(CalcValue::Text(text)) => text.is_empty(),
        _ => false,
    };
    if empty(ours) && empty(saved) {
        return None;
    }
    match (ours, saved) {
        (Some(CalcValue::Number(a)), Some(CalcValue::Number(b))) => {
            (a.to_bits() != b.to_bits() && a != b).then_some("number")
        }
        (None | Some(CalcValue::Empty), Some(_)) => Some("appeared"),
        (Some(_), None | Some(CalcValue::Empty)) => Some("vanished"),
        (Some(a), Some(b)) => (a != b).then_some("value"),
        (None, None) => None,
    }
}

fn run_one(path: &Path, keep: Option<&Path>, examples: bool) -> Report {
    let mut report = Report::default();
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            report.error = Some(error.to_string());
            return report;
        }
    };
    report.source_bytes = bytes.len() as u64;
    let source_spans = spans(&bytes);
    let source_contents = contents(&bytes);

    let mut engine = match opened(bytes.clone()) {
        Ok(engine) => engine,
        Err(error) => {
            report.refused = Some(error);
            return report;
        }
    };
    // A zip our directory reader refuses, that the zip crate takes, is an honest failure.
    let source_spans = match source_spans {
        Ok(found) => found,
        Err(error) => {
            report.error = Some(error);
            return report;
        }
    };

    // First with nothing changed.
    let mut out = Vec::new();
    match engine.save(&mut out) {
        Ok(_) => {
            report.noedit_identical = out == bytes;
            report.noedit_entries_match = spans(&out).is_ok_and(|saved| saved == source_spans);
            if let Err(error) = engine.verify_saved(&out) {
                report.error = Some(format!("no edits: {error}"));
                return report;
            }
        }
        Err(error) => {
            report.error = Some(format!("no edits: {error}"));
            return report;
        }
    }

    // Then with test edits on the first worksheet, and a style.
    let Some(book) = engine.book() else {
        report.error = Some("no book".to_string());
        return report;
    };
    let target = book
        .sheets
        .iter()
        .position(|sheet| sheet.kind == SheetKind::Worksheet && sheet.part.is_some())
        .map(|index| {
            (
                index as u32,
                book.sheets[index].used,
                book.sheets[index].merges.is_empty(),
                book.sheets[index].part.clone(),
            )
        });
    let Some((sheet, used, no_merges, sheet_part)) = target else {
        report.ok = true;
        return report;
    };
    let style = match engine.add_style(
        0,
        &NewStyle {
            font: Some("<b/><sz val=\"11\"/><name val=\"Calibri\"/>".to_string()),
            fill: Some("FFEE00".to_string()),
            number_format: Some("0.000".to_string()),
            ..NewStyle::default()
        },
    ) {
        Ok(style) => style,
        Err(error) => {
            report.error = Some(format!("style: {error}"));
            return report;
        }
    };
    let edits = test_edits(&mut engine, sheet, used, style);
    if let Err(error) = engine.edit_cells(&edits) {
        report.no_edit_possible = Some(error);
        report.ok = true;
        return report;
    }
    if no_merges && !sheet_mentions(&bytes, sheet_part.as_deref(), b"mergeCells") {
        let row = (used.0 + 5).min(cgs_model::MAX_ROWS);
        let element = NewElement {
            name: "mergeCells".to_string(),
            xml: format!(r#"<mergeCells count="1"><mergeCell ref="A{row}:B{row}"/></mergeCells>"#),
            replace: true,
        };
        if let Err(error) = engine.add_sheet_element(sheet, element) {
            report.error = Some(format!("element: {error}"));
            return report;
        }
    }
    let started = Instant::now();
    let mut out = Vec::new();
    let saved = match engine.save(&mut out) {
        Ok(saved) => saved,
        Err(error @ cgs_xlsx::save::SaveError::Damaged(_)) => {
            report.refused_damage = Some(error.to_string());
            report.ok = true;
            return report;
        }
        Err(error) => {
            report.error = Some(format!("edits: {error}"));
            return report;
        }
    };
    report.save_ms = started.elapsed().as_secs_f64() * 1000.0;
    report.edited = true;
    report.saved_bytes = out.len() as u64;
    report.changed_parts = saved.changed_parts.len() as u64;
    report.removed_parts = saved.removed_parts.len() as u64;
    report.added_parts = saved.added_parts.len() as u64;
    report.cells_written = saved.cells_written;
    report.results_left_to_excel = saved.results_left_to_excel;
    if let Err(error) = engine.verify_saved(&out) {
        report.error = Some(format!("edits: {error}"));
        return report;
    }

    // Clean entries are the same, seen two independent ways.
    let touched: Vec<String> = saved
        .changed_parts
        .iter()
        .chain(saved.removed_parts.iter())
        .chain(saved.added_parts.iter())
        .map(|name| normalize(name))
        .collect();
    let saved_spans = match spans(&out) {
        Ok(found) => found,
        Err(error) => {
            report.error = Some(format!("saved directory: {error}"));
            return report;
        }
    };
    let saved_contents = contents(&out);
    for (name, hash) in &source_spans {
        if touched.contains(name) {
            continue;
        }
        report.entries_checked += 1;
        let contents_differ = source_contents.get(name) != saved_contents.get(name);
        if saved_spans.get(name) != Some(hash) || contents_differ {
            report.entries_different += 1;
            if examples {
                report.examples.push(format!("entry differs: {name}"));
            }
        }
    }
    for name in saved_spans.keys() {
        if !source_spans.contains_key(name) && !touched.contains(name) {
            report.entries_different += 1;
            if examples {
                report.examples.push(format!("entry appeared: {name}"));
            }
        }
    }

    // Everything the engine holds reads back the same.
    let again = match opened(out.clone()) {
        Ok(again) => again,
        Err(error) => {
            report.error = Some(format!("the saved file won't open: {error}"));
            return report;
        }
    };
    let count = engine.book().map_or(0, |book| book.sheets.len());
    for index in 0..count as u32 {
        let (Some(info_a), Some(info_b)) = (
            engine
                .book()
                .and_then(|book| book.sheets.get(index as usize)),
            again
                .book()
                .and_then(|book| book.sheets.get(index as usize)),
        ) else {
            *report
                .categories
                .entry("sheet lost".to_string())
                .or_default() += 1;
            continue;
        };
        if info_a.kind != SheetKind::Worksheet {
            continue;
        }
        let rows = info_a.used.0.max(info_b.used.0);
        let cols = info_a.used.1.max(info_b.used.1).min(4096);
        let sheet_name = info_a.name.clone();
        if u64::from(rows) * u64::from(cols) > 400_000_000 {
            continue;
        }
        let mut top = 1;
        while top <= rows {
            let bottom = (top + 255).min(rows);
            let area = SheetRect {
                sheet: index,
                rect: Rect {
                    top,
                    left: 1,
                    bottom,
                    right: cols.max(1),
                },
            };
            let (mut ours, mut theirs) = (Block::default(), Block::default());
            engine.read_cells(area, &mut ours);
            again.read_cells(area, &mut theirs);
            let mut keys: Vec<(u32, u32)> = ours.0.keys().chain(theirs.0.keys()).copied().collect();
            keys.sort_unstable();
            keys.dedup();
            for key in keys {
                report.compared += 1;
                match compare_values(ours.0.get(&key), theirs.0.get(&key)) {
                    None => report.matched += 1,
                    Some(category) => {
                        *report.categories.entry(category.to_string()).or_default() += 1;
                        if examples && report.examples.len() < 6 {
                            report.examples.push(format!(
                                "{sheet_name}!{}{}: engine {:?} saved {:?}",
                                cgs_refs::col_to_letters(i64::from(key.1)),
                                key.0,
                                ours.0.get(&key),
                                theirs.0.get(&key),
                            ));
                        }
                    }
                }
            }
            top = bottom + 1;
        }
    }

    if let Some(keep) = keep {
        if let Err(error) = fs::write(keep, &out) {
            report.error = Some(format!("keep: {error}"));
            return report;
        }
        report.saved = Some(keep.display().to_string());
    }
    report.ok = true;
    report
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // The format zoo has its own test in cgs-numfmt, and the speed books are for speed.
            let name = entry.file_name();
            if name != "format-zoo" && name != "speed" {
                walk(&path, found);
            }
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx") || ext.eq_ignore_ascii_case("xlsm"))
            && !entry.file_name().to_string_lossy().starts_with("~$")
        {
            found.push(path);
        }
    }
}

/// One `--set`: `n:12`, `t:text`, `f:formula`, `b:true`, `e:#N/A` or `clear`.
fn parse_edit(text: &str) -> Result<EditCell, String> {
    let mut pieces = text.splitn(4, ',');
    let mut number = |name: &str| {
        pieces
            .next()
            .and_then(|piece| piece.parse::<u32>().ok())
            .ok_or_else(|| format!("no {name} in {text}"))
    };
    let (sheet, row, col) = (number("sheet")?, number("row")?, number("col")?);
    let content = pieces
        .next()
        .ok_or_else(|| format!("no content in {text}"))?;
    let input = match content.split_once(':') {
        None if content == "clear" => EditInput::Clear,
        Some(("n", value)) => EditInput::Value(CalcValue::Number(
            value
                .parse()
                .map_err(|_| format!("not a number: {value}"))?,
        )),
        Some(("t", value)) => EditInput::Value(CalcValue::Text(
            value.replace("\\n", "\n").replace("\\u0001", "\u{1}"),
        )),
        Some(("b", value)) => EditInput::Value(CalcValue::Bool(value == "true")),
        Some(("e", value)) => EditInput::Value(CalcValue::Error(value.to_string())),
        Some(("f", value)) => EditInput::Formula(value.to_string()),
        _ => return Err(format!("can't read {content}")),
    };
    Ok(EditCell {
        sheet,
        row,
        col,
        input: Some(input),
        style: None,
    })
}

/// Makes one saved file with exactly these edits, for the Excel matrix.
fn run_edit(args: &[String]) -> Result<String, String> {
    let (source, saved) = (&args[1], &args[2]);
    let bytes = if source == "blank" {
        cgs_xlsx::blank::blank_workbook().map_err(|error| error.to_string())?
    } else {
        fs::read(source).map_err(|error| error.to_string())?
    };
    let mut engine = opened(bytes)?;
    let edits = args
        .iter()
        .filter_map(|arg| arg.strip_prefix("--set="))
        .map(parse_edit)
        .collect::<Result<Vec<_>, _>>()?;
    if !edits.is_empty() {
        engine.edit_cells(&edits)?;
    }
    for command in args.iter().filter_map(|arg| arg.strip_prefix("--command=")) {
        // One it refuses changes nothing, as it would for someone at the keyboard.
        if let Err(problem) = engine.apply_json(command) {
            eprintln!("refused: {problem}");
        }
    }
    // A long list goes in a file: an array of commands, each run the same way.
    if let Some(path) = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--command-file="))
    {
        let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
        let list: Vec<serde_json::Value> =
            serde_json::from_str(&text).map_err(|error| error.to_string())?;
        for command in list {
            if let Err(problem) = engine.apply_json(&command.to_string()) {
                return Err(format!("{command}: {problem}"));
            }
            while engine.calc_step(60_000, &[])? {}
        }
    }
    while engine.calc_step(60_000, &[])? {}
    if args.iter().any(|arg| arg == "--refresh-pivots") {
        engine.refresh_pivots_on_load()?;
    }
    let mut out = Vec::new();
    let report = engine.save(&mut out).map_err(|error| error.to_string())?;
    engine
        .verify_saved(&out)
        .map_err(|error| error.to_string())?;
    fs::write(saved, &out).map_err(|error| error.to_string())?;
    Ok(cgs_engine::report_json(&report))
}

/// How our reader understands a saved file's formats: the cells, rows and columns a list names.
fn run_formats(args: &[String]) -> Result<String, String> {
    let bytes = fs::read(&args[1]).map_err(|error| error.to_string())?;
    let list: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&args[2]).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let mut engine = opened(bytes)?;
    let number = |value: &serde_json::Value, key: &str| value[key].as_u64().unwrap_or(0) as u32;
    let empty = Vec::new();
    let mut cells = Vec::new();
    for cell in list["cells"].as_array().unwrap_or(&empty) {
        let (sheet, row, col) = (
            number(cell, "sheet"),
            number(cell, "row"),
            number(cell, "col"),
        );
        let format: serde_json::Value =
            serde_json::from_str(&engine.cell_format_json(sheet, row, col))
                .map_err(|error| error.to_string())?;
        cells.push(serde_json::json!({
            "format": format,
            "display": engine.display_text(sheet, row, col),
        }));
    }
    let book = engine.book().ok_or("nothing is open")?;
    let mut rows = Vec::new();
    for line in list["rows"].as_array().unwrap_or(&empty) {
        let info = &book.sheets[number(line, "sheet") as usize];
        let at = number(line, "line");
        rows.push(serde_json::json!({
            "px": info.rows.px(at),
            "hidden": info.rows.px(at) == 0,
            "custom": info.rows.info(at).is_some_and(|line| line.custom),
        }));
    }
    let mut cols = Vec::new();
    for line in list["cols"].as_array().unwrap_or(&empty) {
        let info = &book.sheets[number(line, "sheet") as usize];
        let at = number(line, "line");
        cols.push(serde_json::json!({
            "px": info.cols.px(at),
            "hidden": info.cols.px(at) == 0,
        }));
    }
    Ok(serde_json::json!({ "cells": cells, "rows": rows, "cols": cols }).to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--formats") {
        match run_formats(&args) {
            Ok(report) => println!("{report}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.first().map(String::as_str) == Some("--edit") {
        match run_edit(&args) {
            Ok(report) => println!("{report}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    let text_option = |name: &str| {
        args.iter()
            .find_map(|arg| arg.strip_prefix(&format!("--{name}=")))
            .map(str::to_string)
    };
    if args.first().map(String::as_str) == Some("--one") {
        let examples = args.iter().any(|arg| arg == "--examples");
        let keep = text_option("keep").map(PathBuf::from);
        let report = run_one(Path::new(&args[1]), keep.as_deref(), examples);
        println!("{}", serde_json::to_string(&report).unwrap_or_default());
        return;
    }
    let number = |name: &str| text_option(name).and_then(|value| value.parse::<u64>().ok());
    let jobs = number("jobs").unwrap_or(8) as usize;
    let timeout = Duration::from_secs(number("timeout").unwrap_or(300));
    let private = args.iter().any(|arg| arg == "--private");
    let keep = text_option("keep").map(PathBuf::from);
    let positional: Vec<&String> = args.iter().filter(|arg| !arg.starts_with("--")).collect();
    let (out, folders) = positional
        .split_first()
        .expect("usage: savecheck <out> <folder>...");
    let mut files = Vec::new();
    for folder in folders {
        walk(Path::new(folder), &mut files);
    }
    files.sort();
    if let Some(keep) = &keep {
        fs::create_dir_all(keep).expect("make the keep folder");
    }
    let me = std::env::current_exe().expect("own path");
    let queue = Arc::new(Mutex::new(
        files.iter().cloned().enumerate().rev().collect::<Vec<_>>(),
    ));
    let lines = Arc::new(Mutex::new(Vec::new()));
    let mut workers = Vec::new();
    for _ in 0..jobs {
        let queue = Arc::clone(&queue);
        let lines = Arc::clone(&lines);
        let me = me.clone();
        let keep = keep.clone();
        workers.push(std::thread::spawn(move || loop {
            let Some((index, path)) = queue.lock().unwrap().pop() else {
                return;
            };
            let mut command = Command::new(&me);
            command.arg("--one").arg(&path);
            if let Some(keep) = &keep {
                let extension = path.extension().map_or_else(String::new, |ext| {
                    ext.to_string_lossy().to_ascii_lowercase()
                });
                command.arg(format!(
                    "--keep={}",
                    keep.join(format!("{index}.{extension}")).display()
                ));
            }
            if !private {
                command.arg("--examples");
            }
            let child = command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            let line = match child {
                Err(error) => format!("{{\"ok\":false,\"error\":\"spawn: {error}\"}}"),
                Ok(mut child) => {
                    // Read while it runs, as a large report could otherwise fill the pipe.
                    let mut stdout = child.stdout.take();
                    let reader = std::thread::spawn(move || {
                        let mut text = String::new();
                        if let Some(stdout) = stdout.as_mut() {
                            let _ = stdout.read_to_string(&mut text);
                        }
                        text
                    });
                    let started = Instant::now();
                    let mut timed_out = false;
                    let status = loop {
                        match child.try_wait() {
                            Ok(Some(status)) => break Some(status),
                            Ok(None) if started.elapsed() > timeout => {
                                timed_out = true;
                                let _ = child.kill();
                                break child.wait().ok();
                            }
                            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                            Err(_) => break None,
                        }
                    };
                    let stdout = reader.join().unwrap_or_default();
                    let last = stdout.lines().last().unwrap_or_default().to_string();
                    if last.starts_with('{') {
                        last
                    } else if timed_out {
                        "{\"ok\":false,\"error\":\"timeout\"}".to_string()
                    } else {
                        let code = status.and_then(|status| status.code()).unwrap_or(-1);
                        format!("{{\"ok\":false,\"error\":\"crashed with exit code {code}\"}}")
                    }
                }
            };
            let name = if private {
                String::new()
            } else {
                path.display().to_string().replace('\\', "/")
            };
            lines.lock().unwrap().push((index, name, path, line));
        }));
    }
    for worker in workers {
        let _ = worker.join();
    }
    let mut lines = std::mem::take(&mut *lines.lock().unwrap());
    lines.sort_by_key(|line| line.0);
    let mut output = fs::File::create(out).expect("create output");
    for (index, name, path, line) in &lines {
        // The source's path is for the runner's own use, and only ever kept for public files.
        let source = if private {
            serde_json::json!(path.display().to_string())
        } else {
            serde_json::json!(name)
        };
        let _ = writeln!(
            output,
            "{{\"index\":{index},\"file\":{},\"source\":{source},\"report\":{line}}}",
            serde_json::json!(name)
        );
    }
    println!("{} files checked", lines.len());
}
