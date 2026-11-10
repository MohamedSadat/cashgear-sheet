//! A bound table and its records: for each business key, the ERP's id, version and original
//! values. They live in the engine beside the cells and never in them, so no saved file holds
//! ids, versions or originals (plan B.3, items 1 and 9).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::base64::{decode_version, encode_version};
use crate::guard::{Content, TableArea};
use crate::marks::{
    wire_of_json, ApiConflict, ApiError, ConflictMark, MarkSummary, Marks, SentRecord, ServerError,
    Unplaced,
};
use crate::rules::{check, Rule};
use crate::schema::{BoundField, EntitySchema, FieldKind, Language, LookupEntry};
use crate::state::{
    field_state, wire_of, FieldState, States, Wire, COMMITTED, CONFLICT, FORMULA, INVALID,
    MODIFIED, SERVER,
};
use crate::value::{cell_value, CellValue};
use crate::wire::{StagedRecord, Staging};

/// The session a binding came from, as Redeem answered it, without its token.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub company: String,
    pub user_id: String,
    pub user_name: String,
    pub language: String,
    pub entity: String,
    pub entity_route: String,
}

impl SessionInfo {
    pub fn language(&self) -> Language {
        if self.language == "ar" {
            Language::Ar
        } else {
            Language::En
        }
    }
}

/// What the binding keeps for one record.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordMeta {
    pub id: i64,
    pub version: [u8; 8],
    /// The editable fields' values as loaded, in `Binding::editable`'s order.
    pub originals: Box<[CellValue]>,
}

#[derive(Debug, Clone)]
pub struct Binding {
    /// The model's table id, which moving rows or columns doesn't change.
    pub table_id: u32,
    pub table_name: String,
    pub sheet: u32,
    pub session: SessionInfo,
    /// The entity's name in the session's language, as the sheet and workbook are named.
    pub entity_label: String,
    /// Its English name, for the app's own messages until they come in Arabic too.
    pub entity_label_en: String,
    pub schema_version: String,
    pub key_field: String,
    pub max_page_size: u32,
    pub max_records: u32,
    pub max_commit_records: u32,
    pub fields: Vec<BoundField>,
    pub key_index: usize,
    /// The columns, by field index, whose originals are kept.
    pub editable: Vec<usize>,
    /// For each field, its place in `editable`, if it has one.
    slots: Vec<Option<usize>>,
    /// The schema's lookup lists, with labels in the session's language.
    pub lookups: BTreeMap<String, Vec<LookupEntry>>,
    /// The records whose editable fields differ from the ERP's.
    pub states: States,
    /// What the ERP said about fields: errors, conflicts and what the last commit saved.
    pub marks: Marks,
    records: HashMap<String, RecordMeta>,
    keys_by_id: HashMap<i64, String>,
    /// Records the last refresh didn't find in the ERP any more.
    pub missing: BTreeSet<i64>,
    pub as_of: String,
}

/// What a refresh writes, row by row, and how the records' bookkeeping changes with it.
#[derive(Debug, Clone, Default)]
pub struct RefreshPlan {
    /// A data row and the whole record's values for it, in column order.
    pub rows: Vec<(u32, Vec<CellValue>)>,
    pub updated: usize,
    pub renamed: usize,
    pub missing: Vec<i64>,
    records: Vec<(String, StagedRecord)>,
}

impl Binding {
    /// A binding of records staged against `schema`, as written into the table `table_id`.
    pub fn new(
        schema: &EntitySchema,
        session: SessionInfo,
        staging: Staging,
        table_id: u32,
        table_name: String,
        sheet: u32,
    ) -> Self {
        let editable: Vec<usize> = staging
            .fields
            .iter()
            .enumerate()
            .filter(|&(index, field)| field.editable && index != staging.key_index)
            .map(|(index, _)| index)
            .collect();
        let mut slots = vec![None; staging.fields.len()];
        for (slot, &index) in editable.iter().enumerate() {
            slots[index] = Some(slot);
        }
        let mut binding = Self {
            table_id,
            table_name,
            sheet,
            entity_label: schema.label.get(session.language()).to_string(),
            entity_label_en: schema.label.en.clone(),
            lookups: schema.lookup_lists(session.language()),
            session,
            schema_version: schema.schema_version.clone(),
            key_field: schema.key_field.clone(),
            max_page_size: schema.max_page_size,
            max_records: schema.max_records,
            max_commit_records: schema.max_commit_records,
            fields: staging.fields.clone(),
            key_index: staging.key_index,
            editable,
            slots,
            states: States::default(),
            marks: Marks::default(),
            records: HashMap::with_capacity(staging.records.len()),
            keys_by_id: HashMap::with_capacity(staging.records.len()),
            missing: BTreeSet::new(),
            as_of: staging.as_of.clone().unwrap_or_default(),
        };
        for record in staging.records {
            binding.keep(record);
        }
        binding
    }

    fn keep(&mut self, record: StagedRecord) {
        let originals: Box<[CellValue]> = self
            .editable
            .iter()
            .map(|&index| record.values[index].clone())
            .collect();
        self.keys_by_id.insert(record.id, record.key.clone());
        self.records.insert(
            record.key,
            RecordMeta {
                id: record.id,
                version: record.version,
                originals,
            },
        );
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn record(&self, key: &str) -> Option<&RecordMeta> {
        self.records.get(key)
    }

    pub fn key_of(&self, id: i64) -> Option<&str> {
        self.keys_by_id.get(&id).map(String::as_str)
    }

    pub fn version_text(&self, key: &str) -> Option<String> {
        self.records.get(key).map(|r| encode_version(&r.version))
    }

    /// Every bound id, ascending, as a refresh asks for them.
    pub fn ids(&self) -> Vec<i64> {
        let mut ids: Vec<i64> = self.keys_by_id.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// The columns' heading row and data rows, given the table's place now.
    pub fn columns(&self, area: &TableArea) -> std::ops::RangeInclusive<u32> {
        area.left..=area.right
    }

    /// Whether a field may be edited in the sheet: what the schema says for this caller. The key
    /// never is, whatever the schema says, since it keeps each row with its record.
    pub fn editable_in_sheet(&self, field: usize) -> bool {
        self.slots.get(field).is_some_and(Option::is_some)
    }

    /// A field's place among the editable fields, whose originals and states are kept.
    pub fn slot(&self, field: usize) -> Option<usize> {
        self.slots.get(field).copied().flatten()
    }

    /// A lookup field's list.
    pub fn lookup_list(&self, field: usize) -> Option<&[LookupEntry]> {
        match &self.fields.get(field)?.kind {
            FieldKind::Lookup(list) => self.lookups.get(list).map(Vec::as_slice),
            _ => None,
        }
    }

    /// A lookup's code as its list spells it, for text typed in another case.
    pub fn lookup_spelling(&self, field: usize, text: &str) -> Option<&str> {
        let list = self.lookup_list(field)?;
        if list.iter().any(|item| item.code == text) {
            return None;
        }
        list.iter()
            .find(|item| item.code.to_lowercase() == text.to_lowercase())
            .map(|item| item.code.as_str())
    }

    /// The rule an editable field's value breaks, compared with its record's original.
    pub fn rule(&self, field: usize, original: &CellValue, now: &Content) -> Option<Rule> {
        check(&self.fields[field], now, original, self.lookup_list(field))
    }

    /// A record's editable fields compared with what the ERP sent, given what its cells hold now
    /// in `editable`'s order. `None` for a key the binding doesn't know.
    pub fn record_states(&self, key: &str, now: &[Content]) -> Option<Vec<FieldState>> {
        let record = self.records.get(key)?;
        Some(
            self.editable
                .iter()
                .zip(record.originals.iter())
                .zip(now)
                .map(|((&field, original), now)| {
                    field_state(&self.fields[field].kind, original, now, || {
                        self.rule(field, original, now)
                    })
                })
                .collect(),
        )
    }

    /// Takes a record's cells in, as they are now, with the ERP's marks laid over them.
    pub fn note_record(&mut self, key: &str, now: &[Content]) {
        if let Some(mut fields) = self.record_states(key, now) {
            self.overlay_marks(key, now, &mut fields);
            self.states.set(key, fields);
        }
    }

    /// Lays the ERP's marks over a record's states, and drops the ones its cells have outgrown:
    /// an error once its cell no longer holds what was sent, a conflict once the record has no
    /// changes, and Committed once its field changes.
    fn overlay_marks(&mut self, key: &str, now: &[Content], fields: &mut [FieldState]) {
        if self.marks.is_empty() {
            return;
        }
        let editable = &self.editable;
        let schema = &self.fields;
        if let Some(list) = self.marks.errors.get_mut(key) {
            list.retain(|(slot, error)| {
                fields[*slot].bits & FORMULA == 0
                    && wire_of(&schema[editable[*slot]].kind, &now[*slot]) == error.sent
            });
            for (slot, _) in list.iter() {
                fields[*slot].bits |= SERVER | INVALID;
            }
            if list.is_empty() {
                self.marks.errors.remove(key);
            }
        }
        if let Some(mark) = self.marks.conflicts.get(key) {
            let modified: Vec<usize> = (0..fields.len())
                .filter(|&slot| fields[slot].bits & MODIFIED != 0)
                .collect();
            if modified.is_empty() {
                self.marks.conflicts.remove(key);
            } else {
                let listed: Vec<usize> = mark
                    .fields
                    .iter()
                    .copied()
                    .filter(|slot| modified.contains(slot))
                    .collect();
                for slot in if listed.is_empty() { modified } else { listed } {
                    fields[slot].bits |= CONFLICT;
                }
            }
        }
        if let Some(slots) = self.marks.committed.get_mut(key) {
            slots.retain(|&slot| !fields[slot].has_change());
            for &slot in slots.iter() {
                fields[slot].bits |= COMMITTED;
            }
            if slots.is_empty() {
                self.marks.committed.remove(key);
            }
        }
    }

    /// A field's index by name.
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|field| field.name == name)
    }

    /// Takes in what Cash Gear ERP answered to a dry run or a commit of `sent`: its errors (400)
    /// by id and field, and its conflicts (409) by id. The sent records' errors are replaced, and
    /// their conflicts too unless the answer has errors, since the ERP checks versions only after
    /// them; other records' marks are kept. A record's own error marks each field it sent; errors that name
    /// nothing the commit carried are given back, since no cell can show them. The engine then
    /// compares the records' rows again, which lays the marks over their states.
    pub fn set_marks(
        &mut self,
        sent: &[SentRecord],
        errors: &[ApiError],
        conflicts: &[ApiConflict],
    ) -> MarkSummary {
        let mut summary = MarkSummary::default();
        let sent_by_key: HashMap<String, &SentRecord> = sent
            .iter()
            .filter_map(|record| Some((self.key_of(record.id)?.to_string(), record)))
            .collect();
        // Every answer is a verdict on errors. Versions are checked only once there are none, so
        // a 400 says nothing of conflicts, and those the records had stay.
        for key in sent_by_key.keys() {
            self.marks.errors.remove(key);
            if errors.is_empty() {
                self.marks.conflicts.remove(key);
            }
        }
        let mut records = HashSet::new();
        for error in errors {
            let unplaced = || Unplaced {
                key: error.key.clone(),
                field: error.field.clone(),
                code: error.code.clone(),
                message: error.message.clone(),
            };
            let found = error
                .id
                .and_then(|id| self.key_of(id))
                .and_then(|key| Some((key.to_string(), *sent_by_key.get(key)?)));
            let Some((key, record)) = found else {
                summary.unplaced.push(unplaced());
                continue;
            };
            let slot_of = |name: &str| self.field_index(name).and_then(|field| self.slot(field));
            let mut targets: Vec<(usize, Wire)> = match &error.field {
                Some(name) => slot_of(name)
                    .zip(record.changes.get(name))
                    .map(|(slot, change)| (slot, wire_of_json(&change.to)))
                    .into_iter()
                    .collect(),
                None => record
                    .changes
                    .iter()
                    .filter_map(|(name, change)| Some((slot_of(name)?, wire_of_json(&change.to))))
                    .collect(),
            };
            if targets.is_empty() {
                summary.unplaced.push(unplaced());
                continue;
            }
            targets.sort_by_key(|(slot, _)| *slot);
            let list = self.marks.errors.entry(key.clone()).or_default();
            for (slot, sent) in targets {
                // One error a field: the first the ERP gave.
                if list.iter().any(|(at, _)| *at == slot) {
                    continue;
                }
                list.push((
                    slot,
                    ServerError {
                        code: error.code.clone(),
                        message: error.message.clone(),
                        sent,
                    },
                ));
                summary.errors += 1;
            }
            records.insert(key);
        }
        summary.error_records = records.len();
        for conflict in conflicts {
            let Some(key) = self.key_of(conflict.id).map(str::to_string) else {
                continue;
            };
            let values = self
                .fields
                .iter()
                .map(|field| {
                    let wire = conflict.values.get(&field.name)?;
                    cell_value(&field.kind, wire.as_ref()).ok()
                })
                .collect();
            let fields = conflict
                .fields
                .iter()
                .filter_map(|name| self.slot(self.field_index(name)?))
                .collect();
            self.marks.conflicts.insert(
                key,
                ConflictMark {
                    version: decode_version(&conflict.version),
                    values,
                    fields,
                    modified_by: conflict.modified_by.clone(),
                    modified_at: conflict.modified_at.clone(),
                },
            );
            summary.conflicts += 1;
        }
        summary
    }

    /// Forgets what the last commit saved, before the next one's result goes in.
    pub fn clear_committed(&mut self) {
        self.marks.committed.clear();
    }

    /// Takes in a committed record: its new version, its persisted values (in column order) as
    /// the originals, and the fields the commit saved, which show Committed. Its errors and
    /// conflict go: the ERP accepted it.
    pub fn take_commit(
        &mut self,
        key: &str,
        version: [u8; 8],
        persisted: &[CellValue],
        committed: Vec<usize>,
    ) {
        let editable = &self.editable;
        if let Some(meta) = self.records.get_mut(key) {
            meta.version = version;
            meta.originals = editable
                .iter()
                .map(|&field| persisted[field].clone())
                .collect();
        }
        self.marks.errors.remove(key);
        self.marks.conflicts.remove(key);
        if committed.is_empty() {
            self.marks.committed.remove(key);
        } else {
            self.marks.committed.insert(key.to_string(), committed);
        }
    }

    /// A staging for a refresh of this binding's records.
    pub fn refresh_staging(&self) -> Staging {
        Staging::new(
            self.fields.clone(),
            self.key_index,
            self.max_records,
            &self.entity_label_en.to_lowercase(),
        )
    }

    /// Plans a refresh as a plain replace by id (Phase 15: nothing can be edited yet). `row_of`
    /// finds a key's data row in the table now. Records the ERP no longer has stay in place and
    /// are counted as missing.
    pub fn plan_refresh(
        &self,
        staging: Staging,
        row_of: &dyn Fn(&str) -> Option<u32>,
    ) -> Result<RefreshPlan, String> {
        let mut plan = RefreshPlan::default();
        let mut seen = BTreeSet::new();
        for record in staging.records {
            let Some(old_key) = self.keys_by_id.get(&record.id) else {
                continue;
            };
            seen.insert(record.id);
            let row =
                row_of(old_key).ok_or_else(|| format!("{old_key} isn't in the table any more."))?;
            if *old_key != record.key {
                plan.renamed += 1;
            }
            plan.updated += 1;
            plan.rows.push((row, record.values.clone()));
            plan.records.push((old_key.clone(), record));
        }
        plan.missing = self
            .keys_by_id
            .keys()
            .filter(|id| !seen.contains(id))
            .copied()
            .collect();
        plan.missing.sort_unstable();
        // Keys stay unique once every rename is in.
        let mut keys: HashMap<&str, i64> = self
            .keys_by_id
            .iter()
            .map(|(id, key)| (key.as_str(), *id))
            .collect();
        for (old, _) in &plan.records {
            keys.remove(old.as_str());
        }
        for (_, record) in &plan.records {
            if keys.insert(record.key.as_str(), record.id).is_some() {
                return Err(format!("The ERP sent {} for two records.", record.key));
            }
        }
        Ok(plan)
    }

    /// Takes a refresh's records in, after its rows were written.
    pub fn apply_refresh(&mut self, plan: RefreshPlan) {
        for (old_key, _) in &plan.records {
            self.records.remove(old_key);
        }
        for (_, record) in plan.records {
            self.keep(record);
        }
        self.missing = plan.missing.into_iter().collect();
        // Until Phase 18 merges, a refresh replaces every change, so nothing the ERP said of
        // them still applies.
        self.marks = Marks::default();
    }
}

/// Every bound table in a workbook.
#[derive(Debug, Clone, Default)]
pub struct Bindings(pub Vec<Binding>);

impl Bindings {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Binding> {
        self.0.iter()
    }

    pub fn by_table(&self, table_id: u32) -> Option<&Binding> {
        self.0.iter().find(|b| b.table_id == table_id)
    }

    pub fn by_table_mut(&mut self, table_id: u32) -> Option<&mut Binding> {
        self.0.iter_mut().find(|b| b.table_id == table_id)
    }

    pub fn holds_table_named(&self, name: &str) -> bool {
        self.0
            .iter()
            .any(|b| b.table_name.eq_ignore_ascii_case(name))
    }

    pub fn on_sheet(&self, sheet: u32) -> impl Iterator<Item = &Binding> {
        self.0.iter().filter(move |b| b.sheet == sheet)
    }

    pub fn push(&mut self, binding: Binding) {
        self.0.push(binding);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::FieldKind;

    fn schema() -> EntitySchema {
        EntitySchema::parse(
            r#"{"entity":"vendors","schemaVersion":"vendors.1","label":{"en":"Vendors","ar":"الموردون"},
            "keyField":"vendorAccount","idField":"recId","maxPageSize":1000,"maxRecords":10000,"maxCommitRecords":500,
            "fields":[
              {"name":"vendorAccount","label":{"en":"Account","ar":"الحساب"},"type":"text","editable":false,"editReason":"notEditable"},
              {"name":"vendorName","label":{"en":"Name","ar":"الاسم"},"type":"text","editable":true,"maxLength":150},
              {"name":"payMethod","label":{"en":"Payment method","ar":"طريقة الدفع"},"type":"lookup","lookup":"paymentModes","editable":true}],
            "lookups":{"paymentModes":[{"code":"CASH","label":{"en":"Cash","ar":"نقدي"}}]}}"#,
        )
        .unwrap()
    }

    fn session() -> SessionInfo {
        SessionInfo {
            company: "Kit".into(),
            user_id: "u17".into(),
            user_name: "Sara".into(),
            language: "ar".into(),
            entity: "vendors".into(),
            entity_route: "api/payable/VendorSheet".into(),
        }
    }

    fn page(records: &[(i64, &str, &str)]) -> Vec<u8> {
        let list: Vec<String> = records
            .iter()
            .map(|(id, key, name)| {
                format!(
                    r#"{{"id":{id},"key":"{key}","version":"AAAAAAAAB9E=","values":{{"vendorAccount":"{key}","vendorName":"{name}","payMethod":"CASH"}}}}"#
                )
            })
            .collect();
        format!(
            r#"{{"records":[{}],"nextAfterId":null,"total":{},"asOf":"2026-11-09T10:00:05Z"}}"#,
            list.join(","),
            records.len()
        )
        .into_bytes()
    }

    fn bound() -> Binding {
        let schema = schema();
        let fields = schema.bound_fields(Language::Ar);
        let mut staging = Staging::new(fields, schema.key_index(), 10_000, "vendors");
        staging
            .page(&page(&[(1, "V-1", "A"), (2, "V-2", "B"), (3, "V-3", "C")]))
            .unwrap();
        Binding::new(&schema, session(), staging, 7, "Vendors".into(), 0)
    }

    #[test]
    fn a_binding_keeps_ids_versions_and_originals_by_key() {
        let binding = bound();
        assert_eq!(binding.len(), 3);
        assert_eq!(binding.entity_label, "الموردون");
        assert_eq!(binding.fields[1].label, "الاسم");
        assert_eq!(
            binding.fields[2].kind,
            FieldKind::Lookup("paymentModes".into())
        );
        assert_eq!(binding.editable, vec![1, 2]);
        let record = binding.record("V-2").unwrap();
        assert_eq!(record.id, 2);
        assert_eq!(
            &*record.originals,
            &[CellValue::Text("B".into()), CellValue::Text("CASH".into())]
        );
        assert_eq!(binding.version_text("V-2").as_deref(), Some("AAAAAAAAB9E="));
        assert_eq!(binding.ids(), vec![1, 2, 3]);
    }

    #[test]
    fn a_refresh_replaces_by_id_follows_renames_and_counts_the_missing() {
        let mut binding = bound();
        let mut staging = binding.refresh_staging();
        staging
            .page(&page(&[(1, "V-1", "A2"), (3, "V-30", "C")]))
            .unwrap();
        let rows: HashMap<&str, u32> = [("V-1", 4), ("V-2", 2), ("V-3", 3)].into_iter().collect();
        let plan = binding
            .plan_refresh(staging, &|key| rows.get(key).copied())
            .unwrap();
        assert_eq!(plan.updated, 2);
        assert_eq!(plan.renamed, 1);
        assert_eq!(plan.missing, vec![2]);
        assert_eq!(plan.rows[0].0, 4);
        assert_eq!(plan.rows[0].1[1], CellValue::Text("A2".into()));
        assert_eq!(plan.rows[1].0, 3);
        binding.apply_refresh(plan);
        assert!(binding.record("V-3").is_none());
        assert_eq!(binding.record("V-30").unwrap().id, 3);
        assert_eq!(binding.key_of(3), Some("V-30"));
        assert_eq!(
            &binding.record("V-1").unwrap().originals[..1],
            &[CellValue::Text("A2".into())]
        );
        assert_eq!(binding.missing.iter().copied().collect::<Vec<_>>(), vec![2]);
    }

    fn text(s: &str) -> Content {
        Content::Text(s.into())
    }

    fn sent(id: i64, changes: &[(&str, serde_json::Value)]) -> SentRecord {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "changes": changes
                .iter()
                .map(|(name, to)| (name.to_string(), serde_json::json!({"from": null, "to": to})))
                .collect::<serde_json::Map<_, _>>(),
        }))
        .unwrap()
    }

    fn error(id: Option<i64>, field: Option<&str>, code: &str) -> ApiError {
        ApiError {
            id,
            key: None,
            field: field.map(str::to_string),
            code: code.into(),
            message: format!("ERP says {code}"),
        }
    }

    #[test]
    fn server_errors_stay_on_their_cells_only_while_they_hold_what_was_sent() {
        let mut binding = bound();
        let now = [text("A2"), text("NOPE")];
        binding.note_record("V-1", &now);
        let summary = binding.set_marks(
            &[sent(
                1,
                &[("vendorName", "A2".into()), ("payMethod", "NOPE".into())],
            )],
            &[
                error(Some(1), Some("payMethod"), "lookup.notFound"),
                error(Some(1), None, "record.locked"),
                error(None, None, "schema.changed"),
                error(Some(2), Some("vendorName"), "required"),
            ],
            &[],
        );
        assert_eq!(summary.errors, 2);
        assert_eq!(summary.error_records, 1);
        assert_eq!(summary.unplaced.len(), 2);
        binding.note_record("V-1", &now);
        let fields = binding.states.get("V-1").unwrap();
        assert_eq!(fields[0].bits, MODIFIED | SERVER | INVALID);
        assert_eq!(fields[1].bits & (SERVER | INVALID), SERVER | INVALID);
        // The first error a field was given stays.
        let code = |slot: usize| {
            binding.marks.errors["V-1"]
                .iter()
                .find(|(at, _)| *at == slot)
                .map(|(_, error)| error.code.clone())
        };
        assert_eq!(code(1).as_deref(), Some("lookup.notFound"));
        assert_eq!(code(0).as_deref(), Some("record.locked"));
        assert_eq!(binding.states.counts().invalid, 2);
        // Editing the name drops its error; the pay method keeps its own.
        binding.note_record("V-1", &[text("A3"), text("NOPE")]);
        let fields = binding.states.get("V-1").unwrap();
        assert_eq!(fields[0].bits, MODIFIED);
        assert_ne!(fields[1].bits & SERVER, 0);
        // Writing the sent value back doesn't bring a dropped error back.
        binding.note_record("V-1", &[text("A2"), text("NOPE")]);
        assert_eq!(binding.states.get("V-1").unwrap()[0].bits, MODIFIED);
        // A dry run that passes clears what the sent records had.
        binding.set_marks(&[sent(1, &[("payMethod", "NOPE".into())])], &[], &[]);
        binding.note_record("V-1", &[text("A2"), text("NOPE")]);
        assert_eq!(binding.states.get("V-1").unwrap()[1].bits & SERVER, 0);
    }

    #[test]
    fn conflicts_mark_their_fields_until_the_record_has_no_changes() {
        let mut binding = bound();
        let conflict = |fields: &[&str]| ApiConflict {
            id: 2,
            key: Some("V-2".into()),
            version: "AAAAAAAACQ0=".into(),
            values: [
                (
                    "vendorAccount".to_string(),
                    Some(crate::wire::WireValue::Text("V-2".into())),
                ),
                (
                    "vendorName".to_string(),
                    Some(crate::wire::WireValue::Text("B by Omar".into())),
                ),
                ("payMethod".to_string(), None),
            ]
            .into_iter()
            .collect(),
            fields: fields.iter().map(|f| f.to_string()).collect(),
            modified_by: Some("omar".into()),
            modified_at: Some("2026-11-09T10:04:11Z".into()),
        };
        let both = sent(
            2,
            &[("vendorName", "B2".into()), ("payMethod", "NET30".into())],
        );
        let summary = binding.set_marks(
            std::slice::from_ref(&both),
            &[],
            &[conflict(&["vendorName"])],
        );
        assert_eq!(summary.conflicts, 1);
        binding.note_record("V-2", &[text("B2"), text("NET30")]);
        let fields = binding.states.get("V-2").unwrap();
        assert_ne!(fields[0].bits & CONFLICT, 0);
        assert_eq!(fields[1].bits & CONFLICT, 0);
        assert_eq!(binding.states.counts().conflicts, 1);
        let mark = &binding.marks.conflicts["V-2"];
        assert_eq!(mark.values[1], Some(CellValue::Text("B by Omar".into())));
        assert_eq!(mark.values[2], Some(CellValue::Empty));
        // The binding keeps its own version, so no commit passes the ERP's check.
        assert_eq!(binding.version_text("V-2").as_deref(), Some("AAAAAAAAB9E="));
        // None of the commit's fields clash: every changed field shows it.
        binding.set_marks(&[both], &[], &[conflict(&[])]);
        binding.note_record("V-2", &[text("B2"), text("NET30")]);
        let fields = binding.states.get("V-2").unwrap();
        assert_ne!(fields[0].bits & fields[1].bits & CONFLICT, 0);
        // A 400 says nothing of versions, so the conflict stays; an answer without errors ends it.
        let sent_two = sent(
            2,
            &[("vendorName", "B2".into()), ("payMethod", "NET30".into())],
        );
        binding.set_marks(
            std::slice::from_ref(&sent_two),
            &[error(Some(2), Some("payMethod"), "lookup.notFound")],
            &[],
        );
        binding.note_record("V-2", &[text("B2"), text("NET30")]);
        assert_eq!(binding.states.counts().conflicts, 1);
        assert!(binding.marks.errors.contains_key("V-2"));
        binding.set_marks(std::slice::from_ref(&sent_two), &[], &[conflict(&[])]);
        binding.note_record("V-2", &[text("B2"), text("NET30")]);
        assert!(binding.marks.errors.is_empty());
        assert_eq!(binding.states.counts().conflicts, 1);
        // Discarding the record's changes ends its conflict.
        binding.note_record("V-2", &[text("B"), text("CASH")]);
        assert!(binding.states.get("V-2").is_none());
        assert!(binding.marks.conflicts.is_empty());
        assert_eq!(binding.states.counts().conflicts, 0);
    }

    #[test]
    fn a_commit_moves_versions_and_originals_and_shows_committed_until_the_field_changes() {
        let mut binding = bound();
        binding.note_record("V-3", &[text("C2"), text("CASH")]);
        binding.set_marks(
            &[sent(3, &[("vendorName", "C2".into())])],
            &[error(Some(3), Some("vendorName"), "maxLength")],
            &[],
        );
        let persisted = [
            CellValue::Text("V-3".into()),
            CellValue::Text("C2".into()),
            CellValue::Text("CASH".into()),
        ];
        binding.clear_committed();
        binding.take_commit("V-3", [0, 0, 0, 0, 0, 0, 9, 9], &persisted, vec![0]);
        assert_eq!(binding.version_text("V-3").as_deref(), Some("AAAAAAAACQk="));
        assert_eq!(binding.record("V-3").unwrap().originals[0], persisted[1]);
        assert!(binding.marks.errors.is_empty());
        binding.note_record("V-3", &[text("C2"), text("CASH")]);
        let fields = binding.states.get("V-3").unwrap();
        assert_eq!(fields[0].bits, COMMITTED);
        assert!(!fields[0].has_change());
        // Committed counts as nothing changed.
        assert_eq!(binding.states.counts(), crate::state::Counts::default());
        // An edit ends it, and changing back doesn't bring it back.
        binding.note_record("V-3", &[text("C3"), text("CASH")]);
        assert_eq!(binding.states.get("V-3").unwrap()[0].bits, MODIFIED);
        binding.note_record("V-3", &[text("C2"), text("CASH")]);
        assert!(binding.states.get("V-3").is_none());
        // A refresh forgets every mark.
        binding.take_commit("V-3", [0; 8], &persisted, vec![0, 1]);
        binding.set_marks(
            &[sent(1, &[("vendorName", "A2".into())])],
            &[error(Some(1), None, "record.locked")],
            &[],
        );
        let mut staging = binding.refresh_staging();
        staging
            .page(&page(&[(1, "V-1", "A"), (2, "V-2", "B"), (3, "V-3", "C2")]))
            .unwrap();
        let rows: HashMap<&str, u32> = [("V-1", 2), ("V-2", 3), ("V-3", 4)].into_iter().collect();
        let plan = binding
            .plan_refresh(staging, &|key| rows.get(key).copied())
            .unwrap();
        binding.apply_refresh(plan);
        assert!(binding.marks.is_empty());
    }

    #[test]
    fn a_refresh_that_would_repeat_a_key_is_refused() {
        let binding = bound();
        let mut staging = binding.refresh_staging();
        staging.page(&page(&[(1, "V-2", "A")])).unwrap();
        let rows: HashMap<&str, u32> = [("V-1", 2), ("V-2", 3), ("V-3", 4)].into_iter().collect();
        assert!(binding
            .plan_refresh(staging, &|key| rows.get(key).copied())
            .is_err());
    }
}
