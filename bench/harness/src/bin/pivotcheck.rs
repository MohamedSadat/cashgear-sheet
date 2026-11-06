//! Refreshes every pivot table in the test workbooks from its own cache records and compares the
//! result with what the file holds: the output cells, the location, and `rowItems` and `colItems`.
//! Only files Excel saved count, since the saved output is Excel's own.
//! `cargo run -p cgs-bench --release --bin pivotcheck -- <file or folder> [--arabic] [--show N] [--json out.json]`

use std::path::{Path, PathBuf};
use std::rc::Rc;

use calamine::{open_workbook_auto, Data, Reader};
use cgs_model::pivot::{CacheRecords, PivotTable};
use cgs_numfmt::{Locale, NumFmt, Options, Value};
use cgs_pivot::{CacheView, Captions, CellValue, Layout};
use cgs_xlsx::package::Package;
use serde_json::json;

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("xlsx" | "xlsm")
        ) {
            out.push(path);
        }
    }
}

pub fn arabic() -> Captions {
    let mut c = Captions {
        row_labels: "تسميات الصفوف".into(),
        col_labels: "تسميات الأعمدة".into(),
        grand_total: "الإجمالي الكلي".into(),
        total: "الإجمالي".into(),
        blank: "(فارغ)".into(),
        all: "(الكل)".into(),
        multiple: "(عدة عناصر)".into(),
        ..Captions::default()
    };
    c.functions[0] = "مجموع".into();
    c.functions[1] = "عدد".into();
    c.functions[3] = "الحد الأقصى".into();
    c
}

fn words(
    row: &str,
    col: &str,
    grand: &str,
    total: &str,
    blank: &str,
    all: &str,
    multiple: &str,
) -> Captions {
    Captions {
        row_labels: row.into(),
        col_labels: col.into(),
        grand_total: grand.into(),
        total: total.into(),
        blank: blank.into(),
        all: all.into(),
        multiple: multiple.into(),
        ..Captions::default()
    }
}

/// The languages test files were saved in. Each pivot table is checked in the one that fits best.
fn languages() -> Vec<Captions> {
    vec![
        Captions::default(),
        arabic(),
        words(
            "Zeilenbeschriftungen",
            "Spaltenbeschriftungen",
            "Gesamtergebnis",
            "Ergebnis",
            "(Leer)",
            "(Alle)",
            "(Mehrere Elemente)",
        ),
        {
            let mut ru = words(
                "Названия строк",
                "Названия столбцов",
                "Общий итог",
                "Итог",
                "(пусто)",
                "(Все)",
                "(несколько элементов)",
            );
            ru.functions = [
                "Сумма",
                "Количество",
                "Среднее",
                "Максимум",
                "Минимум",
                "Произведение",
                "Количество чисел",
                "Смещенное отклонение",
                "Несмещенное отклонение",
                "Смещенная дисперсия",
                "Несмещенная дисперсия",
            ]
            .map(str::to_string);
            ru
        },
        words(
            "Sorcímkék",
            "Oszlopcímkék",
            "Végösszeg",
            "Összeg",
            "(üres)",
            "(Mind)",
            "(Több elem)",
        ),
        words(
            "Étiquettes de lignes",
            "Étiquettes de colonnes",
            "Total général",
            "Total",
            "(vide)",
            "(Tous)",
            "(Plusieurs éléments)",
        ),
        words(
            "Etiquetas de fila",
            "Etiquetas de columna",
            "Total general",
            "Total",
            "(en blanco)",
            "(Todas)",
            "(Varios elementos)",
        ),
    ]
}

fn saved_by_excel(package: &Package) -> bool {
    package
        .read("docProps/app.xml")
        .map(|xml| String::from_utf8_lossy(&xml).contains("<Application>Microsoft Excel"))
        .unwrap_or(false)
}

#[derive(Default)]
struct Tally {
    tables: u32,
    skipped: u32,
    unsupported: u32,
    errors: u32,
    cells_ok: u32,
    lines_ok: u32,
    both_ok: u32,
    cells: u64,
    cells_wrong: u64,
}

fn same_number(a: f64, b: f64) -> bool {
    a == b || (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn expected_text(data: &Data) -> Option<CellValue> {
    Some(match data {
        Data::Empty => return None,
        Data::String(s) => CellValue::Text(s.clone()),
        Data::Float(f) => CellValue::Number(*f),
        Data::Int(i) => CellValue::Number(*i as f64),
        Data::Bool(b) => CellValue::Bool(*b),
        Data::Error(e) => CellValue::Error(format!("{e}")),
        Data::DateTime(d) => CellValue::Number(d.as_f64()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => CellValue::Text(s.clone()),
    })
}

fn same(ours: Option<&CellValue>, theirs: Option<&CellValue>) -> bool {
    match (ours, theirs) {
        (None, None) => true,
        (Some(CellValue::Number(a)), Some(CellValue::Number(b))) => same_number(*a, *b),
        (Some(CellValue::Text(a)), Some(CellValue::Text(b))) => a == b,
        (Some(CellValue::Error(a)), Some(CellValue::Error(b))) => {
            a.trim_start_matches('#')
                .to_lowercase()
                .replace(['/', '!', '?'], "")
                == b.trim_start_matches('#')
                    .to_lowercase()
                    .replace(['/', '!', '?'], "")
        }
        (Some(CellValue::Bool(a)), Some(CellValue::Bool(b))) => a == b,
        (Some(CellValue::Text(a)), None) | (None, Some(CellValue::Text(a))) => a.is_empty(),
        _ => false,
    }
}

fn formatter(locale: &Locale) -> impl Fn(f64, Option<u32>) -> String + '_ {
    move |n, id| {
        let format = id.and_then(NumFmt::builtin).unwrap_or_else(NumFmt::general);
        let mut out = String::new();
        format.format(
            Value::Number(n),
            &Options {
                locale,
                date1904: false,
            },
            &mut out,
        );
        out
    }
}

struct Check {
    cells_ok: bool,
    lines_ok: bool,
    cells: u64,
    wrong: Vec<String>,
}

fn check(
    path: &Path,
    table: &PivotTable,
    layout: &Layout,
    sheet_name: &str,
) -> Result<Check, String> {
    let mut book = open_workbook_auto(path).map_err(|e| e.to_string())?;
    let range = book
        .worksheet_range(sheet_name)
        .map_err(|e| e.to_string())?;
    let mut wrong = Vec::new();
    let mut cells = 0;
    let area = layout.area;
    let lookup = |r: u32, c: u32| -> Option<CellValue> {
        range.get_value((r - 1, c - 1)).and_then(expected_text)
    };
    let mut ours = std::collections::HashMap::new();
    for cell in &layout.cells {
        ours.insert((cell.row, cell.col), &cell.value);
    }
    // The saved output's own area, in case ours is smaller.
    let saved = table.location.range;
    let top = area.top.min(saved.top);
    let bottom = area.bottom.max(saved.bottom);
    let right = area.right.max(saved.right);
    for r in top..=bottom {
        for c in area.left.min(saved.left)..=right {
            let mut theirs = lookup(r, c);
            let mine = ours.get(&(r, c)).copied();
            // Excel saves 0 where it shows the caption for empty cells.
            if let (Some(CellValue::Text(t)), Some(CellValue::Number(n))) = (mine, theirs.as_ref())
            {
                if *n == 0.0 && table.options.missing_caption.as_deref() == Some(t.as_str()) {
                    theirs = Some(CellValue::Text(t.clone()));
                }
            }
            if theirs.is_none() && mine.is_none() {
                continue;
            }
            cells += 1;
            if !same(mine, theirs.as_ref()) {
                wrong.push(format!(
                    "{}{}: ours {:?} theirs {:?}",
                    col_name(c),
                    r,
                    mine,
                    theirs
                ));
            }
        }
    }
    // Older Excel leaves out `i` where the member already says which value field it is.
    let same_lines = |a: &[cgs_model::pivot::AxisLine], b: &[cgs_model::pivot::AxisLine]| {
        a.len() == b.len()
            && a.iter().zip(b).all(|(x, y)| {
                x.kind == y.kind
                    && x.repeat == y.repeat
                    && x.members == y.members
                    && (x.data == y.data || x.data == 0 || y.data == 0)
            })
    };
    let lines_ok = layout.location == table.location
        && same_lines(&layout.row_items, &table.row_items)
        && same_lines(&layout.col_items, &table.col_items);
    if !lines_ok {
        if layout.location != table.location {
            wrong.push(format!(
                "location: ours {:?} theirs {:?}",
                layout.location, table.location
            ));
        }
        if !same_lines(&layout.row_items, &table.row_items) {
            let at = layout
                .row_items
                .iter()
                .zip(&table.row_items)
                .position(|(a, b)| a != b);
            wrong.push(format!(
                "rowItems: ours {} theirs {}, first difference at {:?}: ours {:?} theirs {:?}",
                layout.row_items.len(),
                table.row_items.len(),
                at,
                at.and_then(|i| layout.row_items.get(i)),
                at.and_then(|i| table.row_items.get(i))
            ));
        }
        if !same_lines(&layout.col_items, &table.col_items) {
            let at = layout
                .col_items
                .iter()
                .zip(&table.col_items)
                .position(|(a, b)| a != b);
            wrong.push(format!(
                "colItems: ours {} theirs {}, first difference at {:?}: ours {:?} theirs {:?}",
                layout.col_items.len(),
                table.col_items.len(),
                at,
                at.and_then(|i| layout.col_items.get(i)),
                at.and_then(|i| table.col_items.get(i))
            ));
        }
    }
    let cells_ok = !wrong.iter().any(|w| {
        !w.starts_with("location") && !w.starts_with("rowItems") && !w.starts_with("colItems")
    });
    Ok(Check {
        cells_ok,
        lines_ok,
        cells,
        wrong,
    })
}

fn col_name(mut c: u32) -> String {
    let mut out = Vec::new();
    while c > 0 {
        let rem = (c - 1) % 26;
        out.push(b'A' + rem as u8);
        c = (c - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(args.get(1).map_or("tests/workbooks/public", String::as_str));
    let arabic_captions = args.iter().any(|a| a == "--arabic");
    let all_files = args.iter().any(|a| a == "--all");
    let show: usize = args
        .iter()
        .position(|a| a == "--show")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let json_out = args
        .iter()
        .position(|a| a == "--json")
        .and_then(|i| args.get(i + 1));
    let captions = if arabic_captions {
        arabic()
    } else {
        Captions::default()
    };
    let locale = Locale::en_us();
    let format = formatter(&locale);
    let mut list = Vec::new();
    files(&root, &mut list);
    list.sort();
    let mut tally = Tally::default();
    let mut reasons: std::collections::BTreeMap<String, u32> = Default::default();
    let mut report = Vec::new();
    for path in &list {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(package) = Package::open(Rc::new(bytes)) else {
            continue;
        };
        if !all_files && !saved_by_excel(&package) {
            continue;
        }
        let Ok(book) = cgs_xlsx::workbook::read_workbook(&package) else {
            continue;
        };
        for table in &book.pivot_tables {
            tally.tables += 1;
            let label = format!("{} [{}]", path.display(), table.name);
            if let Some(why) = &table.unsupported {
                tally.unsupported += 1;
                *reasons.entry(why.clone()).or_default() += 1;
                report.push(json!({ "table": label, "unsupported": why }));
                continue;
            }
            let cache = &book.pivot_caches[table.cache as usize];
            let Some(records) = cache
                .records_part
                .as_ref()
                .and_then(|p| package.read(p))
                .map(|xml| cgs_xlsx::pivot::read_records(&xml, cache, book.date1904))
            else {
                tally.skipped += 1;
                *reasons.entry("no records".into()).or_default() += 1;
                continue;
            };
            let records: CacheRecords = records;
            if table.row_items.is_empty() && table.col_items.is_empty() {
                tally.skipped += 1;
                *reasons.entry("never laid out".into()).or_default() += 1;
                continue;
            }
            let view = CacheView::new(cache, &records, book.date1904);
            let sheet = &book.sheets[table.sheet as usize].name;
            let sets = if arabic_captions {
                vec![captions.clone()]
            } else {
                languages()
            };
            let mut best: Option<(usize, Layout)> = None;
            let mut failure = None;
            for set in &sets {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    cgs_pivot::layout(table, &view, set, &format)
                }));
                match outcome {
                    Ok(Ok(layout)) => {
                        let wrong = check(path, table, &layout, sheet)
                            .map_or(usize::MAX, |c| c.wrong.len());
                        if best.as_ref().is_none_or(|(w, _)| wrong < *w) {
                            best = Some((wrong, layout));
                        }
                        if wrong == 0 {
                            break;
                        }
                    }
                    other => {
                        failure = Some(other);
                        break;
                    }
                }
            }
            let outcome = match (best, failure) {
                (Some((_, layout)), None) => Ok(Ok(layout)),
                (_, Some(other)) => other,
                (None, None) => continue,
            };
            let layout = match outcome {
                Ok(Ok(layout)) => layout,
                Ok(Err(error)) => {
                    tally.errors += 1;
                    *reasons.entry(format!("error: {error}")).or_default() += 1;
                    report.push(json!({ "table": label, "error": error }));
                    continue;
                }
                Err(_) => {
                    tally.errors += 1;
                    println!("PANIC {label}");
                    report.push(json!({ "table": label, "error": "panic" }));
                    continue;
                }
            };
            match check(path, table, &layout, sheet) {
                Ok(result) => {
                    tally.cells += result.cells;
                    tally.cells_wrong +=
                        result.wrong.iter().filter(|w| w.contains(": ours")).count() as u64;
                    tally.cells_ok += u32::from(result.cells_ok);
                    tally.lines_ok += u32::from(result.lines_ok);
                    tally.both_ok += u32::from(result.cells_ok && result.lines_ok);
                    if !(result.cells_ok && result.lines_ok) {
                        println!("FAIL {label} ({} wrong)", result.wrong.len());
                        for w in result.wrong.iter().take(show) {
                            println!("    {w}");
                        }
                    }
                    report.push(json!({
                        "table": label,
                        "cellsMatch": result.cells_ok,
                        "linesMatch": result.lines_ok,
                        "cells": result.cells,
                        "wrong": result.wrong.iter().take(20).collect::<Vec<_>>(),
                    }));
                }
                Err(error) => {
                    tally.errors += 1;
                    report.push(json!({ "table": label, "error": error }));
                }
            }
        }
    }
    println!(
        "tables {} | unsupported {} | no records {} | errors {} | cells match {} | lines match {} | both {} | cells compared {} wrong {}",
        tally.tables,
        tally.unsupported,
        tally.skipped,
        tally.errors,
        tally.cells_ok,
        tally.lines_ok,
        tally.both_ok,
        tally.cells,
        tally.cells_wrong
    );
    for (why, n) in &reasons {
        println!("  {n:4}  {why}");
    }
    if let Some(out) = json_out {
        let summary = json!({
            "tables": tally.tables,
            "unsupported": tally.unsupported,
            "noRecords": tally.skipped,
            "errors": tally.errors,
            "cellsMatch": tally.cells_ok,
            "linesMatch": tally.lines_ok,
            "bothMatch": tally.both_ok,
            "cellsCompared": tally.cells,
            "cellsWrong": tally.cells_wrong,
            "reasons": reasons,
            "tablesChecked": report,
        });
        std::fs::write(out, serde_json::to_string_pretty(&summary).unwrap()).unwrap();
    }
}
