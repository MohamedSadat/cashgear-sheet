//! Conditional formats: Excel 2007's own rules, and the extended (x14) ones of later versions,
//! which are drawn and kept but not edited.

use crate::{Color, Dxf, Rect};

/// Where a colour scale, data bar or icon set puts a threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfValueKind {
    Number,
    Percent,
    Max,
    Min,
    Formula,
    Percentile,
    AutoMin,
    AutoMax,
}

impl CfValueKind {
    pub fn from_xml(text: &str) -> Self {
        match text {
            "percent" => Self::Percent,
            "max" => Self::Max,
            "min" => Self::Min,
            "formula" => Self::Formula,
            "percentile" => Self::Percentile,
            "autoMin" => Self::AutoMin,
            "autoMax" => Self::AutoMax,
            _ => Self::Number,
        }
    }

    pub fn xml(self) -> &'static str {
        match self {
            Self::Number => "num",
            Self::Percent => "percent",
            Self::Max => "max",
            Self::Min => "min",
            Self::Formula => "formula",
            Self::Percentile => "percentile",
            Self::AutoMin => "autoMin",
            Self::AutoMax => "autoMax",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cfvo {
    pub kind: CfValueKind,
    pub value: Option<String>,
    /// An icon's threshold includes its value. Excel's default.
    pub gte: bool,
}

impl Cfvo {
    pub fn new(kind: CfValueKind, value: Option<&str>) -> Self {
        Self {
            kind,
            value: value.map(str::to_string),
            gte: true,
        }
    }
}

/// How a cell-value rule compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfOperator {
    LessThan,
    LessThanOrEqual,
    Equal,
    NotEqual,
    GreaterThanOrEqual,
    GreaterThan,
    Between,
    NotBetween,
}

impl CfOperator {
    pub fn from_xml(text: &str) -> Option<Self> {
        Some(match text {
            "lessThan" => Self::LessThan,
            "lessThanOrEqual" => Self::LessThanOrEqual,
            "equal" => Self::Equal,
            "notEqual" => Self::NotEqual,
            "greaterThanOrEqual" => Self::GreaterThanOrEqual,
            "greaterThan" => Self::GreaterThan,
            "between" => Self::Between,
            "notBetween" => Self::NotBetween,
            _ => return None,
        })
    }

    pub fn xml(self) -> &'static str {
        match self {
            Self::LessThan => "lessThan",
            Self::LessThanOrEqual => "lessThanOrEqual",
            Self::Equal => "equal",
            Self::NotEqual => "notEqual",
            Self::GreaterThanOrEqual => "greaterThanOrEqual",
            Self::GreaterThan => "greaterThan",
            Self::Between => "between",
            Self::NotBetween => "notBetween",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataBar {
    pub min: Cfvo,
    pub max: Cfvo,
    pub color: Color,
    pub show_value: bool,
    /// Shortest and longest bars, as shares of the cell in percent.
    pub min_length: u32,
    pub max_length: u32,
    /// From the extended rule: a gradient fill, a border, and colours for bars below zero.
    pub gradient: bool,
    pub border: Option<Color>,
    pub negative_color: Option<Color>,
    pub axis_color: Option<Color>,
    /// Left to right, or the other way.
    pub right_to_left: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IconSet {
    /// `3Arrows`, `4Rating`, `5Quarters` and the rest of Excel's names.
    pub set: String,
    pub cfvos: Vec<Cfvo>,
    pub reverse: bool,
    pub show_value: bool,
    /// An extended set's own icons, as (set, index), one per threshold, where they differ.
    pub custom: Vec<Option<(String, u32)>>,
}

impl IconSet {
    /// How many icons a set has, by its name.
    pub fn size(name: &str) -> usize {
        match name.as_bytes().first() {
            Some(b'4') => 4,
            Some(b'5') => 5,
            _ => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CfKind {
    CellIs(CfOperator),
    Expression,
    ContainsText(String),
    NotContainsText(String),
    BeginsWith(String),
    EndsWith(String),
    ContainsBlanks,
    NotContainsBlanks,
    ContainsErrors,
    NotContainsErrors,
    /// `today`, `yesterday`, `last7Days`, `thisWeek` and the rest.
    TimePeriod(String),
    Top10 {
        bottom: bool,
        percent: bool,
        rank: u32,
    },
    AboveAverage {
        above: bool,
        equal: bool,
        std_dev: u32,
    },
    Duplicates,
    Uniques,
    ColorScale {
        cfvos: Vec<Cfvo>,
        colors: Vec<Color>,
    },
    DataBar(Box<DataBar>),
    IconSet(Box<IconSet>),
    /// A rule this can't read, kept as it was.
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfRule {
    pub kind: CfKind,
    pub priority: i32,
    pub stop: bool,
    /// A differential format of the workbook's, or one of an extended rule's own.
    pub dxf: Option<u32>,
    pub own_dxf: Option<Dxf>,
    /// Formulas, written for the first cell of the block's first area.
    pub formulas: Vec<String>,
    /// The extended rule this one is paired with, or its own id for an extended one.
    pub x14_id: Option<String>,
}

/// One `conditionalFormatting`: the areas it covers and its rules.
#[derive(Debug, Clone, PartialEq)]
pub struct CfBlock {
    pub areas: Vec<Rect>,
    pub rules: Vec<CfRule>,
    /// An extended block, which is drawn and kept but not edited.
    pub extended: bool,
    /// The element as the file had it, while nothing in it has changed.
    pub raw: Option<String>,
}

impl CfBlock {
    /// The cell its formulas are written for.
    pub fn anchor(&self) -> (u32, u32) {
        self.areas
            .first()
            .map_or((1, 1), |area| (area.top, area.left))
    }

    pub fn covers(&self, row: u32, col: u32) -> bool {
        self.areas.iter().any(|area| area.contains(row, col))
    }
}
