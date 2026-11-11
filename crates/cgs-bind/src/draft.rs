//! A bound workbook's recovery draft (Phase 18, plan B.5; decisions, 2026-11-11): its records as
//! compact arrays, versioned, so that a crashed page's binding can be built again exactly. Ids,
//! versions and originals reach the device only here, in the draft OPFS keeps for the crash; never
//! in a saved file.

use serde_json::{json, Value};

use crate::base64::{decode_version, encode_version};
use crate::binding::ServerRecord;
use crate::value::CellValue;
use crate::wire::{StagedRecord, Staging};

/// The draft's format. A draft of another format is refused rather than read wrongly.
pub const DRAFT_VERSION: u64 = 1;

/// A value as the draft keeps it: `null`, a string, a number or a boolean, which read back as
/// exactly the value it was.
pub fn value_json(value: &CellValue) -> Value {
    match value {
        CellValue::Empty => Value::Null,
        CellValue::Text(text) => Value::String(text.clone()),
        CellValue::Number(n) => json!(n),
        CellValue::Bool(b) => Value::Bool(*b),
    }
}

pub fn value_of_json(value: &Value) -> Option<CellValue> {
    Some(match value {
        Value::Null => CellValue::Empty,
        Value::String(text) => CellValue::Text(text.clone()),
        Value::Number(n) => CellValue::Number(n.as_f64()?),
        Value::Bool(b) => CellValue::Bool(*b),
        _ => return None,
    })
}

/// Records as `[id, key, version, [values…]]`.
pub fn records_json(records: &[StagedRecord]) -> Value {
    Value::Array(
        records
            .iter()
            .map(|record| {
                json!([
                    record.id,
                    record.key,
                    encode_version(&record.version),
                    record.values.iter().map(value_json).collect::<Vec<_>>(),
                ])
            })
            .collect(),
    )
}

/// Reads `records_json`'s records back, each with `fields` values.
pub fn records_of_json(value: &Value, fields: usize) -> Result<Vec<StagedRecord>, String> {
    let bad = || "The recovery draft's records can't be read.".to_string();
    let list = value.as_array().ok_or_else(bad)?;
    list.iter()
        .map(|item| {
            let parts = item
                .as_array()
                .filter(|parts| parts.len() == 4)
                .ok_or_else(bad)?;
            let values = parts[3]
                .as_array()
                .filter(|v| v.len() == fields)
                .ok_or_else(bad)?;
            Ok(StagedRecord {
                id: parts[0].as_i64().ok_or_else(bad)?,
                key: parts[1].as_str().ok_or_else(bad)?.to_string(),
                version: parts[2].as_str().and_then(decode_version).ok_or_else(bad)?,
                values: values
                    .iter()
                    .map(value_of_json)
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(bad)?,
            })
        })
        .collect()
}

/// A refresh's records as the draft keeps them. A refresh's records always carry every field.
pub fn server_records_json(records: &[ServerRecord]) -> Value {
    Value::Array(
        records
            .iter()
            .map(|record| {
                json!([
                    record.id,
                    record.key,
                    record.version.map(|version| encode_version(&version)),
                    record
                        .values
                        .iter()
                        .map(|value| value.as_ref().map_or(Value::Null, value_json))
                        .collect::<Vec<_>>(),
                ])
            })
            .collect(),
    )
}

/// Reads `server_records_json`'s records back.
pub fn server_records_of_json(value: &Value, fields: usize) -> Result<Vec<ServerRecord>, String> {
    let bad = || "The recovery draft's refresh can't be read.".to_string();
    let list = value.as_array().ok_or_else(bad)?;
    list.iter()
        .map(|item| {
            let parts = item
                .as_array()
                .filter(|parts| parts.len() == 4)
                .ok_or_else(bad)?;
            let values = parts[3]
                .as_array()
                .filter(|v| v.len() == fields)
                .ok_or_else(bad)?;
            Ok(ServerRecord {
                id: parts[0].as_i64().ok_or_else(bad)?,
                key: parts[1].as_str().map(str::to_string),
                version: parts[2].as_str().and_then(decode_version),
                values: values
                    .iter()
                    .map(|value| value_of_json(value).map(Some))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(bad)?,
                modified_by: None,
                modified_at: None,
            })
        })
        .collect()
}

impl Staging {
    /// A complete staging of records read back from a draft.
    pub fn restored(mut self, records: Vec<StagedRecord>, as_of: Option<String>) -> Self {
        self.records = records;
        self.total = Some(self.records.len() as u32);
        self.as_of = as_of;
        self.finish();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_read_back_exactly_as_they_were() {
        let records = vec![
            StagedRecord {
                id: 4411,
                key: "V-1001".into(),
                version: [0, 0, 0, 0, 0, 0, 7, 209],
                values: vec![
                    CellValue::Text("V-1001".into()),
                    CellValue::Text("001".into()),
                    CellValue::Number(15000.0),
                    CellValue::Number(46335.5),
                    CellValue::Bool(false),
                    CellValue::Empty,
                    CellValue::Text("مؤسسة النور".into()),
                ],
            },
            StagedRecord {
                id: 4412,
                key: "V-1002".into(),
                version: [255; 8],
                values: vec![CellValue::Empty; 7],
            },
        ];
        let json = records_json(&records);
        let text = serde_json::to_string(&json).unwrap();
        assert!(!text.contains('\n'), "a draft line holds no line break");
        let back = records_of_json(&serde_json::from_str(&text).unwrap(), 7).unwrap();
        assert_eq!(back, records);
        assert!(records_of_json(&json, 6).is_err());
        assert!(records_of_json(&json!([[1, "V", "not base64", []]]), 0).is_err());
    }
}
