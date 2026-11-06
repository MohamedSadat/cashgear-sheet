//! `dxfs` and `tableStyles` in `styles.xml`: differential formats and the file's own table styles.

use cgs_model::{
    Color, Dxf, DxfFill, Line, Pattern, TableElement, TableStyleDef, TableStyleElement, Underline,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::styles::{line_style, parse_color, pattern};
use crate::xml::{attr, attr_u32, val_flag};

/// What the differential formats and table styles of a `styles.xml` are.
#[derive(Debug, Default)]
pub struct DxfTables {
    pub dxfs: Vec<Dxf>,
    pub table_styles: Vec<TableStyleDef>,
    pub default_table_style: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    None,
    Left,
    Right,
    Top,
    Bottom,
    Vertical,
    Horizontal,
}

fn side_line(dxf: &mut Dxf, side: Side) -> Option<&mut Option<Line>> {
    Some(match side {
        Side::Left => &mut dxf.border.left,
        Side::Right => &mut dxf.border.right,
        Side::Top => &mut dxf.border.top,
        Side::Bottom => &mut dxf.border.bottom,
        Side::Vertical => &mut dxf.border.vertical,
        Side::Horizontal => &mut dxf.border.horizontal,
        Side::None => return None,
    })
}

/// Starts or fills in what one element inside a `dxf` says.
fn read_dxf_part(
    element: &BytesStart<'_>,
    dxf: &mut Dxf,
    in_fill: &mut bool,
    fill_colors: &mut (Option<Color>, Option<Color>),
    side: &mut Side,
) {
    match element.local_name().as_ref() {
        b"b" => dxf.font.bold = Some(val_flag(element)),
        b"i" => dxf.font.italic = Some(val_flag(element)),
        b"strike" => dxf.font.strike = Some(val_flag(element)),
        b"u" => {
            dxf.font.underline = Some(match attr(element, b"val").as_deref() {
                Some("none") => Underline::None,
                Some("double") => Underline::Double,
                Some("singleAccounting") => Underline::SingleAccounting,
                Some("doubleAccounting") => Underline::DoubleAccounting,
                _ => Underline::Single,
            });
        }
        b"color" if !*in_fill && *side == Side::None => {
            dxf.font.color = Some(parse_color(element));
        }
        b"color" => {
            if let Some(line) = side_line(dxf, *side).and_then(Option::as_mut) {
                line.color = parse_color(element);
            }
        }
        b"fill" => *in_fill = true,
        b"patternFill" => {
            // A differential fill with no pattern named is a solid one.
            let named = attr(element, b"patternType").map(|text| pattern(&text));
            dxf.fill = Some(DxfFill {
                pattern: named.unwrap_or(Pattern::Solid),
                color: Color::AUTO,
                pattern_color: Color::AUTO,
            });
        }
        b"fgColor" => fill_colors.0 = Some(parse_color(element)),
        b"bgColor" => fill_colors.1 = Some(parse_color(element)),
        b"numFmt" => dxf.num_fmt = attr(element, b"formatCode"),
        kind @ (b"left" | b"start" | b"right" | b"end" | b"top" | b"bottom" | b"vertical"
        | b"horizontal") => {
            *side = match kind {
                b"left" | b"start" => Side::Left,
                b"right" | b"end" => Side::Right,
                b"top" => Side::Top,
                b"bottom" => Side::Bottom,
                b"vertical" => Side::Vertical,
                _ => Side::Horizontal,
            };
            let style = line_style(&attr(element, b"style").unwrap_or_default());
            if let Some(slot) = side_line(dxf, *side) {
                // A side named with no style says there is no line, which still counts.
                *slot = Some(Line {
                    style,
                    color: Color::AUTO,
                });
            }
        }
        _ => {}
    }
}

/// Finishes a fill: a solid one shows its `bgColor`, and a pattern draws `fgColor` on it.
fn finish_fill(dxf: &mut Dxf, colors: (Option<Color>, Option<Color>)) {
    if let Some(fill) = dxf.fill.as_mut() {
        match fill.pattern {
            Pattern::Solid => fill.color = colors.1.or(colors.0).unwrap_or(Color::AUTO),
            _ => {
                fill.color = colors.1.unwrap_or(Color::AUTO);
                fill.pattern_color = colors.0.unwrap_or(Color::AUTO);
            }
        }
    }
}

/// A table style as read, with its elements' formats still as indexes.
type StyleRead = (TableStyleDef, Vec<(TableElement, u32, u32)>);

pub fn read_dxf_tables(xml: &[u8]) -> DxfTables {
    let mut out = DxfTables::default();
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut in_dxfs = false;
    let mut current: Option<Dxf> = None;
    let mut in_fill = false;
    let mut fill_colors = (None, None);
    let mut side = Side::None;
    let mut style: Option<StyleRead> = None;
    let mut pending_styles: Vec<StyleRead> = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf);
        let (element, empty) = match &event {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(element)) => (Some(element.clone()), false),
            Ok(Event::Empty(element)) => (Some(element.clone()), true),
            Ok(Event::End(end)) => {
                match end.local_name().as_ref() {
                    b"dxfs" => in_dxfs = false,
                    b"dxf" => {
                        if let Some(mut dxf) = current.take() {
                            finish_fill(&mut dxf, fill_colors);
                            out.dxfs.push(dxf);
                        }
                    }
                    b"fill" => in_fill = false,
                    b"left" | b"start" | b"right" | b"end" | b"top" | b"bottom" | b"vertical"
                    | b"horizontal" => side = Side::None,
                    b"tableStyle" => {
                        if let Some(done) = style.take() {
                            pending_styles.push(done);
                        }
                    }
                    _ => {}
                }
                buf.clear();
                continue;
            }
            _ => {
                buf.clear();
                continue;
            }
        };
        let Some(element) = element else { continue };
        match element.local_name().as_ref() {
            b"dxfs" if !empty => in_dxfs = true,
            b"dxf" if in_dxfs => {
                if empty {
                    out.dxfs.push(Dxf::default());
                } else {
                    current = Some(Dxf::default());
                    in_fill = false;
                    fill_colors = (None, None);
                    side = Side::None;
                }
            }
            b"tableStyles" => {
                out.default_table_style = attr(&element, b"defaultTableStyle");
            }
            b"tableStyle" => {
                let def = TableStyleDef {
                    name: attr(&element, b"name").unwrap_or_default(),
                    elements: Vec::new(),
                };
                // A style only for pivot tables isn't one a table can use.
                let for_tables = attr(&element, b"table").as_deref() != Some("0");
                if for_tables {
                    if empty {
                        pending_styles.push((def, Vec::new()));
                    } else {
                        style = Some((def, Vec::new()));
                    }
                }
            }
            b"tableStyleElement" => {
                if let (Some((_, list)), Some(kind)) = (
                    style.as_mut(),
                    attr(&element, b"type").and_then(|text| TableElement::from_xml(&text)),
                ) {
                    list.push((
                        kind,
                        attr_u32(&element, b"dxfId").unwrap_or(u32::MAX),
                        attr_u32(&element, b"size").unwrap_or(1),
                    ));
                }
            }
            _ => {
                if let Some(dxf) = current.as_mut() {
                    read_dxf_part(&element, dxf, &mut in_fill, &mut fill_colors, &mut side);
                    if empty && matches!(element.local_name().as_ref(), b"fill") {
                        in_fill = false;
                    }
                }
            }
        }
        buf.clear();
    }
    // Elements point at formats by index, which are all read now.
    for (mut def, list) in pending_styles {
        for (kind, dxf, size) in list {
            def.elements.push(TableStyleElement {
                kind,
                dxf: out.dxfs.get(dxf as usize).cloned().unwrap_or_default(),
                size: size.max(1),
            });
        }
        out.table_styles.push(def);
    }
    out
}

fn color_xml(name: &str, color: &Color) -> String {
    match color.kind {
        cgs_model::ColorKind::Rgb(rgb) => format!("<{name} rgb=\"FF{rgb:06X}\"/>"),
        cgs_model::ColorKind::Theme(theme) if color.tint != 0.0 => {
            let tint = (f64::from(color.tint) * 32767.0).trunc() / 32767.0;
            format!("<{name} theme=\"{theme}\" tint=\"{tint}\"/>")
        }
        cgs_model::ColorKind::Theme(theme) => format!("<{name} theme=\"{theme}\"/>"),
        cgs_model::ColorKind::Indexed(index) => format!("<{name} indexed=\"{index}\"/>"),
        cgs_model::ColorKind::Auto => format!("<{name} auto=\"1\"/>"),
    }
}

/// A differential format's insides, as Excel writes them: font, number format, fill, border.
pub fn dxf_xml(dxf: &Dxf) -> String {
    let mut out = String::new();
    let font = &dxf.font;
    let mut inner = String::new();
    let flag = |name: &str, value: Option<bool>, inner: &mut String| {
        if let Some(on) = value {
            inner.push_str(&if on {
                format!("<{name}/>")
            } else {
                format!("<{name} val=\"0\"/>")
            });
        }
    };
    flag("b", font.bold, &mut inner);
    flag("i", font.italic, &mut inner);
    flag("strike", font.strike, &mut inner);
    if let Some(underline) = font.underline {
        let value = match underline {
            Underline::None => "none",
            Underline::Single => "single",
            Underline::Double => "double",
            Underline::SingleAccounting => "singleAccounting",
            Underline::DoubleAccounting => "doubleAccounting",
        };
        inner.push_str(&format!("<u val=\"{value}\"/>"));
    }
    if let Some(color) = &font.color {
        inner.push_str(&color_xml("color", color));
    }
    if !inner.is_empty() {
        out.push_str(&format!("<font>{inner}</font>"));
    }
    if let Some(code) = &dxf.num_fmt {
        let id = 164;
        out.push_str(&format!(
            "<numFmt numFmtId=\"{id}\" formatCode=\"{}\"/>",
            crate::scan::escape_attr(code)
        ));
    }
    if let Some(fill) = &dxf.fill {
        match fill.pattern {
            Pattern::Solid => out.push_str(&format!(
                "<fill><patternFill>{}</patternFill></fill>",
                color_xml("bgColor", &fill.color)
            )),
            Pattern::None => out.push_str("<fill><patternFill patternType=\"none\"/></fill>"),
            Pattern::Hatch(hatch) => out.push_str(&format!(
                "<fill><patternFill patternType=\"{}\">{}{}</patternFill></fill>",
                hatch.name(),
                color_xml("fgColor", &fill.pattern_color),
                color_xml("bgColor", &fill.color)
            )),
        }
    }
    let border = &dxf.border;
    let sides = [
        ("left", border.left),
        ("right", border.right),
        ("top", border.top),
        ("bottom", border.bottom),
        ("vertical", border.vertical),
        ("horizontal", border.horizontal),
    ];
    if sides.iter().any(|(_, line)| line.is_some()) {
        out.push_str("<border>");
        for (name, line) in sides {
            let Some(line) = line else { continue };
            let style = crate::styles::line_style_name(line.style);
            if style == "none" {
                out.push_str(&format!("<{name}/>"));
            } else {
                out.push_str(&format!(
                    "<{name} style=\"{style}\">{}</{name}>",
                    color_xml("color", &line.color)
                ));
            }
        }
        out.push_str("</border>");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgs_model::{ColorKind, LineStyle};

    #[test]
    fn reads_formats_and_the_files_own_table_styles() {
        let xml = r#"<styleSheet><dxfs count="3"><dxf><font><b/><color rgb="FF9C0006"/></font><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill></dxf><dxf><border><top style="thin"><color theme="4"/></top><horizontal style="thin"/></border></dxf><dxf><numFmt numFmtId="164" formatCode="0.0%"/><fill><patternFill patternType="lightGray"><fgColor rgb="FF00FF00"/><bgColor rgb="FF0000FF"/></patternFill></fill></dxf></dxfs><tableStyles count="1" defaultTableStyle="TableStyleMedium9"><tableStyle name="Mine" pivot="0" count="2"><tableStyleElement type="wholeTable" dxfId="1"/><tableStyleElement type="firstRowStripe" size="2" dxfId="0"/></tableStyle></tableStyles></styleSheet>"#;
        let read = read_dxf_tables(xml.as_bytes());
        assert_eq!(read.dxfs.len(), 3);
        let first = &read.dxfs[0];
        assert_eq!(first.font.bold, Some(true));
        assert_eq!(
            first.font.color.map(|c| c.kind),
            Some(ColorKind::Rgb(0x9C0006))
        );
        let fill = first.fill.unwrap();
        assert_eq!(fill.pattern, Pattern::Solid);
        assert_eq!(fill.color.kind, ColorKind::Rgb(0xFFC7CE));
        let lines = &read.dxfs[1].border;
        assert_eq!(lines.top.unwrap().style, LineStyle::Thin);
        assert_eq!(lines.top.unwrap().color.kind, ColorKind::Theme(4));
        assert_eq!(lines.horizontal.unwrap().style, LineStyle::Thin);
        assert!(lines.left.is_none());
        assert_eq!(read.dxfs[2].num_fmt.as_deref(), Some("0.0%"));
        assert_eq!(
            read.default_table_style.as_deref(),
            Some("TableStyleMedium9")
        );
        let mine = &read.table_styles[0];
        assert_eq!(mine.name, "Mine");
        assert_eq!(mine.elements[1].kind, TableElement::FirstRowStripe);
        assert_eq!(mine.elements[1].size, 2);
        assert_eq!(mine.elements[1].dxf.font.bold, Some(true));
    }

    #[test]
    fn a_written_format_reads_back_the_same() {
        let xml = r#"<styleSheet><dxfs count="2"><dxf><font><b/><color rgb="FF9C0006"/></font><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill></dxf><dxf><border><top style="thin"><color theme="4"/></top><horizontal style="thin"><color auto="1"/></horizontal></border></dxf></dxfs></styleSheet>"#;
        let read = read_dxf_tables(xml.as_bytes());
        let written: Vec<String> = read.dxfs.iter().map(dxf_xml).collect();
        let again = read_dxf_tables(
            format!(
                "<styleSheet><dxfs><dxf>{}</dxf><dxf>{}</dxf></dxfs></styleSheet>",
                written[0], written[1]
            )
            .as_bytes(),
        );
        assert_eq!(again.dxfs, read.dxfs);
    }
}
