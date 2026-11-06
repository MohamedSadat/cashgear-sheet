//! How a table's style formats each of its cells: the style's elements laid on each other in
//! Excel's order, each over the part of the table it names. Built-in styles come from Excel 2016's
//! own definitions, read over COM into `cgs-model/data/table-styles.json`.

use std::collections::HashMap;
use std::sync::OnceLock;

use cgs_model::{
    Color, ColorKind, Dxf, DxfFill, Line, LineStyle, Pattern, StyleTable, Table, TableElement,
    TableStyleDef, TableStyleElement, ThemePalette,
};
use serde_json::Value;

const BUILT_IN: &str = include_str!("../../cgs-model/data/table-styles.json");

/// The colours Excel itself gives the built-in styles' tints of the Office theme, which differ by
/// a step here and there from what the tint's formula gives.
fn exact_colors() -> &'static HashMap<(u8, u32), u32> {
    static COLORS: OnceLock<HashMap<(u8, u32), u32>> = OnceLock::new();
    COLORS.get_or_init(|| {
        let mut out = HashMap::new();
        let data: Value = serde_json::from_str(BUILT_IN).expect("the built-in table styles");
        fn walk(value: &Value, out: &mut HashMap<(u8, u32), u32>) {
            match value {
                Value::Object(map) => {
                    if let (Some(theme), Some(rgb)) = (map.get("theme"), map.get("rgb")) {
                        let tint = map.get("tint").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                        if let (Some(theme), Some(rgb)) = (
                            theme.as_u64(),
                            rgb.as_str().and_then(|text| {
                                u32::from_str_radix(text.trim_start_matches('#'), 16).ok()
                            }),
                        ) {
                            out.insert((theme as u8, tint.to_bits()), rgb);
                        }
                    }
                    for child in map.values() {
                        walk(child, out);
                    }
                }
                Value::Array(list) => list.iter().for_each(|child| walk(child, out)),
                _ => {}
            }
        }
        walk(&data, &mut out);
        out
    })
}

/// A built-in style's colour as Excel shows it, when the workbook has the Office theme's colour
/// for it. Text and background colours stay theme colours, which dark mode swaps.
pub fn exact(color: Color, palette: &ThemePalette) -> Color {
    let ColorKind::Theme(index) = color.kind else {
        return color;
    };
    if index < 2 || color.tint == 0.0 {
        return color;
    }
    let office = ThemePalette::default();
    if palette.0.get(index as usize) != office.0.get(index as usize) {
        return color;
    }
    match exact_colors().get(&(index, color.tint.to_bits())) {
        Some(&rgb) => Color {
            kind: ColorKind::Rgb(rgb),
            tint: 0.0,
        },
        None => color,
    }
}

fn color(value: &Value) -> Option<Color> {
    let tint = value["tint"].as_f64().unwrap_or(0.0) as f32;
    if let Some(theme) = value["theme"].as_u64() {
        return Some(Color {
            kind: ColorKind::Theme(theme.min(255) as u8),
            tint,
        });
    }
    let rgb = value["rgb"].as_str()?.trim_start_matches('#');
    Some(Color {
        kind: ColorKind::Rgb(u32::from_str_radix(rgb, 16).ok()?),
        tint,
    })
}

fn line(value: &Value) -> Option<Line> {
    let style = match value["style"].as_str()? {
        "hair" => LineStyle::Hair,
        "dotted" => LineStyle::Dotted,
        "dashed" => LineStyle::Dashed,
        "thin" => LineStyle::Thin,
        "medium" => LineStyle::Medium,
        "thick" => LineStyle::Thick,
        "double" => LineStyle::Double,
        "dashDot" => LineStyle::DashDot,
        "dashDotDot" => LineStyle::DashDotDot,
        "mediumDashed" => LineStyle::MediumDashed,
        "mediumDashDot" => LineStyle::MediumDashDot,
        "mediumDashDotDot" => LineStyle::MediumDashDotDot,
        "slantDashDot" => LineStyle::SlantDashDot,
        _ => LineStyle::None,
    };
    Some(Line {
        style,
        color: color(&value["color"]).unwrap_or(Color::AUTO),
    })
}

fn element(kind: TableElement, value: &Value) -> TableStyleElement {
    let (dxf, size) = element_look(value);
    TableStyleElement { kind, dxf, size }
}

/// One element's format and stripe size, as `table-styles.json` and `pivot-styles.json` hold it.
pub(crate) fn element_look(value: &Value) -> (Dxf, u32) {
    let mut dxf = Dxf::default();
    let font = &value["font"];
    dxf.font.bold = font["bold"].as_bool();
    dxf.font.italic = font["italic"].as_bool();
    dxf.font.color = color(&font["color"]);
    if value["fill"]["pattern"].as_str() == Some("solid") {
        dxf.fill = Some(DxfFill {
            pattern: Pattern::Solid,
            color: color(&value["fill"]["color"]).unwrap_or(Color::AUTO),
            pattern_color: Color::AUTO,
        });
    }
    let border = &value["border"];
    dxf.border.left = line(&border["left"]);
    dxf.border.right = line(&border["right"]);
    dxf.border.top = line(&border["top"]);
    dxf.border.bottom = line(&border["bottom"]);
    dxf.border.vertical = line(&border["vertical"]);
    dxf.border.horizontal = line(&border["horizontal"]);
    (dxf, value["stripeSize"].as_u64().unwrap_or(1).max(1) as u32)
}

/// Excel's 60 built-in table styles, read once.
pub fn built_in() -> &'static [TableStyleDef] {
    static STYLES: OnceLock<Vec<TableStyleDef>> = OnceLock::new();
    STYLES.get_or_init(|| {
        let data: Value = serde_json::from_str(BUILT_IN).expect("the built-in table styles");
        data["styles"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|style| TableStyleDef {
                        name: style["name"].as_str().unwrap_or_default().to_string(),
                        elements: style["elements"]
                            .as_object()
                            .map(|elements| {
                                elements
                                    .iter()
                                    .filter_map(|(name, value)| {
                                        Some(element(TableElement::from_xml(name)?, value))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// A style by its name: the file's own first, as a file's style can share a built-in's name.
pub fn find<'a>(styles: &'a StyleTable, name: &str) -> Option<&'a TableStyleDef> {
    styles
        .table_styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            built_in()
                .iter()
                .find(|style| style.name.eq_ignore_ascii_case(name))
        })
}

/// The cells an element covers in a table, inclusive, for the cell at `row`, `col`.
fn region(
    table: &Table,
    style: &TableStyleDef,
    kind: TableElement,
    row: u32,
    col: u32,
) -> Option<(u32, u32, u32, u32)> {
    let t = table.range;
    let (first, last) = table.data_rows();
    let header = table.header_rows > 0;
    let totals = table.totals_rows > 0;
    let options = &table.style;
    let in_data = row >= first && row <= last;
    // A stripe's span, counted from its start, with the other stripe's size after it.
    let stripe = |at: u32, start: u32, end: u32, first_kind, second_kind, which_first: bool| {
        let one = style
            .element(first_kind)
            .map_or(1, |e: &TableStyleElement| e.size);
        let two = style
            .element(second_kind)
            .map_or(1, |e: &TableStyleElement| e.size);
        let cycle = one + two;
        let place = (at - start) % cycle;
        let in_first = place < one;
        if in_first != which_first {
            return None;
        }
        let from = if in_first {
            at - place
        } else {
            at - (place - one)
        };
        let size = if in_first { one } else { two };
        Some((from, (from + size - 1).min(end)))
    };
    Some(match kind {
        TableElement::WholeTable => (t.top, t.left, t.bottom, t.right),
        TableElement::HeaderRow if header => (t.top, t.left, t.top, t.right),
        TableElement::TotalRow if totals => (t.bottom, t.left, t.bottom, t.right),
        TableElement::FirstColumn if options.first_column => (t.top, t.left, t.bottom, t.left),
        TableElement::LastColumn if options.last_column => (t.top, t.right, t.bottom, t.right),
        TableElement::FirstRowStripe | TableElement::SecondRowStripe
            if options.row_stripes && in_data =>
        {
            let (from, to) = stripe(
                row,
                first,
                last,
                TableElement::FirstRowStripe,
                TableElement::SecondRowStripe,
                kind == TableElement::FirstRowStripe,
            )?;
            (from, t.left, to, t.right)
        }
        TableElement::FirstColumnStripe | TableElement::SecondColumnStripe
            if options.column_stripes && in_data =>
        {
            let (from, to) = stripe(
                col,
                t.left,
                t.right,
                TableElement::FirstColumnStripe,
                TableElement::SecondColumnStripe,
                kind == TableElement::FirstColumnStripe,
            )?;
            (first, from, last, to)
        }
        TableElement::FirstHeaderCell if header && options.first_column => {
            (t.top, t.left, t.top, t.left)
        }
        TableElement::LastHeaderCell if header && options.last_column => {
            (t.top, t.right, t.top, t.right)
        }
        TableElement::FirstTotalCell if totals && options.first_column => {
            (t.bottom, t.left, t.bottom, t.left)
        }
        TableElement::LastTotalCell if totals && options.last_column => {
            (t.bottom, t.right, t.bottom, t.right)
        }
        _ => return None,
    })
}

/// What a table's style does to one of its cells, with each side's line as that cell sees it:
/// an element's outer lines on its edges, and its inside lines between its cells.
pub fn cell_look(table: &Table, style: &TableStyleDef, row: u32, col: u32) -> Dxf {
    let mut elements: Vec<&TableStyleElement> = style.elements.iter().collect();
    elements.sort_by_key(|element| element.kind.layer());
    let mut look = Dxf::default();
    for element in elements {
        let Some((top, left, bottom, right)) = region(table, style, element.kind, row, col) else {
            continue;
        };
        if row < top || row > bottom || col < left || col > right {
            continue;
        }
        let mut here = element.dxf.clone();
        let lines = element.dxf.border;
        here.border.left = if col == left {
            lines.left
        } else {
            lines.vertical
        };
        here.border.right = if col == right {
            lines.right
        } else {
            lines.vertical
        };
        here.border.top = if row == top {
            lines.top
        } else {
            lines.horizontal
        };
        here.border.bottom = if row == bottom {
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

/// A look with its built-in colours made exact for the workbook's theme.
pub fn exact_look(mut look: Dxf, palette: &ThemePalette) -> Dxf {
    let fix = |color: &mut Color| *color = exact(*color, palette);
    if let Some(color) = look.font.color.as_mut() {
        fix(color);
    }
    if let Some(fill) = look.fill.as_mut() {
        fix(&mut fill.color);
        fix(&mut fill.pattern_color);
    }
    for line in [
        &mut look.border.left,
        &mut look.border.right,
        &mut look.border.top,
        &mut look.border.bottom,
    ]
    .into_iter()
    .flatten()
    {
        fix(&mut line.color);
    }
    look
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgs_model::Rect;

    fn sales() -> Table {
        Table::new(
            "Sales".to_string(),
            0,
            Rect {
                top: 1,
                left: 1,
                bottom: 6,
                right: 3,
            },
            vec!["a".into(), "b".into(), "c".into()],
        )
    }

    #[test]
    fn all_sixty_built_in_styles_are_there() {
        assert_eq!(built_in().len(), 60);
        assert!(find(&StyleTable::default(), "tablestylemedium2").is_some());
    }

    #[test]
    fn medium_2_has_a_blue_header_and_stripes_on_alternate_rows() {
        let styles = StyleTable::default();
        let style = find(&styles, "TableStyleMedium2").unwrap();
        let table = sales();
        let header = cell_look(&table, style, 1, 2);
        assert_eq!(header.font.bold, Some(true));
        assert_eq!(header.fill.unwrap().color.kind, ColorKind::Theme(4));
        let striped = cell_look(&table, style, 2, 2);
        let fill = striped.fill.unwrap();
        assert_eq!(fill.color.kind, ColorKind::Theme(4));
        assert!((fill.color.tint - 0.8).abs() < 0.01);
        assert!(cell_look(&table, style, 3, 2).fill.is_none());
        // Thin lines between rows, and none between columns.
        let inside = cell_look(&table, style, 3, 2);
        assert_eq!(inside.border.top.unwrap().style, LineStyle::Thin);
        assert!(inside.border.left.is_none());
        let edge = cell_look(&table, style, 3, 1);
        assert_eq!(edge.border.left.unwrap().style, LineStyle::Thin);
    }

    #[test]
    fn the_totals_row_gets_a_double_line_on_top() {
        let styles = StyleTable::default();
        let style = find(&styles, "TableStyleMedium2").unwrap();
        let mut table = sales();
        table.totals_rows = 1;
        let total = cell_look(&table, style, 6, 1);
        assert_eq!(total.border.top.unwrap().style, LineStyle::Double);
        assert_eq!(total.font.bold, Some(true));
    }
}
