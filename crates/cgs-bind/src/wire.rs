//! Record pages as Query sends them (contract C.3), read from the bytes the connector transfers,
//! so no record object is ever built on the main thread. Pages are checked as they arrive and kept
//! in `Staging` until the binding takes them.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::base64::decode_version;
use crate::schema::BoundField;
use crate::value::{cell_value_owned, CellValue};

/// A value on the wire; `null` is `None` around it.
#[derive(Debug, Clone, PartialEq)]
pub enum WireValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

/// Read straight from the JSON token, rather than as serde's `untagged` reads it, which buffers
/// every value and tries each kind in turn: that cost most of a page's parse (Phase 19).
impl<'de> Deserialize<'de> for WireValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Wire;
        impl<'de> Visitor<'de> for Wire {
            type Value = WireValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a boolean, a number or a string")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<WireValue, E> {
                Ok(WireValue::Bool(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<WireValue, E> {
                Ok(WireValue::Number(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<WireValue, E> {
                Ok(WireValue::Number(v as f64))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<WireValue, E> {
                Ok(WireValue::Number(v))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<WireValue, E> {
                Ok(WireValue::Text(v.to_string()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<WireValue, E> {
                Ok(WireValue::Text(v))
            }
        }
        deserializer.deserialize_any(Wire)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireRecord {
    pub id: i64,
    pub key: String,
    pub version: String,
    pub values: HashMap<String, Option<WireValue>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub records: Vec<WireRecord>,
    pub next_after_id: Option<i64>,
    pub total: u32,
    pub as_of: String,
}

/// A record's values by field name, as a record's values are looked up.
trait RecordValues {
    fn count(&self) -> usize;
    /// Takes a field's value out: `Some(None)` for `null`, `None` when the field isn't there.
    fn take(&mut self, name: &str, at: usize) -> Option<Option<WireValue>>;
}

impl RecordValues for HashMap<String, Option<WireValue>> {
    fn count(&self) -> usize {
        HashMap::len(self)
    }
    fn take(&mut self, name: &str, _at: usize) -> Option<Option<WireValue>> {
        self.remove(name)
    }
}

/// A page's values in the order they came, with names borrowed from the page's bytes where
/// they have no escapes: no map is built per record. They usually come in the schema's order,
/// so a field is looked for at its own place first.
struct PageValues<'de>(Vec<(Cow<'de, str>, Option<Option<WireValue>>)>);

impl RecordValues for PageValues<'_> {
    fn count(&self) -> usize {
        self.0.len()
    }
    fn take(&mut self, name: &str, at: usize) -> Option<Option<WireValue>> {
        let found = match self.0.get(at) {
            Some((key, Some(_))) if key == name => Some(at),
            _ => self
                .0
                .iter()
                .position(|(key, value)| key == name && value.is_some()),
        };
        found.and_then(|index| self.0[index].1.take())
    }
}

/// A field name, borrowed from the page's bytes unless it has escapes.
struct Name<'a>(Cow<'a, str>);

impl<'de: 'a, 'a> Deserialize<'de> for Name<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Text<'a>(std::marker::PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for Text<'a> {
            type Value = Name<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a field name")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Name<'a>, E> {
                Ok(Name(Cow::Borrowed(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Name<'a>, E> {
                Ok(Name(Cow::Owned(v.to_string())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Name<'a>, E> {
                Ok(Name(Cow::Owned(v)))
            }
        }
        deserializer.deserialize_str(Text(std::marker::PhantomData))
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for PageValues<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Values<'a>(std::marker::PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for Values<'a> {
            type Value = PageValues<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a record's values")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<PageValues<'a>, A::Error> {
                let mut list = Vec::with_capacity(map.size_hint().unwrap_or(24));
                while let Some(Name(key)) = map.next_key::<Name<'a>>()? {
                    let value: Option<WireValue> = map.next_value()?;
                    list.push((key, Some(value)));
                }
                Ok(PageValues(list))
            }
        }
        deserializer.deserialize_map(Values(std::marker::PhantomData))
    }
}

#[derive(Deserialize)]
struct PageRecord<'a> {
    id: i64,
    #[serde(borrow)]
    key: Cow<'a, str>,
    #[serde(borrow)]
    version: Cow<'a, str>,
    #[serde(borrow)]
    values: PageValues<'a>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BorrowedPage<'a> {
    #[serde(borrow)]
    records: Vec<PageRecord<'a>>,
    next_after_id: Option<i64>,
    total: u32,
    as_of: String,
}

/// What a page added, for the load panel and the next request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub records: usize,
    pub staged: usize,
    pub next_after_id: Option<i64>,
    pub total: u32,
}

/// A record read off a page, its values in column order.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedRecord {
    pub id: i64,
    pub key: String,
    pub version: [u8; 8],
    pub values: Vec<CellValue>,
}

/// Pages on their way into a binding.
#[derive(Debug, Clone)]
pub struct Staging {
    pub fields: Vec<BoundField>,
    pub key_index: usize,
    pub max_records: u32,
    /// Singular and plural nouns for messages, such as "vendor" and "vendors".
    pub noun: String,
    pub records: Vec<StagedRecord>,
    keys: HashSet<String>,
    pub total: Option<u32>,
    pub as_of: Option<String>,
    done: bool,
}

impl Staging {
    pub fn new(fields: Vec<BoundField>, key_index: usize, max_records: u32, noun: &str) -> Self {
        Self {
            fields,
            key_index,
            max_records,
            noun: noun.to_string(),
            records: Vec::new(),
            keys: HashSet::new(),
            total: None,
            as_of: None,
            done: false,
        }
    }

    /// Whether the last page has come.
    pub fn complete(&self) -> bool {
        self.done
    }

    /// Takes the records it holds as the whole list, as a last page does.
    pub(crate) fn finish(&mut self) {
        self.keys = self
            .records
            .iter()
            .map(|record| record.key.clone())
            .collect();
        self.done = true;
    }

    /// Reads one page's bytes and keeps its records. The first page's `total` past the cap
    /// refuses the whole list, before anything else loads.
    pub fn page(&mut self, bytes: &[u8]) -> Result<PageInfo, String> {
        if self.done {
            return Err("The ERP sent another page after the last one.".to_string());
        }
        let page: BorrowedPage = serde_json::from_slice(bytes)
            .map_err(|error| format!("The ERP sent a page the sheet can't read: {error}"))?;
        if self.total.is_none() && page.total > self.max_records {
            return Err(too_many(page.total, self.max_records, &self.noun));
        }
        let before = self.records.len();
        let mut last = self.records.last().map_or(0, |r| r.id);
        for record in page.records {
            if record.id <= last {
                return Err("The ERP sent records out of id order.".to_string());
            }
            last = record.id;
            let staged = read_values(
                &self.fields,
                self.key_index,
                record.id,
                record.key.into_owned(),
                &record.version,
                record.values,
            )?;
            if !self.keys.insert(staged.key.clone()) {
                return Err(format!("The ERP sent {} twice.", staged.key));
            }
            self.records.push(staged);
            if self.records.len() > self.max_records as usize {
                return Err(too_many(page.total, self.max_records, &self.noun));
            }
        }
        if let Some(next) = page.next_after_id {
            if Some(next) != self.records.last().map(|r| r.id) {
                return Err("The ERP's next page doesn't follow its last record.".to_string());
            }
        } else {
            self.done = true;
        }
        self.total = Some(page.total);
        self.as_of = Some(page.as_of);
        Ok(PageInfo {
            records: self.records.len() - before,
            staged: self.records.len(),
            next_after_id: page.next_after_id,
            total: page.total,
        })
    }
}

/// A record as the ERP sends it, read against a binding's fields: its values in column order,
/// each checked against its field's type, with every field there and no others.
pub fn read_record(
    fields: &[BoundField],
    key_index: usize,
    record: WireRecord,
) -> Result<StagedRecord, String> {
    let WireRecord {
        id,
        key,
        version,
        values,
    } = record;
    read_values(fields, key_index, id, key, &version, values)
}

/// A record's parts read against a binding's fields, wherever its values came from.
fn read_values(
    fields: &[BoundField],
    key_index: usize,
    id: i64,
    key: String,
    version: &str,
    mut values: impl RecordValues,
) -> Result<StagedRecord, String> {
    if id <= 0 || key.is_empty() {
        return Err("The ERP sent a record without its id or key.".to_string());
    }
    let version = decode_version(version)
        .ok_or_else(|| format!("The ERP sent {key} without a version the sheet can read."))?;
    if values.count() != fields.len() {
        return Err(format!(
            "The ERP sent {key} with other fields than its schema's."
        ));
    }
    let mut cells = Vec::with_capacity(fields.len());
    for (index, field) in fields.iter().enumerate() {
        let wire = values
            .take(&field.name, index)
            .ok_or_else(|| format!("The ERP sent {key} without {}.", field.label))?;
        let cell = cell_value_owned(&field.kind, wire)
            .map_err(|problem| format!("{key}'s {} {}.", field.label, problem.describe()))?;
        if index == key_index && !matches!(&cell, CellValue::Text(text) if *text == key) {
            return Err(format!(
                "The ERP sent {key} with another key in its values."
            ));
        }
        cells.push(cell);
    }
    Ok(StagedRecord {
        id,
        key,
        version,
        values: cells,
    })
}

fn too_many(total: u32, max: u32, noun: &str) -> String {
    format!(
        "This list has {} {noun}, and a sheet opens at most {}. Narrow the search in Cash Gear ERP and open it again.",
        group(total),
        group(max)
    )
}

/// `12400` as `12,400`.
pub fn group(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::FieldKind;

    fn fields() -> Vec<BoundField> {
        let field = |name: &str, label: &str, kind: FieldKind| BoundField {
            name: name.into(),
            label: label.into(),
            kind,
            editable: false,
            edit_reason: Some("notEditable".into()),
            required: false,
            max_length: None,
            min: None,
            max: None,
        };
        vec![
            field("vendorAccount", "Account", FieldKind::Text),
            field("vendorName", "Name", FieldKind::Text),
            field("debitLimit", "Credit limit", FieldKind::Decimal(2)),
        ]
    }

    fn record(id: i64, key: &str, limit: &str) -> String {
        format!(
            r#"{{"id":{id},"key":"{key}","version":"AAAAAAAAB9E=","values":{{"vendorAccount":"{key}","vendorName":"N {id}","debitLimit":"{limit}"}}}}"#
        )
    }

    fn page(records: &[String], next: Option<i64>, total: u32) -> Vec<u8> {
        let next = next.map_or("null".to_string(), |n| n.to_string());
        format!(
            r#"{{"records":[{}],"nextAfterId":{next},"total":{total},"asOf":"2026-11-09T10:00:05Z"}}"#,
            records.join(",")
        )
        .into_bytes()
    }

    #[test]
    fn pages_come_in_id_order_until_the_last() {
        let mut staging = Staging::new(fields(), 0, 10, "vendors");
        let info = staging
            .page(&page(
                &[record(1, "V-1", "1.00"), record(5, "V-5", "2.50")],
                Some(5),
                3,
            ))
            .unwrap();
        assert_eq!(
            info,
            PageInfo {
                records: 2,
                staged: 2,
                next_after_id: Some(5),
                total: 3
            }
        );
        assert!(!staging.complete());
        staging
            .page(&page(&[record(9, "V-9", "0")], None, 3))
            .unwrap();
        assert!(staging.complete());
        assert_eq!(staging.records[1].values[2], CellValue::Number(2.5));
        assert!(staging.page(&page(&[], None, 3)).is_err());
    }

    #[test]
    fn a_list_past_the_cap_is_refused_from_its_first_page() {
        let mut staging = Staging::new(fields(), 0, 10_000, "vendors");
        let problem = staging.page(&page(&[record(1, "V-1", "1")], Some(1), 12_400));
        assert_eq!(
            problem.unwrap_err(),
            "This list has 12,400 vendors, and a sheet opens at most 10,000. Narrow the search in Cash Gear ERP and open it again."
        );
    }

    #[test]
    fn records_that_break_the_contract_are_refused() {
        let cases = [
            page(&[record(2, "V-2", "1"), record(1, "V-1", "1")], None, 2),
            page(&[record(1, "V-1", "1"), record(2, "V-1", "1")], None, 2),
            page(&[record(1, "V-1", "1.234")], None, 1),
            page(&[record(1, "V-1", "1")], Some(7), 1),
            page(
                &[r#"{"id":1,"key":"V-1","version":"x","values":{}}"#.to_string()],
                None,
                1,
            ),
            page(
                &[r#"{"id":1,"key":"V-1","version":"AAAAAAAAB9E=","values":{"vendorAccount":"V-2","vendorName":"x","debitLimit":"1"}}"#.to_string()],
                None,
                1,
            ),
            page(
                &[r#"{"id":1,"key":"V-1","version":"AAAAAAAAB9E=","values":{"vendorAccount":"V-1","vendorName":"x","debitLimit":"1","token":"t"}}"#.to_string()],
                None,
                1,
            ),
            b"not json".to_vec(),
        ];
        for (i, bytes) in cases.iter().enumerate() {
            let mut staging = Staging::new(fields(), 0, 10, "vendors");
            assert!(staging.page(bytes).is_err(), "case {i}");
        }
    }

    #[test]
    fn thousands_are_grouped() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(1240), "1,240");
        assert_eq!(group(1_000_000), "1,000,000");
    }
}
