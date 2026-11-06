//! Pivot refresh: from a pivot table's layout and its cache's records, the cells Excel would show and
//! the `rowItems` and `colItems` it would write. Nothing here reads or writes a file or a cell; the
//! engine writes the output into its cells.

// Records, items and lines are all addressed by index, and the indexes say what they mean.
#![allow(clippy::needless_range_loop)]

mod agg;
mod layout;
mod show_as;
pub mod source;
pub mod view;

pub use agg::{Acc, Out};
pub use layout::{layout, AxisInfo, Layout, LineInfo, LineRole};
pub use view::CacheView;

use cgs_model::pivot::Summary;

/// The words Excel puts in a pivot table that the file doesn't hold, in the UI's language.
#[derive(Debug, Clone, PartialEq)]
pub struct Captions {
    pub row_labels: String,
    pub col_labels: String,
    pub grand_total: String,
    /// Follows an item for its subtotal ("North Total") and comes before a value field for its
    /// grand total ("Total Sum of Sales").
    pub total: String,
    pub blank: String,
    pub all: String,
    pub multiple: String,
    /// Each summary's name as a custom subtotal says it ("North Sum"), in `Summary::ALL` order.
    pub functions: [String; 11],
}

impl Default for Captions {
    /// English, as Excel 2016 says them.
    fn default() -> Self {
        Self {
            row_labels: "Row Labels".to_string(),
            col_labels: "Column Labels".to_string(),
            grand_total: "Grand Total".to_string(),
            total: "Total".to_string(),
            blank: "(blank)".to_string(),
            all: "(All)".to_string(),
            multiple: "(Multiple Items)".to_string(),
            functions: [
                "Sum", "Count", "Average", "Max", "Min", "Product", "Count", "StdDev", "StdDevp",
                "Var", "Varp",
            ]
            .map(str::to_string),
        }
    }
}

impl Captions {
    pub fn function(&self, summary: Summary) -> &str {
        let at = Summary::ALL.iter().position(|s| *s == summary).unwrap_or(0);
        &self.functions[at]
    }
}

/// What an output cell holds.
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Text(String),
    Number(f64),
    Bool(bool),
    Error(String),
}

/// What part of the pivot table a cell is, for its look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A report filter's name.
    PageLabel,
    /// A report filter's choice.
    PageValue,
    /// A caption with a filter button: "Row Labels", a field's name.
    Button,
    /// The value field's name over its column, or the top-left caption.
    DataCaption,
    /// An item of a column field, or a column total's label.
    ColLabel,
    /// An item of a row field; `indent` is its level inside a compact column.
    RowLabel { indent: u8 },
    /// A value, from the value field at this index.
    Data { field: u16 },
}

/// One output cell, in sheet coordinates (1-based).
#[derive(Debug, Clone, PartialEq)]
pub struct OutCell {
    pub row: u32,
    pub col: u32,
    pub value: CellValue,
    pub role: Role,
    /// The number format the cell shows, by id, when it isn't General.
    pub num_fmt: Option<u32>,
}
