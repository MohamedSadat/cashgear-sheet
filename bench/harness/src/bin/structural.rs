//! Compares an engine with Excel after each random structural edit. `bench/run-structural.mjs`
//! runs Excel and calls this. Run: `structural <engine> <seed.xlsx> <ops.json> <excel.jsonl>`.

use std::collections::HashMap;

use cgs_bench::book::{read_book, Value};
use cgs_bench::run::{compare, load_for_structural, new_engine_by_name, settle_engine, Verdict};
use cgs_calc::{CalcEngine, CalcValue, CellRef, CellSink, Rect, SheetRect, StructuralOp};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Op {
    kind: String,
    sheet: u32,
    at: u32,
    count: u32,
}

#[derive(Deserialize)]
struct Episode {
    episode: u32,
    ops: Vec<Op>,
}

#[derive(Deserialize)]
struct ExcelValue {
    n: Option<f64>,
    b: Option<bool>,
    e: Option<String>,
    s: Option<String>,
}

#[derive(Deserialize)]
struct ExcelCell {
    r: u32,
    c: u32,
    v: ExcelValue,
    f: Option<String>,
}

#[derive(Deserialize)]
struct ExcelDump {
    episode: u32,
    step: usize,
    sheets: Vec<Vec<ExcelCell>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Divergence {
    episode: u32,
    step: usize,
    op: String,
    sheet: u32,
    cell: String,
    what: String,
    excel: String,
    engine: String,
    formula: String,
    differing_cells: usize,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct Report {
    ops_scheduled: usize,
    ops_passed: usize,
    ops_diverged: usize,
    ops_skipped: usize,
    formula_only_divergences: usize,
    errors: usize,
    cells_compared: u64,
    divergences: Vec<Divergence>,
}

#[derive(Default)]
struct Grid(HashMap<(u32, u32), CalcValue>);

impl CellSink for Grid {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.insert((row, col), value.clone());
    }
}

fn to_value(value: &ExcelValue) -> Value {
    if let Some(number) = value.n {
        Value::Number(number)
    } else if let Some(flag) = value.b {
        Value::Bool(flag)
    } else if let Some(text) = &value.e {
        Value::Error(text.clone())
    } else {
        Value::Text(value.s.clone().unwrap_or_default())
    }
}

/// Formula text compares without spaces or case, and without file-format prefixes. Two
/// spellings mean the same and don't count: Excel's `Sheet!#REF!` for a `#REF!`, and an
/// engine's `@` where the file has implicit intersection, which `.Formula` leaves out.
fn normalize(formula: &str) -> String {
    let mut out = String::with_capacity(formula.len());
    let mut quoted = false;
    for ch in formula.trim_start_matches('=').chars() {
        if ch == '"' {
            quoted = !quoted;
        }
        if quoted {
            out.push(ch);
        } else if !ch.is_whitespace() {
            out.push(ch.to_ascii_uppercase());
        }
    }
    let out = out
        .replace("_XLFN.", "")
        .replace("_XLWS.", "")
        .replace('@', "");
    strip_sheet_before_ref(&out)
}

fn strip_sheet_before_ref(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("!#REF!") {
        let before = &rest[..at];
        // The sheet name is a word, or quoted.
        let start = match before.strip_suffix('\'') {
            Some(quoted) => quoted.rfind('\'').unwrap_or(0),
            None => before
                .rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
                .map_or(0, |index| index + 1),
        };
        out.push_str(&before[..start]);
        out.push_str("#REF!");
        rest = &rest[at + 6..];
    }
    out.push_str(rest);
    out
}

fn describe(value: Option<&CalcValue>) -> String {
    match value {
        None | Some(CalcValue::Empty) => "(empty)".to_string(),
        Some(CalcValue::Number(number)) => number.to_string(),
        Some(CalcValue::Bool(flag)) => flag.to_string(),
        Some(CalcValue::Text(text)) => format!("{text:?}"),
        Some(CalcValue::Error(error)) => error.clone(),
    }
}

fn to_op(op: &Op) -> StructuralOp {
    let (sheet, at, count) = (op.sheet, op.at, op.count);
    match op.kind.as_str() {
        "InsertRows" => StructuralOp::InsertRows { sheet, at, count },
        "DeleteRows" => StructuralOp::DeleteRows { sheet, at, count },
        "InsertCols" => StructuralOp::InsertCols { sheet, at, count },
        _ => StructuralOp::DeleteCols { sheet, at, count },
    }
}

fn check_step(
    engine: &dyn CalcEngine,
    dump: &ExcelDump,
    episode: u32,
    op: &Op,
    report: &mut Report,
) -> bool {
    let mut first: Option<Divergence> = None;
    let mut value_diffs = 0usize;
    let mut text_diffs = 0usize;
    for (sheet, cells) in dump.sheets.iter().enumerate() {
        let sheet = sheet as u32;
        let bottom = cells.iter().map(|cell| cell.r).max().unwrap_or(1) + 8;
        let right = cells.iter().map(|cell| cell.c).max().unwrap_or(1) + 8;
        let mut got = Grid::default();
        engine.read_block(
            SheetRect {
                sheet,
                rect: Rect {
                    top: 1,
                    left: 1,
                    bottom,
                    right,
                },
            },
            &mut got,
        );
        let mut note = |cell_row: u32,
                        cell_col: u32,
                        what: &str,
                        excel: String,
                        mine: String,
                        formula: String| {
            if first.is_none() {
                first = Some(Divergence {
                    episode,
                    step: dump.step,
                    op: format!(
                        "{} sheet {} at {} count {}",
                        op.kind, op.sheet, op.at, op.count
                    ),
                    sheet,
                    cell: format!(
                        "{}{cell_row}",
                        cgs_bench::formula::col_to_letters(i64::from(cell_col))
                    ),
                    what: what.to_string(),
                    excel,
                    engine: mine,
                    formula,
                    differing_cells: 0,
                });
            }
        };
        for cell in cells {
            report.cells_compared += 1;
            let expected = to_value(&cell.v);
            let mine = got.0.remove(&(cell.r, cell.c));
            let verdict = compare(&expected, mine.as_ref());
            if verdict != Verdict::Match {
                value_diffs += 1;
                note(
                    cell.r,
                    cell.c,
                    "value",
                    format!("{expected:?}"),
                    describe(mine.as_ref()),
                    cell.f.clone().unwrap_or_default(),
                );
            }
            if let Some(formula) = &cell.f {
                let text = engine.formula_text(CellRef {
                    sheet,
                    row: cell.r,
                    col: cell.c,
                });
                let same = text
                    .as_deref()
                    .is_some_and(|text| normalize(text) == normalize(formula));
                if !same {
                    text_diffs += 1;
                    note(
                        cell.r,
                        cell.c,
                        "formula",
                        formula.clone(),
                        text.unwrap_or_else(|| "(no formula)".to_string()),
                        formula.clone(),
                    );
                }
            }
        }
        // What Excel doesn't have must be empty in the engine too.
        for ((row, col), value) in &got.0 {
            if *value != CalcValue::Empty && *value != CalcValue::Text(String::new()) {
                value_diffs += 1;
                note(
                    *row,
                    *col,
                    "extra",
                    "(empty)".to_string(),
                    describe(Some(value)),
                    String::new(),
                );
            }
        }
    }
    if let Some(mut divergence) = first {
        divergence.differing_cells = value_diffs + text_diffs;
        if value_diffs == 0 {
            report.formula_only_divergences += 1;
        }
        if report.divergences.len() < 200 {
            report.divergences.push(divergence);
        }
        return false;
    }
    true
}

fn run(
    engine_name: &str,
    seed: &[u8],
    episodes: &[Episode],
    dumps: &str,
) -> Result<Report, String> {
    let book = read_book(seed)?;
    let mut by_step: HashMap<(u32, usize), ExcelDump> = HashMap::new();
    for line in dumps.lines() {
        let dump: ExcelDump = serde_json::from_str(line).map_err(|e| e.to_string())?;
        by_step.insert((dump.episode, dump.step), dump);
    }
    let mut report = Report::default();
    for episode in episodes {
        report.ops_scheduled += episode.ops.len();
        let mut engine = new_engine_by_name(engine_name).ok_or("unknown engine")?;
        load_for_structural(engine.as_mut(), &book).map_err(|e| e.0)?;
        for (step, op) in episode.ops.iter().enumerate() {
            let dump = by_step
                .get(&(episode.episode, step))
                .ok_or(format!("no Excel dump for {} {step}", episode.episode))?;
            let applied = engine
                .structural(&to_op(op))
                .and_then(|_| settle_engine(engine.as_mut()));
            if let Err(error) = applied {
                report.errors += 1;
                report.ops_diverged += 1;
                report.ops_skipped += episode.ops.len() - step - 1;
                if report.divergences.len() < 200 {
                    report.divergences.push(Divergence {
                        episode: episode.episode,
                        step,
                        op: format!(
                            "{} sheet {} at {} count {}",
                            op.kind, op.sheet, op.at, op.count
                        ),
                        sheet: op.sheet,
                        cell: String::new(),
                        what: "error".to_string(),
                        excel: String::new(),
                        engine: error.0,
                        formula: String::new(),
                        differing_cells: 0,
                    });
                }
                break;
            }
            if check_step(engine.as_ref(), dump, episode.episode, op, &mut report) {
                report.ops_passed += 1;
            } else {
                report.ops_diverged += 1;
                report.ops_skipped += episode.ops.len() - step - 1;
                break;
            }
        }
    }
    Ok(report)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let engine = args[1].clone();
    let seed = std::fs::read(&args[2]).expect("seed");
    let episodes: Vec<Episode> =
        serde_json::from_str(&std::fs::read_to_string(&args[3]).expect("ops")).expect("ops json");
    let dumps = std::fs::read_to_string(&args[4]).expect("excel");
    let worker = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || run(&engine, &seed, &episodes, &dumps))
        .expect("spawn");
    match worker.join().expect("worker") {
        Ok(report) => println!("{}", serde_json::to_string(&report).expect("json")),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
