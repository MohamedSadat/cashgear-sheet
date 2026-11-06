//! Pivot tables and their caches, as the file describes them. The output of a pivot table is
//! ordinary cells, which the formula engine holds; this is the layout that makes them, and the
//! cache it summarizes. `cgs-pivot` works the output out.

use crate::Rect;

/// One value in a pivot cache, as a shared item or a record holds it.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheValue {
    /// `<m/>`: an empty cell, shown as "(blank)".
    Missing,
    Number(f64),
    Text(String),
    Bool(bool),
    /// An error such as `#N/A`.
    Error(String),
    /// A date as a serial in the workbook's date system.
    Date(f64),
}

impl CacheValue {
    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Number(n) | Self::Date(n) => Some(*n),
            _ => None,
        }
    }
}

/// Where a cache's records come from.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum CacheSource {
    /// Cells on a sheet, by sheet name and range, or by a defined name or table.
    Worksheet {
        sheet: Option<String>,
        range: Option<String>,
        name: Option<String>,
    },
    /// External data, OLAP, consolidation or scenarios: nothing here can refresh it.
    #[default]
    Other,
}

/// How a grouped field divides its base field's values.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupRange {
    /// By number: from `start` in steps of `interval`, values outside under "<start" and ">end".
    Numbers {
        start: f64,
        end: f64,
        interval: f64,
        auto_start: bool,
        auto_end: bool,
    },
    /// By date part.
    Dates {
        by: DateGrouping,
        /// Serials.
        start: f64,
        end: f64,
        interval: f64,
        auto_start: bool,
        auto_end: bool,
    },
    /// Items picked by hand: the group each of the base field's items falls in.
    Discrete(Vec<u32>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateGrouping {
    Seconds,
    Minutes,
    Hours,
    Days,
    Months,
    Quarters,
    Years,
}

impl DateGrouping {
    pub fn from_xml(text: &str) -> Option<Self> {
        Some(match text {
            "seconds" => Self::Seconds,
            "minutes" => Self::Minutes,
            "hours" => Self::Hours,
            "days" => Self::Days,
            "months" => Self::Months,
            "quarters" => Self::Quarters,
            "years" => Self::Years,
            _ => return None,
        })
    }

    pub fn xml(self) -> &'static str {
        match self {
            Self::Seconds => "seconds",
            Self::Minutes => "minutes",
            Self::Hours => "hours",
            Self::Days => "days",
            Self::Months => "months",
            Self::Quarters => "quarters",
            Self::Years => "years",
        }
    }
}

/// A field made by grouping another one.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldGroup {
    /// The field whose values are grouped: this one, or another for a second level of dates.
    pub base: u32,
    /// The field one level up, for a date grouped into several parts.
    pub parent: Option<u32>,
    pub range: Option<GroupRange>,
    /// The group items' labels, in the file's order.
    pub items: Vec<CacheValue>,
}

/// One column of a cache.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CacheField {
    pub name: String,
    pub num_fmt_id: u32,
    /// Its distinct values, which records and pivot items point at by index. Empty when records
    /// hold their values themselves, as Excel does for a field only ever summed.
    pub shared: Vec<CacheValue>,
    /// `sharedItems` has `count`: items are listed, even when there are none.
    pub listed: bool,
    /// Made from a formula, in `formula`, rather than read from the source.
    pub formula: Option<String>,
    /// Its values come from the source; `false` for groups and formulas.
    pub database: bool,
    pub group: Option<FieldGroup>,
}

/// A cache definition: what it reads, and its fields.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PivotCache {
    /// The workbook's id for it, which pivot tables name in `cacheId`.
    pub id: u32,
    /// The definition part; `None` for a new cache.
    pub part: Option<String>,
    /// The records part, if the file kept the records.
    pub records_part: Option<String>,
    pub source: CacheSource,
    pub fields: Vec<CacheField>,
    pub record_count: u32,
    pub refresh_on_load: bool,
    /// The file kept no records, so the pivot can't change until it is refreshed.
    pub save_data: bool,
    /// Why its pivot tables can't be laid out here, if they can't: OLAP, a calculated field.
    pub unsupported: Option<String>,
    /// Why its source can't be read again, if it can't: external data, another workbook. Its
    /// pivot tables can still be rearranged from the records it holds.
    pub no_refresh: Option<String>,
    /// Its fields differ from the file's, so the writer brings the part in line.
    pub changed: bool,
}

/// Which way a field lies in a pivot table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PivotAxis {
    Row,
    Col,
    Page,
    Values,
}

/// What an item of a field is: one of the field's values, or a subtotal or grand total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemType {
    /// One of the field's items.
    Data,
    /// The automatic subtotal.
    Default,
    Sum,
    CountA,
    Avg,
    Max,
    Min,
    Product,
    Count,
    StdDev,
    StdDevP,
    Var,
    VarP,
    Grand,
    Blank,
}

impl ItemType {
    pub fn from_xml(text: &str) -> Option<Self> {
        Some(match text {
            "data" => Self::Data,
            "default" => Self::Default,
            "sum" => Self::Sum,
            "countA" => Self::CountA,
            "avg" => Self::Avg,
            "max" => Self::Max,
            "min" => Self::Min,
            "product" => Self::Product,
            "count" => Self::Count,
            "stdDev" => Self::StdDev,
            "stdDevP" => Self::StdDevP,
            "var" => Self::Var,
            "varP" => Self::VarP,
            "grand" => Self::Grand,
            "blank" => Self::Blank,
            _ => return None,
        })
    }

    pub fn xml(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Default => "default",
            Self::Sum => "sum",
            Self::CountA => "countA",
            Self::Avg => "avg",
            Self::Max => "max",
            Self::Min => "min",
            Self::Product => "product",
            Self::Count => "count",
            Self::StdDev => "stdDev",
            Self::StdDevP => "stdDevP",
            Self::Var => "var",
            Self::VarP => "varP",
            Self::Grand => "grand",
            Self::Blank => "blank",
        }
    }

    /// The summary a custom subtotal works out.
    pub fn function(self) -> Option<Summary> {
        Some(match self {
            Self::Sum => Summary::Sum,
            Self::CountA => Summary::Count,
            Self::Avg => Summary::Average,
            Self::Max => Summary::Max,
            Self::Min => Summary::Min,
            Self::Product => Summary::Product,
            Self::Count => Summary::CountNums,
            Self::StdDev => Summary::StdDev,
            Self::StdDevP => Summary::StdDevP,
            Self::Var => Summary::Var,
            Self::VarP => Summary::VarP,
            _ => return None,
        })
    }
}

/// One item of a pivot field: a value, in the order the field shows them, or a subtotal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PivotItem {
    pub kind: ItemType,
    /// The cache item it shows, for a data item.
    pub x: Option<u32>,
    /// Filtered out.
    pub hidden: bool,
    /// Its details show (it is expanded).
    pub show_details: bool,
    /// A caption of its own.
    pub name: Option<String>,
    /// No longer in the source, kept by the field.
    pub missing: bool,
}

impl PivotItem {
    pub fn data(x: u32) -> Self {
        Self {
            kind: ItemType::Data,
            x: Some(x),
            hidden: false,
            show_details: true,
            name: None,
            missing: false,
        }
    }

    pub fn total(kind: ItemType) -> Self {
        Self {
            kind,
            x: None,
            hidden: false,
            show_details: true,
            name: None,
            missing: false,
        }
    }
}

/// How a field's items are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldSort {
    /// In the order of the field's items, which can be dragged.
    #[default]
    Manual,
    Ascending,
    Descending,
}

/// One field of a pivot table, matching the cache field at the same index.
#[derive(Debug, Clone, PartialEq)]
pub struct PivotField {
    pub axis: Option<PivotAxis>,
    /// Some value field summarizes it.
    pub data_field: bool,
    pub items: Vec<PivotItem>,
    /// Items with no data are shown.
    pub show_all: bool,
    pub compact: bool,
    pub outline: bool,
    pub subtotal_top: bool,
    pub default_subtotal: bool,
    /// Subtotals asked for by name, besides or instead of the automatic one.
    pub subtotals: Vec<ItemType>,
    pub sort: FieldSort,
    /// Sort by a value field's results rather than by label: its index in `data_fields`.
    pub sort_by_data: Option<u32>,
    pub insert_blank_row: bool,
    /// A caption of its own.
    pub name: Option<String>,
    pub subtotal_caption: Option<String>,
    pub num_fmt_id: Option<u32>,
    pub multiple_item_selection: bool,
    /// Its labels repeat on every row, from Excel 2010's `fillDownLabels`.
    pub repeat_labels: bool,
    /// Items in the field's items but not in the file's are new and need writing.
    pub items_listed: bool,
}

impl Default for PivotField {
    fn default() -> Self {
        Self {
            axis: None,
            data_field: false,
            items: Vec::new(),
            show_all: false,
            compact: true,
            outline: true,
            subtotal_top: true,
            default_subtotal: true,
            subtotals: Vec::new(),
            sort: FieldSort::Manual,
            sort_by_data: None,
            insert_blank_row: false,
            name: None,
            subtotal_caption: None,
            num_fmt_id: None,
            multiple_item_selection: false,
            repeat_labels: false,
            items_listed: false,
        }
    }
}

/// What a value field works out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Summary {
    #[default]
    Sum,
    /// Count of non-empty values (`count` in the file).
    Count,
    Average,
    Max,
    Min,
    Product,
    /// Count of numbers (`countNums`).
    CountNums,
    StdDev,
    StdDevP,
    Var,
    VarP,
}

impl Summary {
    pub const ALL: [Summary; 11] = [
        Self::Sum,
        Self::Count,
        Self::Average,
        Self::Max,
        Self::Min,
        Self::Product,
        Self::CountNums,
        Self::StdDev,
        Self::StdDevP,
        Self::Var,
        Self::VarP,
    ];

    pub fn from_xml(text: &str) -> Option<Self> {
        Some(match text {
            "sum" => Self::Sum,
            "count" => Self::Count,
            "average" => Self::Average,
            "max" => Self::Max,
            "min" => Self::Min,
            "product" => Self::Product,
            "countNums" => Self::CountNums,
            "stdDev" => Self::StdDev,
            "stdDevp" => Self::StdDevP,
            "var" => Self::Var,
            "varp" => Self::VarP,
            _ => return None,
        })
    }

    pub fn xml(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Count => "count",
            Self::Average => "average",
            Self::Max => "max",
            Self::Min => "min",
            Self::Product => "product",
            Self::CountNums => "countNums",
            Self::StdDev => "stdDev",
            Self::StdDevP => "stdDevp",
            Self::Var => "var",
            Self::VarP => "varp",
        }
    }

    /// The English caption Excel starts a value field's name with: "Sum of Sales".
    pub fn caption(self) -> &'static str {
        match self {
            Self::Sum => "Sum",
            Self::Count => "Count",
            Self::Average => "Average",
            Self::Max => "Max",
            Self::Min => "Min",
            Self::Product => "Product",
            Self::CountNums => "Count",
            Self::StdDev => "StdDev",
            Self::StdDevP => "StdDevp",
            Self::Var => "Var",
            Self::VarP => "Varp",
        }
    }
}

/// Show Values As.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ShowAs {
    #[default]
    Normal,
    Difference,
    Percent,
    PercentDiff,
    RunTotal,
    PercentOfRow,
    PercentOfCol,
    PercentOfTotal,
    Index,
    /// Newer kinds (`percentOfParent`, `percentOfRunningTotal`, ranks), from the file's extension.
    PercentOfParentRow,
    PercentOfParentCol,
    PercentOfParent,
    PercentOfRunningTotal,
    RankAscending,
    RankDescending,
}

impl ShowAs {
    pub fn from_xml(text: &str) -> Option<Self> {
        Some(match text {
            "normal" => Self::Normal,
            "difference" => Self::Difference,
            "percent" => Self::Percent,
            "percentDiff" => Self::PercentDiff,
            "runTotal" => Self::RunTotal,
            "percentOfRow" => Self::PercentOfRow,
            "percentOfCol" => Self::PercentOfCol,
            "percentOfTotal" => Self::PercentOfTotal,
            "index" => Self::Index,
            "percentOfParentRow" => Self::PercentOfParentRow,
            "percentOfParentCol" => Self::PercentOfParentCol,
            "percentOfParent" => Self::PercentOfParent,
            "percentOfRunningTotal" => Self::PercentOfRunningTotal,
            "rankAscending" => Self::RankAscending,
            "rankDescending" => Self::RankDescending,
            _ => return None,
        })
    }

    pub fn xml(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Difference => "difference",
            Self::Percent => "percent",
            Self::PercentDiff => "percentDiff",
            Self::RunTotal => "runTotal",
            Self::PercentOfRow => "percentOfRow",
            Self::PercentOfCol => "percentOfCol",
            Self::PercentOfTotal => "percentOfTotal",
            Self::Index => "index",
            Self::PercentOfParentRow => "percentOfParentRow",
            Self::PercentOfParentCol => "percentOfParentCol",
            Self::PercentOfParent => "percentOfParent",
            Self::PercentOfRunningTotal => "percentOfRunningTotal",
            Self::RankAscending => "rankAscending",
            Self::RankDescending => "rankDescending",
        }
    }

    /// Only the extension list can say it, so Excel 2007 sees `normal`.
    pub fn extended(self) -> bool {
        matches!(
            self,
            Self::PercentOfParentRow
                | Self::PercentOfParentCol
                | Self::PercentOfParent
                | Self::PercentOfRunningTotal
                | Self::RankAscending
                | Self::RankDescending
        )
    }
}

/// Which item a Show Values As compares with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BaseItem {
    #[default]
    Previous,
    Next,
    /// An index into the base field's items.
    Item(u32),
}

impl BaseItem {
    pub const PREVIOUS: u32 = 1_048_828;
    pub const NEXT: u32 = 1_048_829;

    pub fn from_xml(value: u32) -> Self {
        match value {
            Self::PREVIOUS => Self::Previous,
            Self::NEXT => Self::Next,
            other => Self::Item(other),
        }
    }

    pub fn xml(self) -> u32 {
        match self {
            Self::Previous => Self::PREVIOUS,
            Self::Next => Self::NEXT,
            Self::Item(item) => item,
        }
    }
}

/// A value field.
#[derive(Debug, Clone, PartialEq)]
pub struct DataField {
    pub name: String,
    pub field: u32,
    pub summary: Summary,
    pub show_as: ShowAs,
    pub base_field: u32,
    pub base_item: BaseItem,
    pub num_fmt_id: Option<u32>,
}

/// A report filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageField {
    pub field: u32,
    /// The one item chosen, as an index into the field's items; `None` for (All) or several.
    pub item: Option<u32>,
    pub name: Option<String>,
}

/// One row or column of a pivot table's layout, as `rowItems` and `colItems` hold it: which items
/// of the axis's fields it shows, given as the members that differ from the line before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxisLine {
    pub kind: ItemType,
    /// How many of the leading fields repeat the line before.
    pub repeat: u32,
    /// Which value field, when values are on this axis.
    pub data: u32,
    /// Indexes into each following field's items.
    pub members: Vec<u32>,
}

/// Where the output sits, and where its parts start inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PivotLocation {
    pub range: Rect,
    /// Rows from the top of `range` to the column fields' labels.
    pub first_header_row: u32,
    /// Rows from the top to the first row of values.
    pub first_data_row: u32,
    /// Columns from the left to the first column of values.
    pub first_data_col: u32,
    pub row_page_count: u32,
    pub col_page_count: u32,
}

/// The look a pivot table takes from a pivot style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PivotStyleInfo {
    pub name: Option<String>,
    pub row_headers: bool,
    pub col_headers: bool,
    pub row_stripes: bool,
    pub col_stripes: bool,
    pub last_column: bool,
}

impl Default for PivotStyleInfo {
    /// What Excel 2016 gives a new pivot table.
    fn default() -> Self {
        Self {
            name: Some("PivotStyleLight16".to_string()),
            row_headers: true,
            col_headers: true,
            row_stripes: false,
            col_stripes: false,
            last_column: true,
        }
    }
}

/// The layout options on the root of a pivot table definition.
#[derive(Debug, Clone, PartialEq)]
pub struct PivotOptions {
    pub data_caption: String,
    pub grand_total_caption: Option<String>,
    pub row_header_caption: Option<String>,
    pub col_header_caption: Option<String>,
    pub missing_caption: Option<String>,
    pub show_missing: bool,
    pub error_caption: Option<String>,
    pub show_error: bool,
    pub row_grand_totals: bool,
    pub col_grand_totals: bool,
    pub data_on_rows: bool,
    /// Where the values field goes among the axis's fields.
    pub data_position: Option<u32>,
    pub compact: bool,
    pub compact_data: bool,
    pub outline: bool,
    pub outline_data: bool,
    pub grid_drop_zones: bool,
    pub indent: u32,
    pub show_headers: bool,
    pub merge_item: bool,
    pub page_over_then_down: bool,
    pub page_wrap: u32,
    pub show_empty_row: bool,
    pub show_empty_col: bool,
    /// Excel 2010's `hideValuesRow`.
    pub hide_values_row: bool,
    pub created_version: u32,
    pub updated_version: u32,
}

impl Default for PivotOptions {
    fn default() -> Self {
        Self {
            data_caption: "Values".to_string(),
            grand_total_caption: None,
            row_header_caption: None,
            col_header_caption: None,
            missing_caption: None,
            show_missing: true,
            error_caption: None,
            show_error: false,
            row_grand_totals: true,
            col_grand_totals: true,
            data_on_rows: false,
            data_position: None,
            compact: true,
            compact_data: true,
            outline: false,
            outline_data: false,
            grid_drop_zones: false,
            indent: 1,
            show_headers: true,
            merge_item: false,
            page_over_then_down: false,
            page_wrap: 0,
            show_empty_row: false,
            show_empty_col: false,
            hide_values_row: false,
            created_version: 0,
            updated_version: 0,
        }
    }
}

/// The field `-2` stands for in `rowFields` and `colFields`: the values themselves.
pub const VALUES_FIELD: i32 = -2;

/// A pivot table: its layout, and where its output goes.
#[derive(Debug, Clone, PartialEq)]
pub struct PivotTable {
    pub name: String,
    pub sheet: u32,
    /// Its part; `None` for a new table.
    pub part: Option<String>,
    /// Index into `Book::pivot_caches`.
    pub cache: u32,
    pub location: PivotLocation,
    pub fields: Vec<PivotField>,
    /// Field indexes, with `VALUES_FIELD` for the values.
    pub row_fields: Vec<i32>,
    pub col_fields: Vec<i32>,
    pub page_fields: Vec<PageField>,
    pub data_fields: Vec<DataField>,
    pub row_items: Vec<AxisLine>,
    pub col_items: Vec<AxisLine>,
    pub options: PivotOptions,
    pub style: PivotStyleInfo,
    /// Why it can't be refreshed or changed here, if it can't.
    pub unsupported: Option<String>,
    /// It differs from its part, so the writer brings the part in line.
    pub changed: bool,
}

impl PivotTable {
    /// The value field ordinals and field indexes of the axis, `VALUES_FIELD` included.
    pub fn axis_fields(&self, axis: PivotAxis) -> &[i32] {
        match axis {
            PivotAxis::Row => &self.row_fields,
            _ => &self.col_fields,
        }
    }
}

/// One field of a cache's records, by column: its distinct values, and which one each record has.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RecordColumn {
    /// The field's shared items first, in order, then any value a record held itself.
    pub values: Vec<CacheValue>,
    /// For each record, an index into `values`.
    pub index: Vec<u32>,
}

/// A cache's records, read from its records part or from the source cells.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CacheRecords {
    /// One per cache field that comes from the source, in field order; empty for the others.
    pub columns: Vec<RecordColumn>,
    pub count: u32,
}
