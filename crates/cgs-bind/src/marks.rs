//! What Cash Gear ERP said about a binding's fields (Phase 17, plan E.4 and E.5): the errors a dry
//! run or a commit refused them with (400), the records it says changed since they were read
//! (409), and the fields a commit saved. They are kept beside the states and laid over them each
//! time a record's cells are compared again, so an error stays on its cell only while the cell
//! holds what was sent, a conflict only while its record has changes, and Committed only until
//! the field changes (decisions, 2026-11-10, Phase 17).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::Wire;
use crate::value::CellValue;
use crate::wire::WireValue;

/// A field's new value, as a commit or a dry run sent it.
#[derive(Debug, Clone, Deserialize)]
pub struct SentChange {
    pub to: Value,
}

/// A record as a commit or a dry run sent it: its id and the fields it changed.
#[derive(Debug, Clone, Deserialize)]
pub struct SentRecord {
    pub id: i64,
    pub changes: HashMap<String, SentChange>,
}

/// One of the ERP's errors (contract `ApiError`): a field error has `id` and `field`, a record
/// error only `id`, a batch error neither.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub field: Option<String>,
    pub code: String,
    #[serde(default)]
    pub message: String,
}

/// One record of a 409 (contract `Conflict`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiConflict {
    pub id: i64,
    #[serde(default)]
    pub key: Option<String>,
    pub version: String,
    #[serde(default)]
    pub values: HashMap<String, Option<WireValue>>,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub modified_by: Option<String>,
    #[serde(default)]
    pub modified_at: Option<String>,
}

/// A value sent as JSON, as `wire_of` would give it for a cell.
pub fn wire_of_json(value: &Value) -> Wire {
    match value {
        Value::String(text) if text.is_empty() => Wire::Null,
        Value::String(text) => Wire::Text(text.clone()),
        Value::Number(n) => n.as_f64().map_or(Wire::Null, Wire::Number),
        Value::Bool(b) => Wire::Bool(*b),
        _ => Wire::Null,
    }
}

/// An error the ERP set on one field, while its cell holds `sent`.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerError {
    pub code: String,
    pub message: String,
    pub sent: Wire,
}

/// A record the ERP says changed since it was read. Its version and values now are kept for
/// Phase 18's Resolve; the binding keeps its own version, so no commit can pass the ERP's check.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictMark {
    pub version: Option<[u8; 8]>,
    /// The record now, in column order; `None` where the ERP sent nothing readable.
    pub values: Vec<Option<CellValue>>,
    /// The editable fields, by slot, whose value now differs from what was sent as `from`.
    pub fields: Vec<usize>,
    pub modified_by: Option<String>,
    pub modified_at: Option<String>,
}

/// Every mark, by key.
#[derive(Debug, Clone, Default)]
pub struct Marks {
    /// Each record's fields, by slot, that the ERP refused.
    pub errors: HashMap<String, Vec<(usize, ServerError)>>,
    pub conflicts: HashMap<String, ConflictMark>,
    /// Each record's fields, by slot, the last commit saved.
    pub committed: HashMap<String, Vec<usize>>,
}

impl Marks {
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty() && self.conflicts.is_empty() && self.committed.is_empty()
    }
}

/// An error that names no field the sheet sent, so it can't go on a cell: a batch error, or one
/// for a record or field the commit didn't carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unplaced {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub code: String,
    pub message: String,
}

/// What `Binding::set_marks` placed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkSummary {
    /// Fields that took an error.
    pub errors: usize,
    /// Records with an error on a field.
    pub error_records: usize,
    /// Records in conflict.
    pub conflicts: usize,
    pub unplaced: Vec<Unplaced>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sent_values_compare_as_cells_go_on_the_wire() {
        assert_eq!(wire_of_json(&json!("")), Wire::Null);
        assert_eq!(wire_of_json(&json!(null)), Wire::Null);
        assert_eq!(wire_of_json(&json!("NET30")), Wire::Text("NET30".into()));
        assert_eq!(wire_of_json(&json!(7)), Wire::Number(7.0));
        assert_eq!(wire_of_json(&json!(true)), Wire::Bool(true));
    }

    #[test]
    fn the_erps_bodies_read_as_the_contract_gives_them() {
        let error: ApiError = serde_json::from_value(json!({
            "id": 4411, "key": "V-1001", "field": "payMethod", "code": "lookup.notFound",
            "message": "طريقة الدفع غير موجودة"
        }))
        .unwrap();
        assert_eq!(error.field.as_deref(), Some("payMethod"));
        let batch: ApiError =
            serde_json::from_value(json!({"code": "schema.changed", "message": "x"})).unwrap();
        assert_eq!(batch.id, None);
        let conflict: ApiConflict = serde_json::from_value(json!({
            "id": 4411, "key": "V-1001", "version": "AAAAAAAACQ0=",
            "values": {"payMethod": "NET60", "mobile": null}, "fields": ["payMethod"],
            "modifiedBy": "omar", "modifiedAt": "2026-11-09T10:04:11Z"
        }))
        .unwrap();
        assert_eq!(conflict.values["mobile"], None);
        assert_eq!(conflict.modified_by.as_deref(), Some("omar"));
    }
}
