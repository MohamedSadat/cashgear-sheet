//! `styles.xml` and the theme's colour scheme.

use cgs_model::{
    Alignment, Border, Color, ColorKind, Fill, Font, HAlign, Line, LineStyle, Pattern,
    ReadingOrder, StyleTable, ThemePalette, Underline, VAlign, Xf,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::xml::{attr, attr_bool, attr_f64, attr_u32, val_flag};

fn parse_rgb(text: &str) -> Option<u32> {
    let text = text.trim();
    // Stored as AARRGGBB, and a few writers leave the alpha off. Excel ignores alpha.
    let hex = if text.len() == 8 { &text[2..] } else { text };
    (hex.len() == 6)
        .then(|| u32::from_str_radix(hex, 16).ok())
        .flatten()
}

fn parse_color(element: &BytesStart<'_>) -> Color {
    let tint = attr_f64(element, b"tint").unwrap_or(0.0) as f32;
    let kind = if let Some(rgb) = attr(element, b"rgb").and_then(|text| parse_rgb(&text)) {
        ColorKind::Rgb(rgb)
    } else if let Some(index) = attr_u32(element, b"theme") {
        ColorKind::Theme(index.min(255) as u8)
    } else if let Some(index) = attr_u32(element, b"indexed") {
        ColorKind::Indexed(index.min(255) as u8)
    } else {
        ColorKind::Auto
    };
    Color { kind, tint }
}

fn line_style(text: &str) -> LineStyle {
    match text {
        "hair" => LineStyle::Hair,
        "dotted" => LineStyle::Dotted,
        "dashed" => LineStyle::Dashed,
        "dashDot" => LineStyle::DashDot,
        "dashDotDot" => LineStyle::DashDotDot,
        "thin" => LineStyle::Thin,
        "medium" => LineStyle::Medium,
        "mediumDashed" => LineStyle::MediumDashed,
        "mediumDashDot" => LineStyle::MediumDashDot,
        "mediumDashDotDot" => LineStyle::MediumDashDotDot,
        "slantDashDot" => LineStyle::SlantDashDot,
        "thick" => LineStyle::Thick,
        "double" => LineStyle::Double,
        _ => LineStyle::None,
    }
}

fn pattern(text: &str) -> Pattern {
    match text {
        "solid" => Pattern::Solid,
        "gray0625" => Pattern::Mixed(6),
        "gray125" => Pattern::Mixed(12),
        "lightGray" | "lightHorizontal" | "lightVertical" | "lightDown" | "lightUp"
        | "lightGrid" | "lightTrellis" => Pattern::Mixed(25),
        "mediumGray" | "darkTrellis" => Pattern::Mixed(50),
        "darkHorizontal" | "darkVertical" | "darkDown" | "darkUp" | "darkGrid" => {
            Pattern::Mixed(50)
        }
        "darkGray" => Pattern::Mixed(75),
        _ => Pattern::None,
    }
}

fn parse_alignment(element: &BytesStart<'_>) -> Alignment {
    let horizontal = match attr(element, b"horizontal").as_deref() {
        Some("left") => HAlign::Left,
        Some("center") => HAlign::Center,
        Some("right") => HAlign::Right,
        Some("fill") => HAlign::Fill,
        Some("justify") => HAlign::Justify,
        Some("centerContinuous") => HAlign::CenterContinuous,
        Some("distributed") => HAlign::Distributed,
        _ => HAlign::General,
    };
    let vertical = match attr(element, b"vertical").as_deref() {
        Some("top") => VAlign::Top,
        Some("center") => VAlign::Center,
        Some("justify") => VAlign::Justify,
        Some("distributed") => VAlign::Distributed,
        _ => VAlign::Bottom,
    };
    Alignment {
        horizontal,
        vertical,
        wrap: attr_bool(element, b"wrapText").unwrap_or(false),
        shrink: attr_bool(element, b"shrinkToFit").unwrap_or(false),
        indent: attr_u32(element, b"indent").unwrap_or(0).min(255) as u8,
        rotation: attr_u32(element, b"textRotation").unwrap_or(0).min(255) as u8,
        reading_order: match attr_u32(element, b"readingOrder") {
            Some(1) => ReadingOrder::LeftToRight,
            Some(2) => ReadingOrder::RightToLeft,
            _ => ReadingOrder::Context,
        },
    }
}

fn parse_xf(element: &BytesStart<'_>) -> Xf {
    Xf {
        font: attr_u32(element, b"fontId").unwrap_or(0),
        fill: attr_u32(element, b"fillId").unwrap_or(0),
        border: attr_u32(element, b"borderId").unwrap_or(0),
        num_fmt: attr_u32(element, b"numFmtId").unwrap_or(0),
        alignment: Alignment::default(),
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Section {
    None,
    Fonts,
    Fills,
    Borders,
    CellXfs,
    Skipped,
}

#[derive(Clone, Copy, PartialEq)]
enum BorderSide {
    None,
    Left,
    Right,
    Top,
    Bottom,
    Diagonal,
}

/// Reads `styles.xml`. An unreadable part leaves the defaults, so the file still opens.
pub fn read_styles(xml: &[u8]) -> StyleTable {
    let mut table = StyleTable::default();
    table.fonts.clear();
    table.fills.clear();
    table.borders.clear();
    table.xfs.clear();
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut section = Section::None;
    let mut font = Font::default();
    let mut fill = Fill::default();
    let mut border = Border::default();
    let mut side = BorderSide::None;
    let mut in_gradient = false;
    let mut gradient_taken = false;
    let mut indexed: Vec<u32> = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(ref element)) => {
                let name = element.local_name();
                match (section, name.as_ref()) {
                    (_, b"fonts") => section = Section::Fonts,
                    (_, b"fills") => section = Section::Fills,
                    (_, b"borders") => section = Section::Borders,
                    (_, b"cellXfs") => section = Section::CellXfs,
                    // A nested section holds fonts and fills that aren't the table's.
                    (_, b"dxfs" | b"cellStyleXfs" | b"cellStyles" | b"extLst") => {
                        section = Section::Skipped;
                    }
                    (Section::Fonts, b"font") => font = Font::default(),
                    (Section::Fills, b"fill") => {
                        fill = Fill::default();
                        in_gradient = false;
                        gradient_taken = false;
                    }
                    (Section::Fills, b"patternFill") => {
                        fill.pattern = pattern(&attr(element, b"patternType").unwrap_or_default());
                    }
                    (Section::Fills, b"gradientFill") => in_gradient = true,
                    (Section::Borders, b"border") => {
                        border = Border {
                            diagonal_up: attr_bool(element, b"diagonalUp").unwrap_or(false),
                            diagonal_down: attr_bool(element, b"diagonalDown").unwrap_or(false),
                            ..Border::default()
                        };
                        side = BorderSide::None;
                    }
                    (
                        Section::Borders,
                        kind @ (b"left" | b"right" | b"start" | b"end" | b"top" | b"bottom"
                        | b"diagonal"),
                    ) => {
                        side = start_side(kind, element, &mut border);
                    }
                    (Section::CellXfs, b"xf") => table.xfs.push(parse_xf(element)),
                    _ => {}
                }
                apply_child(
                    section,
                    element,
                    &mut font,
                    &mut fill,
                    in_gradient,
                    &mut gradient_taken,
                    &mut border,
                    side,
                    &mut table,
                );
            }
            Ok(Event::Empty(ref element)) => {
                let name = element.local_name();
                match (section, name.as_ref()) {
                    (Section::Fonts, b"font") => table.fonts.push(Font::default()),
                    (Section::Fills, b"fill") => table.fills.push(Fill::default()),
                    (Section::Fills, b"patternFill") => {
                        fill.pattern = pattern(&attr(element, b"patternType").unwrap_or_default());
                    }
                    (Section::Borders, b"border") => table.borders.push(Border::default()),
                    (
                        Section::Borders,
                        kind @ (b"left" | b"right" | b"start" | b"end" | b"top" | b"bottom"
                        | b"diagonal"),
                    ) => {
                        start_side(kind, element, &mut border);
                    }
                    (Section::CellXfs, b"xf") => table.xfs.push(parse_xf(element)),
                    (_, b"numFmt") => {
                        if let (Some(id), Some(code)) =
                            (attr_u32(element, b"numFmtId"), attr(element, b"formatCode"))
                        {
                            table.num_fmts.insert(id, code);
                        }
                    }
                    (_, b"rgbColor") => {
                        if let Some(rgb) = attr(element, b"rgb").and_then(|text| parse_rgb(&text)) {
                            indexed.push(rgb);
                        }
                    }
                    _ => {
                        apply_child(
                            section,
                            element,
                            &mut font,
                            &mut fill,
                            in_gradient,
                            &mut gradient_taken,
                            &mut border,
                            side,
                            &mut table,
                        );
                    }
                }
            }
            Ok(Event::End(ref element)) => match element.local_name().as_ref() {
                b"fonts" | b"fills" | b"borders" | b"cellXfs" | b"dxfs" | b"cellStyleXfs"
                | b"cellStyles" | b"extLst" => section = Section::None,
                b"font" if section == Section::Fonts => {
                    table.fonts.push(std::mem::take(&mut font));
                }
                b"fill" if section == Section::Fills => table.fills.push(fill),
                b"border" if section == Section::Borders => table.borders.push(border),
                _ => {}
            },
            _ => {}
        }
        buf.clear();
    }
    if table.fonts.is_empty() {
        table.fonts.push(Font::default());
    }
    if table.fills.is_empty() {
        table.fills.push(Fill::default());
    }
    if table.borders.is_empty() {
        table.borders.push(Border::default());
    }
    if table.xfs.is_empty() {
        table.xfs.push(Xf::default());
    }
    if !indexed.is_empty() {
        table.colors.indexed = indexed;
    }
    table
}

fn start_side(kind: &[u8], element: &BytesStart<'_>, border: &mut Border) -> BorderSide {
    let side = match kind {
        b"left" | b"start" => BorderSide::Left,
        b"right" | b"end" => BorderSide::Right,
        b"top" => BorderSide::Top,
        b"bottom" => BorderSide::Bottom,
        b"diagonal" => BorderSide::Diagonal,
        _ => return BorderSide::None,
    };
    let line = Line {
        style: line_style(&attr(element, b"style").unwrap_or_default()),
        color: Color::AUTO,
    };
    match side {
        BorderSide::Left => border.left = line,
        BorderSide::Right => border.right = line,
        BorderSide::Top => border.top = line,
        BorderSide::Bottom => border.bottom = line,
        BorderSide::Diagonal => border.diagonal = line,
        BorderSide::None => {}
    }
    side
}

#[allow(clippy::too_many_arguments)]
fn apply_child(
    section: Section,
    element: &BytesStart<'_>,
    font: &mut Font,
    fill: &mut Fill,
    in_gradient: bool,
    gradient_taken: &mut bool,
    border: &mut Border,
    side: BorderSide,
    table: &mut StyleTable,
) {
    let name = element.local_name();
    match (section, name.as_ref()) {
        (Section::Fonts, b"name") => font.name = attr(element, b"val").unwrap_or_default(),
        (Section::Fonts, b"sz") => {
            font.size = attr_f64(element, b"val").unwrap_or(11.0).clamp(1.0, 409.0) as f32;
        }
        (Section::Fonts, b"b") => font.bold = val_flag(element),
        (Section::Fonts, b"i") => font.italic = val_flag(element),
        (Section::Fonts, b"strike") => font.strike = val_flag(element),
        (Section::Fonts, b"u") => {
            font.underline = match attr(element, b"val").as_deref() {
                Some("none") => Underline::None,
                Some("double") => Underline::Double,
                Some("singleAccounting") => Underline::SingleAccounting,
                Some("doubleAccounting") => Underline::DoubleAccounting,
                _ => Underline::Single,
            };
        }
        (Section::Fonts, b"color") => font.color = parse_color(element),
        (Section::Fills, b"fgColor") => fill.fg = parse_color(element),
        (Section::Fills, b"bgColor") => fill.bg = parse_color(element),
        // A gradient draws as its first colour until the grid can draw gradients.
        (Section::Fills, b"color") if in_gradient && !*gradient_taken => {
            fill.pattern = Pattern::Solid;
            fill.fg = parse_color(element);
            *gradient_taken = true;
        }
        (Section::Borders, b"color") => {
            let color = parse_color(element);
            match side {
                BorderSide::Left => border.left.color = color,
                BorderSide::Right => border.right.color = color,
                BorderSide::Top => border.top.color = color,
                BorderSide::Bottom => border.bottom.color = color,
                BorderSide::Diagonal => border.diagonal.color = color,
                BorderSide::None => {}
            }
        }
        (Section::CellXfs, b"alignment") => {
            if let Some(xf) = table.xfs.last_mut() {
                xf.alignment = parse_alignment(element);
            }
        }
        _ => {}
    }
}

/// Reads a theme part's colour scheme, in the order `theme="n"` counts them.
pub fn read_theme(xml: &[u8]) -> ThemePalette {
    const ORDER: [&[u8]; 12] = [
        b"lt1",
        b"dk1",
        b"lt2",
        b"dk2",
        b"accent1",
        b"accent2",
        b"accent3",
        b"accent4",
        b"accent5",
        b"accent6",
        b"hlink",
        b"folHlink",
    ];
    let mut palette = ThemePalette::default();
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut in_scheme = false;
    let mut slot = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(ref element)) => {
                let name = element.local_name();
                if name.as_ref() == b"clrScheme" {
                    in_scheme = true;
                } else if in_scheme {
                    slot = ORDER.iter().position(|&entry| entry == name.as_ref());
                }
            }
            Ok(Event::Empty(ref element)) if in_scheme => {
                if let Some(at) = slot {
                    let rgb = match element.local_name().as_ref() {
                        b"srgbClr" => attr(element, b"val").and_then(|text| parse_rgb(&text)),
                        b"sysClr" => attr(element, b"lastClr").and_then(|text| parse_rgb(&text)),
                        _ => None,
                    };
                    if let Some(rgb) = rgb {
                        palette.0[at] = rgb;
                    }
                }
            }
            Ok(Event::End(ref element)) if element.local_name().as_ref() == b"clrScheme" => break,
            _ => {}
        }
        buf.clear();
    }
    palette
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: &str = r##"<styleSheet xmlns="x">
      <numFmts count="1"><numFmt numFmtId="164" formatCode="0.0&quot; kg&quot;"/></numFmts>
      <fonts count="2">
        <font><sz val="11"/><color theme="1"/><name val="Calibri"/></font>
        <font><b/><i val="0"/><u/><sz val="14"/><color rgb="FFFF0000"/><name val="Arial"/></font>
      </fonts>
      <fills count="3">
        <fill><patternFill patternType="none"/></fill>
        <fill><patternFill patternType="gray125"/></fill>
        <fill><patternFill patternType="solid"><fgColor theme="4" tint="0.5"/><bgColor indexed="64"/></patternFill></fill>
      </fills>
      <borders count="2">
        <border><left/><right/><top/><bottom/><diagonal/></border>
        <border diagonalUp="1"><left style="thin"><color auto="1"/></left><right style="thick"><color rgb="FF00FF00"/></right><top/><bottom style="double"><color indexed="8"/></bottom><diagonal/></border>
      </borders>
      <cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
      <cellXfs count="2">
        <xf numFmtId="0" fontId="0" fillId="0" borderId="0"/>
        <xf numFmtId="164" fontId="1" fillId="2" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="top" wrapText="1" indent="2" textRotation="45" readingOrder="2"/></xf>
      </cellXfs>
      <dxfs count="1"><dxf><font><b/></font><fill><patternFill><bgColor rgb="FF000000"/></patternFill></fill></dxf></dxfs>
      <colors><indexedColors><rgbColor rgb="FF112233"/></indexedColors></colors>
    </styleSheet>"##;

    #[test]
    fn reads_every_table_and_ignores_dxfs() {
        let table = read_styles(STYLES.as_bytes());
        assert_eq!(table.fonts.len(), 2);
        assert_eq!(table.fills.len(), 3);
        assert_eq!(table.borders.len(), 2);
        assert_eq!(table.xfs.len(), 2);
        assert_eq!(table.num_fmts[&164], "0.0\" kg\"");
        assert_eq!(table.colors.indexed, vec![0x112233]);

        let font = &table.fonts[1];
        assert!(font.bold && !font.italic);
        assert_eq!(font.underline, Underline::Single);
        assert_eq!(font.name, "Arial");
        assert_eq!(font.size, 14.0);
        assert_eq!(font.color, Color::rgb(0xFF0000));

        assert_eq!(table.fills[1].pattern, Pattern::Mixed(12));
        assert_eq!(table.fills[2].pattern, Pattern::Solid);
        assert_eq!(table.fills[2].fg.kind, ColorKind::Theme(4));
        assert_eq!(table.fills[2].fg.tint, 0.5);

        let border = &table.borders[1];
        assert!(border.diagonal_up);
        assert_eq!(border.left.style, LineStyle::Thin);
        assert_eq!(border.right.style, LineStyle::Thick);
        assert_eq!(border.right.color, Color::rgb(0x00FF00));
        assert_eq!(border.bottom.color.kind, ColorKind::Indexed(8));

        let xf = table.xfs[1];
        assert_eq!((xf.font, xf.fill, xf.border, xf.num_fmt), (1, 2, 1, 164));
        assert_eq!(xf.alignment.horizontal, HAlign::Center);
        assert_eq!(xf.alignment.vertical, VAlign::Top);
        assert!(xf.alignment.wrap);
        assert_eq!(xf.alignment.indent, 2);
        assert_eq!(xf.alignment.rotation, 45);
        assert_eq!(xf.alignment.reading_order, ReadingOrder::RightToLeft);
    }

    #[test]
    fn nothing_readable_leaves_the_defaults() {
        let table = read_styles(b"not xml");
        assert_eq!(table.xfs, vec![Xf::default()]);
        assert_eq!(table.fonts.len(), 1);
    }

    #[test]
    fn reads_the_theme_in_theme_index_order() {
        let palette = read_theme(
            br#"<a:theme xmlns:a="x"><a:themeElements><a:clrScheme name="Office">
              <a:dk1><a:sysClr val="windowText" lastClr="000001"/></a:dk1>
              <a:lt1><a:sysClr val="window" lastClr="FFFFFE"/></a:lt1>
              <a:dk2><a:srgbClr val="44546A"/></a:dk2>
              <a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>
              <a:accent1><a:srgbClr val="123456"/></a:accent1>
            </a:clrScheme></a:themeElements></a:theme>"#,
        );
        assert_eq!(palette.0[0], 0xFFFFFE);
        assert_eq!(palette.0[1], 0x000001);
        assert_eq!(palette.0[4], 0x123456);
    }
}
