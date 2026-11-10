//! Validating and committing ERP data through the engine (Phase 17): what Cash Gear ERP answers
//! goes on cells by id and field and stays only while the cell holds what was sent; conflicts mark
//! their records until they have no changes; a commit holds every write to the table while it is
//! in flight; its result is written only where cells still hold what was sent, moves versions and
//! originals, shows Committed and clears undo; changes in hidden rows are flagged, and left out of
//! Commit Changes in Visible Rows; the Review dialog lists them all.

mod erp_harness;

use cgs_calc::CalcValue;
use cgs_engine::tile::Screen;
use cgs_engine::view_block::{
    CELL_BYTES, CELL_ERP_COMMITTED, CELL_ERP_CONFLICT, CELL_ERP_INVALID, CELL_ERP_MODIFIED,
    HEADER_BYTES,
};
use cgs_engine::Engine;
use erp_harness::*;
use serde_json::{json, Value};

const FROZEN: &str = "Wait until the commit to Cash Gear ERP finishes.";

/// Columns, counted from 1.
const NAME: u32 = 2;
const MOBILE: u32 = 4;
const PAY: u32 = 6;
const ADDRESS: u32 = 7;
const MODIFIED_BY: u32 = 13;

fn ok(engine: &mut Engine, json: &str) {
    engine
        .apply_json(json)
        .unwrap_or_else(|error| panic!("{json}: {error}"));
    settle(engine);
}

fn typed(engine: &mut Engine, row: u32, col: u32, text: &str) {
    ok(
        engine,
        &json!({"kind": "input", "sheet": 0, "row": row, "col": col, "text": text}).to_string(),
    );
}

fn value(engine: &Engine, row: u32, col: u32) -> CalcValue {
    cell(&grid(engine, row, row), row, col)
}

fn text(s: &str) -> CalcValue {
    CalcValue::Text(s.to_string())
}

fn counts(engine: &Engine) -> Value {
    binding(engine)["counts"].clone()
}

fn changes(engine: &Engine) -> Value {
    query(engine, r#"{"kind":"erpChanges"}"#)
}

/// The changeset's records as a commit sends them: without the rows they are on.
fn sent(changeset: &Value) -> Value {
    Value::Array(
        changeset["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|record| {
                let mut record = record.clone();
                let object = record.as_object_mut().unwrap();
                object.remove("row");
                object.remove("hidden");
                record
            })
            .collect(),
    )
}

/// A record as the ERP persisted it: its values as sent at binding, with `changes` (field index,
/// JSON literal) in, under a new version.
fn persisted(record: &Record, changes: &[(usize, &str)], version: &str) -> Value {
    let mut wire = record.wire.clone();
    for (field, literal) in changes {
        wire[*field] = literal.to_string();
    }
    let values: serde_json::Map<String, Value> = FIELDS
        .iter()
        .zip(&wire)
        .map(|((name, _, _, _), literal)| {
            (name.to_string(), serde_json::from_str(literal).unwrap())
        })
        .collect();
    json!({"id": record.id, "key": record.key, "version": version, "values": values})
}

/// A tile's cell flags, read from its bytes.
fn flags(engine: &mut Engine, row: u32, col: u32) -> u16 {
    let u16_at = |bytes: &[u8], at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let screen = Screen {
        dark: false,
        background: 0xffffff,
        text: 0x000000,
    };
    let bytes = engine.view_block(0, 0, 0, screen).into_bytes();
    let rows = u16_at(&bytes, 6) as usize;
    let cols = u16_at(&bytes, 8) as usize;
    let sizes = HEADER_BYTES + (rows + cols) * 2;
    let cells = sizes + (4 - sizes % 4) % 4;
    let index = (row as usize - 1) * cols + col as usize - 1;
    u16_at(&bytes, cells + index * CELL_BYTES + 18)
}

fn cell_info(engine: &Engine, row: u32, col: u32) -> Value {
    query(
        engine,
        &json!({"kind": "erpCell", "sheet": 0, "row": row, "col": col}).to_string(),
    )
}

#[test]
fn the_erps_errors_go_on_cells_by_id_and_field_and_stay_while_they_hold_what_was_sent() {
    let records = records(20, 41);
    let mut engine = bound(&records);
    typed(&mut engine, 2, NAME, "Nile Two");
    typed(&mut engine, 2, PAY, "NET30");
    typed(&mut engine, 3, MOBILE, "01099999999");
    typed(&mut engine, 4, ADDRESS, "9 Nile St");
    let set = changes(&engine);
    assert_eq!(set["counts"]["changedRecords"], 3);
    let summary: Value = serde_json::from_str(
        &engine
            .erp_set_states(
                &json!({
                    "sent": sent(&set),
                    "errors": [
                        {"id": records[0].id, "key": records[0].key, "field": "payMethod",
                         "code": "lookup.notFound", "message": "طريقة الدفع غير موجودة"},
                        {"id": records[1].id, "key": records[1].key, "code": "record.locked",
                         "message": "locked"},
                        {"code": "schema.changed", "message": "The schema changed."},
                        {"id": records[9].id, "field": "vendorName", "code": "required",
                         "message": "not sent"}
                    ],
                    "conflicts": []
                })
                .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    settle(&mut engine);
    assert_eq!(summary["errors"], 2, "{summary}");
    assert_eq!(summary["errorRecords"], 2);
    assert_eq!(summary["unplaced"].as_array().unwrap().len(), 2);
    assert_eq!(counts(&engine)["invalid"], 2);
    // On the cells, in hover details and tiles, and Next Error goes through them.
    let info = cell_info(&engine, 2, PAY);
    assert_eq!(info["state"], "invalid");
    assert_eq!(info["server"]["code"], "lookup.notFound");
    assert_eq!(
        cell_info(&engine, 3, MOBILE)["server"]["code"],
        "record.locked"
    );
    assert_eq!(cell_info(&engine, 2, NAME).get("server"), None);
    assert_eq!(
        flags(&mut engine, 2, PAY) & CELL_ERP_INVALID,
        CELL_ERP_INVALID
    );
    assert_eq!(flags(&mut engine, 2, NAME) & CELL_ERP_INVALID, 0);
    let next = query(
        &engine,
        r#"{"kind":"erpNextError","sheet":0,"row":2,"col":1}"#,
    );
    assert_eq!(next, json!({"row": 2, "col": PAY}));
    // Local rules' refusals are still listed apart; the ERP's don't block a commit by themselves.
    assert_eq!(changes(&engine)["errors"], json!([]));
    // Editing the cell clears its error; writing the sent value back doesn't bring it back.
    typed(&mut engine, 2, PAY, "CASH");
    assert_eq!(counts(&engine)["invalid"], 1);
    ok(&mut engine, r#"{"kind":"undo"}"#);
    assert_eq!(value(&engine, 2, PAY), text("NET30"));
    assert_eq!(counts(&engine)["invalid"], 1);
    // A dry run that passes clears what its records had.
    let set = changes(&engine);
    engine
        .erp_set_states(&json!({"sent": sent(&set), "errors": [], "conflicts": []}).to_string())
        .unwrap();
    assert_eq!(counts(&engine)["invalid"], 0);
    assert_eq!(counts(&engine)["changedRecords"], 3);
}

#[test]
fn conflicts_mark_their_records_until_they_have_no_changes() {
    let records = records(20, 42);
    let mut engine = bound(&records);
    typed(&mut engine, 5, NAME, "Mine");
    typed(&mut engine, 5, ADDRESS, "1 Mine St");
    typed(&mut engine, 6, MOBILE, "01000000006");
    let set = changes(&engine);
    let other = &records[3];
    engine
        .erp_set_states(
            &json!({
                "sent": sent(&set),
                "errors": [],
                "conflicts": [{
                    "id": other.id, "key": other.key, "version": "AAAAAAAACQ0=",
                    "values": persisted(other, &[(1, "\"Omar's name\"")], "AAAAAAAACQ0=")["values"],
                    "fields": ["vendorName"], "modifiedBy": "omar",
                    "modifiedAt": "2026-11-10T10:04:11+02:00"
                }]
            })
            .to_string(),
        )
        .unwrap();
    settle(&mut engine);
    assert_eq!(counts(&engine)["conflicts"], 1);
    assert_eq!(changes(&engine)["counts"]["conflicts"], 1);
    // The clashing field shows it; the record's other change doesn't.
    assert_ne!(flags(&mut engine, 5, NAME) & CELL_ERP_CONFLICT, 0);
    assert_eq!(flags(&mut engine, 5, ADDRESS) & CELL_ERP_CONFLICT, 0);
    let info = cell_info(&engine, 5, NAME);
    assert_eq!(info["state"], "conflict");
    assert_eq!(info["conflict"]["by"], "omar");
    assert_eq!(info["conflict"]["now"], "Omar's name");
    // The record keeps the version it was read at, so a commit can't pass the ERP's check.
    assert_eq!(engine.bound_record(&other.key).unwrap().1, "AAAAAAAAB9E=");
    // Discarding its changes ends the conflict.
    ok(
        &mut engine,
        &json!({"kind": "erpDiscard", "areas": [{"top": 5, "left": 1, "bottom": 5, "right": 14}]})
            .to_string(),
    );
    assert_eq!(counts(&engine)["conflicts"], 0);
    assert_eq!(flags(&mut engine, 5, NAME) & CELL_ERP_CONFLICT, 0);
    assert_eq!(counts(&engine)["changedRecords"], 1);
}

#[test]
fn a_commit_holds_writes_then_applies_only_what_cells_still_hold() {
    let records = records(20, 43);
    let mut engine = bound(&records);
    typed(&mut engine, 2, NAME, "Nile Two");
    typed(&mut engine, 2, MOBILE, "");
    typed(&mut engine, 3, PAY, "net30");
    typed(&mut engine, 4, ADDRESS, "4 Nile St");
    let set = changes(&engine);
    assert_eq!(set["counts"]["changed"], 4);
    let undo_before = engine.undo_state().undo.is_some();
    assert!(undo_before);

    engine.erp_freeze(true);
    // Nothing writes the table while the commit is in flight, not even undo.
    for json in [
        json!({"kind": "input", "sheet": 0, "row": 7, "col": NAME, "text": "x"}),
        json!({"kind": "clear", "sheet": 0, "areas": [{"top": 8, "left": 2, "bottom": 8, "right": 3}]}),
        json!({"kind": "undo"}),
        json!({"kind": "sort", "sheet": 0, "area": {"top": 2, "left": 1, "bottom": 21, "right": 14},
               "keys": [{"index": 2, "descending": true}]}),
    ] {
        assert_eq!(
            engine.apply_json(&json.to_string()).err().as_deref(),
            Some(FROZEN),
            "{json}"
        );
    }
    // Cells beside the table are no ERP data.
    typed(&mut engine, 2, 16, "beside");

    // A cell that changed after the commit was sent keeps what it holds (here, by letting writes
    // go for a moment, as nothing in the app does).
    engine.erp_freeze(false);
    typed(&mut engine, 4, ADDRESS, "Changed since");
    engine.erp_freeze(true);

    let result = json!({
        "sent": sent(&set),
        "records": [
            persisted(&records[0], &[(1, "\"Nile Two\""), (3, "null"), (12, "\"u17\"")], "AAAAAAAACRA="),
            persisted(&records[1], &[(5, "\"NET30\""), (12, "\"u17\"")], "AAAAAAAACRQ="),
            persisted(&records[2], &[(6, "\"4 Nile St\""), (12, "\"u17\"")], "AAAAAAAACRg="),
        ]
    });
    let applied: Value =
        serde_json::from_str(&engine.erp_apply_commit(&result.to_string()).unwrap()).unwrap();
    settle(&mut engine);
    assert_eq!(applied, json!({"records": 3, "saved": 3, "kept": 1}));
    // Persisted values in, read-only cells too; versions moved; undo cleared.
    assert_eq!(value(&engine, 2, NAME), text("Nile Two"));
    assert_eq!(value(&engine, 2, MOBILE), CalcValue::Empty);
    assert_eq!(value(&engine, 3, PAY), text("NET30"));
    assert_eq!(value(&engine, 2, MODIFIED_BY), text("u17"));
    assert_eq!(value(&engine, 4, ADDRESS), text("Changed since"));
    assert_eq!(
        engine.bound_record(&records[1].key).unwrap().1,
        "AAAAAAAACRQ="
    );
    assert!(engine.undo_state().undo.is_none(), "undo cleared");
    // What's left is the one cell that changed since, against the persisted value.
    let left = changes(&engine);
    assert_eq!(left["counts"]["changed"], 1);
    assert_eq!(left["records"][0]["key"], records[2].key);
    assert_eq!(
        left["records"][0]["changes"]["address"],
        json!({"from": "4 Nile St", "to": "Changed since"})
    );
    assert_eq!(left["records"][0]["version"], "AAAAAAAACRg=");
    // Saved fields show Committed, until they change.
    for (row, col) in [(2, NAME), (2, MOBILE), (3, PAY)] {
        assert_eq!(
            flags(&mut engine, row, col) & (CELL_ERP_COMMITTED | CELL_ERP_MODIFIED),
            CELL_ERP_COMMITTED,
            "{row},{col}"
        );
    }
    assert_eq!(flags(&mut engine, 4, ADDRESS) & CELL_ERP_COMMITTED, 0);
    assert_eq!(cell_info(&engine, 3, PAY)["state"], "committed");
    assert_eq!(counts(&engine)["changedRecords"], 1);
    // Writes go again.
    typed(&mut engine, 2, NAME, "Nile Three");
    assert_eq!(flags(&mut engine, 2, NAME) & CELL_ERP_COMMITTED, 0);
    assert_ne!(flags(&mut engine, 2, NAME) & CELL_ERP_MODIFIED, 0);
    // The next commit's result replaces the last one's Committed marks.
    let set = changes(&engine);
    engine.erp_freeze(true);
    engine
        .erp_apply_commit(
            &json!({
                "sent": sent(&set),
                "records": [
                    persisted(&records[0], &[(1, "\"Nile Three\""), (3, "null"), (12, "\"u17\"")], "AAAAAAAACRw="),
                ]
            })
            .to_string(),
        )
        .unwrap();
    assert_eq!(flags(&mut engine, 3, PAY) & CELL_ERP_COMMITTED, 0);
    assert_ne!(flags(&mut engine, 2, NAME) & CELL_ERP_COMMITTED, 0);
}

#[test]
fn an_answer_the_sheet_cant_take_writes_nothing() {
    let records = records(10, 44);
    let mut engine = bound(&records);
    typed(&mut engine, 2, NAME, "Nile Two");
    let set = changes(&engine);
    engine.erp_freeze(true);
    let mut bad = persisted(&records[0], &[(1, "\"Nile Two\"")], "AAAAAAAACRA=");
    bad["values"]["debitLimit"] = json!("12.345");
    for records in [
        json!([persisted(&records[0], &[(1, "\"Nile Two\"")], "AAAAAAAACRA="), {"id": 5, "key": "V-X", "version": "AAAAAAAACRA=", "values": {}}]),
        json!([bad]),
        json!([persisted(
            &records[0],
            &[(1, "\"Nile Two\"")],
            "not a version"
        )]),
    ] {
        assert!(engine
            .erp_apply_commit(&json!({"sent": sent(&set), "records": records}).to_string())
            .is_err());
        assert_eq!(
            engine.bound_record(&records_key(&set)).unwrap().1,
            "AAAAAAAAB9E="
        );
    }
    // Still held, and the change still pending.
    assert_eq!(
        engine
            .apply_json(
                &json!({"kind": "input", "sheet": 0, "row": 3, "col": NAME, "text": "x"})
                    .to_string()
            )
            .err()
            .as_deref(),
        Some(FROZEN)
    );
    assert_eq!(changes(&engine)["counts"]["changed"], 1);
    engine.erp_freeze(false);
    typed(&mut engine, 3, NAME, "x");
}

fn records_key(set: &Value) -> String {
    set["records"][0]["key"].as_str().unwrap().to_string()
}

#[test]
fn changes_in_hidden_rows_are_flagged_and_left_out_of_visible_commits() {
    let records = records(30, 45);
    let mut engine = bound(&records);
    let local = |i: usize| records[i].wire[2] == "\"LOCAL\"";
    let shown = (0..30).find(|&i| local(i)).unwrap();
    let hidden = (0..30).find(|&i| !local(i)).unwrap();
    typed(&mut engine, 2 + shown as u32, NAME, "Shown");
    typed(&mut engine, 2 + hidden as u32, NAME, "Hidden");
    typed(&mut engine, 2 + hidden as u32, ADDRESS, "=\"formula\"");
    ok(
        &mut engine,
        r#"{"kind":"setFilter","sheet":0,"table":"Vendors","col":3,"criteria":{"kind":"values","values":["LOCAL"]}}"#,
    );
    let all = changes(&engine);
    assert_eq!(all["counts"]["changedRecords"], 2);
    let flagged: Vec<(&str, bool)> = all["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["key"].as_str().unwrap(),
                r["hidden"].as_bool().unwrap_or(false),
            )
        })
        .collect();
    assert!(flagged.contains(&(records[hidden].key.as_str(), true)));
    assert!(flagged.contains(&(records[shown].key.as_str(), false)));
    assert_eq!(all["formulas"][0]["hidden"], true);
    let visible = query(&engine, r#"{"kind":"erpChanges","visible":true}"#);
    assert_eq!(visible["counts"]["changedRecords"], 1);
    assert_eq!(visible["records"][0]["key"], records[shown].key);
    assert_eq!(visible["formulas"], json!([]));
    // The Review dialog lists all of them, in reading order, with their names and states.
    let review = query(&engine, r#"{"kind":"erpReview"}"#);
    let entries = review["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    let hidden_entries: Vec<&Value> = entries.iter().filter(|e| e["hidden"] == true).collect();
    assert_eq!(hidden_entries.len(), 2);
    let formula = entries.iter().find(|e| e["state"] == "formula").unwrap();
    assert_eq!(formula["field"], "address");
    let name = entries
        .iter()
        .find(|e| e["key"] == records[shown].key.as_str())
        .unwrap();
    assert_eq!(name["state"], "modified");
    assert_eq!(name["name"], "Shown");
    assert_eq!(name["label"], "Name");
    assert_eq!(name["now"], "Shown");
    assert!(name["original"]
        .as_str()
        .unwrap()
        .contains(&format!("{}", shown + 1)));
    let rows: Vec<u64> = entries.iter().map(|e| e["row"].as_u64().unwrap()).collect();
    let mut sorted = rows.clone();
    sorted.sort_unstable();
    assert_eq!(rows, sorted);
}
