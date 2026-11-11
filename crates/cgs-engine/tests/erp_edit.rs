//! Editing ERP data through the engine (Phase 16): text fields keep what is typed on every input
//! path, editable cells take every edit while read-only ones refuse, each field is tracked
//! against what the ERP sent, local rules mark cells as they change, formulas and spills are left
//! out, Discard Changes, hover details, lookup dropdowns and tile states, and a property test over
//! random typing, paste, fill, move, replace, convert, sort, filter, undo and redo that keys stay
//! with their records, read-only cells never change and the changeset equals an oracle's diff.

mod erp_harness;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cgs_calc::CalcValue;
use cgs_engine::tile::Screen;
use cgs_engine::view_block::{
    CELL_BYTES, CELL_ERP_FORMULA, CELL_ERP_INVALID, CELL_ERP_LOCK, CELL_ERP_MODIFIED,
    CELL_ERP_READ_ONLY, EXTRA_BYTES, HEADER_BYTES, MARK_BYTES, MARK_ERROR, RUN_BYTES, STYLE_BYTES,
};
use cgs_engine::Engine;
use erp_harness::*;
use serde_json::{json, Value};

fn ok(engine: &mut Engine, json: &str) {
    engine
        .apply_json(json)
        .unwrap_or_else(|error| panic!("{json}: {error}"));
    settle(engine);
}

fn refused(engine: &mut Engine, json: &str) {
    let undo = engine.undo_state().undo;
    assert_eq!(
        engine.apply_json(json).err().as_deref(),
        Some(READ_ONLY),
        "{json}"
    );
    settle(engine);
    assert_eq!(engine.undo_state().undo, undo, "{json} left a step");
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

fn changes(engine: &Engine) -> Value {
    query(engine, r#"{"kind":"erpChanges"}"#)
}

fn counts(engine: &Engine) -> Value {
    binding(engine)["counts"].clone()
}

fn area(top: u32, left: u32, bottom: u32, right: u32) -> Value {
    json!({"top": top, "left": left, "bottom": bottom, "right": right})
}

#[test]
fn text_fields_keep_what_is_typed() {
    let records = records(10, 21);
    let mut engine = bound(&records);
    for (typing, kept) in [
        ("0100123456", "0100123456"),
        ("001", "001"),
        ("1-2", "1-2"),
        ("1,23", "1,23"),
        ("'0045", "0045"),
        ("1E5", "1E5"),
        ("TRUE", "TRUE"),
        ("12%", "12%"),
        ("3/4", "3/4"),
    ] {
        typed(&mut engine, 2, 4, typing);
        assert_eq!(value(&engine, 2, 4), text(kept), "{typing}");
    }
    // A formula stays a formula, and an apostrophe makes it text.
    typed(&mut engine, 2, 4, "=1+1");
    assert_eq!(value(&engine, 2, 4), CalcValue::Number(2.0));
    assert_eq!(engine.edit_text(0, 2, 4), "=1+1");
    typed(&mut engine, 2, 4, "'=1+1");
    assert_eq!(value(&engine, 2, 4), text("=1+1"));
    // A code typed in another case takes its list's spelling.
    typed(&mut engine, 2, 3, "services");
    assert_eq!(value(&engine, 2, 3), text("SERVICES"));
    typed(&mut engine, 2, 6, "Net30");
    assert_eq!(value(&engine, 2, 6), text("NET30"));
    // Typing into many cells at once does the same.
    ok(
        &mut engine,
        &json!({"kind": "input", "sheet": 0, "row": 3, "col": 8, "text": "007",
            "areas": [area(3, 8, 5, 8)]})
        .to_string(),
    );
    for row in 3..=5 {
        assert_eq!(value(&engine, row, 8), text("007"));
    }
    // Numbers, dates and booleans are typed as Excel types them.
    typed(&mut engine, 2, 11, "1234.5");
    assert_eq!(value(&engine, 2, 11), CalcValue::Number(1234.5));
    typed(&mut engine, 2, 12, "2024-03-05");
    assert_eq!(value(&engine, 2, 12), CalcValue::Number(45356.0));
    typed(&mut engine, 2, 9, "true");
    assert_eq!(value(&engine, 2, 9), CalcValue::Bool(true));
    assert_keys_with(&engine, &records, false);
}

#[test]
fn every_input_path_keeps_text_fields_text() {
    let records = records(12, 22);
    let mut engine = bound(&records);
    // Text from another program, plain and as HTML with the number behind the text.
    ok(
        &mut engine,
        &json!({"kind": "pasteExternal", "sheet": 0, "row": 2, "col": 7,
            "text": "0100\t001\r\n0200\t002\r\n"})
        .to_string(),
    );
    assert_eq!(
        [
            value(&engine, 2, 7),
            value(&engine, 2, 8),
            value(&engine, 3, 7),
            value(&engine, 3, 8)
        ],
        [text("0100"), text("001"), text("0200"), text("002")]
    );
    ok(
        &mut engine,
        &json!({"kind": "pasteExternal", "sheet": 0, "row": 4, "col": 4, "text": "01001234567",
            "html": "<table><tr><td x:num=\"1001234567\">01001234567</td></tr></table>"})
        .to_string(),
    );
    assert_eq!(value(&engine, 4, 4), text("01001234567"));
    // A number copied from outside the table, all and as values.
    typed(&mut engine, 30, 20, "1001234567");
    ok(
        &mut engine,
        &json!({"kind": "paste", "fromSheet": 0, "from": area(30, 20, 30, 20), "sheet": 0,
            "to": area(5, 4, 5, 4)})
        .to_string(),
    );
    assert_eq!(value(&engine, 5, 4), text("1001234567"));
    typed(&mut engine, 31, 20, "TRUE");
    ok(
        &mut engine,
        &json!({"kind": "paste", "fromSheet": 0, "from": area(31, 20, 31, 20), "sheet": 0,
            "to": area(6, 7, 6, 7), "mode": "values"})
        .to_string(),
    );
    assert_eq!(value(&engine, 6, 7), text("TRUE"));
    // Fill down, as Ctrl+D does.
    ok(
        &mut engine,
        &json!({"kind": "fill", "sheet": 0, "from": area(2, 8, 2, 8), "to": area(2, 8, 5, 8),
            "copy": true})
        .to_string(),
    );
    for row in 2..=5 {
        assert_eq!(value(&engine, row, 8), text("001"));
    }
    // Replace keeps the replaced text as text.
    typed(&mut engine, 7, 7, "0100 St");
    ok(
        &mut engine,
        &json!({"kind": "replaceOne", "query": {"text": " St", "sheet": 0}, "with": "",
            "sheet": 0, "row": 7, "col": 7})
        .to_string(),
    );
    assert_eq!(value(&engine, 7, 7), text("0100"));
    // Convert to Number leaves them as they are.
    ok(
        &mut engine,
        &json!({"kind": "convertToNumber", "sheet": 0, "areas": [area(2, 7, 8, 8)]}).to_string(),
    );
    assert_eq!(value(&engine, 2, 8), text("001"));
    assert_eq!(value(&engine, 7, 7), text("0100"));
    // A dropdown pick is typing.
    typed(&mut engine, 8, 3, "IMPORT");
    assert_eq!(value(&engine, 8, 3), text("IMPORT"));
    assert_keys_with(&engine, &records, false);
}

#[test]
fn editable_cells_take_every_edit_and_read_only_ones_refuse() {
    let records = records(12, 24);
    let mut engine = bound(&records);
    let paste = |from: Value, to: Value| {
        json!({"kind": "paste", "fromSheet": 0, "from": from, "sheet": 0, "to": to}).to_string()
    };
    // Paste within editable columns, and fill, clear, cut and paste and Replace All among them.
    ok(&mut engine, &paste(area(2, 2, 3, 4), area(6, 2, 7, 4)));
    ok(
        &mut engine,
        &json!({"kind": "fill", "sheet": 0, "from": area(2, 7, 2, 8), "to": area(2, 7, 9, 8)})
            .to_string(),
    );
    ok(
        &mut engine,
        &json!({"kind": "clear", "sheet": 0, "areas": [area(10, 6, 11, 9)]}).to_string(),
    );
    ok(
        &mut engine,
        &json!({"kind": "move", "fromSheet": 0, "from": area(3, 2, 3, 2), "sheet": 0, "row": 40,
            "col": 2})
        .to_string(),
    );
    ok(
        &mut engine,
        &json!({"kind": "move", "fromSheet": 0, "from": area(40, 2, 40, 2), "sheet": 0, "row": 3,
            "col": 2})
        .to_string(),
    );
    ok(
        &mut engine,
        &json!({"kind": "replaceAll", "query": {"text": "Nile Trading", "sheet": 0},
            "with": "Nile Traders"})
        .to_string(),
    );
    // Anything that reaches a read-only column or the headings is refused, a paste that grows
    // into one included.
    refused(&mut engine, &paste(area(2, 3, 3, 4), area(6, 4, 7, 5)));
    refused(&mut engine, &paste(area(2, 3, 2, 4), area(6, 4, 6, 4)));
    refused(&mut engine, &paste(area(2, 2, 2, 2), area(1, 2, 1, 2)));
    refused(
        &mut engine,
        &json!({"kind": "fill", "sheet": 0, "from": area(2, 4, 2, 5), "to": area(2, 4, 6, 5)})
            .to_string(),
    );
    refused(
        &mut engine,
        &json!({"kind": "clear", "sheet": 0, "areas": [area(2, 10, 3, 10)]}).to_string(),
    );
    refused(
        &mut engine,
        &json!({"kind": "move", "fromSheet": 0, "from": area(2, 5, 2, 5), "sheet": 0, "row": 40,
            "col": 5})
        .to_string(),
    );
    refused(
        &mut engine,
        &json!({"kind": "pasteExternal", "sheet": 0, "row": 2, "col": 4, "text": "a\tb"})
            .to_string(),
    );
    assert_eq!(
        engine
            .apply_json(
                &json!({"kind": "replaceAll", "query": {"text": "V-0", "sheet": 0}, "with": "W-0"})
                    .to_string()
            )
            .err()
            .as_deref(),
        Some(READ_ONLY)
    );
    settle(&mut engine);
    // The guards inside the engine refuse on their own too, and let editable cells through.
    engine.set_erp_precheck(false);
    assert!(engine
        .apply_json(&paste(area(2, 3, 3, 4), area(6, 4, 7, 5)))
        .is_err());
    ok(&mut engine, &paste(area(2, 6, 3, 9), area(8, 6, 9, 9)));
    engine.set_erp_precheck(true);
    assert_keys_with(&engine, &records, false);
}

#[test]
fn each_changed_field_is_tracked_and_a_value_changed_back_drops_out() {
    let records = records(20, 25);
    let mut engine = bound(&records);
    assert_eq!(changes(&engine)["records"], json!([]));
    assert_eq!(counts(&engine)["changed"], 0);
    let other_group = if records[2].wire[2] == "\"LOCAL\"" {
        "IMPORT"
    } else {
        "LOCAL"
    };
    typed(&mut engine, 2, 2, "Al Noor");
    typed(&mut engine, 3, 4, "01009876543");
    typed(&mut engine, 4, 3, other_group);
    let set = changes(&engine);
    let list = set["records"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(set["company"], "Kit");
    assert_eq!(set["schemaVersion"], "vendors.1");
    assert_eq!(list[0]["id"], records[0].id);
    assert_eq!(list[0]["key"], records[0].key);
    assert_eq!(list[0]["version"], "AAAAAAAAB9E=");
    assert_eq!(list[0]["row"], 2);
    assert_eq!(
        list[0]["changes"],
        json!({"vendorName": {"from": serde_json::from_str::<Value>(&records[0].wire[1]).unwrap(), "to": "Al Noor"}})
    );
    assert_eq!(list[1]["changes"]["mobile"]["to"], "01009876543");
    assert_eq!(list[2]["changes"]["vendorGroup"]["to"], other_group);
    assert_eq!(
        counts(&engine),
        json!({"changed": 3, "changedRecords": 3, "invalid": 0, "formulas": 0, "conflicts": 0, "gone": 0})
    );
    // Typing the original back drops the field.
    let name: String = serde_json::from_str(&records[0].wire[1]).unwrap();
    typed(&mut engine, 2, 2, &name);
    assert_eq!(counts(&engine)["changed"], 2);
    // Undo and redo follow, and undoing everything leaves nothing changed.
    ok(&mut engine, r#"{"kind":"undo"}"#);
    assert_eq!(counts(&engine)["changed"], 3);
    for _ in 0..3 {
        ok(&mut engine, r#"{"kind":"undo"}"#);
    }
    assert_eq!(changes(&engine)["records"], json!([]));
    assert_eq!(counts(&engine)["changedRecords"], 0);
    for _ in 0..4 {
        ok(&mut engine, r#"{"kind":"redo"}"#);
    }
    assert_eq!(counts(&engine)["changed"], 2);
    // Sorting and filtering move rows, not changes.
    ok(
        &mut engine,
        r#"{"kind":"sort","sheet":0,"area":{"top":2,"left":1,"bottom":21,"right":14},"keys":[{"index":2,"descending":true}]}"#,
    );
    let sorted = changes(&engine);
    assert_eq!(sorted["records"].as_array().unwrap().len(), 2);
    assert_ne!(sorted["records"][0]["row"], 3, "the record moved");
    ok(
        &mut engine,
        r#"{"kind":"setFilter","sheet":0,"table":"Vendors","col":3,"criteria":{"kind":"values","values":["LOCAL"]}}"#,
    );
    // The changes stay, and those the filter hides say so (Phase 17's Review lists them).
    let filtered = changes(&engine);
    let mut records_now = filtered["records"].as_array().unwrap().clone();
    for record in &mut records_now {
        let hidden = record.as_object_mut().unwrap().remove("hidden");
        let local = value(&engine, record["row"].as_u64().unwrap() as u32, 3) == text("LOCAL");
        assert_eq!(hidden.is_some(), !local, "{record}");
    }
    assert_eq!(Value::Array(records_now), sorted["records"]);
    assert_keys_with(&engine, &records, false);
}

#[test]
fn empty_and_null_decimals_dates_and_booleans_compare_as_the_erp_does() {
    let records = records(30, 26);
    let mut engine = bound(&records);
    // A record with no mobile: clearing it or typing nothing changes nothing.
    let row = 2 + records.iter().position(|r| r.wire[3] == "null").unwrap() as u32;
    ok(
        &mut engine,
        &json!({"kind": "clear", "sheet": 0, "areas": [area(row, 4, row, 4)]}).to_string(),
    );
    typed(&mut engine, row, 4, "");
    assert_eq!(counts(&engine)["changed"], 0);
    // Decimals compare at their scale, and go as strings at it.
    let limit = match records[0].cells[10] {
        CalcValue::Number(n) => n,
        _ => unreachable!(),
    };
    typed(&mut engine, 2, 11, &format!("{}", limit + 0.004));
    assert_eq!(counts(&engine)["changed"], 0, "the same at two decimals");
    typed(&mut engine, 2, 11, &format!("{}", limit + 1.0));
    assert_eq!(
        changes(&engine)["records"][0]["changes"]["debitLimit"]["to"],
        format!("{:.2}", limit + 1.0)
    );
    typed(&mut engine, 3, 12, "2025-01-31");
    typed(
        &mut engine,
        4,
        9,
        if records[2].wire[8] == "true" {
            "FALSE"
        } else {
            "TRUE"
        },
    );
    let set = changes(&engine);
    let by_key: HashMap<String, Value> = set["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["key"].as_str().unwrap().to_string(), r["changes"].clone()))
        .collect();
    assert_eq!(by_key[&records[1].key]["since"]["to"], "2025-01-31");
    assert_eq!(
        by_key[&records[1].key]["since"]["from"],
        serde_json::from_str::<Value>(&records[1].wire[11]).unwrap()
    );
    assert_eq!(
        by_key[&records[2].key]["blocked"]["to"],
        records[2].wire[8] != "true"
    );
    // A cleared text field goes as null.
    ok(
        &mut engine,
        &json!({"kind": "clear", "sheet": 0, "areas": [area(5, 2, 5, 2)]}).to_string(),
    );
    let set = changes(&engine);
    let cleared = set["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == records[3].key)
        .unwrap();
    assert_eq!(cleared["changes"]["vendorName"]["to"], Value::Null);
}

#[test]
fn local_rules_mark_cells_as_they_change() {
    let records = records(12, 27);
    let mut engine = bound(&records);
    ok(
        &mut engine,
        &json!({"kind": "clear", "sheet": 0, "areas": [area(2, 2, 2, 2)]}).to_string(),
    );
    typed(&mut engine, 3, 2, &"x".repeat(151));
    typed(&mut engine, 4, 3, "OLD");
    typed(&mut engine, 5, 3, "NOPE");
    typed(&mut engine, 6, 11, "-1");
    typed(&mut engine, 7, 11, "1.005");
    typed(&mut engine, 8, 11, "1234567890123456");
    typed(&mut engine, 9, 9, "yes");
    typed(&mut engine, 10, 12, "abc");
    typed(&mut engine, 11, 8, &"0".repeat(51));
    let set = changes(&engine);
    let found: Vec<(u64, u64, &str)> = set["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["row"].as_u64().unwrap(),
                e["col"].as_u64().unwrap(),
                e["code"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        found,
        vec![
            (2, 2, "required"),
            (3, 2, "maxLength"),
            (4, 3, "lookup.notFound"),
            (5, 3, "lookup.notFound"),
            (6, 11, "range"),
            (7, 11, "scale"),
            (8, 11, "scale"),
            (9, 9, "type"),
            (10, 12, "type"),
            (11, 8, "maxLength"),
        ]
    );
    assert_eq!(counts(&engine)["invalid"], 10);
    // Invalid changes are still changes, and go in the changeset for the server to judge too.
    assert_eq!(counts(&engine)["changed"], 10);
    // Next Error goes round in reading order.
    let next = |engine: &Engine, row: u32, col: u32, back: bool| {
        let at = query(
            engine,
            &json!({"kind": "erpNextError", "sheet": 0, "row": row, "col": col, "back": back})
                .to_string(),
        );
        (at["row"].as_u64().unwrap(), at["col"].as_u64().unwrap())
    };
    assert_eq!(next(&engine, 1, 1, false), (2, 2));
    assert_eq!(next(&engine, 2, 2, false), (3, 2));
    assert_eq!(next(&engine, 11, 8, false), (2, 2));
    assert_eq!(next(&engine, 2, 2, true), (11, 8));
    // A cell explains its rule.
    let details = query(
        engine_ref(&engine),
        r#"{"kind":"erpCell","sheet":0,"row":3,"col":2}"#,
    );
    assert_eq!(details["state"], "invalid");
    assert_eq!(details["rule"]["code"], "maxLength");
    assert_eq!(details["rule"]["maxLength"], 150);
    let details = query(&engine, r#"{"kind":"erpCell","sheet":0,"row":4,"col":3}"#);
    assert_eq!(details["rule"]["inactive"], true);
    // Fixing a cell clears it, and with none left Next Error finds nothing.
    typed(&mut engine, 2, 2, "Fixed");
    assert_eq!(counts(&engine)["invalid"], 9);
    ok(&mut engine, r#"{"kind":"erpDiscard"}"#);
    assert_eq!(counts(&engine)["invalid"], 0);
    assert_eq!(
        query(
            &engine,
            r#"{"kind":"erpNextError","sheet":0,"row":1,"col":1}"#
        ),
        Value::Null
    );
}

fn engine_ref(engine: &Engine) -> &Engine {
    engine
}

#[test]
fn formulas_and_spills_are_flagged_and_left_out() {
    let records = records(12, 28);
    let mut engine = bound(&records);
    typed(&mut engine, 2, 2, "=B3");
    let set = changes(&engine);
    assert_eq!(set["records"], json!([]), "a formula is no change to send");
    assert_eq!(
        set["formulas"],
        json!([{"key": records[0].key, "field": "vendorName", "row": 2, "col": 2}])
    );
    assert_eq!(counts(&engine)["formulas"], 1);
    assert_eq!(
        query(&engine, r#"{"kind":"erpCell","sheet":0,"row":2,"col":2}"#)["state"],
        "formula"
    );
    // Converted to values, it is an ordinary change.
    ok(
        &mut engine,
        r#"{"kind":"paste","fromSheet":0,"from":{"top":2,"left":2,"bottom":2,"right":2},"sheet":0,"to":{"top":2,"left":2,"bottom":2,"right":2},"mode":"values"}"#,
    );
    let set = changes(&engine);
    assert_eq!(set["formulas"], json!([]));
    assert_eq!(
        set["records"][0]["changes"]["vendorName"]["to"],
        serde_json::from_str::<Value>(&records[1].wire[1]).unwrap()
    );
    // A dynamic array spilling into empty editable cells: each spilled value counts as a formula.
    let empty: Vec<u32> = (0..12)
        .filter(|&i| records[i].wire[6] == "null")
        .map(|i| i as u32 + 2)
        .collect();
    let row = *empty
        .windows(3)
        .find(|w| w[1] == w[0] + 1 && w[2] == w[0] + 2)
        .map(|w| &w[0])
        .unwrap_or(&0);
    if row != 0 {
        typed(&mut engine, row, 7, "=SEQUENCE(3)");
        let set = changes(&engine);
        let places: Vec<(u64, u64)> = set["formulas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| (f["row"].as_u64().unwrap(), f["col"].as_u64().unwrap()))
            .collect();
        let row = u64::from(row);
        assert_eq!(places, vec![(row, 7), (row + 1, 7), (row + 2, 7)]);
        assert_eq!(counts(&engine)["formulas"], 3);
        assert_eq!(
            query(
                &engine,
                &json!({"kind": "erpCell", "sheet": 0, "row": row + 1, "col": 7}).to_string()
            )["state"],
            "formula"
        );
    }
}

#[test]
fn discard_changes_writes_the_originals_back_as_one_step() {
    let records = records(12, 29);
    let mut engine = bound(&records);
    for row in 2..=6 {
        typed(&mut engine, row, 2, &format!("Edited {row}"));
    }
    typed(&mut engine, 7, 4, "=1+1");
    assert_eq!(counts(&engine)["changed"], 5);
    // The selection's changes only.
    ok(
        &mut engine,
        &json!({"kind": "erpDiscard", "areas": [area(2, 1, 3, 14)]}).to_string(),
    );
    assert_eq!(counts(&engine)["changed"], 3);
    assert_eq!(value(&engine, 2, 2), records[0].cells[1]);
    assert_eq!(value(&engine, 4, 2), text("Edited 4"));
    // All of them, the formula too, then undo brings them back.
    ok(&mut engine, r#"{"kind":"erpDiscard"}"#);
    assert_eq!(
        counts(&engine),
        json!({"changed": 0, "changedRecords": 0, "invalid": 0, "formulas": 0, "conflicts": 0, "gone": 0})
    );
    assert_keys_with_records(&engine, &records);
    ok(&mut engine, r#"{"kind":"undo"}"#);
    assert_eq!(counts(&engine)["changed"], 3);
    assert_eq!(counts(&engine)["formulas"], 1);
    // Nothing to discard is no step.
    ok(&mut engine, r#"{"kind":"erpDiscard"}"#);
    let undo = engine.undo_state().undo;
    ok(&mut engine, r#"{"kind":"erpDiscard"}"#);
    assert_eq!(engine.undo_state().undo, undo);
}

#[test]
fn cells_explain_themselves_and_lookups_drop_down_without_a_rule() {
    let records = records(6, 30);
    let mut engine = bound(&records);
    let details = |engine: &Engine, row: u32, col: u32| {
        query(
            engine,
            &json!({"kind": "erpCell", "sheet": 0, "row": row, "col": col}).to_string(),
        )
    };
    let heading = details(&engine, 1, 1);
    assert_eq!(heading["heading"], true);
    assert_eq!(heading["reason"], "key");
    assert_eq!(heading["label"], "Account");
    assert_eq!(details(&engine, 2, 5)["reason"], "needsAdvEdit");
    assert_eq!(details(&engine, 2, 10)["reason"], "notEditable");
    let clean = details(&engine, 2, 2);
    assert_eq!(clean["editable"], true);
    assert!(clean.get("state").is_none() && clean.get("original").is_none());
    assert_eq!(clean["key"], records[0].key);
    typed(&mut engine, 2, 11, "99");
    let edited = details(&engine, 2, 11);
    assert_eq!(edited["state"], "modified");
    assert_eq!(edited["now"], "99.00");
    assert!(edited["original"].as_str().unwrap().contains('.'));
    assert_eq!(details(&engine, 20, 20), Value::Null, "outside the table");
    // Lookups drop down their active codes with labels, and no rule reaches the file.
    let list = query(
        &engine,
        r#"{"kind":"validationAt","sheet":0,"row":3,"col":3}"#,
    );
    assert_eq!(list["dropdown"], true);
    assert_eq!(list["lookup"], true);
    assert_eq!(list["items"], json!(["LOCAL", "IMPORT", "SERVICES"]));
    assert_eq!(list["labels"], json!(["Local", "Import", "Services"]));
    for (row, col) in [(3, 5), (3, 2), (1, 3), (20, 3)] {
        assert_eq!(
            query(
                &engine,
                &json!({"kind": "validationAt", "sheet": 0, "row": row, "col": col}).to_string()
            ),
            Value::Null,
            "({row}, {col})"
        );
    }
    let mut out = Vec::new();
    engine.save(&mut out).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(out)).unwrap();
    let mut sheet = String::new();
    std::io::Read::read_to_string(
        &mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(),
        &mut sheet,
    )
    .unwrap();
    assert!(!sheet.contains("dataValidation"), "no rule in the file");
}

/// A tile's cell flags and mark kinds, read from its bytes.
struct TileBytes {
    bytes: Vec<u8>,
    rows: usize,
    cols: usize,
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

impl TileBytes {
    fn new(engine: &mut Engine, dark: bool) -> Self {
        let screen = Screen {
            dark,
            background: if dark { 0x151b24 } else { 0xffffff },
            text: if dark { 0xe6edf7 } else { 0x000000 },
        };
        let bytes = engine.view_block(0, 0, 0, screen).into_bytes();
        let rows = u16_at(&bytes, 6) as usize;
        let cols = u16_at(&bytes, 8) as usize;
        Self { bytes, rows, cols }
    }

    fn cells_at(&self) -> usize {
        let sizes = HEADER_BYTES + (self.rows + self.cols) * 2;
        sizes + (4 - sizes % 4) % 4
    }

    /// Row and column counted from 1.
    fn flags(&self, row: u32, col: u32) -> u16 {
        let index = (row as usize - 1) * self.cols + col as usize - 1;
        u16_at(&self.bytes, self.cells_at() + index * CELL_BYTES + 18)
    }

    /// (row, col) counted from 1 of each mark of a kind.
    fn marks(&self, kind: u8) -> Vec<(u32, u32)> {
        let styles = u32_at(&self.bytes, 20) as usize;
        let extras = u32_at(&self.bytes, 24) as usize;
        let runs = u32_at(&self.bytes, 32) as usize;
        let count = u32_at(&self.bytes, 36) as usize;
        let first = self.cells_at()
            + self.rows * self.cols * CELL_BYTES
            + styles * STYLE_BYTES
            + extras * EXTRA_BYTES
            + runs * RUN_BYTES;
        (0..count)
            .filter_map(|i| {
                let at = first + i * MARK_BYTES;
                let index = u32_at(&self.bytes, at) as usize;
                (self.bytes[at + 4] == kind).then(|| {
                    (
                        (index / self.cols) as u32 + 1,
                        (index % self.cols) as u32 + 1,
                    )
                })
            })
            .collect()
    }
}

const STATES: u16 =
    CELL_ERP_MODIFIED | CELL_ERP_INVALID | CELL_ERP_FORMULA | CELL_ERP_READ_ONLY | CELL_ERP_LOCK;

#[test]
fn tiles_show_states_and_hide_number_as_text_triangles_on_text_fields() {
    let records = records(20, 31);
    let mut engine = bound(&records);
    typed(&mut engine, 2, 2, "Edited");
    ok(
        &mut engine,
        &json!({"kind": "clear", "sheet": 0, "areas": [area(3, 2, 3, 2)]}).to_string(),
    );
    typed(&mut engine, 4, 2, "=B5");
    // Numbers as text outside the table keep their triangle.
    typed(&mut engine, 2, 16, "'0123");
    for dark in [false, true] {
        let tile = TileBytes::new(&mut engine, dark);
        assert_eq!(
            tile.flags(1, 1) & STATES,
            CELL_ERP_LOCK,
            "the key's heading"
        );
        assert_eq!(tile.flags(1, 2) & STATES, 0, "an editable heading");
        assert_eq!(tile.flags(1, 5) & STATES, CELL_ERP_LOCK);
        assert_eq!(tile.flags(2, 1) & STATES, CELL_ERP_READ_ONLY);
        assert_eq!(tile.flags(2, 2) & STATES, CELL_ERP_MODIFIED);
        assert_eq!(
            tile.flags(3, 2) & STATES,
            CELL_ERP_MODIFIED | CELL_ERP_INVALID
        );
        assert_eq!(tile.flags(4, 2) & STATES, CELL_ERP_FORMULA);
        assert_eq!(tile.flags(5, 2) & STATES, 0);
        assert_eq!(tile.flags(2, 15) & STATES, 0, "beside the table");
        let triangles = tile.marks(MARK_ERROR);
        assert_eq!(triangles, vec![(2, 16)], "only outside the text fields");
    }
    // Undo back to the original clears Modified.
    for _ in 0..4 {
        ok(&mut engine, r#"{"kind":"undo"}"#);
    }
    let tile = TileBytes::new(&mut engine, false);
    assert_eq!(tile.flags(2, 2) & STATES, 0);
    // Copies and pages carry no states: they show the cells as any workbook's.
    let (copied, html) = engine.copy_text(0, cgs_engine::command::Area::cell(2, 2));
    assert!(!copied.is_empty() && !html.contains("erp"));
}

#[test]
fn a_refresh_after_edits_keeps_them_where_the_erp_changed_nothing() {
    let records = records(10, 32);
    let mut engine = bound(&records);
    typed(&mut engine, 2, 2, "Edited");
    typed(&mut engine, 3, 4, "0100");
    assert_eq!(counts(&engine)["changed"], 2);
    engine.erp_begin_refresh().unwrap();
    for page in pages(&records, 1000, records.len()) {
        engine.erp_page(&page).unwrap();
    }
    engine.erp_refresh().unwrap();
    settle(&mut engine);
    // Phase 18's merge: the ERP changed neither record, so both edits stay pending.
    assert_eq!(
        counts(&engine),
        json!({"changed": 2, "changedRecords": 2, "invalid": 0, "formulas": 0, "conflicts": 0, "gone": 0})
    );
    assert!(engine.undo_state().undo.is_none(), "a refresh clears undo");
}

// ---- The property test ----

/// Excel's 1900 date serial as `YYYY-MM-DD`, by counting, apart from the engine's arithmetic.
fn date_of(serial: f64) -> String {
    let mut days = serial as i64;
    if days >= 60 {
        days -= 1;
    }
    let mut year = 1900;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let length = if leap { 366 } else { 365 };
        if days <= length {
            break;
        }
        days -= length;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while days > months[month] {
        days -= months[month];
        month += 1;
    }
    format!("{year:04}-{:02}-{days:02}", month + 1)
}

/// What the oracle sees in a cell: its value, and whether a formula makes it.
#[derive(Clone, Debug, PartialEq)]
enum Held {
    Value(CalcValue),
    Formula,
}

fn wire_of(col: u32, value: &CalcValue) -> Value {
    match value {
        CalcValue::Empty => Value::Null,
        CalcValue::Text(t) if t.is_empty() => Value::Null,
        CalcValue::Text(t) => json!(t),
        CalcValue::Bool(b) => json!(b),
        CalcValue::Error(e) => json!(e),
        CalcValue::Number(n) => match col {
            11 => {
                let text = format!("{n:.2}");
                json!(if text == "-0.00" {
                    "0.00".to_string()
                } else {
                    text
                })
            }
            12 => json!(date_of(*n)),
            _ => json!(n),
        },
    }
}

fn same_as_erp(col: u32, original: &CalcValue, now: &CalcValue) -> bool {
    let blank =
        |v: &CalcValue| matches!(v, CalcValue::Empty) || *v == CalcValue::Text(String::new());
    match (original, now) {
        _ if blank(original) && blank(now) => true,
        (CalcValue::Number(a), CalcValue::Number(b)) if col == 11 => {
            (a * 100.0).round() == (b * 100.0).round()
        }
        _ => original == now,
    }
}

/// The local rule a changed value breaks, as the schema in `erp_harness` sets them.
fn oracle_rule(col: u32, original: &CalcValue, now: &CalcValue) -> Option<&'static str> {
    let blank = matches!(now, CalcValue::Empty) || *now == CalcValue::Text(String::new());
    let required = matches!(col, 2 | 3 | 6 | 9);
    if blank {
        return required.then_some("required");
    }
    if matches!(now, CalcValue::Error(_)) {
        return Some("type");
    }
    let length = |t: &str| t.encode_utf16().count();
    match col {
        2 | 7 => match now {
            CalcValue::Text(t) if length(t) > 150 => Some("maxLength"),
            CalcValue::Text(_) => None,
            _ => Some("type"),
        },
        4 | 8 => match now {
            CalcValue::Text(t) if length(t) > 50 => Some("maxLength"),
            CalcValue::Text(_) => None,
            _ => Some("type"),
        },
        3 | 6 => {
            let codes: &[&str] = if col == 3 {
                &["LOCAL", "IMPORT", "SERVICES"]
            } else {
                &["CASH", "NET30"]
            };
            match now {
                CalcValue::Text(t) if codes.contains(&t.as_str()) || original == now => None,
                CalcValue::Text(_) => Some("lookup.notFound"),
                _ => Some("type"),
            }
        }
        9 => (!matches!(now, CalcValue::Bool(_))).then_some("type"),
        11 => match now {
            CalcValue::Number(n) => {
                let shortest = format!("{}", n.abs());
                let (whole, fraction) = shortest.split_once('.').unwrap_or((&shortest, ""));
                let digits = format!("{whole}{fraction}");
                let significant = digits.trim_start_matches('0').len()
                    - if fraction.is_empty() {
                        whole.len() - whole.trim_end_matches('0').len()
                    } else {
                        0
                    };
                if significant > 15 || fraction.len() > 2 {
                    Some("scale")
                } else if *n < 0.0 {
                    Some("range")
                } else {
                    None
                }
            }
            _ => Some("type"),
        },
        12 => match now {
            CalcValue::Number(n) if n.fract() == 0.0 && *n >= 1.0 => None,
            _ => Some("type"),
        },
        _ => None,
    }
}

/// What the oracle expects of the changeset: changes by key and field, formula cells, and the
/// rules broken, by key and field.
type Changes = BTreeMap<String, BTreeMap<String, (Value, Value)>>;

struct Oracle {
    changes: Changes,
    formulas: BTreeSet<(String, String)>,
    errors: BTreeSet<(String, String, String)>,
}

fn oracle(engine: &mut Engine, records: &[Record]) -> Oracle {
    let by_key: HashMap<&str, &Record> = records.iter().map(|r| (r.key.as_str(), r)).collect();
    let top = binding(engine)["top"].as_u64().unwrap() as u32;
    let bottom = binding(engine)["bottom"].as_u64().unwrap() as u32;
    let cells = grid(engine, top + 1, bottom);
    let mut out = Oracle {
        changes: BTreeMap::new(),
        formulas: BTreeSet::new(),
        errors: BTreeSet::new(),
    };
    for row in top + 1..=bottom {
        let CalcValue::Text(key) = cell(&cells, row, 1) else {
            panic!("row {row} has no key");
        };
        let record = by_key[key.as_str()];
        for col in editable_cols() {
            let name = FIELDS[col as usize - 1].0.to_string();
            let now = cell(&cells, row, col);
            let held = if engine.edit_text(0, row, col).starts_with('=') {
                Held::Formula
            } else {
                Held::Value(now.clone())
            };
            let original = &record.cells[col as usize - 1];
            match held {
                Held::Formula => {
                    out.formulas.insert((key.clone(), name));
                }
                Held::Value(now) => {
                    if matches!(col, 2 | 3 | 4 | 6 | 7 | 8) {
                        assert!(
                            matches!(
                                now,
                                CalcValue::Text(_) | CalcValue::Empty | CalcValue::Error(_)
                            ),
                            "{key}'s {name} holds {now:?}, not text"
                        );
                    }
                    if same_as_erp(col, original, &now) {
                        continue;
                    }
                    if let Some(rule) = oracle_rule(col, original, &now) {
                        out.errors
                            .insert((key.clone(), name.clone(), rule.to_string()));
                    }
                    out.changes
                        .entry(key.clone())
                        .or_default()
                        .insert(name, (wire_of(col, original), wire_of(col, &now)));
                }
            }
        }
    }
    out
}

fn assert_changeset_is_the_oracles(engine: &mut Engine, records: &[Record], step: usize, op: &str) {
    let expected = oracle(engine, records);
    let set = changes(engine);
    let mut actual: Changes = BTreeMap::new();
    for record in set["records"].as_array().unwrap() {
        let key = record["key"].as_str().unwrap().to_string();
        let meta = engine.bound_record(&key).unwrap();
        assert_eq!(record["id"], meta.0, "step {step}");
        assert_eq!(record["version"], meta.1, "step {step}");
        for (field, change) in record["changes"].as_object().unwrap() {
            actual.entry(key.clone()).or_default().insert(
                field.clone(),
                (change["from"].clone(), change["to"].clone()),
            );
        }
    }
    if actual != expected.changes {
        for (key, fields) in &expected.changes {
            if actual.get(key) != Some(fields) {
                panic!(
                    "step {step} ({op}): {key} expected {fields:?}, got {:?}",
                    actual.get(key)
                );
            }
        }
        panic!("step {step} ({op}): the changeset has more: {actual:?}");
    }
    let formulas: BTreeSet<(String, String)> = set["formulas"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["key"].as_str().unwrap().to_string(),
                f["field"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(formulas, expected.formulas, "step {step} ({op}): formulas");
    let errors: BTreeSet<(String, String, String)> = set["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["key"].as_str().unwrap().to_string(),
                e["field"].as_str().unwrap().to_string(),
                e["code"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(errors, expected.errors, "step {step} ({op}): errors");
    // The badge's counts, kept up to date as rows are written, agree with the full diff.
    let changed: usize = expected.changes.values().map(BTreeMap::len).sum();
    assert_eq!(
        counts(engine),
        json!({
            "changed": changed,
            "changedRecords": expected.changes.len(),
            "invalid": expected.errors.len(),
            "formulas": expected.formulas.len(),
            "conflicts": 0,
            "gone": 0,
        }),
        "step {step} ({op}): counts"
    );
}

/// What may be typed into each column, typing that must be refused in read-only ones included.
fn typing_for(col: u32, pick: &mut Random) -> String {
    let long_name = "N".repeat(151);
    let long_tax = "9".repeat(51);
    let pool: Vec<&str> = match col {
        2 => vec![
            "Al Noor",
            "مؤسسة النور",
            "0100",
            "",
            "=C2",
            &long_name,
            "Nile Trading 5",
        ],
        3 => vec!["LOCAL", "local", "IMPORT", "OLD", "NOPE", ""],
        4 => vec!["01001234567", "0100", "1-2", "1,23", "", "'0045", "=1+1"],
        6 => vec!["CASH", "net30", "NET60", ""],
        7 => vec!["12 St", "", "0012", "3/4"],
        8 => vec!["0123", "001", &long_tax, ""],
        9 => vec!["TRUE", "FALSE", "yes", ""],
        11 => vec!["1234.5", "1.005", "-3", "abc", "15000", ""],
        12 => vec!["2024-03-05", "45000.5", "abc", ""],
        _ => vec!["x", "USD", "0012"],
    };
    pick.pick(&pool).to_string()
}

/// Runs of neighbouring editable columns: Name to Mobile, Payment method to Blocked, Credit limit
/// and Since.
const RUNS: [(u32, u32); 3] = [(2, 4), (6, 9), (11, 12)];

fn touches_read_only(left: u32, right: u32) -> bool {
    (left..=right).any(|col| read_only_cols().contains(&col))
}

#[test]
fn random_edits_keep_keys_read_only_cells_and_the_changeset_with_the_oracle() {
    let records = records(1000, 20261110);
    let mut engine = bound(&records);
    let mut pick = Random(1616);
    let data = r#"{"top":2,"left":1,"bottom":1001,"right":14}"#;
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    let by_row_original = |engine: &mut Engine, row: u32, col: u32| -> String {
        let CalcValue::Text(key) = value(engine, row, 1) else {
            unreachable!()
        };
        let record = records.iter().find(|r| r.key == key).unwrap();
        match &record.cells[col as usize - 1] {
            CalcValue::Empty => String::new(),
            CalcValue::Text(t) => format!("'{t}"),
            CalcValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            CalcValue::Number(n) if col == 12 => date_of(*n),
            CalcValue::Number(n) => format!("{n}"),
            CalcValue::Error(e) => e.clone(),
        }
    };
    for step in 0..160 {
        let roll = pick.below(20);
        let row = 2 + pick.below(1000) as u32;
        let rows = 1 + pick.below(4) as u32;
        let last = (row + rows - 1).min(1001);
        let (run_left, run_right) = *pick.pick(&RUNS);
        let (op, json, refuse) = match roll {
            0..=4 => {
                let col = 1 + pick.below(14) as u32;
                let text = typing_for(col, &mut pick);
                (
                    "type",
                    json!({"kind": "input", "sheet": 0, "row": row, "col": col, "text": text}),
                    !editable_cols().contains(&col),
                )
            }
            5 => {
                let col = *pick.pick(&editable_cols());
                let text = typing_for(col, &mut pick);
                (
                    "type many",
                    json!({"kind": "input", "sheet": 0, "row": row, "col": col, "text": text,
                        "areas": [area(row, col, last, col)]}),
                    false,
                )
            }
            6 | 7 => {
                // Paste a block, sometimes shifted so that it reaches a read-only column.
                let to_row = 2 + pick.below(1000 - 4) as u32;
                let shift = if pick.next() < 0.25 { 1 } else { 0 };
                let mode = *pick.pick(&["all", "values"]);
                (
                    "paste",
                    json!({"kind": "paste", "fromSheet": 0,
                        "from": area(row.min(998), run_left, row.min(998) + rows - 1, run_right),
                        "sheet": 0,
                        "to": area(to_row, run_left + shift, to_row + rows - 1, run_right + shift),
                        "mode": mode}),
                    touches_read_only(run_left + shift, run_right + shift),
                )
            }
            8 => {
                let lines: Vec<String> = (0..rows)
                    .map(|_| {
                        (run_left..=run_right)
                            .map(|col| typing_for(col, &mut pick).replace('\'', ""))
                            .collect::<Vec<_>>()
                            .join("\t")
                    })
                    .collect();
                (
                    "paste text",
                    json!({"kind": "pasteExternal", "sheet": 0, "row": row.min(998),
                        "col": run_left, "text": lines.join("\r\n")}),
                    false,
                )
            }
            9 => (
                "fill down",
                json!({"kind": "fill", "sheet": 0, "from": area(row, run_left, row, run_right),
                    "to": area(row, run_left, last, run_right), "copy": pick.next() < 0.5}),
                false,
            ),
            10 => (
                "clear",
                json!({"kind": "clear", "sheet": 0, "areas": [area(row, run_left, last, run_right)]}),
                false,
            ),
            11 => {
                let col = *pick.pick(&editable_cols());
                let to = 2 + pick.below(1000 - 4) as u32;
                (
                    "move",
                    json!({"kind": "move", "fromSheet": 0, "from": area(row.min(998), col, row.min(998) + rows - 1, col),
                        "sheet": 0, "row": to, "col": col}),
                    false,
                )
            }
            12 => {
                let (col, find, with) = *pick.pick(&[
                    (4u32, "0", "9"),
                    (2, "Nile", "Delta"),
                    (8, "0", "00"),
                    (7, "St", "Street"),
                ]);
                (
                    "replace",
                    json!({"kind": "replaceOne", "query": {"text": find, "sheet": 0}, "with": with,
                        "sheet": 0, "row": row, "col": col}),
                    false,
                )
            }
            13 => (
                "convert",
                json!({"kind": "convertToNumber", "sheet": 0, "areas": [area(row, 7, last, 8)]}),
                false,
            ),
            14 => {
                let keys: Vec<Value> = (0..1 + pick.below(2))
                    .map(|_| json!({"index": 1 + pick.below(14), "descending": pick.next() < 0.5}))
                    .collect();
                (
                    "sort",
                    serde_json::from_str::<Value>(&format!(
                        r#"{{"kind":"sort","sheet":0,"area":{data},"keys":{}}}"#,
                        Value::Array(keys)
                    ))
                    .unwrap(),
                    false,
                )
            }
            15 => {
                if pick.next() < 0.5 {
                    let kept: Vec<&str> = ["LOCAL", "IMPORT", "SERVICES"]
                        .into_iter()
                        .filter(|_| pick.next() < 0.6)
                        .collect();
                    (
                        "filter",
                        json!({"kind": "setFilter", "sheet": 0, "table": "Vendors", "col": 3,
                            "criteria": {"kind": "values", "values": kept}}),
                        false,
                    )
                } else {
                    (
                        "clear filter",
                        json!({"kind": "clearFilter", "sheet": 0, "table": "Vendors"}),
                        false,
                    )
                }
            }
            16 | 17 => ("undo", json!({"kind": "undo"}), false),
            18 => ("redo", json!({"kind": "redo"}), false),
            _ => {
                if pick.next() < 0.15 {
                    let areas = (pick.next() < 0.5).then(|| vec![area(row, 1, last, 14)]);
                    (
                        "discard",
                        json!({"kind": "erpDiscard", "areas": areas}),
                        false,
                    )
                } else {
                    // A changed value typed back as it was.
                    let col = *pick.pick(&editable_cols());
                    let text = by_row_original(&mut engine, row, col);
                    (
                        "type back",
                        json!({"kind": "input", "sheet": 0, "row": row, "col": col, "text": text}),
                        false,
                    )
                }
            }
        };
        let json = json.to_string();
        let answer = engine.apply_json(&json);
        settle(&mut engine);
        match (&answer, refuse) {
            (Err(message), true) => assert_eq!(message, READ_ONLY, "step {step}: {json}"),
            (Ok(_), true) => panic!("step {step}: {json} was let through"),
            (Err(message), false) if op == "undo" || op == "redo" || op == "move" => {
                // Nothing to undo, or a move the engine refuses for its own reasons.
                let _ = message;
            }
            (Err(message), false) => panic!("step {step}: {json}: {message}"),
            (Ok(_), false) => {}
        }
        *tally.entry(op).or_default() += 1;
        assert_keys_with(&engine, &records, false);
        assert_changeset_is_the_oracles(&mut engine, &records, step, op);
    }
    let changed = counts(&engine)["changed"].as_u64().unwrap();
    assert!(
        tally.len() >= 15 && changed > 0,
        "{tally:?}, {changed} changed at the end"
    );
}

/// Where editing's time goes, natively: `cargo test --profile fast -p cgs-engine --test erp_edit
/// edit_steps -- --ignored --nocapture`. The browser's numbers are `bench/measure-erp-edit.mjs`'s.
#[test]
#[ignore]
fn edit_steps() {
    for n in [1000usize, 10_000] {
        let records = records(n, 5);
        let mut engine = bound(&records);
        let column: Vec<String> = (0..n).map(|i| format!("Name {i}")).collect();
        let paste = json!({"kind": "pasteExternal", "sheet": 0, "row": 2, "col": 2,
            "text": column.join("\r\n")})
        .to_string();
        let started = std::time::Instant::now();
        engine.apply_json(&paste).unwrap();
        let pasted = started.elapsed();
        let mut times = Vec::new();
        let mut size = 0;
        for _ in 0..5 {
            let started = std::time::Instant::now();
            size = engine.query(r#"{"kind":"erpChanges"}"#).unwrap().len();
            times.push(started.elapsed());
        }
        times.sort();
        eprintln!(
            "{n}: paste one column {pasted:?}, changeset {:?} median for {size} bytes",
            times[2]
        );
    }
}
