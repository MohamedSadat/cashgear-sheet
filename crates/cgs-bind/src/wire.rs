//! Record pages as Query sends them (contract C.3), read from the bytes the connector transfers,
//! so no record object is ever built on the main thread. Pages are checked as they arrive and kept
//! in `Staging` until the binding takes them.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::base64::decode_version;
use crate::schema::BoundField;
use crate::value::{cell_value, CellValue};

/// A value on the wire; `null` is `None` around it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum WireValue {
    Bool(bool),
    Number(f64),
    Text(String),
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

    /// Reads one page's bytes and keeps its records. The first page's `total` past the cap
    /// refuses the whole list, before anything else loads.
    pub fn page(&mut self, bytes: &[u8]) -> Result<PageInfo, String> {
        if self.done {
            return Err("The ERP sent another page after the last one.".to_string());
        }
        let page: Page = serde_json::from_slice(bytes)
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
            let staged = self.read(record)?;
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

    fn read(&self, record: WireRecord) -> Result<StagedRecord, String> {
        let WireRecord {
            id,
            key,
            version,
            mut values,
        } = record;
        if id <= 0 || key.is_empty() {
            return Err("The ERP sent a record without its id or key.".to_string());
        }
        let version = decode_version(&version)
            .ok_or_else(|| format!("The ERP sent {key} without a version the sheet can read."))?;
        if values.len() != self.fields.len() {
            return Err(format!(
                "The ERP sent {key} with other fields than its schema's."
            ));
        }
        let mut cells = Vec::with_capacity(self.fields.len());
        for (index, field) in self.fields.iter().enumerate() {
            let wire = values
                .remove(&field.name)
                .ok_or_else(|| format!("The ERP sent {key} without {}.", field.label))?;
            let cell = cell_value(&field.kind, wire.as_ref())
                .map_err(|problem| format!("{key}'s {} {}.", field.label, problem.describe()))?;
            if index == self.key_index && cell != CellValue::Text(key.clone()) {
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
