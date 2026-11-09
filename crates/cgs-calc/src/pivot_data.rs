//! GETPIVOTDATA, over the pivot tables as the engine last laid them out. The engine hands over
//! each table's values with the items they sum; the function finds the table its reference is in
//! and the value whose items are exactly the ones asked for, as Excel does, or `#REF!`.

use std::sync::{Arc, RwLock};

use formualizer_common::{ExcelError, ExcelErrorKind, LiteralValue};
use formualizer_workbook::CUSTOM_REFERENCE_SEPARATOR;

/// An item as a value is summed over it: its label, and its number or date serial if it has one.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PivotItemKey {
    pub label: String,
    pub number: Option<f64>,
    /// The number is a date serial.
    pub date: bool,
}

impl PivotItemKey {
    fn matches(&self, wanted: &LiteralValue) -> bool {
        match wanted {
            LiteralValue::Text(text) => self.label.eq_ignore_ascii_case(text.trim()),
            LiteralValue::Number(n) => self.number == Some(*n) || self.label == format_number(*n),
            LiteralValue::Int(n) => self.number == Some(*n as f64) || self.label == n.to_string(),
            LiteralValue::Boolean(b) => {
                self.label
                    .eq_ignore_ascii_case(if *b { "TRUE" } else { "FALSE" })
            }
            LiteralValue::Date(_) | LiteralValue::DateTime(_) => false,
            _ => false,
        }
    }
}

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

/// One value of a pivot table: which value field, the items it sums by field name, and itself.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PivotValueLookup {
    /// Its cell, 1-based.
    pub row: u32,
    pub col: u32,
    pub data: u32,
    pub items: Vec<(String, PivotItemKey)>,
    pub value: f64,
}

/// A pivot table as GETPIVOTDATA reads it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PivotLookup {
    pub sheet: String,
    /// Everything it covers, report filters included, 1-based.
    pub top: u32,
    pub left: u32,
    pub bottom: u32,
    pub right: u32,
    /// Each value field's names: its caption and its source field's.
    pub data_names: Vec<Vec<String>>,
    /// Report filters showing one item: a value is asked for under that item or under none.
    pub pages: Vec<(String, PivotItemKey)>,
    pub values: Vec<PivotValueLookup>,
}

pub type PivotIndex = Arc<RwLock<Vec<PivotLookup>>>;

fn reference_error() -> ExcelError {
    ExcelError::new(ExcelErrorKind::Ref)
}

fn text_of(value: &LiteralValue) -> Option<String> {
    match value {
        LiteralValue::Text(text) => Some(text.trim().to_string()),
        LiteralValue::Number(n) => Some(format_number(*n)),
        LiteralValue::Int(n) => Some(n.to_string()),
        _ => None,
    }
}

/// GETPIVOTDATA(data_field, pivot_table, [field1, item1], ...), its second argument given as the
/// place it points at.
pub fn get_pivot_data(
    args: &[LiteralValue],
    index: &PivotIndex,
) -> Result<LiteralValue, ExcelError> {
    if let Some(LiteralValue::Error(error)) =
        args.iter().find(|a| matches!(a, LiteralValue::Error(_)))
    {
        return Err(error.clone());
    }
    let name = args.first().and_then(text_of).ok_or_else(reference_error)?;
    let LiteralValue::Text(place) = args.get(1).ok_or_else(reference_error)? else {
        return Err(reference_error());
    };
    let mut parts = place.split(CUSTOM_REFERENCE_SEPARATOR);
    let (Some(sheet), Some(row), Some(col)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(reference_error());
    };
    let row: u32 = row.parse().map_err(|_| reference_error())?;
    let col: u32 = col.parse().map_err(|_| reference_error())?;
    let tables = index.read().map_err(|_| reference_error())?;
    let table = tables
        .iter()
        .find(|t| {
            t.sheet.eq_ignore_ascii_case(sheet)
                && (t.top..=t.bottom).contains(&row)
                && (t.left..=t.right).contains(&col)
        })
        .ok_or_else(reference_error)?;
    let data = table
        .data_names
        .iter()
        .position(|names| names.iter().any(|n| n.eq_ignore_ascii_case(&name)))
        .ok_or_else(reference_error)? as u32;
    let rest = &args[2..];
    if !rest.len().is_multiple_of(2) {
        return Err(reference_error());
    }
    let mut wanted: Vec<(String, &LiteralValue)> = Vec::new();
    for pair in rest.chunks(2) {
        let field = text_of(&pair[0]).ok_or_else(reference_error)?;
        // A report filter's own item may be named; another of its items isn't shown.
        if let Some((_, item)) = table
            .pages
            .iter()
            .find(|(f, _)| f.eq_ignore_ascii_case(&field))
        {
            if !item.matches(&pair[1]) {
                return Err(reference_error());
            }
            continue;
        }
        wanted.push((field, &pair[1]));
    }
    let found = table.values.iter().find(|v| {
        v.data == data
            && v.items.len() == wanted.len()
            && wanted.iter().all(|(field, item)| {
                v.items
                    .iter()
                    .any(|(f, key)| f.eq_ignore_ascii_case(field) && key.matches(item))
            })
    });
    found
        .map(|v| LiteralValue::Number(v.value))
        .ok_or_else(reference_error)
}
