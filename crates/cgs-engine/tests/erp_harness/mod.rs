//! What the ERP tests share: seeded vendor records as the ERP sends them, a schema like the real
//! Vendors one for an editor (with a decimal, a date and a boolean editable too, to test them),
//! binding them into a new workbook, and reading the table back.
#![allow(dead_code)]

use std::collections::HashMap;
use std::rc::Rc;

use cgs_calc::{CalcValue, CellSink, Rect, SheetRect};
use cgs_engine::{Engine, LoadPhase};

pub const READ_ONLY: &str = "This column is read-only in Cash Gear ERP.";
pub const ROWS: &str = "Rows can't be added or deleted in ERP data.";
pub const COLUMNS: &str = "Columns can't be added or deleted in ERP data.";
pub const TABLE: &str = "The table that holds ERP data can't be resized, renamed or reshaped.";
pub const SIDEWAYS: &str = "ERP data can only be sorted top to bottom.";
pub const WHOLE: &str = "ERP data is sorted as a whole table. Select a cell in it and sort again.";
pub const SHEET: &str = "A sheet that holds ERP data can't be deleted.";

/// mulberry32, so every run sees the same records and steps.
pub struct Random(pub u32);

impl Random {
    pub fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() * n as f64) as usize % n.max(1)
    }

    pub fn pick<'a, T>(&mut self, list: &'a [T]) -> &'a T {
        &list[self.below(list.len())]
    }
}

/// Each field: its name, English label, type and constraints, and whether an editor may edit it.
pub const FIELDS: [(&str, &str, &str, bool); 14] = [
    (
        "vendorAccount",
        "Account",
        r#""type":"text","maxLength":50"#,
        false,
    ),
    (
        "vendorName",
        "Name",
        r#""type":"text","maxLength":150,"required":true"#,
        true,
    ),
    (
        "vendorGroup",
        "Group",
        r#""type":"lookup","lookup":"vendorGroups","required":true"#,
        true,
    ),
    ("mobile", "Mobile", r#""type":"text","maxLength":50"#, true),
    (
        "currency",
        "Currency",
        r#""type":"lookup","lookup":"currencies","required":true"#,
        false,
    ),
    (
        "payMethod",
        "Payment method",
        r#""type":"lookup","lookup":"paymentModes","required":true"#,
        true,
    ),
    (
        "address",
        "Address",
        r#""type":"text","maxLength":150"#,
        true,
    ),
    (
        "taxFile",
        "Tax file",
        r#""type":"text","maxLength":50"#,
        true,
    ),
    (
        "blocked",
        "Blocked",
        r#""type":"boolean","required":true"#,
        true,
    ),
    ("bankAccount", "Bank account", r#""type":"text""#, false),
    (
        "debitLimit",
        "Credit limit",
        r#""type":"decimal","scale":2,"min":0"#,
        true,
    ),
    ("since", "Since", r#""type":"date""#, true),
    ("modifiedBy", "Modified by", r#""type":"text""#, false),
    ("modifiedDate", "Modified", r#""type":"dateTime""#, false),
];

/// The columns, counted from 1, an editor may edit.
pub fn editable_cols() -> Vec<u32> {
    FIELDS
        .iter()
        .enumerate()
        .filter(|(_, field)| field.3)
        .map(|(index, _)| index as u32 + 1)
        .collect()
}

pub fn read_only_cols() -> Vec<u32> {
    FIELDS
        .iter()
        .enumerate()
        .filter(|(_, field)| !field.3)
        .map(|(index, _)| index as u32 + 1)
        .collect()
}

pub fn schema(max_records: u32) -> String {
    let fields: Vec<String> = FIELDS
        .iter()
        .map(|(name, label, kind, editable)| {
            let reason = match (*editable, *name) {
                (true, _) => "",
                (false, "currency") => r#","editReason":"needsAdvEdit""#,
                _ => r#","editReason":"notEditable""#,
            };
            format!(
                r#"{{"name":"{name}","label":{{"en":"{label}","ar":"{label} ع"}},{kind},"editable":{editable}{reason}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"entity":"vendors","schemaVersion":"vendors.1","label":{{"en":"Vendors","ar":"الموردون"}},
        "keyField":"vendorAccount","idField":"recId","maxPageSize":1000,"maxRecords":{max_records},"maxCommitRecords":500,
        "fields":[{}],
        "lookups":{{"vendorGroups":[{{"code":"LOCAL","label":{{"en":"Local","ar":"محلي"}}}},{{"code":"IMPORT","label":{{"en":"Import","ar":"Import"}}}},{{"code":"SERVICES","label":{{"en":"Services","ar":"خدمات"}}}},{{"code":"OLD","label":{{"en":"Old","ar":"Old"}},"active":false}}],
          "paymentModes":[{{"code":"CASH","label":{{"en":"Cash","ar":"نقدي"}}}},{{"code":"NET30","label":{{"en":"30 days","ar":"30 days"}}}}],
          "currencies":[{{"code":"EGP","label":{{"en":"EGP","ar":"EGP"}}}},{{"code":"USD","label":{{"en":"USD","ar":"USD"}}}}]}}}}"#,
        fields.join(",")
    )
}

pub const SESSION: &str = r#"{"company":"Kit","userId":"u17","userName":"Sara","language":"en","entity":"vendors","entityRoute":"api/payable/VendorSheet","filter":null,"idleExpiresAt":"x"}"#;

/// One record as the ERP sends it, and what its cells must hold.
#[derive(Clone)]
pub struct Record {
    pub id: i64,
    pub key: String,
    pub wire: Vec<String>,
    pub cells: Vec<CalcValue>,
}

pub fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

/// Days in each month of 2024, a leap year.
const MONTHS_2024: [u32; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

pub fn records(n: usize, seed: u32) -> Vec<Record> {
    let mut pick = Random(seed);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let id = 900_001 + (i as i64) * 3;
        let key = format!("V-{:05}", i + 1);
        let arabic = pick.next() < 0.4;
        let name = if arabic {
            format!("شركة النيل {}", i + 1)
        } else {
            format!("Nile Trading {}", i + 1)
        };
        let group = ["LOCAL", "IMPORT", "SERVICES"][pick.below(3)];
        let mobile = (pick.next() < 0.85).then(|| format!("01{:09}", pick.below(1_000_000_000)));
        let currency = ["EGP", "USD"][pick.below(2)];
        let pay = ["CASH", "NET30"][pick.below(2)];
        let address = (pick.next() < 0.5).then(|| format!("{} St", pick.below(200)));
        let tax = (pick.next() < 0.7).then(|| format!("{:04}", pick.below(10_000)));
        let blocked = pick.next() < 0.1;
        let bank = (pick.next() < 0.5).then(|| format!("00{:08}", pick.below(100_000_000)));
        let cents = pick.below(5_000_000);
        let limit = format!("{}.{:02}", cents / 100, cents % 100);
        let day = 1 + pick.below(28) as u32;
        let month = 1 + pick.below(12) as u32;
        let since = format!("2024-{month:02}-{day:02}");
        let wire = vec![
            quoted(&key),
            quoted(&name),
            quoted(group),
            mobile.as_deref().map_or("null".into(), quoted),
            quoted(currency),
            quoted(pay),
            address.as_deref().map_or("null".into(), quoted),
            tax.as_deref().map_or("null".into(), quoted),
            blocked.to_string(),
            bank.as_deref().map_or("null".into(), quoted),
            quoted(&limit),
            quoted(&since),
            quoted("seed"),
            quoted("2026-10-30T08:12:00+02:00"),
        ];
        let text = |t: Option<&str>| t.map_or(CalcValue::Empty, |t| CalcValue::Text(t.to_string()));
        // Days from 2024-01-01, serial 45292.
        let since_serial =
            45292.0 + MONTHS_2024[..month as usize - 1].iter().sum::<u32>() as f64 + f64::from(day)
                - 1.0;
        let cells = vec![
            text(Some(&key)),
            text(Some(&name)),
            text(Some(group)),
            text(mobile.as_deref()),
            text(Some(currency)),
            text(Some(pay)),
            text(address.as_deref()),
            text(tax.as_deref()),
            CalcValue::Bool(blocked),
            text(bank.as_deref()),
            CalcValue::Number(limit.parse().unwrap()),
            CalcValue::Number(since_serial),
            text(Some("seed")),
            CalcValue::Number(46325.0 + (8.0 * 3600.0 + 12.0 * 60.0) / 86400.0),
        ];
        out.push(Record {
            id,
            key,
            wire,
            cells,
        });
    }
    out
}

pub fn wire_record(record: &Record) -> String {
    let values: Vec<String> = FIELDS
        .iter()
        .zip(&record.wire)
        .map(|((name, _, _, _), value)| format!("{}:{value}", quoted(name)))
        .collect();
    format!(
        r#"{{"id":{},"key":{},"version":"AAAAAAAAB9E=","values":{{{}}}}}"#,
        record.id,
        quoted(&record.key),
        values.join(",")
    )
}

pub fn pages(records: &[Record], size: usize, total: usize) -> Vec<Vec<u8>> {
    let chunks: Vec<&[Record]> = records.chunks(size.max(1)).collect();
    if chunks.is_empty() {
        return vec![format!(
            r#"{{"records":[],"nextAfterId":null,"total":{total},"asOf":"2026-11-09T10:00:05Z"}}"#
        )
        .into_bytes()];
    }
    chunks
        .iter()
        .enumerate()
        .map(|(i, chunk)| {
            let next = if i + 1 < chunks.len() {
                chunk.last().unwrap().id.to_string()
            } else {
                "null".to_string()
            };
            let list: Vec<String> = chunk.iter().map(wire_record).collect();
            format!(
                r#"{{"records":[{}],"nextAfterId":{next},"total":{total},"asOf":"2026-11-09T10:00:05Z"}}"#,
                list.join(",")
            )
            .into_bytes()
        })
        .collect()
}

pub fn blank() -> Engine {
    let mut engine = Engine::new();
    engine
        .open(Rc::new(cgs_xlsx::blank::blank_workbook().unwrap()))
        .unwrap();
    while engine.load_step(1000.0).unwrap().phase == LoadPhase::Reading {}
    engine
}

pub fn bound(records: &[Record]) -> Engine {
    let mut engine = blank();
    engine.erp_begin(&schema(10_000)).unwrap();
    for page in pages(records, 1000, records.len()) {
        engine.erp_page(&page).unwrap();
    }
    engine.erp_bind(SESSION).unwrap();
    settle(&mut engine);
    engine
}

pub fn settle(engine: &mut Engine) {
    while engine.calc_step(1000, &[]).unwrap() {}
    engine.take_dirty();
}

#[derive(Default)]
pub struct Grid(pub HashMap<(u32, u32), CalcValue>);

impl CellSink for Grid {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.insert((row, col), value.clone());
    }
}

pub fn grid(engine: &Engine, top: u32, bottom: u32) -> Grid {
    let mut grid = Grid::default();
    engine.read_cells(
        SheetRect {
            sheet: 0,
            rect: Rect {
                top,
                left: 1,
                bottom,
                right: FIELDS.len() as u32 + 2,
            },
        },
        &mut grid,
    );
    grid
}

pub fn cell(grid: &Grid, row: u32, col: u32) -> CalcValue {
    grid.0.get(&(row, col)).cloned().unwrap_or(CalcValue::Empty)
}

pub fn binding(engine: &Engine) -> serde_json::Value {
    serde_json::from_str(&engine.binding_json()).unwrap()
}

pub fn query(engine: &Engine, json: &str) -> serde_json::Value {
    serde_json::from_str(&engine.query(json).unwrap()).unwrap()
}

/// Every data row's key is there once and holds its own record's read-only values, and the
/// binding still knows each key's id. With `all`, the editable values too, as nothing edited them.
pub fn assert_keys_with(engine: &Engine, records: &[Record], all: bool) {
    let summary = binding(engine);
    let (top, bottom) = (
        summary["top"].as_u64().unwrap() as u32,
        summary["bottom"].as_u64().unwrap() as u32,
    );
    assert_eq!(
        (bottom - top) as usize,
        records.len(),
        "the table keeps its rows"
    );
    let by_key: HashMap<&str, &Record> = records.iter().map(|r| (r.key.as_str(), r)).collect();
    let cells = grid(engine, top + 1, bottom);
    let mut seen = std::collections::HashSet::new();
    for row in top + 1..=bottom {
        let CalcValue::Text(key) = cell(&cells, row, 1) else {
            panic!("row {row} has no key");
        };
        assert!(seen.insert(key.clone()), "{key} twice");
        let record = by_key[key.as_str()];
        for (index, expected) in record.cells.iter().enumerate() {
            if !all && FIELDS[index].3 {
                continue;
            }
            assert_eq!(
                &cell(&cells, row, index as u32 + 1),
                expected,
                "{key}'s {}",
                FIELDS[index].0
            );
        }
        assert_eq!(engine.bound_record(&key).unwrap().0, record.id);
    }
}

pub fn assert_keys_with_records(engine: &Engine, records: &[Record]) {
    assert_keys_with(engine, records, true);
}
