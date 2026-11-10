//! Each record's editable fields compared with what the ERP sent (plan B.3, items 5 and 6): which
//! are modified, which break a local rule, and which hold a formula. Only records with something
//! to show are kept, so the set is the dirty-record set the status badge counts, kept up to date
//! as rows are written, and the changeset reads only those records.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use crate::guard::Content;
use crate::rules::Rule;
use crate::schema::FieldKind;
use crate::value::CellValue;

/// The value differs from the original.
pub const MODIFIED: u8 = 1;
/// A local rule refuses the value.
pub const INVALID: u8 = 2;
/// A formula or a spilled value, left out of commits until converted to values.
pub const FORMULA: u8 = 4;
/// Cash Gear ERP refused the value in a dry run or a commit (Phase 17); `INVALID` is set too.
pub const SERVER: u8 = 8;
/// The record changed in Cash Gear ERP since it was read, and this field is among its changes.
pub const CONFLICT: u8 = 16;
/// The last commit saved this field, and it hasn't changed since.
pub const COMMITTED: u8 = 32;

/// One editable field of one record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldState {
    pub bits: u8,
    pub rule: Option<Rule>,
}

impl FieldState {
    /// Whether the field shows nothing at all.
    pub fn is_clean(&self) -> bool {
        self.bits == 0
    }

    /// Whether the cell differs from what the ERP sent: a changed value or a formula.
    pub fn has_change(&self) -> bool {
        self.bits & (MODIFIED | FORMULA) != 0
    }
}

/// What the status badge shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    /// Fields whose values differ from the ERP's, formulas left out.
    pub changed: usize,
    /// Records with a changed field.
    pub changed_records: usize,
    /// Fields a local rule or Cash Gear ERP refuses.
    pub invalid: usize,
    pub formulas: usize,
    /// Records in conflict with Cash Gear ERP.
    pub conflicts: usize,
}

impl Counts {
    fn add(&mut self, fields: &[FieldState], sign: isize) {
        let mut changed = 0;
        for field in fields {
            changed += usize::from(field.bits & MODIFIED != 0);
            let step = |n: &mut usize, on: bool| {
                if on {
                    *n = n.wrapping_add_signed(sign);
                }
            };
            step(&mut self.changed, field.bits & MODIFIED != 0);
            step(&mut self.invalid, field.bits & INVALID != 0);
            step(&mut self.formulas, field.bits & FORMULA != 0);
        }
        if changed > 0 {
            self.changed_records = self.changed_records.wrapping_add_signed(sign);
        }
        if fields.iter().any(|field| field.bits & CONFLICT != 0) {
            self.conflicts = self.conflicts.wrapping_add_signed(sign);
        }
    }
}

/// The records that differ from the ERP's, by key.
#[derive(Debug, Clone, Default)]
pub struct States {
    by_key: HashMap<String, Box<[FieldState]>>,
    counts: Counts,
}

impl States {
    pub fn counts(&self) -> Counts {
        self.counts
    }

    pub fn get(&self, key: &str) -> Option<&[FieldState]> {
        self.by_key.get(key).map(|fields| &**fields)
    }

    /// Records with something to show, in no order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.by_key.keys().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Sets one record's fields, forgetting it if they are all clean.
    pub fn set(&mut self, key: &str, fields: Vec<FieldState>) {
        if let Some(old) = self.by_key.remove(key) {
            self.counts.add(&old, -1);
        }
        if fields.iter().any(|field| !field.is_clean()) {
            self.counts.add(&fields, 1);
            self.by_key
                .insert(key.to_string(), fields.into_boxed_slice());
        }
    }

    pub fn clear(&mut self) {
        self.by_key.clear();
        self.counts = Counts::default();
    }
}

impl Content {
    /// A value as the ERP sent it, as a cell holds it.
    pub fn of_cell(value: &CellValue) -> Self {
        match value {
            CellValue::Empty => Self::Empty,
            CellValue::Text(text) => Self::Text(text.clone()),
            CellValue::Number(n) => Self::number(*n),
            CellValue::Bool(b) => Self::Bool(*b),
        }
    }
}

/// `n` at `scale` decimals, as a whole number of the smallest unit.
fn units(n: f64, scale: u32) -> f64 {
    (n * 10f64.powi(scale as i32)).round()
}

/// Whether a cell still holds what the ERP sent. Empty and `""` are the same, decimals compare at
/// their scale, and a formula never equals a value.
pub fn same(kind: &FieldKind, original: &CellValue, now: &Content) -> bool {
    match (original, now) {
        (CellValue::Empty, Content::Empty) => true,
        (CellValue::Empty, Content::Text(text)) | (CellValue::Text(text), Content::Empty) => {
            text.is_empty()
        }
        (CellValue::Text(a), Content::Text(b)) => a == b,
        (CellValue::Number(a), Content::Number(bits)) => {
            let b = f64::from_bits(*bits);
            match kind {
                FieldKind::Decimal(scale) => units(*a, *scale) == units(b, *scale),
                _ => *a == b,
            }
        }
        (CellValue::Bool(a), Content::Bool(b)) => a == b,
        _ => false,
    }
}

/// A field's state from what its cell holds now.
pub fn field_state(
    kind: &FieldKind,
    original: &CellValue,
    now: &Content,
    rule: impl FnOnce() -> Option<Rule>,
) -> FieldState {
    if matches!(now, Content::Formula(_) | Content::Spill) {
        return FieldState {
            bits: FORMULA,
            rule: None,
        };
    }
    if same(kind, original, now) {
        return FieldState::default();
    }
    let rule = rule();
    FieldState {
        bits: MODIFIED | if rule.is_some() { INVALID } else { 0 },
        rule,
    }
}

/// A date serial in the 1900 date system as `YYYY-MM-DD`.
pub fn date_text(serial: f64) -> String {
    let mut days = serial.floor() as i64;
    // Excel counts a 1900-02-29 that never was.
    if days >= 60 {
        days -= 1;
    }
    // Days from 1970-01-01 to 1899-12-31 is -25568.
    let z = days - 25_568 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// A value as the contract sends it, ready to serialise: `null`, a string, a number or a boolean.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Wire {
    Text(String),
    Number(f64),
    Bool(bool),
    Null,
}

/// A value as the contract sends it: text as a string, decimals as strings at their scale, dates
/// as `YYYY-MM-DD`, booleans as booleans and empty as `null`. A value of the wrong type goes as it
/// is, for the server to refuse, though local rules refuse it first.
pub fn wire_of(kind: &FieldKind, value: &Content) -> Wire {
    match (kind, value) {
        (_, Content::Empty) => Wire::Null,
        (_, Content::Text(text)) if text.is_empty() => Wire::Null,
        (_, Content::Text(text)) => Wire::Text(text.clone()),
        (_, Content::Bool(b)) => Wire::Bool(*b),
        (_, Content::Error(error)) => Wire::Text(error.clone()),
        (FieldKind::Decimal(scale), Content::Number(bits)) => {
            let n = f64::from_bits(*bits);
            let text = format!("{:.*}", *scale as usize, n);
            let negative_zero =
                text.starts_with('-') && text[1..].bytes().all(|b| b == b'0' || b == b'.');
            Wire::Text(if negative_zero {
                text[1..].to_string()
            } else {
                text
            })
        }
        (FieldKind::Date, Content::Number(bits)) => Wire::Text(date_text(f64::from_bits(*bits))),
        (_, Content::Number(bits)) => {
            let n = f64::from_bits(*bits);
            if n.is_finite() {
                Wire::Number(n)
            } else {
                Wire::Null
            }
        }
        (_, Content::Formula(_) | Content::Spill) => Wire::Null,
    }
}

/// `wire_of` as a JSON value.
pub fn wire(kind: &FieldKind, value: &Content) -> Value {
    serde_json::to_value(wire_of(kind, value)).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::date;

    fn text(s: &str) -> Content {
        Content::Text(s.into())
    }

    #[test]
    fn a_value_changed_back_is_clean() {
        let kind = FieldKind::Text;
        let original = CellValue::Text("Al Noor".into());
        let clean = field_state(&kind, &original, &text("Al Noor"), || None);
        assert!(clean.is_clean());
        let changed = field_state(&kind, &original, &text("Al Noor Co"), || None);
        assert_eq!(changed.bits, MODIFIED);
        let bad = field_state(&kind, &original, &Content::Empty, || Some(Rule::Required));
        assert_eq!(bad.bits, MODIFIED | INVALID);
        let formula = field_state(&kind, &original, &Content::Formula("=B2".into()), || {
            Some(Rule::Type)
        });
        assert_eq!(
            formula,
            FieldState {
                bits: FORMULA,
                rule: None
            }
        );
    }

    #[test]
    fn empty_and_blank_text_are_the_same_and_decimals_compare_at_scale() {
        assert!(same(&FieldKind::Text, &CellValue::Empty, &text("")));
        assert!(same(
            &FieldKind::Text,
            &CellValue::Text(String::new()),
            &Content::Empty
        ));
        let limit = FieldKind::Decimal(2);
        assert!(same(
            &limit,
            &CellValue::Number(15000.0),
            &Content::number(15000.004)
        ));
        assert!(!same(
            &limit,
            &CellValue::Number(15000.0),
            &Content::number(15000.01)
        ));
        assert!(!same(
            &FieldKind::Integer,
            &CellValue::Number(3.0),
            &text("3")
        ));
        assert!(!same(
            &FieldKind::Boolean,
            &CellValue::Bool(true),
            &Content::Bool(false)
        ));
    }

    #[test]
    fn counts_follow_the_records_set() {
        let mut states = States::default();
        let modified = FieldState {
            bits: MODIFIED,
            rule: None,
        };
        let invalid = FieldState {
            bits: MODIFIED | INVALID,
            rule: Some(Rule::Required),
        };
        let formula = FieldState {
            bits: FORMULA,
            rule: None,
        };
        states.set("V-1", vec![modified, invalid]);
        states.set("V-2", vec![formula, FieldState::default()]);
        assert_eq!(
            states.counts(),
            Counts {
                changed: 2,
                changed_records: 1,
                invalid: 1,
                formulas: 1,
                conflicts: 0
            }
        );
        states.set("V-1", vec![FieldState::default(), modified]);
        assert_eq!(
            states.counts(),
            Counts {
                changed: 1,
                changed_records: 1,
                invalid: 0,
                formulas: 1,
                conflicts: 0
            }
        );
        states.set("V-1", vec![FieldState::default(); 2]);
        states.set("V-2", vec![FieldState::default(); 2]);
        assert!(states.is_empty());
        assert_eq!(states.counts(), Counts::default());
    }

    #[test]
    fn values_go_on_the_wire_as_the_contract_writes_them() {
        assert_eq!(wire(&FieldKind::Text, &text("01001234567")), "01001234567");
        assert_eq!(wire(&FieldKind::Text, &Content::Empty), Value::Null);
        assert_eq!(
            wire(&FieldKind::Decimal(2), &Content::number(15000.0)),
            "15000.00"
        );
        assert_eq!(
            wire(&FieldKind::Decimal(2), &Content::number(-0.001)),
            "0.00"
        );
        assert_eq!(wire(&FieldKind::Integer, &Content::number(7.0)), 7.0);
        assert_eq!(wire(&FieldKind::Boolean, &Content::Bool(true)), true);
        for day in [
            "1900-01-01",
            "1900-02-28",
            "1900-03-01",
            "2024-02-29",
            "2026-11-10",
            "9999-12-31",
        ] {
            let serial = date(day).unwrap();
            assert_eq!(wire(&FieldKind::Date, &Content::number(serial)), day);
        }
    }
}
