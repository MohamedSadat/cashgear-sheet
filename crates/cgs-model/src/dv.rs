//! Data validation: what a cell takes, the message it shows, and what it says when refused.

use crate::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DvKind {
    #[default]
    Any,
    Whole,
    Decimal,
    List,
    Date,
    Time,
    TextLength,
    Custom,
}

impl DvKind {
    pub fn from_xml(text: &str) -> Self {
        match text {
            "whole" => Self::Whole,
            "decimal" => Self::Decimal,
            "list" => Self::List,
            "date" => Self::Date,
            "time" => Self::Time,
            "textLength" => Self::TextLength,
            "custom" => Self::Custom,
            _ => Self::Any,
        }
    }

    pub fn xml(self) -> &'static str {
        match self {
            Self::Any => "none",
            Self::Whole => "whole",
            Self::Decimal => "decimal",
            Self::List => "list",
            Self::Date => "date",
            Self::Time => "time",
            Self::TextLength => "textLength",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DvOp {
    #[default]
    Between,
    NotBetween,
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

impl DvOp {
    pub fn from_xml(text: &str) -> Self {
        match text {
            "notBetween" => Self::NotBetween,
            "equal" => Self::Equal,
            "notEqual" => Self::NotEqual,
            "lessThan" => Self::LessThan,
            "lessThanOrEqual" => Self::LessThanOrEqual,
            "greaterThan" => Self::GreaterThan,
            "greaterThanOrEqual" => Self::GreaterThanOrEqual,
            _ => Self::Between,
        }
    }

    pub fn xml(self) -> &'static str {
        match self {
            Self::Between => "between",
            Self::NotBetween => "notBetween",
            Self::Equal => "equal",
            Self::NotEqual => "notEqual",
            Self::LessThan => "lessThan",
            Self::LessThanOrEqual => "lessThanOrEqual",
            Self::GreaterThan => "greaterThan",
            Self::GreaterThanOrEqual => "greaterThanOrEqual",
        }
    }

    /// Whether two bounds hold `value`, or one does.
    pub fn holds(self, value: f64, first: f64, second: Option<f64>) -> bool {
        let (low, high) = match second {
            Some(second) => (first.min(second), first.max(second)),
            None => (first, first),
        };
        match self {
            Self::Between => value >= low && value <= high,
            Self::NotBetween => value < low || value > high,
            Self::Equal => value == first,
            Self::NotEqual => value != first,
            Self::LessThan => value < first,
            Self::LessThanOrEqual => value <= first,
            Self::GreaterThan => value > first,
            Self::GreaterThanOrEqual => value >= first,
        }
    }
}

/// What a refusal lets you do: Stop refuses, Warning asks, Information tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DvStyle {
    #[default]
    Stop,
    Warning,
    Information,
}

impl DvStyle {
    pub fn from_xml(text: &str) -> Self {
        match text {
            "warning" => Self::Warning,
            "information" => Self::Information,
            _ => Self::Stop,
        }
    }

    pub fn xml(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Warning => "warning",
            Self::Information => "information",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Validation {
    pub areas: Vec<Rect>,
    pub kind: DvKind,
    pub op: DvOp,
    /// Formulas, written for the first cell of the first area, without `=`.
    pub formula1: Option<String>,
    pub formula2: Option<String>,
    pub allow_blank: bool,
    /// A list shows its in-cell dropdown. The file's `showDropDown` says the opposite.
    pub dropdown: bool,
    pub show_input: bool,
    pub input_title: String,
    pub input_text: String,
    pub show_error: bool,
    pub error_title: String,
    pub error_text: String,
    pub style: DvStyle,
    /// From an extension, which is drawn and checked but not edited.
    pub extended: bool,
    /// The element as the file had it, while nothing in it has changed.
    pub raw: Option<String>,
}

impl Validation {
    pub fn covers(&self, row: u32, col: u32) -> bool {
        self.areas.iter().any(|area| area.contains(row, col))
    }

    pub fn anchor(&self) -> (u32, u32) {
        self.areas
            .first()
            .map_or((1, 1), |area| (area.top, area.left))
    }
}
