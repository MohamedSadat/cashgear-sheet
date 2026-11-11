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
use crate::merge::{is_change, merge_field, merged_originals, FieldMerge};
use crate::rules::{check, Rule};
use crate::schema::{BoundField, EntitySchema, FieldKind, Language, LookupEntry};
use crate::state::{
    field_state, same, wire_of, FieldState, States, Wire, COMMITTED, CONFLICT, FORMULA, GONE,
    INVALID, SERVER,
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

/// What a binding keeps in memory: see `Binding::memory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingMemory {
    pub editable_values: usize,
    pub originals_bytes: usize,
    pub binding_bytes: usize,
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
    /// The schema as the ERP sent it, for a recovery draft to bind again from.
    pub schema_json: String,
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
            schema_json: String::new(),
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

    /// What the binding keeps in memory for its records, as plan B.6 budgets it (at most 64 bytes
    /// an editable value): the editable values' count, the originals' bytes, and the bytes of
    /// all it keeps per record (originals, keys, ids and versions, and both maps' slots).
    /// Counted with this build's own sizes, so in WASM they are wasm32's, and with what an
    /// allocator adds to each allocation.
    pub fn memory(&self) -> BindingMemory {
        use std::mem::size_of;
        // dlmalloc's chunk header, and its 8-byte rounding.
        let heap = |bytes: usize| {
            if bytes == 0 {
                0
            } else {
                bytes.next_multiple_of(8) + 8
            }
        };
        let text = |value: &CellValue| match value {
            CellValue::Text(text) => heap(text.capacity()),
            _ => 0,
        };
        let mut values = 0;
        let mut originals = 0;
        let mut keys = 0;
        for (key, meta) in &self.records {
            values += meta.originals.len();
            originals += heap(size_of::<CellValue>() * meta.originals.len());
            originals += meta.originals.iter().map(text).sum::<usize>();
            keys += heap(key.capacity());
        }
        for key in self.keys_by_id.values() {
            keys += heap(key.capacity());
        }
        // hashbrown keeps a control byte a slot, and its slots at most 7/8 full.
        let slots = |map_len: usize, entry: usize| map_len * (entry + 1) * 8 / 7;
        let maps = slots(self.records.len(), size_of::<(String, RecordMeta)>())
            + slots(self.keys_by_id.len(), size_of::<(i64, String)>());
        BindingMemory {
            editable_values: values,
            originals_bytes: originals,
            binding_bytes: originals + keys + maps,
        }
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
            // A record the ERP no longer has shows so on every field, whatever its cells hold.
            if self.is_gone(key) {
                for field in &mut fields {
                    field.bits |= GONE;
                }
            }
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
                .filter(|&slot| fields[slot].has_change())
                .collect();
            if modified.is_empty() {
                self.marks.conflicts.remove(key);
            } else {
                // The fields that clashed stay marked until resolved, even typed back to their
                // originals, since the ERP holds another value there; with none listed, every
                // change is.
                let listed = if mark.fields.is_empty() {
                    &modified
                } else {
                    &mark.fields
                };
                for &slot in listed {
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

    /// Takes in the errors (400) Cash Gear ERP answered a dry run or a commit of `sent` with, by
    /// id and field. The sent records' errors are replaced, and their conflicts too unless the
    /// answer has errors, since the ERP checks versions only after them; other records' marks are
    /// kept. A record's own error marks each field it sent; errors that name nothing the commit
    /// carried are given back, since no cell can show them. A 409's conflicts are merged by the
    /// engine (`conflict_records`, `plan_merge`), which then compares the records' rows again and
    /// so lays the marks over their states.
    pub fn set_marks(&mut self, sent: &[SentRecord], errors: &[ApiError]) -> MarkSummary {
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

    /// What a 409's conflicts say the ERP holds now, record by record. A field the answer left
    /// out, or sent in a form the sheet can't read, is unknown, so its record can't be merged.
    pub fn conflict_records(&self, conflicts: &[ApiConflict]) -> Vec<ServerRecord> {
        conflicts
            .iter()
            .map(|conflict| ServerRecord {
                id: conflict.id,
                key: conflict.key.clone(),
                version: decode_version(&conflict.version),
                values: self
                    .fields
                    .iter()
                    .map(|field| match conflict.values.get(&field.name) {
                        None => None,
                        Some(wire) => cell_value(&field.kind, wire.as_ref()).ok(),
                    })
                    .collect(),
                modified_by: conflict.modified_by.clone(),
                modified_at: conflict.modified_at.clone(),
            })
            .collect()
    }

    /// What a record's conflict mark says the ERP holds now, for Resolve to merge with.
    pub fn conflict_record(&self, key: &str) -> Option<ServerRecord> {
        let mark = self.marks.conflicts.get(key)?;
        let id = self.records.get(key)?.id;
        Some(ServerRecord {
            id,
            key: None,
            version: mark.version,
            values: mark.values.clone(),
            modified_by: mark.modified_by.clone(),
            modified_at: mark.modified_at.clone(),
        })
    }

    /// Moves one editable field's original, as Keep Mine does with the ERP's value: the cell's
    /// change then stays pending against what the ERP holds.
    pub fn set_original(&mut self, key: &str, slot: usize, value: CellValue) {
        if let Some(meta) = self.records.get_mut(key) {
            if let Some(original) = meta.originals.get_mut(slot) {
                *original = value;
            }
        }
    }

    /// Plans a three-way merge of what the ERP holds now with the sheet's cells and originals
    /// (plan B.3, item 8). `now_of` finds a key's data row in the table now and what each of its
    /// fields' cells holds, in column order. With `complete`, `records` are every record the ERP
    /// still has, as a refresh asks for them, so any other bound record is missing.
    pub fn plan_merge(
        &self,
        records: Vec<ServerRecord>,
        now_of: &dyn Fn(&str) -> Option<RowCells>,
        complete: bool,
    ) -> Result<MergePlan, String> {
        let mut plan = MergePlan::default();
        let mut seen = BTreeSet::new();
        for server in records {
            let Some(old_key) = self.keys_by_id.get(&server.id) else {
                continue;
            };
            let Some(meta) = self.records.get(old_key) else {
                continue;
            };
            if server.values.len() != self.fields.len() {
                return Err(format!(
                    "The ERP sent {old_key} with other fields than its schema's."
                ));
            }
            seen.insert(server.id);
            let (row, now) =
                now_of(old_key).ok_or_else(|| format!("{old_key} isn't in the table any more."))?;
            let key = server
                .key
                .clone()
                .or_else(|| match &server.values[self.key_index] {
                    Some(CellValue::Text(key)) => Some(key.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| old_key.clone());
            let mut fields = Vec::with_capacity(self.editable.len());
            let mut blocking = Vec::new();
            let mut had_changes = false;
            for (slot, &field) in self.editable.iter().enumerate() {
                let kind = &self.fields[field].kind;
                let original = &meta.originals[slot];
                let merge = merge_field(kind, original, &now[field], server.values[field].as_ref());
                let changed = is_change(kind, original, &now[field]);
                had_changes |= changed;
                if merge == FieldMerge::Clash || (merge == FieldMerge::Unknown && changed) {
                    blocking.push(slot);
                }
                fields.push(merge);
            }
            let mut writes = Vec::new();
            for (field, bound) in self.fields.iter().enumerate() {
                let value = if field == self.key_index {
                    CellValue::Text(key.clone())
                } else {
                    match &server.values[field] {
                        Some(value) => value.clone(),
                        None => continue,
                    }
                };
                let write = match self.slot(field) {
                    // Read-only cells and the key always show what the ERP has.
                    None => now[field] != Content::of_cell(&value),
                    Some(slot) => {
                        fields[slot] == FieldMerge::Take && !same(&bound.kind, &value, &now[field])
                    }
                };
                if write {
                    writes.push((field, value));
                }
            }
            // The ERP changed it: its version or key moved, or a value differs from what the
            // sheet read, which a cell takes or a change meets.
            let changed = server.version != Some(meta.version)
                || key != *old_key
                || !writes.is_empty()
                || fields
                    .iter()
                    .any(|field| matches!(field, FieldMerge::Clash | FieldMerge::Converged));
            let merge = RecordMerge {
                id: server.id,
                old_key: old_key.clone(),
                key,
                row,
                writes,
                fields,
                blocking,
                changed,
                had_changes,
                server,
            };
            let renamed = merge.key != merge.old_key;
            plan.counts.records += 1;
            plan.counts.renamed += usize::from(renamed);
            plan.counts.changed += usize::from(merge.changed);
            if merge.adopts() {
                if merge.changed && merge.had_changes {
                    plan.counts.merged += 1;
                }
            } else {
                plan.counts.conflicts += 1;
            }
            plan.records.push(merge);
        }
        if complete {
            let mut missing: Vec<i64> = self
                .keys_by_id
                .keys()
                .filter(|id| !seen.contains(id))
                .copied()
                .collect();
            missing.sort_unstable();
            plan.counts.missing = missing.len();
            plan.missing = Some(missing);
        }
        // Keys stay unique once every rename is in.
        let mut keys: HashMap<&str, i64> = self
            .keys_by_id
            .iter()
            .map(|(id, key)| (key.as_str(), *id))
            .collect();
        for merge in &plan.records {
            keys.remove(merge.old_key.as_str());
        }
        for merge in &plan.records {
            if keys.insert(merge.key.as_str(), merge.id).is_some() {
                return Err(format!("The ERP sent {} for two records.", merge.key));
            }
        }
        Ok(plan)
    }

    /// Takes a merge in, after its cells were written: originals move to the ERP's values
    /// wherever they don't clash, a record with no clash adopts the ERP's version, and one with a
    /// clash keeps its own and a conflict mark that holds the ERP's version and record now. Marks
    /// follow a renamed key. A refresh also forgets which fields the last commit saved, and
    /// replaces the records missing from the ERP.
    pub fn apply_merge(&mut self, plan: MergePlan, refresh: bool) {
        let mut taken = Vec::with_capacity(plan.records.len());
        for merge in &plan.records {
            let meta = self.records.remove(&merge.old_key);
            let errors = self.marks.errors.remove(&merge.old_key);
            let committed = self.marks.committed.remove(&merge.old_key);
            let conflict = self.marks.conflicts.remove(&merge.old_key);
            taken.push((meta, errors, committed, conflict));
        }
        for (merge, (meta, errors, committed, conflict)) in plan.records.into_iter().zip(taken) {
            let Some(mut meta) = meta else {
                continue;
            };
            let server: Vec<Option<CellValue>> = self
                .editable
                .iter()
                .map(|&field| merge.server.values[field].clone())
                .collect();
            meta.originals = merged_originals(&merge.fields, &meta.originals, &server).into();
            let adopts = merge.adopts();
            if adopts {
                if let Some(version) = merge.server.version {
                    meta.version = version;
                }
            } else {
                // Who changed it is kept from an earlier answer about the same version.
                let earlier = conflict.filter(|mark| mark.version == merge.server.version);
                let by = merge
                    .server
                    .modified_by
                    .clone()
                    .or_else(|| earlier.as_ref().and_then(|mark| mark.modified_by.clone()));
                let at = merge
                    .server
                    .modified_at
                    .clone()
                    .or_else(|| earlier.as_ref().and_then(|mark| mark.modified_at.clone()));
                self.marks.conflicts.insert(
                    merge.key.clone(),
                    ConflictMark {
                        version: merge.server.version,
                        values: merge.server.values,
                        fields: merge.blocking,
                        modified_by: by,
                        modified_at: at,
                    },
                );
            }
            if let Some(errors) = errors {
                self.marks.errors.insert(merge.key.clone(), errors);
            }
            if let Some(committed) = committed.filter(|_| !refresh) {
                self.marks.committed.insert(merge.key.clone(), committed);
            }
            self.keys_by_id.insert(merge.id, merge.key.clone());
            self.records.insert(merge.key, meta);
        }
        if refresh {
            self.marks.committed.clear();
            if let Some(missing) = plan.missing {
                self.missing = missing.into_iter().collect();
                for id in &self.missing {
                    if let Some(key) = self.keys_by_id.get(id) {
                        self.marks.conflicts.remove(key);
                    }
                }
            }
        }
    }

    /// Whether the last refresh didn't find a record in the ERP.
    pub fn is_gone(&self, key: &str) -> bool {
        self.records
            .get(key)
            .is_some_and(|meta| self.missing.contains(&meta.id))
    }

    /// Every record as the binding knows it, for a recovery draft: id, key, version and each
    /// field's value, the originals for editable fields and `read_only(key, field)` for the rest.
    pub fn snapshot(
        &self,
        read_only: &dyn Fn(&str, usize) -> Option<CellValue>,
    ) -> Vec<StagedRecord> {
        let mut out: Vec<StagedRecord> = self
            .records
            .iter()
            .map(|(key, meta)| StagedRecord {
                id: meta.id,
                key: key.clone(),
                version: meta.version,
                values: (0..self.fields.len())
                    .map(|field| match self.slot(field) {
                        Some(slot) => meta.originals[slot].clone(),
                        None if field == self.key_index => CellValue::Text(key.clone()),
                        None => read_only(key, field).unwrap_or(CellValue::Empty),
                    })
                    .collect(),
            })
            .collect();
        out.sort_unstable_by_key(|record| record.id);
        out
    }
}

/// A key's data row in the table now, and what each of its fields' cells holds, in column order.
pub type RowCells = (u32, Vec<Content>);

/// What Cash Gear ERP holds now for one bound record, from a refresh or a 409.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerRecord {
    pub id: i64,
    /// Its key now, when the answer says.
    pub key: Option<String>,
    /// Its version now; `None` where the answer's can't be read, so it can't be adopted.
    pub version: Option<[u8; 8]>,
    /// Its values in column order; `None` where the answer didn't carry a field.
    pub values: Vec<Option<CellValue>>,
    pub modified_by: Option<String>,
    pub modified_at: Option<String>,
}

impl From<StagedRecord> for ServerRecord {
    fn from(record: StagedRecord) -> Self {
        Self {
            id: record.id,
            key: Some(record.key),
            version: Some(record.version),
            values: record.values.into_iter().map(Some).collect(),
            modified_by: None,
            modified_at: None,
        }
    }
}

/// One record's merge.
#[derive(Debug, Clone)]
pub struct RecordMerge {
    pub id: i64,
    pub old_key: String,
    pub key: String,
    pub row: u32,
    /// Cells to write, by field index: read-only fields and the key where they differ, and
    /// editable fields that take the ERP's value.
    pub writes: Vec<(usize, CellValue)>,
    /// Each editable field's case, in `Binding::editable`'s order.
    pub fields: Vec<FieldMerge>,
    /// The editable fields, by slot, that keep the record in conflict: those that clash, and
    /// changed ones the ERP's answer didn't carry.
    pub blocking: Vec<usize>,
    /// The ERP's version or key moved since the sheet read it.
    pub changed: bool,
    /// The record had changes in the sheet.
    pub had_changes: bool,
    pub server: ServerRecord,
}

impl RecordMerge {
    /// Whether the record takes the ERP's version: no field clashes, and the ERP's answer
    /// carried every field and a version the sheet can read.
    pub fn adopts(&self) -> bool {
        self.server.version.is_some() && !self.fields.iter().any(|field| field.blocks())
    }
}

/// What a merge found, for the status line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeCounts {
    /// Records the ERP answered for.
    pub records: usize,
    /// Records whose version or key the ERP moved since the sheet read them.
    pub changed: usize,
    /// Of those, records with changes in the sheet that merged without a clash.
    pub merged: usize,
    /// Records left in conflict.
    pub conflicts: usize,
    pub renamed: usize,
    /// Bound records the ERP no longer has.
    pub missing: usize,
}

/// A planned merge: what each record's cells take, and how the bookkeeping changes.
#[derive(Debug, Clone, Default)]
pub struct MergePlan {
    pub records: Vec<RecordMerge>,
    /// Every bound record the ERP no longer has, for a refresh.
    pub missing: Option<Vec<i64>>,
    pub counts: MergeCounts,
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
    use crate::state::MODIFIED;

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
    fn memory_counts_every_original_and_its_text() {
        let binding = bound();
        let memory = binding.memory();
        assert_eq!(memory.editable_values, 6);
        // Each record's slots, and each short text's 8 bytes and header, at the least.
        let slots = std::mem::size_of::<CellValue>() * 2;
        assert!(memory.originals_bytes >= 3 * (slots.next_multiple_of(8) + 8 + 2 * 16));
        assert!(memory.binding_bytes > memory.originals_bytes);
    }

    const V2: [u8; 8] = [0, 0, 0, 0, 0, 0, 9, 9];

    fn server(id: i64, key: &str, name: &str, pay: &str) -> ServerRecord {
        ServerRecord {
            id,
            key: Some(key.into()),
            version: Some(V2),
            values: vec![
                Some(CellValue::Text(key.into())),
                Some(CellValue::Text(name.into())),
                Some(CellValue::Text(pay.into())),
            ],
            modified_by: None,
            modified_at: None,
        }
    }

    /// The table's rows as cells now: key, name and pay method.
    type Rows = HashMap<String, (u32, Vec<Content>)>;

    fn rows(list: &[(&str, u32, &str, &str)]) -> Rows {
        list.iter()
            .map(|&(key, row, name, pay)| {
                (
                    key.to_string(),
                    (row, vec![text(key), text(name), text(pay)]),
                )
            })
            .collect()
    }

    fn merge(
        binding: &mut Binding,
        records: Vec<ServerRecord>,
        rows: &Rows,
        complete: bool,
    ) -> MergeCounts {
        let plan = binding
            .plan_merge(records, &|key| rows.get(key).cloned(), complete)
            .unwrap();
        let counts = plan.counts;
        binding.apply_merge(plan, complete);
        counts
    }

    #[test]
    fn a_refresh_merges_by_id_follows_renames_and_counts_the_missing() {
        let mut binding = bound();
        let now = rows(&[
            ("V-1", 4, "A", "CASH"),
            ("V-2", 2, "B", "CASH"),
            ("V-3", 3, "C", "CASH"),
        ]);
        let plan = binding
            .plan_merge(
                vec![
                    server(1, "V-1", "A2", "CASH"),
                    server(3, "V-30", "C", "CASH"),
                ],
                &|key| now.get(key).cloned(),
                true,
            )
            .unwrap();
        assert_eq!(plan.counts.records, 2);
        assert_eq!(plan.counts.renamed, 1);
        assert_eq!(plan.counts.changed, 2);
        assert_eq!(plan.counts.merged, 0, "neither had changes in the sheet");
        assert_eq!(plan.missing, Some(vec![2]));
        assert_eq!(plan.records[0].row, 4);
        assert_eq!(
            plan.records[0].writes,
            vec![(1, CellValue::Text("A2".into()))]
        );
        assert_eq!(
            plan.records[1].writes,
            vec![(0, CellValue::Text("V-30".into()))]
        );
        binding.apply_merge(plan, true);
        assert!(binding.record("V-3").is_none());
        assert_eq!(binding.record("V-30").unwrap().id, 3);
        assert_eq!(binding.key_of(3), Some("V-30"));
        assert_eq!(binding.record("V-1").unwrap().version, V2);
        assert_eq!(
            &binding.record("V-1").unwrap().originals[..1],
            &[CellValue::Text("A2".into())]
        );
        assert_eq!(binding.missing.iter().copied().collect::<Vec<_>>(), vec![2]);
        assert!(binding.is_gone("V-2"));
        // A record gone from the ERP shows so, and counts as nothing else.
        binding.note_record("V-2", &[text("B9"), text("CASH")]);
        let fields = binding.states.get("V-2").unwrap();
        assert!(fields.iter().all(|field| field.bits & GONE != 0));
        assert_eq!(binding.states.counts().gone, 1);
        assert_eq!(binding.states.counts().changed, 0);
    }

    #[test]
    fn edits_the_erp_didnt_touch_rebase_and_clashes_keep_the_old_version() {
        let mut binding = bound();
        // The sheet changed V-1's name and V-2's pay method; the ERP changed V-1's pay method
        // and V-2's pay method too, to another value.
        let now = rows(&[
            ("V-1", 2, "A mine", "CASH"),
            ("V-2", 3, "B", "NET30"),
            ("V-3", 4, "C", "CASH"),
        ]);
        for (key, (_, cells)) in &now {
            binding.note_record(key, &cells[1..]);
        }
        let counts = merge(
            &mut binding,
            vec![
                server(1, "V-1", "A", "NET60"),
                server(2, "V-2", "B", "NET60"),
                server(3, "V-3", "C", "CASH"),
            ],
            &now,
            true,
        );
        assert_eq!(counts.merged, 1);
        assert_eq!(counts.conflicts, 1);
        // V-1 merged: the ERP's version, its pay method as the original, the name pending.
        let one = binding.record("V-1").unwrap();
        assert_eq!(one.version, V2);
        assert_eq!(
            &*one.originals,
            &[CellValue::Text("A".into()), CellValue::Text("NET60".into())]
        );
        assert!(!binding.marks.conflicts.contains_key("V-1"));
        // V-2 clashes: its own version, its pay method's original kept, and a mark.
        let two = binding.record("V-2").unwrap();
        assert_eq!(binding.version_text("V-2").as_deref(), Some("AAAAAAAAB9E="));
        assert_eq!(two.originals[1], CellValue::Text("CASH".into()));
        let mark = &binding.marks.conflicts["V-2"];
        assert_eq!(mark.version, Some(V2));
        assert_eq!(mark.fields, vec![1]);
        binding.note_record("V-2", &[text("B"), text("NET30")]);
        assert_ne!(binding.states.get("V-2").unwrap()[1].bits & CONFLICT, 0);
        // Keep Mine moves the original to the ERP's value; merged again, the record adopts its
        // version and the change stays pending.
        binding.set_original("V-2", 1, CellValue::Text("NET60".into()));
        let again = binding.conflict_record("V-2").unwrap();
        let counts = merge(&mut binding, vec![again], &now, false);
        assert_eq!(counts.conflicts, 0);
        assert_eq!(binding.record("V-2").unwrap().version, V2);
        assert!(binding.marks.conflicts.is_empty());
        binding.note_record("V-2", &[text("B"), text("NET30")]);
        assert_eq!(
            binding.states.get("V-2").unwrap()[1].bits & (MODIFIED | CONFLICT),
            MODIFIED
        );
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
        binding.set_marks(&[sent(1, &[("payMethod", "NOPE".into())])], &[]);
        binding.note_record("V-1", &[text("A2"), text("NOPE")]);
        assert_eq!(binding.states.get("V-1").unwrap()[1].bits & SERVER, 0);
    }

    #[test]
    fn a_409_merges_what_it_carries_and_marks_only_what_clashes() {
        let mut binding = bound();
        let conflict = |name: Option<&str>, pay: Option<Option<&str>>| ApiConflict {
            id: 2,
            key: Some("V-2".into()),
            version: "AAAAAAAACQ0=".into(),
            values: [
                Some((
                    "vendorAccount".to_string(),
                    Some(crate::wire::WireValue::Text("V-2".into())),
                )),
                name.map(|name| {
                    (
                        "vendorName".to_string(),
                        Some(crate::wire::WireValue::Text(name.into())),
                    )
                }),
                pay.map(|pay| {
                    (
                        "payMethod".to_string(),
                        pay.map(|pay| crate::wire::WireValue::Text(pay.into())),
                    )
                }),
            ]
            .into_iter()
            .flatten()
            .collect(),
            fields: vec!["vendorName".into()],
            modified_by: Some("omar".into()),
            modified_at: Some("2026-11-09T10:04:11Z".into()),
        };
        let now = rows(&[("V-2", 3, "B2", "NET30")]);
        binding.note_record("V-2", &[text("B2"), text("NET30")]);
        let both = sent(
            2,
            &[("vendorName", "B2".into()), ("payMethod", "NET30".into())],
        );
        // Omar changed the name too: it clashes, the pay method merges.
        binding.set_marks(std::slice::from_ref(&both), &[]);
        let records = binding.conflict_records(&[conflict(Some("B by Omar"), Some(Some("CASH")))]);
        let counts = merge(&mut binding, records, &now, false);
        assert_eq!((counts.conflicts, counts.merged), (1, 0));
        binding.note_record("V-2", &[text("B2"), text("NET30")]);
        let fields = binding.states.get("V-2").unwrap();
        assert_ne!(fields[0].bits & CONFLICT, 0);
        assert_eq!(fields[1].bits & CONFLICT, 0);
        assert_eq!(binding.states.counts().conflicts, 1);
        let mark = &binding.marks.conflicts["V-2"];
        assert_eq!(mark.values[1], Some(CellValue::Text("B by Omar".into())));
        assert_eq!(mark.modified_by.as_deref(), Some("omar"));
        // The binding keeps its own version, so no commit passes the ERP's check.
        assert_eq!(binding.version_text("V-2").as_deref(), Some("AAAAAAAAB9E="));
        // A field the answer didn't carry can't be compared, so the record stays in conflict.
        let mut fresh = bound();
        fresh.note_record("V-2", &[text("B2"), text("NET30")]);
        let records = fresh.conflict_records(&[conflict(Some("B"), None)]);
        assert_eq!(merge(&mut fresh, records, &now, false).conflicts, 1);
        assert_eq!(fresh.marks.conflicts["V-2"].fields, vec![1]);
        // Nothing clashes: the record takes the ERP's version, and the next commit sends it.
        let mut clean = bound();
        clean.note_record("V-2", &[text("B2"), text("NET30")]);
        let records = clean.conflict_records(&[conflict(Some("B"), Some(Some("CASH")))]);
        let counts = merge(&mut clean, records, &now, false);
        assert_eq!((counts.conflicts, counts.merged), (0, 1));
        assert_eq!(clean.version_text("V-2").as_deref(), Some("AAAAAAAACQ0="));
        // A 400 says nothing of versions, so the conflict stays; an answer without errors ends it.
        binding.set_marks(
            std::slice::from_ref(&both),
            &[error(Some(2), Some("payMethod"), "lookup.notFound")],
        );
        binding.note_record("V-2", &[text("B2"), text("NET30")]);
        assert_eq!(binding.states.counts().conflicts, 1);
        assert!(binding.marks.errors.contains_key("V-2"));
        binding.set_marks(std::slice::from_ref(&both), &[]);
        binding.note_record("V-2", &[text("B2"), text("NET30")]);
        assert!(binding.marks.errors.is_empty());
        assert_eq!(binding.states.counts().conflicts, 0);
        // Discarding a conflicting record's changes ends its conflict.
        let records = binding.conflict_records(&[conflict(Some("B by Omar"), Some(Some("CASH")))]);
        merge(&mut binding, records, &now, false);
        binding.note_record("V-2", &[text("B2"), text("NET30")]);
        assert_eq!(binding.states.counts().conflicts, 1);
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
        // A refresh forgets what the last commit saved; an error stays while its cell holds
        // what was sent.
        binding.take_commit("V-3", [0; 8], &persisted, vec![0, 1]);
        binding.note_record("V-1", &[text("A2"), text("CASH")]);
        binding.set_marks(
            &[sent(1, &[("vendorName", "A2".into())])],
            &[error(Some(1), None, "record.locked")],
        );
        let now = rows(&[
            ("V-1", 2, "A2", "CASH"),
            ("V-2", 3, "B", "CASH"),
            ("V-3", 4, "C2", "CASH"),
        ]);
        merge(
            &mut binding,
            vec![
                server(1, "V-1", "A", "CASH"),
                server(2, "V-2", "B", "CASH"),
                server(3, "V-3", "C2", "CASH"),
            ],
            &now,
            true,
        );
        assert!(binding.marks.committed.is_empty());
        assert!(binding.marks.errors.contains_key("V-1"));
    }

    #[test]
    fn a_refresh_that_would_repeat_a_key_is_refused() {
        let binding = bound();
        let now = rows(&[
            ("V-1", 2, "A", "CASH"),
            ("V-2", 3, "B", "CASH"),
            ("V-3", 4, "C", "CASH"),
        ]);
        assert!(binding
            .plan_merge(
                vec![server(1, "V-2", "A", "CASH")],
                &|key| now.get(key).cloned(),
                true
            )
            .is_err());
    }
}
