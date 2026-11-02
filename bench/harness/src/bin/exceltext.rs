//! Compares what the shipped engine shows with what Excel showed in the same cells, as read by
//! `bench/excel-text.ps1`. See `bench/run-excel-text.mjs`.
//!
//! Usage: exceltext <workbook> <excel.json> [--private]

use std::collections::BTreeMap;
use std::fs;
use std::rc::Rc;

use cgs_engine::{Engine, LoadPhase};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Seen {
    sheet: u32,
    row: u32,
    col: u32,
    text: String,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    compared: u64,
    matched: u64,
    /// Excel showed `###` because of its column's width, which says nothing about the value.
    skipped_width: u64,
    kinds: BTreeMap<String, u64>,
    examples: Vec<String>,
}

/// Why our text differs from Excel's, or nothing if it doesn't. A column's width also shortens
/// General numbers, and that belongs to the tile worker, so a number Excel has shortened only
/// has to agree with ours to the digits it shows.
fn compare(ours: &str, theirs: &str) -> Option<&'static str> {
    if ours == theirs {
        return None;
    }
    let squeeze = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    if squeeze(ours) == squeeze(theirs) {
        return None;
    }
    // A `*x` fill repeats to the column's width, which only the tile worker can measure.
    if without_runs(ours) == without_runs(theirs) {
        return None;
    }
    if let (Ok(a), Ok(b)) = (ours.parse::<f64>(), theirs.parse::<f64>()) {
        if (a - b).abs() <= 0.5 * 10f64.powi(-(decimals(theirs) as i32)) {
            return None;
        }
        return Some("number");
    }
    if ours.is_empty() {
        return Some("missing in ours");
    }
    if ours.chars().any(|ch| ch.is_ascii_digit()) {
        Some("formatted number or date")
    } else {
        Some("text")
    }
}

/// Text without any run of four or more of one character that isn't a letter or a digit, and
/// without the spaces beside where one was.
fn without_runs(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    while at < chars.len() {
        let mut end = at;
        while end < chars.len() && chars[end] == chars[at] {
            end += 1;
        }
        if end - at < 4 || chars[at].is_alphanumeric() {
            out.extend(&chars[at..end]);
        }
        at = end;
    }
    out.split_whitespace().collect()
}

fn decimals(text: &str) -> usize {
    text.split_once('.').map_or(0, |(_, rest)| rest.len())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let private = args.iter().any(|arg| arg == "--private");
    let bytes = fs::read(&args[0]).expect("read the workbook");
    let seen = fs::read(&args[1]).expect("read Excel's text");
    let seen: Vec<Seen> = serde_json::from_slice(&seen).expect("parse Excel's text");

    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).expect("open the workbook");
    while engine.load_step(1000.0).expect("read the workbook").phase == LoadPhase::Reading {}

    let mut report = Report::default();
    for cell in &seen {
        if !cell.text.is_empty() && cell.text.chars().all(|ch| ch == '#') {
            report.skipped_width += 1;
            continue;
        }
        let ours = engine.display_text(cell.sheet, cell.row, cell.col);
        report.compared += 1;
        match compare(&ours, &cell.text) {
            None => report.matched += 1,
            Some(kind) => {
                *report.kinds.entry(kind.to_string()).or_default() += 1;
                if !private && report.examples.len() < 12 {
                    report.examples.push(format!(
                        "sheet {} {}{}: ours {ours:?}, Excel {:?}",
                        cell.sheet,
                        cgs_refs::col_to_letters(i64::from(cell.col)),
                        cell.row,
                        cell.text
                    ));
                }
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string(&report).expect("write the report")
    );
}
