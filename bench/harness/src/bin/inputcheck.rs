//! Compares `parse_input` with what Excel made of the same typed text, as written by
//! `bench/excel-input.ps1`. See `bench/run-excel-input.mjs`.
//!
//! Usage: inputcheck <excel.json> <today serial>

use std::fs;

use cgs_calc::CalcValue;
use cgs_engine::input::{parse_input, Format, Typed};
use cgs_numfmt::Locale;
use serde::Deserialize;

#[derive(Deserialize)]
struct Seen {
    text: String,
    kind: String,
    #[serde(default)]
    value: serde_json::Value,
    #[serde(default)]
    format: String,
}

/// Built-in number formats as Excel's `NumberFormat` spells them on an en-US machine.
fn builtin(id: u32) -> &'static str {
    match id {
        3 => "#,##0",
        4 => "#,##0.00",
        6 => "$#,##0_);[Red]($#,##0)",
        8 => "$#,##0.00_);[Red]($#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yyyy h:mm",
        46 => "[h]:mm:ss",
        47 => "mm:ss.0",
        _ => "?",
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= a.abs().max(b.abs()) * 1e-13
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seen: Vec<Seen> = serde_json::from_str(
        fs::read_to_string(&args[1])
            .expect("the results")
            .trim_start_matches('\u{feff}'),
    )
    .expect("well formed");
    let today: f64 = args[2].parse().expect("a serial");
    let locale = Locale::en_us();
    let mut matched = 0;
    let mut typed_only = 0;
    let mut differ = Vec::new();
    for case in &seen {
        let ours = parse_input(&case.text, &locale, false, today);
        // Over COM these stay text. Typed at the keyboard, Excel makes them formulas: `=+A1`.
        if matches!(&ours, Typed::Formula(formula) if formula.starts_with(['+', '-']))
            && case.kind == "text"
        {
            typed_only += 1;
            continue;
        }
        // This PC's Office has an Arabic UI, and spells colour names in Arabic.
        let excel_format = case
            .format
            .replace("[\u{623}\u{62d}\u{645}\u{631}]", "[Red]");
        let number = case.value.as_f64();
        let text = case.value.as_str().unwrap_or("");
        let same = match (&ours, case.kind.as_str()) {
            (Typed::Clear, "clear") => true,
            // `check_formula` refuses this, which comes after reading the text.
            (Typed::Formula(formula), "refused") => formula.starts_with('='),
            (Typed::Formula(formula), "formula") => {
                let ours = format!("={formula}").replace(' ', "");
                ours.eq_ignore_ascii_case(&text.replace(' ', ""))
            }
            (Typed::Value(CalcValue::Number(value)), "number") => {
                number.is_some_and(|other| close(*value, other)) && excel_format == "General"
            }
            (Typed::Value(CalcValue::Text(value)), "text") => value == text,
            (Typed::Value(CalcValue::Bool(value)), "bool") => case.value.as_bool() == Some(*value),
            (Typed::Value(CalcValue::Error(value)), "error") => value == text,
            (Typed::Formatted(value, format), "number") => {
                let Format::Builtin(id) = format;
                number.is_some_and(|other| close(*value, other)) && builtin(*id) == excel_format
            }
            _ => false,
        };
        if same {
            matched += 1;
        } else {
            differ.push(format!(
                "{:?}: ours {ours:?}, Excel {} {} [{}]",
                case.text, case.kind, case.value, case.format
            ));
        }
    }
    println!(
        "{matched} of {} match, and {typed_only} go by what typing does, not COM",
        seen.len() - typed_only
    );
    for line in &differ {
        println!("  {line}");
    }
}
