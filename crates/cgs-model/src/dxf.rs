//! Differential formats: what a table style's parts and a conditional format change about a cell,
//! leaving the rest as the cell has it. `styles.xml` keeps them in `dxfs`.

use crate::{Color, Line, Pattern, Underline};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DxfFont {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub strike: Option<bool>,
    pub underline: Option<Underline>,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DxfFill {
    pub pattern: Pattern,
    /// What a solid fill shows, which a differential format keeps in `bgColor`.
    pub color: Color,
    /// A pattern's own colour.
    pub pattern_color: Color,
}

/// Lines a format sets. Inside lines are a range's, between its cells.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DxfBorder {
    pub left: Option<Line>,
    pub right: Option<Line>,
    pub top: Option<Line>,
    pub bottom: Option<Line>,
    pub vertical: Option<Line>,
    pub horizontal: Option<Line>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dxf {
    pub font: DxfFont,
    pub fill: Option<DxfFill>,
    pub border: DxfBorder,
    /// A number format's code.
    pub num_fmt: Option<String>,
}

impl Dxf {
    /// `self` with what `over` sets laid on top.
    pub fn under(&mut self, over: &Dxf) {
        let font = &over.font;
        self.font.bold = font.bold.or(self.font.bold);
        self.font.italic = font.italic.or(self.font.italic);
        self.font.strike = font.strike.or(self.font.strike);
        self.font.underline = font.underline.or(self.font.underline);
        self.font.color = font.color.or(self.font.color);
        if over.fill.is_some() {
            self.fill = over.fill;
        }
        let border = &over.border;
        let take = |mine: &mut Option<Line>, theirs: Option<Line>| {
            if theirs.is_some() {
                *mine = theirs;
            }
        };
        take(&mut self.border.left, border.left);
        take(&mut self.border.right, border.right);
        take(&mut self.border.top, border.top);
        take(&mut self.border.bottom, border.bottom);
        take(&mut self.border.vertical, border.vertical);
        take(&mut self.border.horizontal, border.horizontal);
        if over.num_fmt.is_some() {
            self.num_fmt.clone_from(&over.num_fmt);
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Dxf::default()
    }
}

/// Which part of a table a table style element formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TableElement {
    WholeTable,
    HeaderRow,
    TotalRow,
    FirstColumn,
    LastColumn,
    FirstRowStripe,
    SecondRowStripe,
    FirstColumnStripe,
    SecondColumnStripe,
    FirstHeaderCell,
    LastHeaderCell,
    FirstTotalCell,
    LastTotalCell,
}

impl TableElement {
    pub fn from_xml(text: &str) -> Option<Self> {
        Some(match text {
            "wholeTable" => Self::WholeTable,
            "headerRow" => Self::HeaderRow,
            "totalRow" => Self::TotalRow,
            "firstColumn" => Self::FirstColumn,
            "lastColumn" => Self::LastColumn,
            "firstRowStripe" => Self::FirstRowStripe,
            "secondRowStripe" => Self::SecondRowStripe,
            "firstColumnStripe" => Self::FirstColumnStripe,
            "secondColumnStripe" => Self::SecondColumnStripe,
            "firstHeaderCell" => Self::FirstHeaderCell,
            "lastHeaderCell" => Self::LastHeaderCell,
            "firstTotalCell" => Self::FirstTotalCell,
            "lastTotalCell" => Self::LastTotalCell,
            _ => return None,
        })
    }

    /// The order Excel lays them on, lowest first.
    pub fn layer(self) -> u8 {
        match self {
            Self::WholeTable => 0,
            Self::FirstColumnStripe => 1,
            Self::SecondColumnStripe => 2,
            Self::FirstRowStripe => 3,
            Self::SecondRowStripe => 4,
            Self::LastColumn => 5,
            Self::FirstColumn => 6,
            Self::HeaderRow => 7,
            Self::TotalRow => 8,
            Self::FirstHeaderCell => 9,
            Self::LastHeaderCell => 10,
            Self::FirstTotalCell => 11,
            Self::LastTotalCell => 12,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableStyleElement {
    pub kind: TableElement,
    pub dxf: Dxf,
    /// How many rows or columns a stripe spans.
    pub size: u32,
}

/// A table style: a built-in one, or one of a file's own.
#[derive(Debug, Clone, PartialEq)]
pub struct TableStyleDef {
    pub name: String,
    pub elements: Vec<TableStyleElement>,
}

impl TableStyleDef {
    pub fn element(&self, kind: TableElement) -> Option<&TableStyleElement> {
        self.elements.iter().find(|element| element.kind == kind)
    }
}
