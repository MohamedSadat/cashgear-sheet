//! Compares the shipped engine with Excel after each random edit, and an episode's saved file with
//! what Excel made of the same edits. `bench/run-edit-fuzz.mjs` runs Excel and calls this.
//!
//! Usage: editfuzz --check <seed.xlsx> <ops.json> <excel.jsonl> <saved folder>
//!        editfuzz --saved <excel.jsonl> <saved.jsonl>

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use cgs_bench::book::Value;
use cgs_bench::run::{compare, Verdict};
use cgs_calc::{CalcValue, CellSink, Rect, SheetRect};
use cgs_engine::{Command, Engine, LoadPhase};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

#[derive(Deserialize)]
struct Step {
    kind: String,
    #[serde(default)]
    op: Option<Json>,
}

#[derive(Deserialize)]
struct Episode {
    episode: u32,
    steps: Vec<Step>,
}

#[derive(Deserialize, Clone)]
struct ExcelValue {
    n: Option<f64>,
    b: Option<bool>,
    e: Option<String>,
    s: Option<String>,
}

#[derive(Deserialize, Clone)]
struct ExcelCell {
    r: u32,
    c: u32,
    v: ExcelValue,
    f: Option<String>,
}

#[derive(Deserialize, Clone)]
struct ExcelSheet {
    name: String,
    cells: Vec<ExcelCell>,
}

#[derive(Deserialize)]
struct ExcelDump {
    episode: u32,
    step: usize,
    refused: bool,
    #[serde(default)]
    circular: bool,
    #[serde(default)]
    problem: String,
    sheets: Vec<ExcelSheet>,
}

#[derive(Deserialize)]
struct SavedDump {
    file: String,
    opened: bool,
    renamed: bool,
    sheets: Vec<ExcelSheet>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Divergence {
    episode: u32,
    step: usize,
    op: String,
    sheet: String,
    cell: String,
    what: String,
    excel: String,
    engine: String,
    differing_cells: usize,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct Report {
    steps_scheduled: usize,
    steps_passed: usize,
    steps_diverged: usize,
    steps_skipped: usize,
    refused_by_both: usize,
    refused_by_excel_only: usize,
    refused_by_engine_only: usize,
    /// Excel took the edit and did nothing, where the engine refused it.
    ignored_by_excel: usize,
    /// Episodes that stopped at a circular reference, which Excel's result for depends on how many
    /// times it has calculated.
    circular: usize,
    formula_only: usize,
    cells_compared: u64,
    saves: usize,
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
        // Excel here has an Arabic UI, so a new table column's default name is in Arabic.
        Value::Text(
            value
                .s
                .clone()
                .unwrap_or_default()
                .replace("\u{639}\u{645}\u{648}\u{62f}", "Column"),
        )
    }
}

/// Formula text compares without spaces, case or file-format prefixes. Spellings that mean the same
/// don't count: Excel's `Sheet!#REF!` or `#REF!A1` for a `#REF!`, and a range of one cell.
fn normalize(formula: &str) -> String {
    let mut out = String::with_capacity(formula.len());
    let mut quoted = None;
    for ch in formula.trim_start_matches('=').chars() {
        match quoted {
            Some(open) => {
                if ch == open {
                    quoted = None;
                }
                out.push(ch);
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quoted = Some(ch);
                    out.push(ch);
                } else if !ch.is_whitespace() {
                    out.push(ch.to_ascii_uppercase());
                }
            }
        }
    }
    let out = out
        .replace("_XLFN.", "")
        .replace("_XLWS.", "")
        .replace("_XLPM.", "")
        .replace('@', "");
    let out = out.replace("#REF!#REF!", "#REF!");
    let out = strip_sheet_before_ref(&out);
    // A sheet name written with or without quotes means the same.
    let out = out.replace('\'', "");
    let out = strip_after_ref(&out);
    let out = out.replace("#REF!#REF!", "#REF!");
    let out = out.replace("\u{639}\u{645}\u{648}\u{62f}", "COLUMN");
    let out = out.replace("[[", "[").replace("]]", "]");
    collapse_ranges(&out)
}

fn strip_sheet_before_ref(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("!#REF!") {
        let before = &rest[..at];
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

/// `#REF!A1` and `#REF!A1:B2`, which Excel writes for a sheet that has gone.
fn strip_after_ref(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("#REF!") {
        out.push_str(&rest[..at + 5]);
        rest = &rest[at + 5..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '$' || c == ':'))
            .unwrap_or(rest.len());
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// `A1:A1` is `A1`.
fn collapse_ranges(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, part) in text.split(':').enumerate() {
        let last = out
            .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '$')
            .next()
            .unwrap_or("");
        let same = !last.is_empty()
            && part.starts_with(last)
            && !part[last.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '$')
            && last.chars().any(|c| c.is_ascii_digit());
        if same {
            out.push_str(&part[last.len()..]);
        } else {
            if index > 0 {
                out.push(':');
            }
            out.push_str(part);
        }
    }
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

fn describe_excel(value: &ExcelValue) -> String {
    format!("{:?}", to_value(value))
}

fn letters(col: u32) -> String {
    cgs_bench::formula::col_to_letters(i64::from(col))
}

fn settle(engine: &mut Engine) -> Result<(), String> {
    while engine.calc_step(60_000, &[])? {}
    engine.take_dirty();
    Ok(())
}

fn command_of(op: &Json) -> Result<Command, String> {
    serde_json::from_value(op.clone()).map_err(|error| error.to_string())
}

fn op_text(op: Option<&Json>, kind: &str) -> String {
    match op {
        Some(op) => {
            let mut text = op.to_string();
            text.truncate(160);
            text
        }
        None => kind.to_string(),
    }
}

/// Compares every sheet of the engine with what Excel shows.
fn check(
    engine: &Engine,
    dump: &[ExcelSheet],
    mut on_problem: impl FnMut(String, String, &str, String, String),
    compared: &mut u64,
) -> (usize, usize) {
    let (mut values, mut texts) = (0, 0);
    let Some(book) = engine.book() else {
        return (0, 0);
    };
    let order = book.tab_order();
    let names: Vec<String> = order
        .iter()
        .map(|&sheet| book.sheets[sheet as usize].name.clone())
        .collect();
    let expected: Vec<&str> = dump.iter().map(|sheet| sheet.name.as_str()).collect();
    if names.iter().map(String::as_str).collect::<Vec<_>>() != expected {
        on_problem(
            String::new(),
            String::new(),
            "sheets",
            expected.join(", "),
            names.join(", "),
        );
        return (1, 0);
    }
    for (position, sheet_dump) in dump.iter().enumerate() {
        let sheet = order[position];
        let bottom = sheet_dump
            .cells
            .iter()
            .map(|cell| cell.r)
            .max()
            .unwrap_or(1)
            + 12;
        let right = sheet_dump
            .cells
            .iter()
            .map(|cell| cell.c)
            .max()
            .unwrap_or(1)
            + 12;
        let mut got = Grid::default();
        engine.read_cells(
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
        let name = sheet_dump.name.clone();
        for cell in &sheet_dump.cells {
            *compared += 1;
            let expected = to_value(&cell.v);
            let mine = got.0.remove(&(cell.r, cell.c));
            if compare(&expected, mine.as_ref()) != Verdict::Match {
                values += 1;
                on_problem(
                    name.clone(),
                    format!("{}{}", letters(cell.c), cell.r),
                    "value",
                    format!(
                        "{} {}",
                        describe_excel(&cell.v),
                        cell.f.as_deref().unwrap_or("")
                    ),
                    describe(mine.as_ref()),
                );
            }
            if let Some(formula) = &cell.f {
                let text = engine.formula_text(sheet, cell.r, cell.c);
                // A cell of an array shows the array's formula in Excel and holds none here.
                let Some(text) = text else {
                    if !in_array(formula, cell, sheet_dump) {
                        texts += 1;
                        on_problem(
                            name.clone(),
                            format!("{}{}", letters(cell.c), cell.r),
                            "formula",
                            formula.clone(),
                            "(no formula)".to_string(),
                        );
                    }
                    continue;
                };
                if normalize(&text) != normalize(formula) {
                    texts += 1;
                    on_problem(
                        name.clone(),
                        format!("{}{}", letters(cell.c), cell.r),
                        "formula",
                        formula.clone(),
                        text,
                    );
                }
            }
        }
        for ((row, col), value) in &got.0 {
            if *value != CalcValue::Empty && *value != CalcValue::Text(String::new()) {
                values += 1;
                on_problem(
                    name.clone(),
                    format!("{}{row}", letters(*col)),
                    "extra",
                    "(empty)".to_string(),
                    describe(Some(value)),
                );
            }
        }
    }
    (values, texts)
}

/// Whether a neighbour above or to the left holds the same text: a cell of an array in Excel's dump.
fn in_array(formula: &str, cell: &ExcelCell, sheet: &ExcelSheet) -> bool {
    sheet.cells.iter().any(|other| {
        other.f.as_deref() == Some(formula)
            && (other.r, other.c) != (cell.r, cell.c)
            && other.r <= cell.r
            && other.c <= cell.c
    })
}

fn open(bytes: Vec<u8>) -> Result<Engine, String> {
    let mut engine = Engine::new();
    engine.set_today(46_331.0);
    engine
        .open(Rc::new(bytes))
        .map_err(|error| error.to_string())?;
    while engine
        .load_step(10_000.0)
        .map_err(|error| error.to_string())?
        .phase
        == LoadPhase::Reading
    {}
    while engine.prepare_step().map_err(|error| error.to_string())? {}
    settle(&mut engine)?;
    Ok(engine)
}

fn run_check(args: &[String]) -> Result<Report, String> {
    let seed = std::fs::read(&args[0]).map_err(|error| error.to_string())?;
    let episodes: Vec<Episode> = serde_json::from_str(
        &std::fs::read_to_string(&args[1]).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let dumps = std::fs::read_to_string(&args[2]).map_err(|error| error.to_string())?;
    let folder = &args[3];
    let mut by_step: HashMap<(u32, usize), ExcelDump> = HashMap::new();
    for line in dumps.lines() {
        let dump: ExcelDump = serde_json::from_str(line).map_err(|error| error.to_string())?;
        by_step.insert((dump.episode, dump.step), dump);
    }
    let mut report = Report::default();
    for episode in &episodes {
        report.steps_scheduled += episode.steps.len();
        let mut engine = open(seed.clone())?;
        // A step that goes as planned, up to where the episode stops, is worth saving and comparing.
        let mut last_good = None;
        let mut alive = true;
        for (step, plan) in episode.steps.iter().enumerate() {
            let dump = by_step
                .get(&(episode.episode, step))
                .ok_or(format!("no Excel dump for {} {step}", episode.episode))?;
            let command = match plan.kind.as_str() {
                "undo" => Command::Undo,
                "redo" => Command::Redo,
                _ => command_of(plan.op.as_ref().ok_or("an edit with no op")?)?,
            };
            let label = op_text(plan.op.as_ref(), &plan.kind);
            if dump.circular {
                report.circular += 1;
                report.steps_skipped += episode.steps.len() - step;
                alive = false;
                break;
            }
            let applied = engine.apply(command);
            let refused_here = applied.is_err();
            // After a refusal the two undo stacks hold different things, so the episode ends.
            if refused_here {
                let mut ignored = 0u64;
                let (values, texts) =
                    check(&engine, &dump.sheets, |_, _, _, _, _| {}, &mut ignored);
                if dump.refused {
                    report.refused_by_both += 1;
                    report.steps_skipped += episode.steps.len() - step - 1;
                    alive = false;
                    break;
                }
                if values == 0 && texts == 0 {
                    report.ignored_by_excel += 1;
                    report.steps_skipped += episode.steps.len() - step - 1;
                    alive = false;
                    break;
                }
            }
            match (dump.refused, refused_here) {
                (true, true) => report.refused_by_both += 1,
                (true, false) => {
                    report.refused_by_excel_only += 1;
                    report.divergences.push(Divergence {
                        episode: episode.episode,
                        step,
                        op: label.clone(),
                        sheet: String::new(),
                        cell: String::new(),
                        what: "excel refused".to_string(),
                        excel: dump.problem.clone(),
                        engine: "accepted".to_string(),
                        differing_cells: 0,
                    });
                    report.steps_diverged += 1;
                    report.steps_skipped += episode.steps.len() - step - 1;
                    alive = false;
                    break;
                }
                (false, true) => {
                    report.refused_by_engine_only += 1;
                    report.divergences.push(Divergence {
                        episode: episode.episode,
                        step,
                        op: label.clone(),
                        sheet: String::new(),
                        cell: String::new(),
                        what: "engine refused".to_string(),
                        excel: "accepted".to_string(),
                        engine: applied.err().unwrap_or_default(),
                        differing_cells: 0,
                    });
                    report.steps_diverged += 1;
                    report.steps_skipped += episode.steps.len() - step - 1;
                    alive = false;
                    break;
                }
                (false, false) => {}
            }
            settle(&mut engine)?;
            let mut first = None;
            let mut noted = 0;
            let (values, texts) = check(
                &engine,
                &dump.sheets,
                |sheet, cell, what, excel, mine| {
                    noted += 1;
                    if std::env::var_os("EDITFUZZ_ALL").is_some() {
                        eprintln!(
                            "ep {} step {step} {sheet}!{cell} {what} excel={excel} engine={mine}",
                            episode.episode
                        );
                    }
                    if first.is_none() {
                        first = Some(Divergence {
                            episode: episode.episode,
                            step,
                            op: label.clone(),
                            sheet,
                            cell,
                            what: what.to_string(),
                            excel,
                            engine: mine,
                            differing_cells: 0,
                        });
                    }
                },
                &mut report.cells_compared,
            );
            if let Some(mut divergence) = first {
                divergence.differing_cells = noted;
                if values == 0 && texts > 0 {
                    report.formula_only += 1;
                }
                report.divergences.push(divergence);
                report.steps_diverged += 1;
                report.steps_skipped += episode.steps.len() - step - 1;
                alive = false;
                break;
            }
            report.steps_passed += 1;
            last_good = Some(step);
        }
        let _ = last_good;
        if alive && !by_step.is_empty() {
            let mut out = Vec::new();
            engine.save(&mut out).map_err(|error| error.to_string())?;
            engine
                .verify_saved(&out)
                .map_err(|error| error.to_string())?;
            let path = format!("{folder}/{:05}.xlsx", episode.episode);
            std::fs::write(&path, out).map_err(|error| error.to_string())?;
            report.saves += 1;
        }
    }
    Ok(report)
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct SavedReport {
    files: usize,
    opened: usize,
    matched: usize,
    cells_compared: u64,
    problems: Vec<String>,
}

fn run_saved(args: &[String]) -> Result<SavedReport, String> {
    let dumps = std::fs::read_to_string(&args[0]).map_err(|error| error.to_string())?;
    let mut last: HashMap<u32, ExcelDump> = HashMap::new();
    for line in dumps.lines() {
        let dump: ExcelDump = serde_json::from_str(line).map_err(|error| error.to_string())?;
        let later = last
            .get(&dump.episode)
            .is_none_or(|known| known.step < dump.step);
        if later {
            last.insert(dump.episode, dump);
        }
    }
    let saved = std::fs::read_to_string(&args[1]).map_err(|error| error.to_string())?;
    let mut report = SavedReport::default();
    for line in saved.lines() {
        let dump: SavedDump = serde_json::from_str(line).map_err(|error| error.to_string())?;
        report.files += 1;
        if !dump.opened || dump.renamed {
            if report.problems.len() < 200 {
                report
                    .problems
                    .push(format!("{}: Excel refused or repaired it", dump.file));
            }
            continue;
        }
        report.opened += 1;
        let episode: u32 = dump
            .file
            .trim_end_matches(".xlsx")
            .parse()
            .map_err(|_| "a saved file's name")?;
        let Some(expected) = last.get(&episode) else {
            continue;
        };
        let mut problem = None;
        let names: Vec<&str> = dump
            .sheets
            .iter()
            .map(|sheet| sheet.name.as_str())
            .collect();
        let wanted: Vec<&str> = expected
            .sheets
            .iter()
            .map(|sheet| sheet.name.as_str())
            .collect();
        if names != wanted {
            problem = Some(format!("sheets {names:?}, Excel has {wanted:?}"));
        }
        for (mine, theirs) in dump.sheets.iter().zip(&expected.sheets) {
            let in_file: HashMap<(u32, u32), &ExcelCell> = mine
                .cells
                .iter()
                .map(|cell| ((cell.r, cell.c), cell))
                .collect();
            let mut seen = HashSet::new();
            for cell in &theirs.cells {
                report.cells_compared += 1;
                seen.insert((cell.r, cell.c));
                let got = in_file.get(&(cell.r, cell.c));
                let same = match got {
                    Some(other) => {
                        let value = match to_value(&other.v) {
                            Value::Number(number) => CalcValue::Number(number),
                            Value::Bool(flag) => CalcValue::Bool(flag),
                            Value::Error(error) => CalcValue::Error(error),
                            Value::Text(text) => CalcValue::Text(text),
                            Value::Empty => CalcValue::Empty,
                        };
                        compare(&to_value(&cell.v), Some(&value)) == Verdict::Match
                            && other.f.as_deref().map(normalize) == cell.f.as_deref().map(normalize)
                    }
                    None => matches!(to_value(&cell.v), Value::Text(text) if text.is_empty()),
                };
                if !same && problem.is_none() {
                    problem = Some(format!(
                        "{}!{}{}: file has {} {}, Excel has {} {}",
                        theirs.name,
                        letters(cell.c),
                        cell.r,
                        got.map_or("(nothing)".to_string(), |other| describe_excel(&other.v)),
                        got.and_then(|other| other.f.as_deref()).unwrap_or(""),
                        describe_excel(&cell.v),
                        cell.f.as_deref().unwrap_or(""),
                    ));
                }
            }
            for cell in &mine.cells {
                if !seen.contains(&(cell.r, cell.c)) && problem.is_none() {
                    problem = Some(format!(
                        "{}!{}{}: file has {} where Excel has nothing",
                        theirs.name,
                        letters(cell.c),
                        cell.r,
                        describe_excel(&cell.v),
                    ));
                }
            }
        }
        match problem {
            None => report.matched += 1,
            Some(problem) => {
                if report.problems.len() < 200 {
                    report.problems.push(format!("{}: {problem}", dump.file));
                }
            }
        }
    }
    Ok(report)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args[0].clone();
    let rest: Vec<String> = args[1..].to_vec();
    let worker = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || -> Result<String, String> {
            if mode == "--check" {
                Ok(serde_json::to_string(&run_check(&rest)?).map_err(|e| e.to_string())?)
            } else {
                Ok(serde_json::to_string(&run_saved(&rest)?).map_err(|e| e.to_string())?)
            }
        })
        .expect("spawn");
    match worker.join().expect("worker") {
        Ok(text) => println!("{text}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
