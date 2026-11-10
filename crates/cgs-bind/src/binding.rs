//! A bound table and its records: for each business key, the ERP's id, version and original
//! values. They live in the engine beside the cells and never in them, so no saved file holds
//! ids, versions or originals (plan B.3, items 1 and 9).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::base64::encode_version;
use crate::guard::{Content, TableArea};
use crate::rules::{check, Rule};
use crate::schema::{BoundField, EntitySchema, FieldKind, Language, LookupEntry};
use crate::state::{field_state, FieldState, States};
use crate::value::CellValue;
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

    /// Takes a record's cells in, as they are now.
    pub fn note_record(&mut self, key: &str, now: &[Content]) {
        if let Some(fields) = self.record_states(key, now) {
            self.states.set(key, fields);
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
