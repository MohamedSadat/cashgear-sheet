//! Differential test against Excel's own display text. `tests/data/excel-text.jsonl` comes from
//! `scripts/excel-format-zoo.mjs`: each cell's `.Text` in Excel, in a column 255 wide, on a PC set
//! to en-US, so CI can run this without Excel. The agreed target is 99.5% of the cells holding a
//! built-in format or a code from the public test files. The hand-written codes go well beyond
//! what files hold, and include calendars this crate leaves out, so they have a lower floor that
//! only catches regressions.
//!
//! What the worker finishes by measured width is finished here the way `.Text` does it at that
//! width. These are the only adjustments, and neither needs the comparison to be looser:
//!
//! - `*x` fill: `.Text` repeats the fill character until the cell holds 255 characters, so the
//!   formatter's `Display::fill` is expanded to 255 here.
//! - `####`: `.Text` is 255 `#` when the text can't fit, so `Display::hashes` and a text longer
//!   than 255 characters both become that.
//! - A run of 50 or more equal characters that aren't digits, which only those two make, is stored
//!   in the data file as the character followed by U+0001, and the formatter's text gets the same
//!   treatment. Runs of digits are kept, so a miscount in a very long number still shows.
//!
//! `_x` padding needs nothing: `.Text` holds one space for it, as the formatter does. Every other
//! difference is a miss. Misses are printed by code and written in full to `numfmt-misses.txt`
//! under the target directory's `tmp` folder.

use cgs_numfmt::{text_function, Locale, NumFmt, Options, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;

const WIDTH: usize = 255;
const TARGET: f64 = 99.5;
/// For the hand-written codes and the 1904 workbook, and for TEXT().
const FLOOR: f64 = 98.0;
const RUN_MARK: char = '\u{1}';

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn text(&self) -> &str {
        match self {
            Json::Text(text) => text,
            other => panic!("expected text, found {other:?}"),
        }
    }

    fn array(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            other => panic!("expected an array, found {other:?}"),
        }
    }
}

/// Just enough JSON for the data file, so the crate needs no dev-dependency for it.
struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> u8 {
        self.text.as_bytes()[self.at]
    }

    fn eat(&mut self, byte: u8) {
        assert_eq!(self.peek() as char, byte as char, "at byte {}", self.at);
        self.at += 1;
    }

    fn value(&mut self) -> Json {
        match self.peek() {
            b'{' => {
                self.at += 1;
                let mut fields = Vec::new();
                while self.peek() != b'}' {
                    if !fields.is_empty() {
                        self.eat(b',');
                    }
                    let key = self.string();
                    self.eat(b':');
                    fields.push((key, self.value()));
                }
                self.at += 1;
                Json::Object(fields)
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                while self.peek() != b']' {
                    if !items.is_empty() {
                        self.eat(b',');
                    }
                    items.push(self.value());
                }
                self.at += 1;
                Json::Array(items)
            }
            b'"' => Json::Text(self.string()),
            b't' => {
                self.at += 4;
                Json::Bool(true)
            }
            b'f' => {
                self.at += 5;
                Json::Bool(false)
            }
            b'n' => {
                self.at += 4;
                Json::Null
            }
            _ => {
                let start = self.at;
                while matches!(self.peek(), b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.at += 1;
                }
                Json::Number(self.text[start..self.at].parse().unwrap())
            }
        }
    }

    fn string(&mut self) -> String {
        self.eat(b'"');
        let mut out = String::new();
        loop {
            let start = self.at;
            while !matches!(self.peek(), b'"' | b'\\') {
                self.at += 1;
            }
            out.push_str(&self.text[start..self.at]);
            let stop = self.peek();
            self.at += 1;
            if stop == b'"' {
                return out;
            }
            let escape = self.peek();
            self.at += 1;
            match escape {
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                b'b' => out.push('\u{8}'),
                b'f' => out.push('\u{c}'),
                b'u' => {
                    let mut unit = self.hex4();
                    if (0xd800..0xdc00).contains(&unit) {
                        self.at += 2;
                        unit = 0x10000 + ((unit - 0xd800) << 10) + (self.hex4() - 0xdc00);
                    }
                    out.push(char::from_u32(unit).unwrap());
                }
                other => out.push(other as char),
            }
        }
    }

    fn hex4(&mut self) -> u32 {
        let unit = u32::from_str_radix(&self.text[self.at..self.at + 4], 16).unwrap();
        self.at += 4;
        unit
    }
}

fn parse(line: &str) -> Json {
    Reader { text: line, at: 0 }.value()
}

fn value_of(json: &Json) -> Value<'_> {
    match json {
        Json::Number(number) => Value::Number(*number),
        Json::Bool(flag) => Value::Bool(*flag),
        Json::Text(text) => Value::Text(text),
        Json::Object(_) => Value::Error(json.get("error").unwrap().text()),
        other => panic!("not a value: {other:?}"),
    }
}

fn describe(json: &Json) -> String {
    match json {
        Json::Number(number) => number.to_string(),
        Json::Bool(flag) => flag.to_string().to_uppercase(),
        Json::Text(text) => format!("{text:?}"),
        Json::Object(_) => json.get("error").unwrap().text().to_string(),
        other => format!("{other:?}"),
    }
}

/// The same run shortening as the script that wrote the data.
fn compress(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < chars.len() {
        let mut end = at + 1;
        while end < chars.len() && chars[end] == chars[at] {
            end += 1;
        }
        if end - at >= 50 && !chars[at].is_numeric() {
            out.push(chars[at]);
            out.push(RUN_MARK);
        } else {
            out.extend(&chars[at..end]);
        }
        at = end;
    }
    out
}

/// The formatter's answer at this column width, in the form the data file holds.
fn display_text(format: &NumFmt, value: Value<'_>, options: &Options<'_>) -> String {
    let mut out = String::new();
    let display = format.format(value, options, &mut out);
    if display.hashes {
        return compress(&"#".repeat(WIDTH));
    }
    let length = out.chars().count();
    if length > WIDTH {
        return compress(&"#".repeat(WIDTH));
    }
    if let Some((fill, at)) = display.fill {
        out.insert_str(at, &fill.to_string().repeat(WIDTH - length));
    }
    compress(&out)
}

#[derive(Default)]
struct Tally {
    cells: usize,
    matched: usize,
    codes: usize,
}

impl Tally {
    fn rate(&self) -> f64 {
        if self.cells == 0 {
            100.0
        } else {
            self.matched as f64 * 100.0 / self.cells as f64
        }
    }
}

struct Miss {
    value: String,
    excel: String,
    ours: String,
}

fn show(text: &str) -> String {
    text.replace(RUN_MARK, "~")
}

#[test]
fn display_text_matches_excel() {
    let data = include_str!("data/excel-text.jsonl");
    let mut lines = data.lines();
    let head = parse(lines.next().unwrap());
    let sets = head.get("sets").unwrap();
    let locale = Locale::en_us();

    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut misses: BTreeMap<(String, String), Vec<Miss>> = BTreeMap::new();
    let mut validity_misses = Vec::new();

    for line in lines {
        let record = parse(line);
        let group = record.get("group").unwrap().text().to_string();
        let code = record.get("code").unwrap().text().to_string();
        let builtin = record.get("builtin").map(|id| match id {
            Json::Number(number) => *number as u32,
            other => panic!("bad id {other:?}"),
        });
        let date1904 = record.get("date1904").is_some();
        let options = Options {
            locale: &locale,
            date1904,
        };
        let tally = tallies.entry(group.clone()).or_default();
        tally.codes += 1;

        let invalid = record.get("invalid").is_some();
        let format = match builtin {
            Some(id) => match NumFmt::builtin(id) {
                Some(format) => format,
                None => {
                    validity_misses.push(format!("built-in {id} ({code}) has no format"));
                    NumFmt::parse(&code)
                }
            },
            None => NumFmt::parse(&code),
        };
        if invalid == format.is_valid() {
            validity_misses.push(format!(
                "[{code}] Excel {}, ours {}",
                if invalid { "refuses it" } else { "accepts it" },
                if format.is_valid() {
                    "accepts it"
                } else {
                    "refuses it"
                },
            ));
            tally.cells += 1;
        } else if invalid {
            tally.cells += 1;
            tally.matched += 1;
        }

        let Some(texts) = record.get("text") else {
            continue;
        };
        let set = sets.get(record.get("set").unwrap().text()).unwrap().array();
        for (json, excel) in set.iter().zip(texts.array()) {
            let ours = display_text(&format, value_of(json), &options);
            tally.cells += 1;
            if ours == excel.text() {
                tally.matched += 1;
            } else {
                misses
                    .entry((group.clone(), code.clone()))
                    .or_default()
                    .push(Miss {
                        value: describe(json),
                        excel: show(excel.text()),
                        ours: show(&ours),
                    });
            }
        }
    }

    let mut report = String::new();
    for ((group, code), list) in &misses {
        writeln!(report, "{group} [{code}]: {} cells", list.len()).unwrap();
        for miss in list {
            writeln!(
                report,
                "  {:<24} excel [{}]  ours [{}]",
                miss.value, miss.excel, miss.ours
            )
            .unwrap();
        }
    }
    if !validity_misses.is_empty() {
        writeln!(report, "validity:").unwrap();
        for miss in &validity_misses {
            writeln!(report, "  {miss}").unwrap();
        }
    }
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("numfmt-misses.txt");
    std::fs::write(&path, &report).unwrap();

    let mut total = Tally::default();
    let mut agreed = Tally::default();
    let mut others = Tally::default();
    for (group, tally) in &tallies {
        let bucket = if group == "builtin" || group == "harvested" {
            &mut agreed
        } else {
            &mut others
        };
        bucket.cells += tally.cells;
        bucket.matched += tally.matched;
        println!(
            "{group}: {:.3}% of {} cells match, {} of {} codes have a miss",
            tally.rate(),
            tally.cells,
            misses.keys().filter(|(g, _)| g == group).count(),
            tally.codes,
        );
        total.cells += tally.cells;
        total.matched += tally.matched;
        total.codes += tally.codes;
    }
    println!(
        "built-in and harvested: {:.3}% of {} cells",
        agreed.rate(),
        agreed.cells
    );
    println!(
        "all: {:.3}% of {} cells, {} records, full list in {}",
        total.rate(),
        total.cells,
        total.codes,
        path.display()
    );

    if agreed.rate() < TARGET || others.rate() < FLOOR {
        let mut brief = report;
        brief.truncate(brief.floor_char_boundary(20_000));
        panic!(
            "built-in and harvested {:.3}% (target {TARGET}%), others {:.3}% (floor {FLOOR}%)\n{brief}",
            agreed.rate(),
            others.rate()
        );
    }
}

/// A code harvested later than the data was written would otherwise go unmeasured.
#[test]
fn the_data_covers_every_harvested_code() {
    let data = include_str!("data/excel-text.jsonl");
    let mut seen = std::collections::BTreeSet::new();
    for line in data.lines().skip(1) {
        let record = parse(line);
        if record.get("group").unwrap().text() == "harvested" {
            seen.insert(record.get("code").unwrap().text().to_string());
        }
    }
    for code in include_str!("data/format-codes.txt").lines() {
        assert!(seen.contains(code), "[{code}] is not in excel-text.jsonl");
    }
}

/// `=TEXT(value, code)` read from the same workbooks, including the codes Excel refuses.
#[test]
fn text_function_matches_excel() {
    let data = include_str!("data/excel-text.jsonl");
    let mut lines = data.lines();
    let head = parse(lines.next().unwrap());
    let values = head.get("fn").unwrap().array();
    let locale = Locale::en_us();
    let options = Options {
        locale: &locale,
        date1904: false,
    };

    let mut cells = 0;
    let mut matched = 0;
    let mut report = String::new();
    for line in lines {
        let record = parse(line);
        let Some(texts) = record.get("fn") else {
            continue;
        };
        let code = record.get("code").unwrap().text();
        for (json, excel) in values.iter().zip(texts.array()) {
            cells += 1;
            let ours = match text_function(value_of(json), code, &options) {
                Ok(text) => compress(&text),
                Err(()) => "#VALUE!".to_string(),
            };
            if ours == excel.text() {
                matched += 1;
            } else {
                writeln!(
                    report,
                    "[{code}] {:<24} excel [{}]  ours [{}]",
                    describe(json),
                    show(excel.text()),
                    show(&ours)
                )
                .unwrap();
            }
        }
    }
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("numfmt-text-misses.txt");
    std::fs::write(&path, &report).unwrap();
    let rate = matched as f64 * 100.0 / cells as f64;
    println!(
        "TEXT(): {rate:.3}% of {cells} cells, full list in {}",
        path.display()
    );
    assert!(
        rate >= FLOOR,
        "TEXT() matches on {rate:.3}% of {cells} cells\n{report}"
    );
}
