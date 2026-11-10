//! ERP-bound tables through the engine (Phase 15): typed binding, the identity guard on every
//! refused path, refresh by id, and a property test over random sorts, filters, undo and redo
//! that every key stays with its own record's values. Phase 16's editing is in `erp_edit.rs`.

mod erp_harness;

use cgs_calc::CalcValue;
use cgs_engine::Engine;
use erp_harness::*;

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
    for (index, (_, label, _, _)) in FIELDS.iter().enumerate() {
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
        r#"{"kind":"input","sheet":0,"row":3,"col":5,"text":"USD"}"#.to_string(),
        r#"{"kind":"input","sheet":0,"row":1,"col":2,"text":"Heading"}"#.to_string(),
        r#"{"kind":"input","sheet":0,"row":20,"col":20,"text":"x","areas":[{"top":5,"left":4,"bottom":6,"right":5}]}"#.to_string(),
        r#"{"kind":"arrayInput","sheet":0,"area":{"top":2,"left":5,"bottom":3,"right":5},"text":"=1"}"#.to_string(),
        r#"{"kind":"clear","sheet":0,"areas":[{"top":4,"left":4,"bottom":4,"right":5}]}"#.to_string(),
        r#"{"kind":"fill","sheet":0,"from":{"top":2,"left":1,"bottom":2,"right":14},"to":{"top":2,"left":1,"bottom":5,"right":14}}"#.to_string(),
        r#"{"kind":"paste","fromSheet":0,"from":{"top":2,"left":1,"bottom":2,"right":14},"sheet":0,"to":{"top":5,"left":1,"bottom":5,"right":14}}"#.to_string(),
        r#"{"kind":"paste","fromSheet":0,"from":{"top":30,"left":1,"bottom":30,"right":1},"sheet":0,"to":{"top":5,"left":1,"bottom":5,"right":1},"mode":"values"}"#.to_string(),
        r#"{"kind":"move","fromSheet":0,"from":{"top":2,"left":1,"bottom":3,"right":14},"sheet":0,"row":40,"col":1}"#.to_string(),
        r#"{"kind":"move","fromSheet":0,"from":{"top":40,"left":1,"bottom":40,"right":1},"sheet":0,"row":2,"col":1}"#.to_string(),
        r#"{"kind":"pasteExternal","sheet":0,"row":4,"col":1,"text":"a\tb"}"#.to_string(),
        r#"{"kind":"replaceOne","query":{"text":"V-","sheet":0},"with":"W-","sheet":0,"row":2,"col":1}"#.to_string(),
        r#"{"kind":"convertToNumber","sheet":0,"areas":[{"top":2,"left":10,"bottom":13,"right":10}]}"#.to_string(),
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
    r#"{"kind":"input","sheet":0,"row":3,"col":5,"text":"USD"}"#,
    r#"{"kind":"input","sheet":0,"row":1,"col":2,"text":"Heading"}"#,
    r#"{"kind":"input","sheet":0,"row":20,"col":20,"text":"x","areas":[{"top":5,"left":4,"bottom":6,"right":5}]}"#,
    r#"{"kind":"arrayInput","sheet":0,"area":{"top":2,"left":5,"bottom":3,"right":5},"text":"=1"}"#,
    r#"{"kind":"clear","sheet":0,"areas":[{"top":4,"left":4,"bottom":4,"right":5}]}"#,
    r#"{"kind":"fill","sheet":0,"from":{"top":2,"left":1,"bottom":2,"right":14},"to":{"top":2,"left":1,"bottom":5,"right":14}}"#,
    r#"{"kind":"paste","fromSheet":0,"from":{"top":2,"left":1,"bottom":2,"right":14},"sheet":0,"to":{"top":5,"left":1,"bottom":5,"right":14}}"#,
    r#"{"kind":"move","fromSheet":0,"from":{"top":2,"left":1,"bottom":3,"right":14},"sheet":0,"row":40,"col":1}"#,
    r#"{"kind":"move","fromSheet":0,"from":{"top":40,"left":1,"bottom":40,"right":1},"sheet":0,"row":2,"col":1}"#,
    r#"{"kind":"pasteExternal","sheet":0,"row":4,"col":1,"text":"a\tb"}"#,
    r#"{"kind":"replaceOne","query":{"text":"V-","sheet":0},"with":"W-","sheet":0,"row":2,"col":1}"#,
    r#"{"kind":"convertToNumber","sheet":0,"areas":[{"top":2,"left":10,"bottom":13,"right":10}]}"#,
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
