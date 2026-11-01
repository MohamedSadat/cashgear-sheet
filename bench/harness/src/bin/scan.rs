//! Reports each workbook's provenance and formula counts as JSON lines, so the corpus
//! can be judged before any engine runs. Run: `cargo run -p cgs-bench --bin scan <dir>`.

use std::fs;
use std::path::{Path, PathBuf};

use cgs_bench::book::{read_book, Content, FormulaKind};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx") || ext.eq_ignore_ascii_case("xlsm"))
        {
            out.push(path);
        }
    }
}

fn quote(text: &str) -> String {
    let clean: String = text
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    format!("\"{}\"", clean.replace('\\', "\\\\").replace('"', "\\\""))
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: scan <dir>");
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    files.sort();
    for path in files {
        let bytes = fs::read(&path).unwrap_or_default();
        let name = path.to_string_lossy().replace('\\', "/");
        match read_book(&bytes) {
            Err(error) => println!("{{\"path\":{},\"error\":{}}}", quote(&name), quote(&error)),
            Ok(book) => {
                let (mut formulas, mut with_value, mut arrays, mut cells) = (0, 0, 0, 0);
                for cell in book.sheets.iter().flat_map(|sheet| &sheet.cells) {
                    cells += 1;
                    if let Content::Formula { kind, cached, .. } = &cell.content {
                        formulas += 1;
                        with_value += usize::from(cached.is_some());
                        arrays += usize::from(!matches!(kind, FormulaKind::Normal));
                    }
                }
                println!(
                    "{{\"path\":{},\"bytes\":{},\"app\":{},\"appVersion\":{},\"fileApp\":{},\"rupBuild\":{},\"calcId\":{},\"calcMode\":{},\"fullCalc\":{},\"iterate\":{},\"excelMarkup\":{},\"externalLinks\":{},\"sheets\":{},\"names\":{},\"tables\":{},\"cells\":{},\"formulas\":{},\"withValue\":{},\"arrays\":{}}}",
                    quote(&name),
                    bytes.len(),
                    quote(&book.application),
                    quote(&book.app_version),
                    quote(&book.file_app),
                    quote(&book.rup_build),
                    quote(&book.calc_id),
                    quote(&book.calc_mode),
                    book.full_calc_on_load,
                    book.iterate,
                    book.has_excel_only_markup,
                    book.has_external_links,
                    book.sheets.len(),
                    book.names.len(),
                    book.tables.len(),
                    cells,
                    formulas,
                    with_value,
                    arrays
                );
            }
        }
    }
}
