//! How a pivot table's style formats its cells: the style's elements laid on each other, each over
//! the part of the pivot table it names, worked out from the table's location and lines. Built-in
//! styles come from Excel 2016's own definitions, read over COM into `cgs-model/data/pivot-styles.json`.

use std::sync::OnceLock;

use cgs_model::pivot::{ItemType, PivotTable, VALUES_FIELD};
use cgs_model::{Dxf, Rect};
use serde_json::Value;

const BUILT_IN: &str = include_str!("../../cgs-model/data/pivot-styles.json");

/// The parts of a pivot table a style element formats, in the order Excel lays them on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PivotElement {
    WholeTable,
    PageFieldLabels,
    PageFieldValues,
    FirstColumnStripe,
    SecondColumnStripe,
    FirstRowStripe,
    SecondRowStripe,
    FirstColumn,
    HeaderRow,
    FirstHeaderCell,
    SubtotalColumn(u8),
    SubtotalRow(u8),
    BlankRow,
    ColumnSubheading(u8),
    RowSubheading(u8),
    LastColumn,
    TotalRow,
    LastHeaderCell,
    FirstTotalCell,
    LastTotalCell,
}

impl PivotElement {
    fn from_xml(text: &str) -> Option<Self> {
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
            "firstSubtotalColumn" => Self::SubtotalColumn(1),
            "secondSubtotalColumn" => Self::SubtotalColumn(2),
            "thirdSubtotalColumn" => Self::SubtotalColumn(3),
            "firstSubtotalRow" => Self::SubtotalRow(1),
            "secondSubtotalRow" => Self::SubtotalRow(2),
            "thirdSubtotalRow" => Self::SubtotalRow(3),
            "blankRow" => Self::BlankRow,
            "firstColumnSubheading" => Self::ColumnSubheading(1),
            "secondColumnSubheading" => Self::ColumnSubheading(2),
            "thirdColumnSubheading" => Self::ColumnSubheading(3),
            "firstRowSubheading" => Self::RowSubheading(1),
            "secondRowSubheading" => Self::RowSubheading(2),
            "thirdRowSubheading" => Self::RowSubheading(3),
            "pageFieldLabels" => Self::PageFieldLabels,
            "pageFieldValues" => Self::PageFieldValues,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PivotStyleElement {
    pub kind: PivotElement,
    pub dxf: Dxf,
    pub size: u32,
}

#[derive(Debug, Clone)]
pub struct PivotStyleDef {
    pub name: String,
    pub elements: Vec<PivotStyleElement>,
}

/// Excel's built-in pivot styles, read once.
pub fn built_in() -> &'static [PivotStyleDef] {
    static STYLES: OnceLock<Vec<PivotStyleDef>> = OnceLock::new();
    STYLES.get_or_init(|| {
        let data: Value = serde_json::from_str(BUILT_IN).expect("the built-in pivot styles");
        data["styles"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|style| {
                        let mut elements: Vec<PivotStyleElement> = style["elements"]
                            .as_object()
                            .map(|map| {
                                map.iter()
                                    .filter_map(|(name, value)| {
                                        let kind = PivotElement::from_xml(name)?;
                                        let (dxf, size) = crate::table_style::element_look(value);
                                        Some(PivotStyleElement { kind, dxf, size })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        elements.sort_by_key(|e| e.kind);
                        PivotStyleDef {
                            name: style["name"].as_str().unwrap_or_default().to_string(),
                            elements,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

pub fn find(name: &str) -> Option<&'static PivotStyleDef> {
    built_in()
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(name))
}

/// A line's members in full, filling in what `repeat` leaves out.
fn depth(lines: &[cgs_model::pivot::AxisLine], index: usize) -> usize {
    lines
        .get(index)
        .map_or(0, |line| line.repeat as usize + line.members.len())
}

fn rect(top: u32, left: u32, bottom: u32, right: u32) -> Rect {
    Rect {
        top,
        left,
        bottom,
        right,
    }
}

/// The part of the pivot table an element covers, if the cell is in it.
fn region(table: &PivotTable, kind: PivotElement, row: u32, col: u32, size: u32) -> Option<Rect> {
    let l = &table.location;
    let r = l.range;
    let style = &table.style;
    let data_top = r.top + l.first_data_row;
    let data_left = r.left + l.first_data_col;
    let pages = table.page_fields.len() as u32;
    if row < r.top {
        // Report filters, above the table.
        let first = r.top.saturating_sub(pages + 1);
        if pages == 0 || row < first || row >= first + pages {
            return None;
        }
        return match kind {
            // The report filters' labels are one block, and their choices another.
            PivotElement::PageFieldLabels if col == r.left => {
                Some(rect(first, col, first + pages - 1, col))
            }
            PivotElement::PageFieldValues if col == r.left + 1 => {
                Some(rect(first, col, first + pages - 1, col))
            }
            _ => None,
        };
    }
    if row > r.bottom || col < r.left || col > r.right {
        return None;
    }
    let real_rows = table
        .row_fields
        .iter()
        .filter(|&&f| f != VALUES_FIELD)
        .count();
    let real_cols = table
        .col_fields
        .iter()
        .filter(|&&f| f != VALUES_FIELD)
        .count();
    let row_line = (row >= data_top).then(|| (row - data_top) as usize);
    let col_line = (col >= data_left).then(|| (col - data_left) as usize);
    let row_kind = row_line
        .and_then(|k| table.row_items.get(k))
        .map(|line| line.kind);
    let col_kind = col_line
        .and_then(|k| table.col_items.get(k))
        .map(|line| line.kind);
    let level = |d: usize| -> u8 { d.clamp(1, 3) as u8 };
    match kind {
        PivotElement::WholeTable => Some(r),
        PivotElement::HeaderRow if style.col_headers && row < data_top => {
            Some(rect(r.top, r.left, data_top - 1, r.right))
        }
        // With no row fields, the label of the one data row is a heading too.
        PivotElement::HeaderRow
            if style.col_headers
                && real_rows == 0
                && col < data_left
                && row_kind == Some(ItemType::Data) =>
        {
            // Its bottom edge is not the heading's: Excel draws no line under it.
            Some(rect(row, r.left, row + 1, data_left - 1))
        }
        PivotElement::FirstHeaderCell if row < data_top && col < data_left => {
            Some(rect(r.top, r.left, data_top - 1, data_left - 1))
        }
        PivotElement::FirstColumn if style.row_headers && row >= data_top && col < data_left => {
            Some(rect(data_top, r.left, r.bottom, data_left - 1))
        }
        PivotElement::TotalRow if row_kind == Some(ItemType::Grand) => {
            Some(rect(row, r.left, row, r.right))
        }
        PivotElement::LastColumn if style.last_column && col_kind == Some(ItemType::Grand) => {
            Some(rect(r.top + l.first_header_row, col, r.bottom, col))
        }
        PivotElement::FirstRowStripe | PivotElement::SecondRowStripe if style.row_stripes => {
            let k = row_line? as u32;
            let size = size.max(1);
            let band = k / size;
            let first = band.is_multiple_of(2);
            let wanted = kind == PivotElement::FirstRowStripe;
            (first == wanted && row_kind != Some(ItemType::Grand)).then(|| {
                let top = data_top + band * size;
                rect(top, r.left, (top + size - 1).min(r.bottom), r.right)
            })
        }
        PivotElement::FirstColumnStripe | PivotElement::SecondColumnStripe if style.col_stripes => {
            let k = col_line? as u32;
            let size = size.max(1);
            let band = k / size;
            let first = band.is_multiple_of(2);
            let wanted = kind == PivotElement::FirstColumnStripe;
            (first == wanted && col_kind != Some(ItemType::Grand)).then(|| {
                let left = data_left + band * size;
                rect(data_top, left, r.bottom, (left + size - 1).min(r.right))
            })
        }
        PivotElement::SubtotalRow(n) => {
            let k = row_line?;
            let line = table.row_items.get(k)?;
            let is_total = !matches!(
                line.kind,
                ItemType::Data | ItemType::Grand | ItemType::Blank
            );
            // Only a subtotal's own line: a parent item showing its subtotal at the top takes the
            // row subheading instead.
            (is_total && level(depth(&table.row_items, k)) == n)
                .then(|| rect(row, r.left, row, r.right))
        }
        PivotElement::RowSubheading(n) if style.row_headers => {
            let k = row_line?;
            let line = table.row_items.get(k)?;
            if line.kind != ItemType::Data {
                return None;
            }
            let d = depth(&table.row_items, k);
            // A parent item on a line of its own, in compact and outline form: its whole row.
            let outline = d >= 1
                && table
                    .row_fields
                    .get(d - 1)
                    .is_some_and(|&f| f == VALUES_FIELD || table.fields[f as usize].outline);
            if d < table.row_fields.len() && outline {
                return (level(d) == n).then(|| rect(row, r.left, row, r.right));
            }
            // In tabular form a parent item shares its first child's line: its own label cell.
            let columns = crate::pivots::row_columns(table);
            let c = col.checked_sub(r.left)?;
            (line.repeat as usize..d.saturating_sub(1))
                .find(|&lv| columns.get(lv) == Some(&c) && columns.get(lv + 1) != Some(&c))
                .filter(|&lv| table.row_fields.get(lv) != Some(&VALUES_FIELD) && level(lv + 1) == n)
                .map(|_| rect(row, col, row, col))
        }
        PivotElement::BlankRow if row_kind == Some(ItemType::Blank) => {
            Some(rect(row, r.left, row, r.right))
        }
        PivotElement::SubtotalColumn(n) => {
            let k = col_line?;
            let line = table.col_items.get(k)?;
            (!matches!(
                line.kind,
                ItemType::Data | ItemType::Grand | ItemType::Blank
            ) && level(depth(&table.col_items, k)) == n)
                .then(|| rect(r.top + l.first_header_row, col, r.bottom, col))
        }
        PivotElement::ColumnSubheading(n) if style.col_headers && row < data_top => {
            let k = col_line?;
            let first_item_row = data_top - table.col_fields.len() as u32;
            if row < first_item_row {
                return None;
            }
            let lv = (row - first_item_row) as usize + 1;
            (lv < real_cols.max(1) + usize::from(real_cols == 0)
                && level(lv) == n
                && table.col_items.get(k).is_some())
            .then(|| rect(row, col, row, col))
        }
        PivotElement::FirstTotalCell if row_kind == Some(ItemType::Grand) && col < data_left => {
            Some(rect(row, r.left, row, data_left - 1))
        }
        PivotElement::LastTotalCell
            if row_kind == Some(ItemType::Grand) && col_kind == Some(ItemType::Grand) =>
        {
            Some(rect(row, col, row, col))
        }
        PivotElement::LastHeaderCell if row < data_top && col_kind == Some(ItemType::Grand) => {
            Some(rect(r.top, col, data_top - 1, col))
        }
        _ => None,
    }
}

/// What a pivot table's style does to one of its cells, with each side's line as the cell sees
/// it: an element's outer lines on its edges, its inside lines between its cells.
pub fn cell_look(table: &PivotTable, style: &PivotStyleDef, row: u32, col: u32) -> Dxf {
    let mut look = Dxf::default();
    for element in &style.elements {
        let Some(area) = region(table, element.kind, row, col, element.size) else {
            continue;
        };
        if row < area.top || row > area.bottom || col < area.left || col > area.right {
            continue;
        }
        let mut here = element.dxf.clone();
        let lines = element.dxf.border;
        here.border.left = if col == area.left {
            lines.left
        } else {
            lines.vertical
        };
        here.border.right = if col == area.right {
            lines.right
        } else {
            lines.vertical
        };
        here.border.top = if row == area.top {
            lines.top
        } else {
            lines.horizontal
        };
        here.border.bottom = if row == area.bottom {
            lines.bottom
        } else {
            lines.horizontal
        };
        here.border.vertical = None;
        here.border.horizontal = None;
        look.under(&here);
    }
    look
}

/// The cells a pivot table's style can reach: the table and its report filters.
pub fn area(table: &PivotTable) -> Rect {
    crate::pivots::pivot_area(table)
}
