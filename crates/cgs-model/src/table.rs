//! Tables: where they are and their columns, which the formula engine also holds, and how they
//! look, which only the model holds.

use crate::{AutoFilter, Rect};

/// What a totals row cell works out, as the file names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TotalsFunction {
    Sum,
    Min,
    Max,
    Average,
    Count,
    CountNums,
    StdDev,
    Var,
    /// A formula of its own, in `totals_formula`.
    Custom,
}

impl TotalsFunction {
    pub fn xml(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Min => "min",
            Self::Max => "max",
            Self::Average => "average",
            Self::Count => "count",
            Self::CountNums => "countNums",
            Self::StdDev => "stdDev",
            Self::Var => "var",
            Self::Custom => "custom",
        }
    }

    pub fn from_xml(text: &str) -> Option<Self> {
        Some(match text {
            "sum" => Self::Sum,
            "min" => Self::Min,
            "max" => Self::Max,
            "average" => Self::Average,
            "count" => Self::Count,
            "countNums" => Self::CountNums,
            "stdDev" => Self::StdDev,
            "var" => Self::Var,
            "custom" => Self::Custom,
            _ => return None,
        })
    }

    /// `SUBTOTAL`'s number for it, which leaves out rows a filter hides.
    pub fn subtotal(self) -> Option<u32> {
        Some(match self {
            Self::Average => 101,
            Self::CountNums => 102,
            Self::Count => 103,
            Self::Max => 104,
            Self::Min => 105,
            Self::StdDev => 107,
            Self::Sum => 109,
            Self::Var => 110,
            Self::Custom => return None,
        })
    }
}

/// What a table keeps about one of its columns, beyond its name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableColumnInfo {
    pub totals_function: Option<TotalsFunction>,
    pub totals_label: Option<String>,
    pub totals_formula: Option<String>,
    /// The formula a calculated column fills its rows with, as the file holds it.
    pub calculated: Option<String>,
}

/// A table's style and which of its parts the style picks out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableStyle {
    /// A built-in style such as `TableStyleMedium2`, or one of the file's own. None draws plain.
    pub name: Option<String>,
    pub first_column: bool,
    pub last_column: bool,
    pub row_stripes: bool,
    pub column_stripes: bool,
}

impl Default for TableStyle {
    /// What Excel 2016 gives a new table.
    fn default() -> Self {
        Self {
            name: Some("TableStyleMedium2".to_string()),
            first_column: false,
            last_column: false,
            row_stripes: true,
            column_stripes: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub name: String,
    pub sheet: u32,
    pub range: Rect,
    pub header_rows: u32,
    pub totals_rows: u32,
    pub columns: Vec<String>,
    /// Its `id`, unique in the workbook.
    pub id: u32,
    /// Its part in the package, once it has one.
    pub part: Option<String>,
    pub style: TableStyle,
    /// Its filter buttons, and what they filter. None hides the buttons.
    pub filter: Option<AutoFilter>,
    /// One per column, in the same order.
    pub column_info: Vec<TableColumnInfo>,
    /// How it looks or what its part says differs from the file, apart from what moving rows and
    /// columns does to it.
    pub edited: bool,
}

impl Table {
    pub fn new(name: String, sheet: u32, range: Rect, columns: Vec<String>) -> Self {
        let column_info = vec![TableColumnInfo::default(); columns.len()];
        Self {
            name,
            sheet,
            range,
            header_rows: 1,
            totals_rows: 0,
            columns,
            id: 0,
            part: None,
            style: TableStyle::default(),
            filter: None,
            column_info,
            edited: true,
        }
    }

    /// Its data rows, between its header and its totals.
    pub fn data_rows(&self) -> (u32, u32) {
        (
            self.range.top + self.header_rows,
            self.range.bottom.saturating_sub(self.totals_rows),
        )
    }

    /// What it keeps about a column, by the column's name.
    pub fn info(&self, column: &str) -> Option<&TableColumnInfo> {
        let index = self
            .columns
            .iter()
            .position(|other| other.eq_ignore_ascii_case(column))?;
        self.column_info.get(index)
    }

    /// Lines its column records up with new column names, keeping what each name had.
    pub fn set_columns(&mut self, columns: Vec<String>) {
        if columns == self.columns {
            return;
        }
        let renamed_only = columns.len() == self.columns.len();
        let info = columns
            .iter()
            .enumerate()
            .map(|(index, name)| {
                self.info(name)
                    .cloned()
                    .or_else(|| {
                        renamed_only
                            .then(|| self.column_info.get(index).cloned())
                            .flatten()
                    })
                    .unwrap_or_default()
            })
            .collect();
        self.columns = columns;
        self.column_info = info;
    }
}
