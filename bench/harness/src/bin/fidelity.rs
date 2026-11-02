//! Opens workbooks through the shipped reader and engine, and compares every cell with
//! `calamine`, an independent reader. Nothing calculates. Each file runs in a child process,
//! so a hang or a crash costs one file.
//!
//! Usage: fidelity <out.jsonl> <folder>... [--jobs=N] [--timeout=seconds] [--private]
//!        fidelity --one <file> [--examples]
//! With `--private`, results hold no file names or cell values.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use calamine::{open_workbook_auto, Data, Reader};
use cgs_calc::formualizer::FormualizerEngine;
use cgs_calc::{CalcEngine, CalcValue, CellSink, Rect, SheetRect};
use cgs_model::SheetKind;
use cgs_xlsx::{book_info, Loader};
use serde::Serialize;

#[derive(Default, Serialize)]
struct Report {
    ok: bool,
    error: Option<String>,
    /// Calamine couldn't read the file, so there's nothing to compare it with.
    calamine_error: Option<String>,
    sheets: usize,
    compared: u64,
    matched: u64,
    categories: BTreeMap<String, u64>,
    examples: Vec<String>,
    load_ms: f64,
}

#[derive(Default)]
struct Block(HashMap<(u32, u32), CalcValue>);

impl CellSink for Block {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.insert((row, col), value.clone());
    }
}

fn close(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
}

/// Why two values differ, or nothing when they agree.
fn compare(ours: Option<&CalcValue>, theirs: &Data) -> Option<&'static str> {
    let ours = match ours {
        None | Some(CalcValue::Empty) => None,
        Some(value) => Some(value),
    };
    match (ours, theirs) {
        (None, Data::Empty) => None,
        (None, Data::String(text)) if text.is_empty() => None,
        (Some(CalcValue::Text(text)), Data::Empty) if text.is_empty() => None,
        (Some(CalcValue::Number(a)), Data::Float(b)) => (!close(*a, *b)).then_some("number"),
        (Some(CalcValue::Number(a)), Data::Int(b)) => (!close(*a, *b as f64)).then_some("number"),
        (Some(CalcValue::Number(a)), Data::DateTime(b)) => {
            (!close(*a, b.as_f64())).then_some("date")
        }
        (Some(CalcValue::Number(_)), Data::DateTimeIso(_) | Data::DurationIso(_)) => None,
        // Calamine leaves `_x0001_` escapes in place in a formula's saved text, and Excel
        // doesn't show them.
        (Some(CalcValue::Text(a)), Data::String(b)) => {
            (a != b && *a != cgs_xlsx::xml::unescape_x(b)).then_some("text")
        }
        (Some(CalcValue::Bool(a)), Data::Bool(b)) => (a != b).then_some("bool"),
        (Some(CalcValue::Error(a)), Data::Error(b)) => {
            (normalize_error(a) != normalize_error(&b.to_string())).then_some("error")
        }
        (None, _) => Some("missing in ours"),
        (Some(_), Data::Empty) => Some("missing in calamine"),
        _ => Some("different type"),
    }
}

fn normalize_error(text: &str) -> String {
    text.trim().to_ascii_uppercase()
}

fn show(value: &Option<&CalcValue>) -> String {
    match value {
        None => "(empty)".to_string(),
        Some(value) => format!("{value:?}"),
    }
}

fn run_one(path: &Path, examples: bool) -> Report {
    let mut report = Report::default();
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            report.error = Some(error.to_string());
            return report;
        }
    };
    let started = Instant::now();
    let (mut loader, mut book) = match Loader::open(Rc::new(bytes)) {
        Ok(opened) => opened,
        Err(error) => {
            report.error = Some(error.to_string());
            return report;
        }
    };
    let mut engine = FormualizerEngine::new();
    if let Err(error) = engine.begin_load(&book_info(&book)) {
        report.error = Some(format!("names or tables: {}", error.0));
        return report;
    }
    loop {
        match loader.step(&mut book, &mut engine, &mut || true) {
            Ok(true) => break,
            Ok(false) => {}
            Err(error) => {
                report.error = Some(error.to_string());
                return report;
            }
        }
    }
    if let Err(error) = engine.finish_load() {
        report.error = Some(error.0);
        return report;
    }
    report.load_ms = started.elapsed().as_secs_f64() * 1000.0;
    report.sheets = book.sheets.len();
    // If calamine runs out of memory now, the parent still knows that ours opened the file.
    println!("opened");
    let _ = std::io::stdout().flush();

    let mut theirs = match open_workbook_auto(path) {
        Ok(workbook) => workbook,
        Err(error) => {
            report.ok = true;
            report.calamine_error = Some(error.to_string());
            return report;
        }
    };
    for (index, sheet) in book.sheets.iter().enumerate() {
        if sheet.kind != SheetKind::Worksheet {
            continue;
        }
        // Calamine builds a full matrix of its 32-byte cells, which won't fit for a cell
        // at a far corner of a sheet. Those files are checked by the reader's own tests.
        if u64::from(sheet.used.0) * u64::from(sheet.used.1) > 400_000_000 {
            *report
                .categories
                .entry("sheet too sparse for calamine".to_string())
                .or_default() += 1;
            continue;
        }
        let range = match theirs.worksheet_range(&sheet.name) {
            Ok(range) => range,
            Err(error) => {
                *report
                    .categories
                    .entry(format!("calamine sheet error: {error}"))
                    .or_default() += 1;
                continue;
            }
        };
        let Some((start_row, start_col)) = range.start() else {
            continue;
        };
        // Calamine counts from 0 and from its first used cell.
        let (height, width) = range.get_size();
        let width_limit = width.min(4096);
        let mut row = 0usize;
        while row < height {
            let rows_here = 256.min(height - row);
            let top = start_row + row as u32 + 1;
            let left = start_col + 1;
            let mut block = Block::default();
            engine.read_block(
                SheetRect {
                    sheet: index as u32,
                    rect: Rect {
                        top,
                        left,
                        bottom: top + rows_here as u32 - 1,
                        right: left + width_limit as u32 - 1,
                    },
                },
                &mut block,
            );
            for offset_row in 0..rows_here {
                for offset_col in 0..width_limit {
                    let value = range
                        .get((row + offset_row, offset_col))
                        .unwrap_or(&Data::Empty);
                    let at = (top + offset_row as u32, left + offset_col as u32);
                    let ours = block.0.get(&at);
                    if matches!(value, Data::Empty) && ours.is_none() {
                        continue;
                    }
                    report.compared += 1;
                    match compare(ours, value) {
                        None => report.matched += 1,
                        Some(category) => {
                            *report.categories.entry(category.to_string()).or_default() += 1;
                            if examples && report.examples.len() < 6 {
                                report.examples.push(format!(
                                    "{}!{}{}: ours {} calamine {value:?}",
                                    sheet.name,
                                    cgs_refs::col_to_letters(i64::from(at.1)),
                                    at.0,
                                    show(&ours),
                                ));
                            }
                        }
                    }
                }
            }
            row += rows_here;
        }
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
            walk(&path, found);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx") || ext.eq_ignore_ascii_case("xlsm"))
            && !entry.file_name().to_string_lossy().starts_with("~$")
        {
            found.push(path);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--one") {
        let examples = args.iter().any(|arg| arg == "--examples");
        let report = run_one(Path::new(&args[1]), examples);
        println!("{}", serde_json::to_string(&report).unwrap_or_default());
        return;
    }
    let option = |name: &str| {
        args.iter()
            .find_map(|arg| arg.strip_prefix(&format!("--{name}=")))
            .and_then(|value| value.parse::<u64>().ok())
    };
    let jobs = option("jobs").unwrap_or(8) as usize;
    let timeout = Duration::from_secs(option("timeout").unwrap_or(120));
    let private = args.iter().any(|arg| arg == "--private");
    let positional: Vec<&String> = args.iter().filter(|arg| !arg.starts_with("--")).collect();
    let (out, folders) = positional
        .split_first()
        .expect("usage: fidelity <out> <folder>...");
    let mut files = Vec::new();
    for folder in folders {
        walk(Path::new(folder), &mut files);
    }
    files.sort();
    let me = std::env::current_exe().expect("own path");
    let queue = Arc::new(Mutex::new(
        files.clone().into_iter().rev().collect::<Vec<_>>(),
    ));
    let lines = Arc::new(Mutex::new(Vec::new()));
    let mut workers = Vec::new();
    for _ in 0..jobs {
        let queue = Arc::clone(&queue);
        let lines = Arc::clone(&lines);
        let me = me.clone();
        workers.push(std::thread::spawn(move || loop {
            let Some(path) = queue.lock().unwrap().pop() else {
                return;
            };
            let mut command = Command::new(&me);
            command.arg("--one").arg(&path);
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
                    let started = Instant::now();
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) => break,
                            Ok(None) if started.elapsed() > timeout => {
                                let _ = child.kill();
                                break;
                            }
                            _ => std::thread::sleep(Duration::from_millis(5)),
                        }
                    }
                    let timed_out = started.elapsed() > timeout;
                    let output = child.wait_with_output().ok();
                    let stdout = output
                        .as_ref()
                        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                        .unwrap_or_default();
                    let opened = stdout.starts_with("opened");
                    let last = stdout.lines().last().unwrap_or_default().to_string();
                    if last.starts_with('{') {
                        last
                    } else if opened {
                        "{\"ok\":true,\"calamine_error\":\"calamine crashed\"}".to_string()
                    } else if timed_out {
                        "{\"ok\":false,\"error\":\"timeout\"}".to_string()
                    } else {
                        let code = output.and_then(|output| output.status.code()).unwrap_or(-1);
                        format!("{{\"ok\":false,\"error\":\"crashed with exit code {code}\"}}")
                    }
                }
            };
            let name = if private {
                String::new()
            } else {
                path.display().to_string().replace('\\', "/")
            };
            lines.lock().unwrap().push((name, line));
        }));
    }
    for worker in workers {
        let _ = worker.join();
    }
    let lines = lines.lock().unwrap();
    let mut output = fs::File::create(out).expect("create output");
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    let (mut opened, mut failed, mut compared, mut matched, mut clean) = (0, 0, 0u64, 0u64, 0);
    let mut errors: BTreeMap<String, u64> = BTreeMap::new();
    for (name, line) in lines.iter() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
        if value["ok"].as_bool() == Some(true) {
            opened += 1;
            let here_compared = value["compared"].as_u64().unwrap_or(0);
            let here_matched = value["matched"].as_u64().unwrap_or(0);
            compared += here_compared;
            matched += here_matched;
            if here_compared == here_matched {
                clean += 1;
            }
            if let Some(map) = value["categories"].as_object() {
                for (key, count) in map {
                    *totals.entry(key.clone()).or_default() += count.as_u64().unwrap_or(0);
                }
            }
        } else {
            failed += 1;
            let error: String = value["error"]
                .as_str()
                .unwrap_or("?")
                .chars()
                .take(48)
                .collect();
            *errors.entry(error).or_default() += 1;
        }
        let _ = writeln!(
            output,
            "{{\"file\":{},\"report\":{line}}}",
            serde_json::json!(name)
        );
    }
    println!(
        "{} files: {opened} opened, {failed} refused. {compared} cells compared, {matched} match ({:.4}%). {clean} files match in full.",
        lines.len(),
        if compared == 0 { 100.0 } else { matched as f64 * 100.0 / compared as f64 },
    );
    println!("differences: {totals:?}");
    println!("refused: {errors:?}");
}
