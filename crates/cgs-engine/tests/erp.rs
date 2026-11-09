//! ERP-bound tables through the engine (Phase 15): typed binding, the identity guard on every
//! refused path, refresh by id, and a property test over random sorts, filters, undo and redo
//! that every key stays with its own record's values.

use std::collections::HashMap;
use std::rc::Rc;

use cgs_calc::{CalcValue, CellSink, Rect, SheetRect};
use cgs_engine::{Engine, LoadPhase};

const READ_ONLY: &str = "This column is read-only in Cash Gear ERP.";
const ROWS: &str = "Rows can't be added or deleted in ERP data.";
const COLUMNS: &str = "Columns can't be added or deleted in ERP data.";
const TABLE: &str = "The table that holds ERP data can't be resized, renamed or reshaped.";
const SIDEWAYS: &str = "ERP data can only be sorted top to bottom.";
const WHOLE: &str = "ERP data is sorted as a whole table. Select a cell in it and sort again.";
const SHEET: &str = "A sheet that holds ERP data can't be deleted.";

/// mulberry32, so every run sees the same records and steps.
struct Random(u32);

impl Random {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() * n as f64) as usize % n.max(1)
    }
}

const FIELDS: [(&str, &str, &str); 14] = [
    (
        "vendorAccount",
        "Account",
        r#""type":"text","maxLength":50"#,
    ),
    ("vendorName", "Name", r#""type":"text","maxLength":150"#),
    (
        "vendorGroup",
        "Group",
        r#""type":"lookup","lookup":"vendorGroups""#,
    ),
    ("mobile", "Mobile", r#""type":"text","maxLength":50"#),
    (
        "currency",
        "Currency",
        r#""type":"lookup","lookup":"currencies""#,
    ),
    (
        "payMethod",
        "Payment method",
        r#""type":"lookup","lookup":"paymentModes""#,
    ),
    ("address", "Address", r#""type":"text""#),
    ("taxFile", "Tax file", r#""type":"text""#),
    ("blocked", "Blocked", r#""type":"boolean""#),
    ("bankAccount", "Bank account", r#""type":"text""#),
    (
        "debitLimit",
        "Credit limit",
        r#""type":"decimal","scale":2"#,
    ),
    ("since", "Since", r#""type":"date""#),
    ("modifiedBy", "Modified by", r#""type":"text""#),
    ("modifiedDate", "Modified", r#""type":"dateTime""#),
];

fn schema(max_records: u32) -> String {
    let fields: Vec<String> = FIELDS
        .iter()
        .map(|(name, label, kind)| {
            let editable = matches!(*name, "vendorName" | "mobile");
            let reason = if editable { "" } else { r#","editReason":"notEditable""# };
            format!(
                r#"{{"name":"{name}","label":{{"en":"{label}","ar":"{label} ع"}},{kind},"editable":{editable}{reason}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"entity":"vendors","schemaVersion":"vendors.1","label":{{"en":"Vendors","ar":"الموردون"}},
        "keyField":"vendorAccount","idField":"recId","maxPageSize":1000,"maxRecords":{max_records},"maxCommitRecords":500,
        "fields":[{}],
        "lookups":{{"vendorGroups":[{{"code":"LOCAL","label":{{"en":"Local","ar":"محلي"}}}},{{"code":"IMPORT","label":{{"en":"Import","ar":"Import"}}}},{{"code":"SERVICES","label":{{"en":"Services","ar":"خدمات"}}}}],
          "paymentModes":[{{"code":"CASH","label":{{"en":"Cash","ar":"نقدي"}}}},{{"code":"NET30","label":{{"en":"30 days","ar":"30 days"}}}}],
          "currencies":[{{"code":"EGP","label":{{"en":"EGP","ar":"EGP"}}}},{{"code":"USD","label":{{"en":"USD","ar":"USD"}}}}]}}}}"#,
        fields.join(",")
    )
}

const SESSION: &str = r#"{"company":"Kit","userId":"u17","userName":"Sara","language":"en","entity":"vendors","entityRoute":"api/payable/VendorSheet","filter":null,"idleExpiresAt":"x"}"#;

/// One record as the ERP sends it, and what its cells must hold.
#[derive(Clone)]
struct Record {
    id: i64,
    key: String,
    wire: Vec<String>,
    cells: Vec<CalcValue>,
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

fn records(n: usize, seed: u32) -> Vec<Record> {
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
        let cents = pick.below(5_000_000);
        let limit = format!("{}.{:02}", cents / 100, cents % 100);
        let day = 1 + pick.below(28);
        let since = format!("2024-{:02}-{day:02}", 1 + pick.below(12));
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
            "null".to_string(),
            quoted(&limit),
            quoted(&since),
            quoted("seed"),
            quoted("2026-10-30T08:12:00+02:00"),
        ];
        let text = |t: Option<&str>| t.map_or(CalcValue::Empty, |t| CalcValue::Text(t.to_string()));
        let since_serial = {
            // Days from 2024-01-01 (serial 45292), months of 2024, a leap year.
            let month: usize = since[5..7].parse().unwrap();
            let lengths = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            45292.0 + lengths[..month - 1].iter().sum::<u32>() as f64 + day as f64 - 1.0
        };
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
            CalcValue::Empty,
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

fn wire_record(record: &Record) -> String {
    let values: Vec<String> = FIELDS
        .iter()
        .zip(&record.wire)
        .map(|((name, _, _), value)| format!("{}:{value}", quoted(name)))
        .collect();
    format!(
        r#"{{"id":{},"key":{},"version":"AAAAAAAAB9E=","values":{{{}}}}}"#,
        record.id,
        quoted(&record.key),
        values.join(",")
    )
}

fn pages(records: &[Record], size: usize, total: usize) -> Vec<Vec<u8>> {
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

fn blank() -> Engine {
    let mut engine = Engine::new();
    engine
        .open(Rc::new(cgs_xlsx::blank::blank_workbook().unwrap()))
        .unwrap();
    while engine.load_step(1000.0).unwrap().phase == LoadPhase::Reading {}
    engine
}

fn bound(records: &[Record]) -> Engine {
    let mut engine = blank();
    engine.erp_begin(&schema(10_000)).unwrap();
    for page in pages(records, 1000, records.len()) {
        engine.erp_page(&page).unwrap();
    }
    engine.erp_bind(SESSION).unwrap();
    settle(&mut engine);
    engine
}

fn settle(engine: &mut Engine) {
    while engine.calc_step(1000, &[]).unwrap() {}
    engine.take_dirty();
}

#[derive(Default)]
struct Grid(HashMap<(u32, u32), CalcValue>);

impl CellSink for Grid {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.insert((row, col), value.clone());
    }
}

fn grid(engine: &Engine, top: u32, bottom: u32) -> Grid {
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

fn cell(grid: &Grid, row: u32, col: u32) -> CalcValue {
    grid.0.get(&(row, col)).cloned().unwrap_or(CalcValue::Empty)
}

fn binding(engine: &Engine) -> serde_json::Value {
    serde_json::from_str(&engine.binding_json()).unwrap()
}

/// Every data row's key holds exactly its own record's values, each key once, and the binding
/// still knows each key's id.
fn assert_keys_with_records(engine: &Engine, records: &[Record]) {
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

#[test]
fn records_bind_as_a_typed_table_in_a_new_workbook() {
    let records = records(25, 7);
    let engine = bound(&records);
    let summary = binding(&engine);
    assert_eq!(summary["table"], "Vendors");
    assert_eq!(summary["records"], 25);
    assert_eq!(summary["company"], "Kit");
    assert_eq!(
        (summary["top"].as_u64(), summary["bottom"].as_u64()),
        (Some(1), Some(26))
    );
    assert_eq!(summary["right"], FIELDS.len());
    let cells = grid(&engine, 1, 1);
    for (index, (_, label, _)) in FIELDS.iter().enumerate() {
        assert_eq!(
            cell(&cells, 1, index as u32 + 1),
            CalcValue::Text(label.to_string())
        );
    }
    assert_keys_with_records(&engine, &records);
    let book = engine.book().unwrap();
    assert_eq!(book.sheets[0].name, "Vendors");
    assert_eq!(book.tables.len(), 1);
    assert!(book.tables[0].filter.is_some(), "filter buttons on");
    assert_eq!(
        book.sheets[0].view.freeze.map(|f| (f.rows, f.cols)),
        Some((1, 0))
    );
    assert!(
        engine.undo_state().undo.is_none(),
        "binding leaves nothing to undo"
    );
    // Text columns are text, so leading zeros never go if they are typed again.
    let format = |col: u32| -> serde_json::Value {
        serde_json::from_str(&engine.cell_format_json(0, 2, col)).unwrap()
    };
    assert_eq!(format(4)["numberFormat"], "@");
    assert_eq!(format(11)["numberFormat"], "#,##0.00");
    assert_eq!(format(12)["numberFormat"], "yyyy-mm-dd");
}

#[test]
fn headings_follow_the_sessions_language_and_ids_stay_out_of_cells() {
    let records = records(3, 9);
    let mut engine = blank();
    engine.erp_begin(&schema(10_000)).unwrap();
    engine.erp_page(&pages(&records, 10, 3)[0]).unwrap();
    engine
        .erp_bind(&SESSION.replace(r#""language":"en""#, r#""language":"ar""#))
        .unwrap();
    let cells = grid(&engine, 1, 4);
    assert_eq!(cell(&cells, 1, 2), CalcValue::Text("Name ع".into()));
    assert_eq!(engine.book().unwrap().sheets[0].name, "الموردون");
    for value in cells.0.values() {
        if let CalcValue::Number(n) = value {
            assert!(*n < 900_000.0, "an id in a cell: {n}");
        }
        if let CalcValue::Text(text) = value {
            assert!(!text.contains("AAAAAAAAB9E="), "a version in a cell");
        }
    }
}

#[test]
fn a_list_past_the_cap_and_a_workbook_with_cells_are_refused() {
    let records = records(5, 3);
    let mut engine = blank();
    engine.erp_begin(&schema(4)).unwrap();
    let problem = engine.erp_page(&pages(&records, 10, 5)[0]).unwrap_err();
    assert!(
        problem.starts_with("This list has 5 vendors, and a sheet opens at most 4."),
        "{problem}"
    );
    assert!(
        engine.erp_bind(SESSION).is_err(),
        "nothing staged after a refusal"
    );

    let mut engine = blank();
    engine
        .apply_json(r#"{"kind":"input","sheet":0,"row":1,"col":1,"text":"mine"}"#)
        .unwrap();
    engine.erp_begin(&schema(10)).unwrap();
    engine.erp_page(&pages(&records, 10, 5)[0]).unwrap();
    assert_eq!(
        engine.erp_bind(SESSION).unwrap_err(),
        "ERP data opens in a new workbook."
    );

    let mut engine = blank();
    engine.erp_begin(&schema(10)).unwrap();
    engine.erp_page(&pages(&[], 10, 0)[0]).unwrap();
    assert_eq!(
        engine.erp_bind(SESSION).unwrap_err(),
        "No vendors match this list in Cash Gear ERP."
    );
}

/// Applies a command that must be refused with `message`, and checks nothing changed.
fn refused(engine: &mut Engine, records: &[Record], json: &str, message: &str) {
    let undo_before = engine.undo_state().undo;
    let answer = engine.apply_json(json);
    assert_eq!(
        answer.as_ref().err().map(String::as_str),
        Some(message),
        "{json}"
    );
    settle(engine);
    assert_eq!(engine.undo_state().undo, undo_before, "{json} left a step");
    assert_keys_with_records(engine, records);
}

#[test]
fn every_path_that_would_break_a_record_is_refused() {
    let records = records(12, 11);
    let mut engine = bound(&records);
    let data = r#"{"top":2,"left":1,"bottom":13,"right":14}"#;
    for json in [
        r#"{"kind":"input","sheet":0,"row":3,"col":1,"text":"V-99999"}"#.to_string(),
        r#"{"kind":"input","sheet":0,"row":3,"col":2,"text":"Renamed"}"#.to_string(),
        r#"{"kind":"input","sheet":0,"row":1,"col":2,"text":"Heading"}"#.to_string(),
        r#"{"kind":"input","sheet":0,"row":20,"col":20,"text":"x","areas":[{"top":5,"left":5,"bottom":6,"right":6}]}"#.to_string(),
        r#"{"kind":"arrayInput","sheet":0,"area":{"top":2,"left":2,"bottom":3,"right":2},"text":"=1"}"#.to_string(),
        r#"{"kind":"clear","sheet":0,"areas":[{"top":4,"left":3,"bottom":4,"right":3}]}"#.to_string(),
        r#"{"kind":"fill","sheet":0,"from":{"top":2,"left":1,"bottom":2,"right":14},"to":{"top":2,"left":1,"bottom":5,"right":14}}"#.to_string(),
        r#"{"kind":"paste","fromSheet":0,"from":{"top":2,"left":1,"bottom":2,"right":14},"sheet":0,"to":{"top":5,"left":1,"bottom":5,"right":14}}"#.to_string(),
        r#"{"kind":"paste","fromSheet":0,"from":{"top":30,"left":1,"bottom":30,"right":1},"sheet":0,"to":{"top":5,"left":1,"bottom":5,"right":1},"mode":"values"}"#.to_string(),
        r#"{"kind":"move","fromSheet":0,"from":{"top":2,"left":1,"bottom":3,"right":14},"sheet":0,"row":40,"col":1}"#.to_string(),
        r#"{"kind":"move","fromSheet":0,"from":{"top":40,"left":1,"bottom":40,"right":1},"sheet":0,"row":2,"col":1}"#.to_string(),
        r#"{"kind":"pasteExternal","sheet":0,"row":4,"col":1,"text":"a\tb"}"#.to_string(),
        r#"{"kind":"replaceOne","query":{"text":"V-","sheet":0},"with":"W-","sheet":0,"row":2,"col":1}"#.to_string(),
        r#"{"kind":"convertToNumber","sheet":0,"areas":[{"top":2,"left":4,"bottom":13,"right":4}]}"#.to_string(),
    ] {
        refused(&mut engine, &records, &json, READ_ONLY);
    }
    // Replace All passes the command check and meets the content rule instead.
    refused(
        &mut engine,
        &records,
        r#"{"kind":"replaceAll","query":{"text":"V-0","sheet":0},"with":"W-0"}"#,
        READ_ONLY,
    );
    for (json, message) in [
        (
            r#"{"kind":"axis","sheet":0,"axis":"row","at":5,"count":1,"delete":false}"#,
            ROWS,
        ),
        (
            r#"{"kind":"axis","sheet":0,"axis":"row","at":13,"count":2,"delete":true}"#,
            ROWS,
        ),
        (
            r#"{"kind":"axis","sheet":0,"axis":"row","at":1,"count":1,"delete":true}"#,
            ROWS,
        ),
        (
            r#"{"kind":"axis","sheet":0,"axis":"col","at":3,"count":1,"delete":false}"#,
            COLUMNS,
        ),
        (
            r#"{"kind":"axis","sheet":0,"axis":"col","at":14,"count":1,"delete":true}"#,
            COLUMNS,
        ),
        (
            r#"{"kind":"shiftCells","sheet":0,"area":{"top":3,"left":2,"bottom":3,"right":2},"down":true}"#,
            TABLE,
        ),
        (
            r#"{"kind":"shiftCells","sheet":0,"area":{"top":1,"left":20,"bottom":1,"right":20},"down":false,"delete":true}"#,
            "pass",
        ),
        (
            r#"{"kind":"merge","sheet":0,"areas":[{"top":2,"left":2,"bottom":2,"right":3}]}"#,
            TABLE,
        ),
        (
            r#"{"kind":"tableOptions","table":"Vendors","headerRow":false}"#,
            TABLE,
        ),
        (
            r#"{"kind":"tableOptions","table":"Vendors","totalsRow":true}"#,
            TABLE,
        ),
        (
            r#"{"kind":"totalsFunction","table":"Vendors","column":"Credit limit","function":"sum"}"#,
            TABLE,
        ),
        (
            r#"{"kind":"renameTable","table":"Vendors","name":"Other"}"#,
            TABLE,
        ),
        (
            r#"{"kind":"resizeTable","table":"Vendors","area":{"top":1,"left":1,"bottom":14,"right":14}}"#,
            TABLE,
        ),
        (r#"{"kind":"convertToRange","table":"Vendors"}"#, TABLE),
        (
            r#"{"kind":"createTable","sheet":0,"area":{"top":2,"left":2,"bottom":4,"right":3}}"#,
            TABLE,
        ),
        (r#"{"kind":"deleteSheet","sheet":0}"#, SHEET),
        (
            r#"{"kind":"removeDuplicates","sheet":0,"area":{"top":1,"left":1,"bottom":13,"right":14},"columns":[3],"header":true}"#,
            ROWS,
        ),
    ] {
        if message == "pass" {
            // Cells shifting to the right of the table move nothing of it.
            engine.apply_json(json).unwrap();
            settle(&mut engine);
            engine.apply_json(r#"{"kind":"undo"}"#).unwrap();
            settle(&mut engine);
            continue;
        }
        refused(&mut engine, &records, json, message);
    }
    for (json, message) in [
        (format!(r#"{{"kind":"sort","sheet":0,"area":{data},"keys":[{{"index":2}}],"byColumns":true}}"#), SIDEWAYS),
        (r#"{"kind":"sort","sheet":0,"area":{"top":2,"left":1,"bottom":13,"right":3},"keys":[{"index":2}]}"#.to_string(), WHOLE),
        (r#"{"kind":"sort","sheet":0,"area":{"top":5,"left":1,"bottom":13,"right":14},"keys":[{"index":2}]}"#.to_string(), WHOLE),
        (r#"{"kind":"sort","sheet":0,"area":{"top":1,"left":1,"bottom":13,"right":14},"keys":[{"index":2}]}"#.to_string(), WHOLE),
    ] {
        refused(&mut engine, &records, &json, message);
    }
    assert!(engine.undo_state().undo.is_none());
}

/// The commands refused before they run, which the guards inside the engine must refuse too.
const GUARDED: [&str; 26] = [
    r#"{"kind":"input","sheet":0,"row":3,"col":1,"text":"V-99999"}"#,
    r#"{"kind":"input","sheet":0,"row":3,"col":2,"text":"Renamed"}"#,
    r#"{"kind":"input","sheet":0,"row":1,"col":2,"text":"Heading"}"#,
    r#"{"kind":"input","sheet":0,"row":20,"col":20,"text":"x","areas":[{"top":5,"left":5,"bottom":6,"right":6}]}"#,
    r#"{"kind":"arrayInput","sheet":0,"area":{"top":2,"left":2,"bottom":3,"right":2},"text":"=1"}"#,
    r#"{"kind":"clear","sheet":0,"areas":[{"top":4,"left":3,"bottom":4,"right":3}]}"#,
    r#"{"kind":"fill","sheet":0,"from":{"top":2,"left":1,"bottom":2,"right":14},"to":{"top":2,"left":1,"bottom":5,"right":14}}"#,
    r#"{"kind":"paste","fromSheet":0,"from":{"top":2,"left":1,"bottom":2,"right":14},"sheet":0,"to":{"top":5,"left":1,"bottom":5,"right":14}}"#,
    r#"{"kind":"move","fromSheet":0,"from":{"top":2,"left":1,"bottom":3,"right":14},"sheet":0,"row":40,"col":1}"#,
    r#"{"kind":"move","fromSheet":0,"from":{"top":40,"left":1,"bottom":40,"right":1},"sheet":0,"row":2,"col":1}"#,
    r#"{"kind":"pasteExternal","sheet":0,"row":4,"col":1,"text":"a\tb"}"#,
    r#"{"kind":"replaceOne","query":{"text":"V-","sheet":0},"with":"W-","sheet":0,"row":2,"col":1}"#,
    r#"{"kind":"convertToNumber","sheet":0,"areas":[{"top":2,"left":4,"bottom":13,"right":4}]}"#,
    r#"{"kind":"axis","sheet":0,"axis":"row","at":5,"count":1,"delete":false}"#,
    r#"{"kind":"axis","sheet":0,"axis":"row","at":13,"count":2,"delete":true}"#,
    r#"{"kind":"axis","sheet":0,"axis":"col","at":3,"count":1,"delete":false}"#,
    r#"{"kind":"axis","sheet":0,"axis":"col","at":14,"count":1,"delete":true}"#,
    r#"{"kind":"shiftCells","sheet":0,"area":{"top":3,"left":2,"bottom":3,"right":2},"down":true}"#,
    r#"{"kind":"tableOptions","table":"Vendors","headerRow":false}"#,
    r#"{"kind":"renameTable","table":"Vendors","name":"Other"}"#,
    r#"{"kind":"resizeTable","table":"Vendors","area":{"top":1,"left":1,"bottom":14,"right":14}}"#,
    r#"{"kind":"convertToRange","table":"Vendors"}"#,
    r#"{"kind":"deleteSheet","sheet":0}"#,
    r#"{"kind":"removeDuplicates","sheet":0,"area":{"top":1,"left":1,"bottom":13,"right":14},"columns":[3],"header":true}"#,
    r#"{"kind":"sort","sheet":0,"area":{"top":2,"left":1,"bottom":13,"right":14},"keys":[{"index":2}],"byColumns":true}"#,
    r#"{"kind":"sort","sheet":0,"area":{"top":2,"left":1,"bottom":13,"right":3},"keys":[{"index":2}]}"#,
];

#[test]
fn the_guards_inside_the_engine_refuse_on_their_own() {
    let records = records(12, 11);
    let mut engine = bound(&records);
    // A sheet to delete instead, so deleting the bound one isn't refused for being the last.
    engine
        .apply_json(r#"{"kind":"insertSheet","position":1}"#)
        .unwrap();
    engine.set_erp_precheck(false);
    for json in GUARDED {
        let undo_before = engine.undo_state().undo;
        let answer = engine.apply_json(json);
        // Refused by a guard, or by a rule the engine already had, such as arrays in tables.
        assert!(answer.is_err(), "{json} was let through");
        settle(&mut engine);
        assert_eq!(engine.undo_state().undo, undo_before, "{json} left a step");
        assert_keys_with_records(&engine, &records);
    }
}

#[test]
fn formatting_moving_the_table_and_typing_beside_it_are_allowed() {
    let records = records(12, 13);
    let mut engine = bound(&records);
    for json in [
        r#"{"kind":"format","sheet":0,"areas":[{"top":2,"left":1,"bottom":13,"right":14}],"change":{"bold":true}}"#,
        r#"{"kind":"paste","fromSheet":0,"from":{"top":2,"left":2,"bottom":2,"right":2},"sheet":0,"to":{"top":5,"left":2,"bottom":5,"right":2},"mode":"formats"}"#,
        r#"{"kind":"tableOptions","table":"Vendors","rowStripes":false,"style":"TableStyleLight9"}"#,
        r#"{"kind":"input","sheet":0,"row":14,"col":1,"text":"below"}"#,
        r#"{"kind":"input","sheet":0,"row":2,"col":15,"text":"beside"}"#,
        r#"{"kind":"input","sheet":0,"row":3,"col":15,"text":"=SUM(1,2)"}"#,
        r#"{"kind":"setNote","sheet":0,"row":3,"col":2,"text":"A note"}"#,
        r#"{"kind":"axis","sheet":0,"axis":"row","at":1,"count":2,"delete":false}"#,
        r#"{"kind":"axis","sheet":0,"axis":"col","at":1,"count":1,"delete":false}"#,
    ] {
        engine
            .apply_json(json)
            .unwrap_or_else(|e| panic!("{json}: {e}"));
        settle(&mut engine);
    }
    let summary = binding(&engine);
    assert_eq!(
        (
            summary["top"].as_u64(),
            summary["left"].as_u64(),
            summary["bottom"].as_u64()
        ),
        (Some(3), Some(2), Some(15)),
        "the table moved whole and didn't grow"
    );
    // Moved, the table still sorts and keeps its records.
    engine
        .apply_json(r#"{"kind":"sort","sheet":0,"area":{"top":4,"left":2,"bottom":15,"right":15},"keys":[{"index":3,"descending":true}]}"#)
        .unwrap();
    settle(&mut engine);
    for _ in 0..12 {
        engine.apply_json(r#"{"kind":"undo"}"#).unwrap();
    }
    settle(&mut engine);
    assert_keys_with_records(&engine, &records);
}

#[test]
fn a_refresh_replaces_records_by_id_wherever_a_sort_moved_them() {
    let mut records = records(30, 17);
    let mut engine = bound(&records);
    engine
        .apply_json(r#"{"kind":"sort","sheet":0,"area":{"top":2,"left":1,"bottom":31,"right":14},"keys":[{"index":2,"descending":true}]}"#)
        .unwrap();
    settle(&mut engine);
    assert!(engine.undo_state().undo.is_some());
    // The ERP changed one name, renamed one key and lost one record.
    records[4].wire[1] = quoted("Renamed in the ERP");
    records[4].cells[1] = CalcValue::Text("Renamed in the ERP".into());
    records[9].key = "V-RENAMED".into();
    records[9].wire[0] = quoted("V-RENAMED");
    records[9].cells[0] = CalcValue::Text("V-RENAMED".into());
    let gone = records.remove(20);
    let ids: Vec<f64> = engine.binding_ids();
    assert_eq!(ids.len(), 30);
    assert_eq!(ids[0], 900_001.0);
    engine.erp_begin_refresh().unwrap();
    for page in pages(&records, 7, records.len()) {
        engine.erp_page(&page).unwrap();
    }
    let summary: serde_json::Value = serde_json::from_str(&engine.erp_refresh().unwrap()).unwrap();
    settle(&mut engine);
    assert_eq!(summary["updated"], 29);
    assert_eq!(summary["changed"], 2);
    assert_eq!(summary["renamed"], 1);
    assert_eq!(summary["missing"], 1);
    assert!(engine.undo_state().undo.is_none(), "a refresh clears undo");
    assert_eq!(binding(&engine)["missing"], 1);
    // The record the ERP lost stays in its row, as it was.
    let mut kept = records.clone();
    kept.push(gone);
    assert_keys_with_records(&engine, &kept);
}

#[test]
fn a_saved_workbook_holds_no_ids_versions_or_connection() {
    let records = records(40, 19);
    let mut engine = bound(&records);
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(out)).unwrap();
    let mut everything = String::new();
    for i in 0..zip.len() {
        let mut part = zip.by_index(i).unwrap();
        let name = part.name().unwrap().to_string();
        assert!(!name.to_lowercase().contains("customxml"), "{name}");
        let mut text = String::new();
        if std::io::Read::read_to_string(&mut part, &mut text).is_ok() {
            everything.push_str(&text);
        }
    }
    assert!(
        everything.contains("V-00001"),
        "the keys are ordinary cells"
    );
    for secret in [
        "900001",
        "900004",
        "AAAAAAAAB9E=",
        "recId",
        "api/payable",
        "u17",
    ] {
        assert!(!everything.contains(secret), "the file holds {secret}");
    }
}

#[test]
fn random_sorts_filters_undo_and_redo_keep_every_key_with_its_record() {
    let records = records(1000, 20261109);
    let mut engine = bound(&records);
    let mut pick = Random(42);
    let data = r#"{"top":2,"left":1,"bottom":1001,"right":14}"#;
    let mut sorts = 0;
    let mut undos = 0;
    let mut refusals = 0;
    for step in 0..120 {
        let roll = pick.below(10);
        let json = match roll {
            0..=3 => {
                sorts += 1;
                let mut keys = Vec::new();
                for _ in 0..1 + pick.below(3) {
                    keys.push(format!(
                        r#"{{"index":{},"descending":{}}}"#,
                        1 + pick.below(14),
                        pick.next() < 0.5
                    ));
                }
                format!(
                    r#"{{"kind":"sort","sheet":0,"area":{data},"keys":[{}]}}"#,
                    keys.join(",")
                )
            }
            4 => {
                let groups = ["LOCAL", "IMPORT", "SERVICES"];
                let kept: Vec<String> = groups
                    .iter()
                    .filter(|_| pick.next() < 0.6)
                    .map(|g| quoted(g))
                    .collect();
                format!(
                    r#"{{"kind":"setFilter","sheet":0,"table":"Vendors","col":3,"criteria":{{"kind":"values","values":[{}]}}}}"#,
                    kept.join(",")
                )
            }
            5 => r#"{"kind":"clearFilter","sheet":0,"table":"Vendors"}"#.to_string(),
            6 | 7 => {
                undos += 1;
                r#"{"kind":"undo"}"#.to_string()
            }
            8 => r#"{"kind":"redo"}"#.to_string(),
            _ => {
                refusals += 1;
                let row = 2 + pick.below(1000);
                [
                    format!(r#"{{"kind":"input","sheet":0,"row":{row},"col":1,"text":"X"}}"#),
                    format!(r#"{{"kind":"axis","sheet":0,"axis":"row","at":{row},"count":1,"delete":true}}"#),
                    format!(r#"{{"kind":"fill","sheet":0,"from":{{"top":{row},"left":1,"bottom":{row},"right":14}},"to":{{"top":{row},"left":1,"bottom":{},"right":14}}}}"#, (row + 3).min(1001)),
                ][pick.below(3)]
                .clone()
            }
        };
        let answer = engine.apply_json(&json);
        if roll == 9 {
            assert!(answer.is_err(), "step {step}: {json} was let through");
        } else {
            answer.unwrap_or_else(|e| panic!("step {step}: {json}: {e}"));
        }
        settle(&mut engine);
        assert_keys_with_records(&engine, &records);
    }
    assert!(
        sorts > 30 && undos > 10 && refusals > 5,
        "{sorts} {undos} {refusals}"
    );
}

/// Where binding's time goes, natively: `cargo test --release -p cgs-engine --test erp
/// bind_steps -- --ignored --nocapture`.
#[test]
#[ignore]
fn bind_steps() {
    for n in [1000usize, 10_000] {
        let records = records(n, 5);
        let mut engine = blank();
        let started = std::time::Instant::now();
        engine.erp_begin(&schema(10_000)).unwrap();
        for page in pages(&records, 1000, n) {
            engine.erp_page(&page).unwrap();
        }
        let staged = started.elapsed();
        engine.erp_bind(SESSION).unwrap();
        let bound = started.elapsed();
        let fits = engine.col_fits(0, 1, FIELDS.len() as u32);
        let fitted = started.elapsed();
        settle(&mut engine);
        eprintln!(
            "{n}: stage {:?}, bind {:?}, col fits {:?} ({} bytes), settle {:?}",
            staged,
            bound - staged,
            fitted - bound,
            fits.len(),
            started.elapsed() - fitted
        );
    }
}
