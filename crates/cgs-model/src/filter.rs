//! Autofilters, on a sheet or a table, and the sort a file remembers with them.

use crate::Rect;

/// How one condition of a custom filter compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

impl FilterOp {
    /// The `operator` attribute's value. Equal is the default and isn't written.
    pub fn xml(self) -> &'static str {
        match self {
            Self::Equal => "equal",
            Self::NotEqual => "notEqual",
            Self::Less => "lessThan",
            Self::LessOrEqual => "lessThanOrEqual",
            Self::Greater => "greaterThan",
            Self::GreaterOrEqual => "greaterThanOrEqual",
        }
    }

    pub fn from_xml(text: &str) -> Self {
        match text {
            "notEqual" => Self::NotEqual,
            "lessThan" => Self::Less,
            "lessThanOrEqual" => Self::LessOrEqual,
            "greaterThan" => Self::Greater,
            "greaterThanOrEqual" => Self::GreaterOrEqual,
            _ => Self::Equal,
        }
    }
}

/// A year, month or day kept by a value filter, with what's under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateGroup {
    pub year: u16,
    pub month: Option<u8>,
    pub day: Option<u8>,
    /// Finer than days: hour, minute, second.
    pub hour: Option<u8>,
    pub minute: Option<u8>,
    pub second: Option<u8>,
}

/// What a filter column keeps.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterCriteria {
    /// Ticked values in the list, as their shown text, and dates by year, month or day.
    Values {
        values: Vec<String>,
        blank: bool,
        dates: Vec<DateGroup>,
    },
    /// One or two comparisons. Text may hold `*` and `?`.
    Custom {
        and: bool,
        conditions: Vec<(FilterOp, String)>,
    },
    Top10 {
        top: bool,
        percent: bool,
        value: f64,
        /// The cut-off Excel worked out, which it writes alongside.
        filter_value: Option<f64>,
    },
    /// Above average, today, this month and the rest: `type` as the file writes it.
    Dynamic {
        kind: String,
        value: Option<f64>,
        max_value: Option<f64>,
    },
    /// By cell fill, or by font colour, as a differential format.
    Color { cell: bool, dxf: Option<u32> },
    /// By a conditional format's icon. No `id` means cells with no icon.
    Icon { set: String, id: Option<u32> },
    /// Kept as the file had it.
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilterColumn {
    /// Counted from the filter's first column, from 0.
    pub col_id: u32,
    /// The column's button is hidden.
    pub hidden_button: bool,
    pub criteria: Option<FilterCriteria>,
}

/// How a sort orders one column, or row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortBy {
    Value,
    CellColor(Option<u32>),
    FontColor(Option<u32>),
    Icon { set: String, id: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    /// The column, or the row for a sort by columns, as the file's `ref`.
    pub area: Rect,
    pub descending: bool,
    pub by: SortBy,
    /// A custom list, such as months, as comma-separated text.
    pub custom_list: Option<String>,
}

/// The last sort, as the file remembers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortState {
    pub area: Rect,
    pub case_sensitive: bool,
    /// Sorted left to right, by rows.
    pub by_columns: bool,
    pub keys: Vec<SortKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutoFilter {
    /// Its header row and the rows it filters.
    pub range: Rect,
    pub columns: Vec<FilterColumn>,
    pub sort: Option<SortState>,
}

impl AutoFilter {
    pub fn new(range: Rect) -> Self {
        Self {
            range,
            columns: Vec::new(),
            sort: None,
        }
    }

    /// Whether any column filters anything.
    pub fn is_filtering(&self) -> bool {
        self.columns.iter().any(|column| column.criteria.is_some())
    }

    pub fn column(&self, col_id: u32) -> Option<&FilterColumn> {
        self.columns.iter().find(|column| column.col_id == col_id)
    }
}
