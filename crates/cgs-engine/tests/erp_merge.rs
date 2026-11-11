//! The three-way merge (Phase 18, plan B.3 item 8 and G "Phase 18"): a property test of Refresh,
//! a 409 and Resolve Conflicts through the engine, against an oracle that applies the plan's table
//! to every field by itself. Seeded rounds of edits in the sheet and in the ERP (values that
//! clash, values that converge, renamed keys, records deleted and restored), sorts, refreshes,
//! commits the ERP checks by version, Resolve per field and for all, and Discard. After every step
//! each key is in its row once with its id and the version the oracle expects, every tracked cell
//! holds what the oracle says, the badge counts conflicts, gone records and changes as it does,
//! and no record the changeset would send with the ERP's current version could overwrite a value
//! the sheet never saw.

mod erp_harness;

use std::collections::HashMap;

use cgs_bind::base64::encode_version;
use cgs_calc::CalcValue;
use cgs_engine::Engine;
use erp_harness::*;
use serde_json::{json, Value};

/// The fields the rounds edit, by column (counted from 1): name, group, mobile, pay method and
/// address. The other editable fields never change, on either side.
const TRACKED: [u32; 5] = [2, 3, 4, 6, 7];
const MODIFIED_BY: u32 = 13;

#[derive(Clone, Debug, PartialEq)]
enum V {
    Empty,
    Text(String),
    Formula(String),
}

impl V {
    fn of(value: &CalcValue) -> Self {
        match value {
            CalcValue::Text(text) if !text.is_empty() => V::Text(text.clone()),
            _ => V::Empty,
        }
    }

    /// The same value as a cell compares it: a formula never equals anything.
    fn same(&self, other: &V) -> bool {
        !matches!(self, V::Formula(_)) && self == other
    }

    fn wire(&self) -> Value {
        match self {
            V::Text(text) => Value::String(text.clone()),
            _ => Value::Null,
        }
    }
}

/// A record as the ERP holds it.
#[derive(Clone)]
struct Server {
    base: Record,
    key: String,
    version: u64,
    s: [V; 5],
    by: String,
    present: bool,
}

impl Server {
    fn version_bytes(&self) -> [u8; 8] {
        self.version.to_be_bytes()
    }

    fn wire_values(&self) -> serde_json::Map<String, Value> {
        let mut values = serde_json::Map::new();
        for (index, (name, _, _, _)) in FIELDS.iter().enumerate() {
            let col = index as u32 + 1;
            let value = if col == 1 {
                Value::String(self.key.clone())
            } else if col == MODIFIED_BY {
                Value::String(self.by.clone())
            } else if let Some(slot) = TRACKED.iter().position(|&c| c == col) {
                self.s[slot].wire()
            } else {
                serde_json::from_str(&self.base.wire[index]).unwrap()
            };
            values.insert(name.to_string(), value);
        }
        values
    }

    fn wire(&self) -> Value {
        json!({
            "id": self.base.id,
            "key": self.key,
            "version": encode_version(&self.version_bytes()),
            "values": self.wire_values(),
        })
    }
}

/// What the oracle expects of one bound record.
#[derive(Clone, Debug)]
struct Expect {
    id: i64,
    key: String,
    o: [V; 5],
    c: [V; 5],
    v: u64,
    by: String,
    /// In conflict: the ERP's version and values then, and the slots that clashed.
    mark: Option<(u64, [V; 5], Vec<usize>)>,
    gone: bool,
}

impl Expect {
    fn has_changes(&self) -> bool {
        self.c.iter().zip(&self.o).any(|(c, o)| !c.same(o))
    }
}

#[derive(Default, Debug)]
struct Cases {
    take: usize,
    keep: usize,
    converged: usize,
    clash: usize,
    renamed: usize,
    gone: usize,
    merged_409: usize,
    resolved: usize,
    committed: usize,
}

/// The plan's table applied to one record, apart from the engine's code.
#[allow(clippy::needless_range_loop)]
fn merge(expect: &mut Expect, s: &[V; 5], version: u64, key: &str, by: &str, cases: &mut Cases) {
    let mut clashes = Vec::new();
    for slot in 0..5 {
        let (c, o, sv) = (&expect.c[slot], &expect.o[slot], &s[slot]);
        if c.same(o) {
            cases.take += 1;
            expect.c[slot] = sv.clone();
            expect.o[slot] = sv.clone();
        } else if sv.same(o) {
            cases.keep += 1;
            expect.o[slot] = sv.clone();
        } else if c.same(sv) {
            cases.converged += 1;
            expect.o[slot] = sv.clone();
        } else {
            cases.clash += 1;
            clashes.push(slot);
        }
    }
    if expect.key != key {
        cases.renamed += 1;
    }
    expect.key = key.to_string();
    expect.by = by.to_string();
    if clashes.is_empty() {
        expect.v = version;
        expect.mark = None;
    } else {
        expect.mark = Some((version, s.clone(), clashes));
    }
}

struct Run {
    engine: Engine,
    server: Vec<Server>,
    expect: Vec<Expect>,
    pick: Random,
    counter: u64,
    renames: usize,
    cases: Cases,
}

impl Run {
    fn rows(&self) -> HashMap<String, u32> {
        let summary = binding(&self.engine);
        let (top, bottom) = (
            summary["top"].as_u64().unwrap() as u32,
            summary["bottom"].as_u64().unwrap() as u32,
        );
        let cells = grid(&self.engine, top + 1, bottom);
        let mut rows = HashMap::new();
        for row in top + 1..=bottom {
            let CalcValue::Text(key) = cell(&cells, row, 1) else {
                panic!("row {row} has no key");
            };
            assert!(rows.insert(key.clone(), row).is_none(), "{key} twice");
        }
        rows
    }

    fn cell_now(&mut self, row: u32, col: u32) -> V {
        let edit = self.engine.edit_text(0, row, col);
        if edit.starts_with('=') {
            return V::Formula(edit);
        }
        let cells = grid(&self.engine, row, row);
        V::of(&cell(&cells, row, col))
    }

    fn type_into(&mut self, row: u32, col: u32, text: &str) {
        self.engine
            .apply_json(
                &json!({"kind": "input", "sheet": 0, "row": row, "col": col, "text": text})
                    .to_string(),
            )
            .unwrap_or_else(|error| panic!("typing {text}: {error}"));
        settle(&mut self.engine);
    }

    fn after_sheet_edit(&mut self) {
        for expect in &mut self.expect {
            if expect.mark.is_some() && !expect.has_changes() {
                expect.mark = None;
            }
        }
    }

    fn sheet_edits(&mut self) {
        let rows = self.rows();
        for _ in 0..self.pick.below(25) {
            let index = self.pick.below(self.expect.len());
            let slot = self.pick.below(5);
            let row = rows[&self.expect[index].key];
            let roll = self.pick.next();
            let (text, value) = if roll < 0.2 {
                // Back to the original.
                match &self.expect[index].o[slot] {
                    V::Text(text) => (text.clone(), V::Text(text.clone())),
                    _ => continue,
                }
            } else if roll < 0.25 && slot == 0 {
                let text = format!("=\"F{}\"", self.pick.below(3));
                (text.clone(), V::Formula(text))
            } else {
                let text = pool(slot, &mut self.pick).to_string();
                (text.clone(), V::Text(text))
            };
            self.type_into(row, TRACKED[slot], &text);
            self.expect[index].c[slot] = value;
        }
        self.after_sheet_edit();
    }

    fn erp_edits(&mut self) {
        let edited: Vec<usize> = (0..self.expect.len())
            .filter(|&i| self.expect[i].has_changes())
            .collect();
        for _ in 0..self.pick.below(15) {
            // Most often a record the sheet changed too, so changes meet.
            let index = if !edited.is_empty() && self.pick.next() < 0.6 {
                *self.pick.pick(&edited)
            } else {
                self.pick.below(self.server.len())
            };
            let roll = self.pick.next();
            self.counter += 1;
            let server = &mut self.server[index];
            if roll < 0.04 {
                server.present = false;
                continue;
            }
            if !server.present {
                if roll < 0.5 {
                    server.present = true;
                    server.version = self.counter;
                }
                continue;
            }
            if roll < 0.12 {
                self.renames += 1;
                server.key = format!("V-R{:04}", self.renames);
            }
            for slot in 0..5 {
                if self.pick.next() < 0.45 {
                    // Sometimes what the sheet typed, so the change converges.
                    let typed = self.expect[index].c[slot].clone();
                    server.s[slot] = if matches!(typed, V::Text(_)) && self.pick.next() < 0.3 {
                        typed
                    } else if slot >= 2 && slot != 3 && self.pick.next() < 0.15 {
                        V::Empty
                    } else {
                        V::Text(pool(slot, &mut self.pick).to_string())
                    };
                }
            }
            server.by = ["omar", "mona"][self.pick.below(2)].to_string();
            server.version = self.counter;
        }
    }

    fn refresh(&mut self) {
        let present: Vec<&Server> = self.server.iter().filter(|s| s.present).collect();
        let records: Vec<Value> = present.iter().map(|s| s.wire()).collect();
        self.engine.erp_begin_refresh().unwrap();
        let chunks: Vec<&[Value]> = records.chunks(97).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let next = if i + 1 < chunks.len() {
                chunk.last().unwrap()["id"].clone()
            } else {
                Value::Null
            };
            let page = json!({"records": chunk, "nextAfterId": next, "total": records.len(), "asOf": "2026-11-11T09:00:00Z"});
            self.engine.erp_page(page.to_string().as_bytes()).unwrap();
        }
        if records.is_empty() {
            let page = json!({"records": [], "nextAfterId": null, "total": 0, "asOf": "2026-11-11T09:00:00Z"});
            self.engine.erp_page(page.to_string().as_bytes()).unwrap();
        }
        let summary: Value = serde_json::from_str(&self.engine.erp_refresh().unwrap()).unwrap();
        settle(&mut self.engine);
        let mut conflicts = 0;
        for (expect, server) in self.expect.iter_mut().zip(&self.server) {
            expect.gone = !server.present;
            if !server.present {
                expect.mark = None;
                self.cases.gone += 1;
                continue;
            }
            merge(
                expect,
                &server.s,
                server.version,
                &server.key,
                &server.by,
                &mut self.cases,
            );
            conflicts += usize::from(expect.mark.is_some());
        }
        assert_eq!(summary["conflicts"], conflicts, "the refresh's conflicts");
        assert_eq!(
            summary["missing"],
            self.server.iter().filter(|s| !s.present).count(),
            "the refresh's missing records"
        );
    }

    /// Commits what the changeset holds as the ERP would: all of it when every version is
    /// current, else a 409 for those that aren't.
    fn commit(&mut self) {
        let changes = query(&self.engine, r#"{"kind":"erpChanges"}"#);
        let records = changes["records"].as_array().cloned().unwrap_or_default();
        if records.is_empty() {
            return;
        }
        let by_id: HashMap<i64, usize> = self
            .server
            .iter()
            .enumerate()
            .map(|(i, s)| (s.base.id, i))
            .collect();
        let sent: Vec<Value> = records
            .iter()
            .map(|r| json!({"id": r["id"], "changes": r["changes"]}))
            .collect();
        let stale: Vec<usize> = records
            .iter()
            .map(|r| by_id[&r["id"].as_i64().unwrap()])
            .filter(|&i| {
                let server = &self.server[i];
                !server.present
                    || encode_version(&server.version_bytes())
                        != records.iter().find(|r| r["id"] == server.base.id).unwrap()["version"]
                            .as_str()
                            .unwrap()
            })
            .collect();
        if !stale.is_empty() {
            let conflicts: Vec<Value> = stale
                .iter()
                .filter(|&&i| self.server[i].present)
                .map(|&i| {
                    let server = &self.server[i];
                    json!({
                        "id": server.base.id, "key": server.key,
                        "version": encode_version(&server.version_bytes()),
                        "values": server.wire_values(), "fields": [],
                        "modifiedBy": server.by, "modifiedAt": "2026-11-11T09:30:00Z",
                    })
                })
                .collect();
            self.engine
                .erp_set_states(&json!({"sent": sent, "conflicts": conflicts}).to_string())
                .unwrap();
            settle(&mut self.engine);
            for &i in &stale {
                let server = self.server[i].clone();
                if !server.present {
                    continue;
                }
                self.cases.merged_409 += 1;
                merge(
                    &mut self.expect[i],
                    &server.s,
                    server.version,
                    &server.key,
                    &server.by,
                    &mut self.cases,
                );
            }
            return;
        }
        let mut persisted = Vec::new();
        for record in &records {
            let i = by_id[&record["id"].as_i64().unwrap()];
            self.counter += 1;
            let server = &mut self.server[i];
            for (name, change) in record["changes"].as_object().unwrap() {
                let col = FIELDS.iter().position(|f| f.0 == name).unwrap() as u32 + 1;
                let slot = TRACKED.iter().position(|&c| c == col).unwrap();
                server.s[slot] = match &change["to"] {
                    Value::String(text) => V::Text(text.clone()),
                    _ => V::Empty,
                };
            }
            server.by = "u17".into();
            server.version = self.counter;
            persisted.push(server.wire());
            let expect = &mut self.expect[i];
            expect.o = server.s.clone();
            expect.v = server.version;
            expect.by = "u17".into();
            self.cases.committed += 1;
        }
        self.engine
            .erp_apply_commit(&json!({"sent": sent, "records": persisted}).to_string())
            .unwrap();
        settle(&mut self.engine);
    }

    fn resolve(&mut self) {
        let list = query(&self.engine, r#"{"kind":"erpConflicts"}"#);
        let entries = list["entries"].as_array().cloned().unwrap_or_default();
        if entries.is_empty() {
            return;
        }
        let roll = self.pick.next();
        let mut chosen: Vec<(usize, usize, bool)> = Vec::new();
        let index_of =
            |key: &str, expect: &[Expect]| expect.iter().position(|e| e.key == key).unwrap();
        let input = if roll < 0.3 {
            for (i, expect) in self.expect.iter().enumerate() {
                if let Some((_, _, slots)) = &expect.mark {
                    chosen.extend(slots.iter().map(|&slot| (i, slot, false)));
                }
            }
            json!({"all": "mine"})
        } else if roll < 0.6 {
            for (i, expect) in self.expect.iter().enumerate() {
                if let Some((_, _, slots)) = &expect.mark {
                    chosen.extend(slots.iter().map(|&slot| (i, slot, true)));
                }
            }
            json!({"all": "erp"})
        } else {
            let mut choices = Vec::new();
            for entry in &entries {
                if self.pick.next() < 0.5 {
                    continue;
                }
                let erp = self.pick.next() < 0.5;
                let key = entry["key"].as_str().unwrap();
                let col = entry["col"].as_u64().unwrap() as u32;
                let slot = TRACKED.iter().position(|&c| c == col).unwrap();
                chosen.push((index_of(key, &self.expect), slot, erp));
                choices.push(json!({"key": key, "field": entry["field"], "take": if erp { "erp" } else { "mine" }}));
            }
            json!({"choices": choices})
        };
        // Every listed field is one the oracle has in conflict.
        let listed: usize = self
            .expect
            .iter()
            .filter_map(|e| e.mark.as_ref())
            .map(|(_, _, slots)| slots.len())
            .sum();
        assert_eq!(entries.len(), listed, "Resolve lists every clashing field");
        self.engine.erp_resolve(&input.to_string()).unwrap();
        settle(&mut self.engine);
        let mut touched: Vec<usize> = Vec::new();
        for &(i, slot, erp) in &chosen {
            let (_, s, _) = self.expect[i].mark.clone().unwrap();
            if erp {
                self.expect[i].c[slot] = s[slot].clone();
            } else {
                self.expect[i].o[slot] = s[slot].clone();
            }
            if !touched.contains(&i) {
                touched.push(i);
            }
            self.cases.resolved += 1;
        }
        for i in touched {
            let (version, s, _) = self.expect[i].mark.clone().unwrap();
            let (key, by) = (self.expect[i].key.clone(), self.expect[i].by.clone());
            merge(&mut self.expect[i], &s, version, &key, &by, &mut self.cases);
        }
    }

    fn discard(&mut self) {
        self.engine.apply_json(r#"{"kind":"erpDiscard"}"#).unwrap();
        settle(&mut self.engine);
        for expect in &mut self.expect {
            expect.c = expect.o.clone();
        }
        self.after_sheet_edit();
    }

    fn sort(&mut self) {
        let col = *self.pick.pick(&[1u32, 2, 4, 6, 7]);
        let bottom = self.expect.len() as u32 + 1;
        self.engine
            .apply_json(
                &json!({"kind": "sort", "sheet": 0,
                    "area": {"top": 2, "left": 1, "bottom": bottom, "right": FIELDS.len()},
                    "keys": [{"index": col, "descending": self.pick.next() < 0.5}]})
                .to_string(),
            )
            .unwrap();
        settle(&mut self.engine);
    }

    fn check(&mut self, step: &str) {
        let rows = self.rows();
        assert_eq!(
            rows.len(),
            self.expect.len(),
            "{step}: the table keeps its rows"
        );
        let mut conflicts = 0;
        let mut gone = 0;
        let mut changed = 0;
        for i in 0..self.expect.len() {
            let expect = self.expect[i].clone();
            let row = *rows
                .get(&expect.key)
                .unwrap_or_else(|| panic!("{step}: {} isn't in the table", expect.key));
            let (id, version) = self.engine.bound_record(&expect.key).unwrap();
            assert_eq!(id, expect.id, "{step}: {}'s id", expect.key);
            assert_eq!(
                version,
                encode_version(&expect.v.to_be_bytes()),
                "{step}: {}'s version",
                expect.key
            );
            for slot in 0..5 {
                let now = self.cell_now(row, TRACKED[slot]);
                match (&expect.c[slot], &now) {
                    (V::Formula(_), V::Formula(_)) => {}
                    (wanted, now) => assert_eq!(
                        now,
                        wanted,
                        "{step}: {}'s {}",
                        expect.key,
                        FIELDS[TRACKED[slot] as usize - 1].0
                    ),
                }
                if !expect.gone
                    && !matches!(expect.c[slot], V::Formula(_))
                    && !expect.c[slot].same(&expect.o[slot])
                {
                    changed += 1;
                }
            }
            let cells = grid(&self.engine, row, row);
            assert_eq!(
                V::of(&cell(&cells, row, MODIFIED_BY)),
                V::Text(expect.by.clone()),
                "{step}: {}'s read-only Modified by",
                expect.key
            );
            gone += usize::from(expect.gone);
            conflicts += usize::from(expect.mark.is_some() && !expect.gone);
        }
        let counts = &binding(&self.engine)["counts"];
        assert_eq!(counts["conflicts"], conflicts, "{step}: conflicts");
        assert_eq!(counts["gone"], gone, "{step}: records gone");
        assert_eq!(counts["changed"], changed, "{step}: changed fields");
        // No record the changeset would send at the ERP's current version can overwrite a value
        // the sheet never saw: every field it sends was the ERP's value as `from`, and every one
        // it doesn't send holds the ERP's value.
        let changes = query(&self.engine, r#"{"kind":"erpChanges"}"#);
        for record in changes["records"].as_array().unwrap() {
            let id = record["id"].as_i64().unwrap();
            let i = self.server.iter().position(|s| s.base.id == id).unwrap();
            let server = &self.server[i];
            assert!(!self.expect[i].gone, "{step}: a gone record is never sent");
            if !server.present
                || record["version"].as_str() != Some(&encode_version(&server.version_bytes()))
            {
                continue;
            }
            for slot in 0..5 {
                let name = FIELDS[TRACKED[slot] as usize - 1].0;
                match record["changes"].get(name) {
                    Some(change) => assert_eq!(
                        change["from"],
                        server.s[slot].wire(),
                        "{step}: {} sends {name} from what the ERP holds",
                        server.key
                    ),
                    None => {
                        let c = &self.expect[i].c[slot];
                        assert!(
                            matches!(c, V::Formula(_)) || c.same(&server.s[slot]),
                            "{step}: {} would leave {name} at {c:?} where the ERP holds {:?}",
                            server.key,
                            server.s[slot]
                        );
                    }
                }
            }
        }
    }
}

fn pool(slot: usize, pick: &mut Random) -> &'static str {
    match slot {
        0 => pick.pick(&["Alpha", "Beta", "Gamma"]),
        1 => pick.pick(&["LOCAL", "IMPORT", "SERVICES"]),
        2 => pick.pick(&["0100", "0111", "0122"]),
        3 => pick.pick(&["CASH", "NET30"]),
        _ => pick.pick(&["1 St", "2 St", "3 St"]),
    }
}

fn start(n: usize, seed: u32) -> Run {
    let records = records(n, seed);
    let engine = bound(&records);
    let mut server = Vec::with_capacity(n);
    let mut expect = Vec::with_capacity(n);
    for record in &records {
        let s: [V; 5] = TRACKED.map(|col| V::of(&record.cells[col as usize - 1]));
        server.push(Server {
            base: record.clone(),
            key: record.key.clone(),
            version: u64::from_be_bytes(record.version),
            s: s.clone(),
            by: "seed".into(),
            present: true,
        });
        expect.push(Expect {
            id: record.id,
            key: record.key.clone(),
            o: s.clone(),
            c: s,
            v: u64::from_be_bytes(record.version),
            by: "seed".into(),
            mark: None,
            gone: false,
        });
    }
    Run {
        engine,
        server,
        expect,
        pick: Random(seed ^ 0x5eed),
        counter: 10_000,
        renames: 0,
        cases: Cases::default(),
    }
}

#[test]
fn random_merges_keep_every_record_and_never_let_a_stale_value_through() {
    let mut run = start(240, 18);
    run.check("bound");
    for round in 0..90 {
        run.sheet_edits();
        run.check(&format!("round {round}, sheet edits"));
        run.erp_edits();
        if run.pick.next() < 0.3 {
            run.sort();
            run.check(&format!("round {round}, sort"));
        }
        let roll = run.pick.next();
        let conflicts = run.expect.iter().any(|e| e.mark.is_some() && !e.gone);
        let step = if conflicts && roll < 0.3 {
            run.resolve();
            "resolve"
        } else if roll < 0.5 {
            run.refresh();
            "refresh"
        } else if roll < 0.75 {
            // As a user would: Commit waits while anything is in conflict, and a refresh first
            // often leaves nothing stale.
            if run.pick.next() < 0.7 {
                run.refresh();
                run.check(&format!("round {round}, refresh before commit"));
            }
            // A 409 merges what it can; with nothing left in conflict, commit again.
            for attempt in 0..2 {
                if run.expect.iter().any(|e| e.mark.is_some() && !e.gone) {
                    break;
                }
                run.commit();
                run.check(&format!("round {round}, commit {attempt}"));
            }
            "commit"
        } else if roll < 0.93 {
            run.resolve();
            "resolve"
        } else {
            run.discard();
            "discard"
        };
        run.check(&format!("round {round}, {step}"));
    }
    let cases = &run.cases;
    println!("{cases:?}");
    for (name, n) in [
        ("take", cases.take),
        ("keep", cases.keep),
        ("converged", cases.converged),
        ("clash", cases.clash),
        ("renamed", cases.renamed),
        ("gone", cases.gone),
        ("409", cases.merged_409),
        ("resolved", cases.resolved),
        ("committed", cases.committed),
    ] {
        assert!(n > 0, "the rounds never reached {name}: {cases:?}");
    }
}

/// Every cell of the bound table, every key's id and version, and the badge's counts.
type TableState = (Vec<Vec<CalcValue>>, Vec<(String, i64, String)>, Value);

fn state_of(engine: &Engine) -> TableState {
    let summary = binding(engine);
    let (top, bottom) = (
        summary["top"].as_u64().unwrap() as u32,
        summary["bottom"].as_u64().unwrap() as u32,
    );
    let cells = grid(engine, top + 1, bottom);
    let mut rows = Vec::new();
    let mut keys = Vec::new();
    for row in top + 1..=bottom {
        let values: Vec<CalcValue> = (1..=FIELDS.len() as u32)
            .map(|col| cell(&cells, row, col))
            .collect();
        if let CalcValue::Text(key) = &values[0] {
            let (id, version) = engine.bound_record(key).unwrap();
            keys.push((key.clone(), id, version));
        }
        rows.push(values);
    }
    keys.sort();
    (rows, keys, summary["counts"].clone())
}

#[test]
fn a_recovery_draft_replays_to_the_state_it_left() {
    let mut run = start(60, 7);
    // What the worker journals: the bind payload, then each step.
    let draft = run
        .engine
        .erp_draft()
        .expect("a bound workbook has a draft");
    assert!(!draft.contains('\n'), "the draft is one line");
    let mut commands: Vec<String> = Vec::new();
    let rows = run.rows();
    for (i, text) in [(0usize, "Alpha"), (1, "Beta"), (2, "Gamma"), (3, "Alpha")] {
        let command = json!({"kind": "input", "sheet": 0, "row": rows[&run.expect[i].key], "col": 2, "text": text}).to_string();
        run.engine.apply_json(&command).unwrap();
        settle(&mut run.engine);
        commands.push(command);
    }
    // The ERP changes two of those records (one clashes, one doesn't), renames one and loses one.
    run.server[0].s[0] = V::Text("Omega".into());
    run.server[1].s[2] = V::Text("0199".into());
    run.server[5].key = "V-RENAMED".into();
    run.server[9].present = false;
    for (n, i) in [0usize, 1, 5].into_iter().enumerate() {
        run.server[i].version = 20_000 + n as u64;
    }
    let present: Vec<Value> = run
        .server
        .iter()
        .filter(|s| s.present)
        .map(|s| s.wire())
        .collect();
    run.engine.erp_begin_refresh().unwrap();
    let page = json!({"records": present, "nextAfterId": null, "total": present.len(), "asOf": "2026-11-11T09:00:00Z"});
    run.engine.erp_page(page.to_string().as_bytes()).unwrap();
    let summary: Value = serde_json::from_str(&run.engine.erp_refresh().unwrap()).unwrap();
    settle(&mut run.engine);
    assert_eq!(summary["conflicts"], 1);
    assert_eq!(summary["merged"], 1);
    let journal = summary["journal"].to_string();
    assert!(
        journal.len() < draft.len() / 4,
        "a refresh journals only what it changed ({} of {} bytes)",
        journal.len(),
        draft.len()
    );
    let resolve = json!({"all": "mine"}).to_string();
    run.engine.erp_resolve(&resolve).unwrap();
    settle(&mut run.engine);
    let left = state_of(&run.engine);

    // A new page replays it.
    let mut again = blank();
    again.erp_restore(&draft).unwrap();
    settle(&mut again);
    for command in &commands {
        again.apply_json(command).unwrap();
        settle(&mut again);
    }
    again.erp_replay_refresh(&journal).unwrap();
    settle(&mut again);
    again.erp_resolve(&resolve).unwrap();
    settle(&mut again);
    let back = state_of(&again);
    assert_eq!(back.0, left.0, "every cell");
    assert_eq!(back.1, left.1, "every key's id and version");
    assert_eq!(back.2, left.2, "the counts");
    assert_eq!(binding(&again)["missing"], 1);
    // A draft of another format is refused, not read wrongly.
    let mut other = blank();
    let wrong = draft.replacen("\"v\":1", "\"v\":2", 1);
    assert!(other.erp_restore(&wrong).is_err());
    assert!(blank().erp_restore("{not json").is_err());
}
