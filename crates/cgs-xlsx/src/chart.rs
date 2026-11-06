//! Reads a chart part into `cgs_model::chart`. The part's bytes stay as they are; `chart_write`
//! brings them in line with an edited model.

use cgs_model::chart::*;

use crate::structure::{parse_tree, Element, Node};

/// The first element below `nodes` that isn't markup-compatibility wrapping, so `c:chartSpace`.
fn root(nodes: &[Node]) -> Option<&Element> {
    nodes.iter().find_map(|node| match node {
        Node::Element(element) => Some(element),
        Node::Text(_) => None,
    })
}

pub(crate) fn child<'a>(element: &'a Element, name: &[u8]) -> Option<&'a Element> {
    element.elements().find(|child| child.local() == name)
}

/// A child, looking inside `mc:AlternateContent`'s choice too, as Excel puts `c14:style` there.
fn child_any<'a>(element: &'a Element, name: &[u8]) -> Option<&'a Element> {
    child(element, name).or_else(|| {
        element
            .elements()
            .filter(|child| child.local() == b"AlternateContent")
            .flat_map(|alternate| alternate.elements())
            .find_map(|branch| child(branch, name))
    })
}

pub(crate) fn val(element: &Element, name: &[u8]) -> Option<String> {
    child(element, name).and_then(|child| child.attr_text(b"val"))
}

/// A `CT_Boolean`: a missing `val` means true.
pub(crate) fn flag(element: &Element, name: &[u8]) -> Option<bool> {
    child(element, name).map(|child| {
        child
            .attr_text(b"val")
            .is_none_or(|text| text == "1" || text == "true")
    })
}

fn number<T: std::str::FromStr>(element: &Element, name: &[u8]) -> Option<T> {
    val(element, name).and_then(|text| text.trim().parse().ok())
}

fn hex(text: &str) -> Option<u32> {
    u32::from_str_radix(text.trim(), 16).ok()
}

fn preset_rgb(name: &str) -> u32 {
    match name {
        "white" => 0xFFFFFF,
        "red" => 0xFF0000,
        "green" => 0x008000,
        "blue" => 0x0000FF,
        "yellow" => 0xFFFF00,
        "gray" | "grey" => 0x808080,
        "lightGray" | "ltGray" => 0xD3D3D3,
        "darkGray" | "dkGray" => 0xA9A9A9,
        "orange" => 0xFFA500,
        "purple" => 0x800080,
        _ => 0x000000,
    }
}

/// A colour element: `a:srgbClr`, `a:schemeClr`, `a:sysClr`, `a:prstClr`, `a:scrgbClr`, `a:hslClr`.
pub(crate) fn color_of(element: &Element) -> Option<DmlColor> {
    let base = match element.local() {
        b"srgbClr" => DmlBase::Rgb(hex(&element.attr_text(b"val")?)?),
        b"schemeClr" => DmlBase::Scheme(element.attr_text(b"val")?),
        b"sysClr" => DmlBase::System(
            element
                .attr_text(b"lastClr")
                .and_then(|text| hex(&text))
                .unwrap_or(if element.attr_text(b"val").as_deref() == Some("window") {
                    0xFFFFFF
                } else {
                    0x000000
                }),
        ),
        b"prstClr" => DmlBase::Preset(preset_rgb(&element.attr_text(b"val")?)),
        b"scrgbClr" => {
            let part = |name: &[u8]| {
                let value: f64 = element
                    .attr_text(name)
                    .and_then(|text| text.parse().ok())
                    .unwrap_or(0.0);
                ((value / 100_000.0).clamp(0.0, 1.0) * 255.0).round() as u32
            };
            DmlBase::Rgb(part(b"r") << 16 | part(b"g") << 8 | part(b"b"))
        }
        _ => return None,
    };
    let mods = element
        .elements()
        .filter_map(|change| {
            let kind = match change.local() {
                b"lumMod" => ColorModKind::LumMod,
                b"lumOff" => ColorModKind::LumOff,
                b"tint" => ColorModKind::Tint,
                b"shade" => ColorModKind::Shade,
                b"alpha" => ColorModKind::Alpha,
                b"satMod" => ColorModKind::SatMod,
                _ => return None,
            };
            let value = change.attr_text(b"val")?.trim().parse().ok()?;
            Some(ColorMod { kind, value })
        })
        .collect();
    Some(DmlColor { base, mods })
}

fn first_color(element: &Element) -> Option<DmlColor> {
    element.elements().find_map(color_of)
}

/// A fill among an element's children, if it has one.
pub(crate) fn fill_of(element: &Element) -> Option<ChartFill> {
    element.elements().find_map(|child| match child.local() {
        b"noFill" => Some(ChartFill::None),
        b"solidFill" => Some(first_color(child).map_or(ChartFill::Other(None), ChartFill::Solid)),
        b"gradFill" => Some(
            child
                .elements()
                .find(|list| list.local() == b"gsLst")
                .and_then(|list| list.elements().next())
                .and_then(first_color)
                .map_or(ChartFill::Other(None), ChartFill::Gradient),
        ),
        b"pattFill" => Some(ChartFill::Other(
            child
                .elements()
                .find(|c| c.local() == b"fgClr")
                .and_then(first_color),
        )),
        b"blipFill" | b"grpFill" => Some(ChartFill::Other(None)),
        _ => None,
    })
}

pub(crate) fn line_of(element: &Element) -> ChartLine {
    ChartLine {
        fill: fill_of(element),
        width: element.attr_text(b"w").and_then(|text| text.parse().ok()),
        dash: child(element, b"prstDash")
            .and_then(|dash| dash.attr_text(b"val"))
            .map(|name| Dash::from_name(&name)),
    }
}

/// `c:spPr`.
pub(crate) fn shape_of(parent: &Element) -> ShapeLook {
    let Some(shape) = child(parent, b"spPr") else {
        return ShapeLook::default();
    };
    ShapeLook {
        fill: fill_of(shape),
        line: child(shape, b"ln").map(line_of),
    }
}

fn run_props(props: &Element, look: &mut TextLook) {
    if look.size.is_none() {
        look.size = props.attr_text(b"sz").and_then(|text| text.parse().ok());
    }
    if look.bold.is_none() {
        look.bold = props
            .attr_text(b"b")
            .map(|text| text == "1" || text == "true");
    }
    if look.italic.is_none() {
        look.italic = props
            .attr_text(b"i")
            .map(|text| text == "1" || text == "true");
    }
    if look.color.is_none() {
        look.color = match fill_of(props) {
            Some(ChartFill::Solid(color)) => Some(color),
            _ => None,
        };
    }
    if look.font.is_none() {
        look.font = child(props, b"latin").and_then(|latin| latin.attr_text(b"typeface"));
    }
}

/// The look of `c:txPr` or `c:rich`: body rotation, then the first paragraph's default run
/// properties, then its first run's.
fn text_body_look(body: &Element) -> TextLook {
    let mut look = TextLook {
        rotation: child(body, b"bodyPr")
            .and_then(|props| props.attr_text(b"rot"))
            .and_then(|text| text.parse().ok()),
        ..TextLook::default()
    };
    if let Some(paragraph) = body.elements().find(|child| child.local() == b"p") {
        if let Some(props) = child(paragraph, b"pPr").and_then(|p| child(p, b"defRPr")) {
            run_props(props, &mut look);
        }
        for run in paragraph.elements().filter(|child| child.local() == b"r") {
            if let Some(props) = child(run, b"rPr") {
                run_props(props, &mut look);
            }
        }
        if let Some(props) = child(paragraph, b"endParaRPr") {
            run_props(props, &mut look);
        }
    }
    look
}

pub(crate) fn text_look_of(parent: &Element) -> TextLook {
    child(parent, b"txPr")
        .map(text_body_look)
        .unwrap_or_default()
}

/// The text of a `c:rich`: each paragraph's runs, paragraphs joined by line breaks.
fn rich_text(rich: &Element) -> String {
    let mut out = String::new();
    let mut first = true;
    for paragraph in rich.elements().filter(|child| child.local() == b"p") {
        if !first {
            out.push('\n');
        }
        first = false;
        for run in paragraph.elements() {
            match run.local() {
                b"r" | b"fld" => {
                    if let Some(text) = child(run, b"t") {
                        out.push_str(&text.text());
                    }
                }
                b"br" => out.push('\n'),
                _ => {}
            }
        }
    }
    out
}

/// `c:tx`: typed text, or a cell's text, with its look when typed.
fn chart_text(tx: &Element) -> (Option<ChartText>, TextLook) {
    if let Some(rich) = child(tx, b"rich") {
        return (Some(ChartText::Rich(rich_text(rich))), text_body_look(rich));
    }
    if let Some(reference) = child(tx, b"strRef") {
        let formula = child(reference, b"f")
            .map(Element::text)
            .unwrap_or_default();
        let cache = child(reference, b"strCache")
            .and_then(|cache| cache.elements().find(|c| c.local() == b"pt"))
            .and_then(|point| child(point, b"v"))
            .map(Element::text)
            .unwrap_or_default();
        return (
            Some(ChartText::Cell {
                formula: formula.trim().trim_start_matches('=').to_string(),
                cache,
            }),
            TextLook::default(),
        );
    }
    if let Some(value) = child(tx, b"v") {
        return (Some(ChartText::Rich(value.text())), TextLook::default());
    }
    (None, TextLook::default())
}

fn title_of(element: &Element) -> ChartTitle {
    let (text, mut look) = child(element, b"tx").map(chart_text).unwrap_or_default();
    let own = text_look_of(element);
    // A rich title's runs say its look; `txPr` says an automatic one's.
    if look.is_empty() {
        look = own;
    }
    ChartTitle {
        text,
        overlay: flag(element, b"overlay").unwrap_or(false),
        look,
        shape: shape_of(element),
    }
}

fn cache_points(cache: &Element, out: &mut DataSource) {
    out.count = number(cache, b"ptCount").unwrap_or(0);
    if out.format.is_none() {
        out.format = child(cache, b"formatCode").map(Element::text);
    }
    for point in cache.elements().filter(|child| child.local() == b"pt") {
        let index = point
            .attr_text(b"idx")
            .and_then(|text| text.parse().ok())
            .unwrap_or(out.cache.len() as u32);
        let value = child(point, b"v").map(Element::text).unwrap_or_default();
        out.cache.push(CachePoint { index, value });
    }
}

/// `c:cat`, `c:val`, `c:xVal` or `c:yVal`.
fn source_of(element: &Element) -> DataSource {
    let mut out = DataSource::default();
    for inner in element.elements() {
        let (kind, cache_name): (SourceKind, &[u8]) = match inner.local() {
            b"numRef" => (SourceKind::Number, b"numCache"),
            b"strRef" => (SourceKind::Text, b"strCache"),
            b"multiLvlStrRef" => (SourceKind::MultiLevel, b"multiLvlStrCache"),
            b"numLit" | b"strLit" => {
                out.kind = if inner.local() == b"numLit" {
                    SourceKind::Number
                } else {
                    SourceKind::Text
                };
                cache_points(inner, &mut out);
                return out;
            }
            _ => continue,
        };
        out.kind = kind;
        out.formula =
            child(inner, b"f").map(|f| f.text().trim().trim_start_matches('=').to_string());
        if let Some(cache) = child(inner, cache_name) {
            if kind == SourceKind::MultiLevel {
                // The innermost level is what each point is labelled with.
                out.count = number(cache, b"ptCount").unwrap_or(0);
                if let Some(level) = cache.elements().find(|c| c.local() == b"lvl") {
                    let mut level_source = DataSource::default();
                    cache_points(level, &mut level_source);
                    out.cache = level_source.cache;
                }
            } else {
                cache_points(cache, &mut out);
            }
        }
        return out;
    }
    out
}

fn marker_of(element: &Element) -> Marker {
    Marker {
        symbol: val(element, b"symbol")
            .map_or(MarkerSymbol::Auto, |code| MarkerSymbol::from_code(&code)),
        size: number(element, b"size"),
        shape: shape_of(element),
    }
}

pub(crate) fn labels_of(element: &Element) -> DataLabels {
    let on = |name: &[u8]| flag(element, name).unwrap_or(false);
    DataLabels {
        value: on(b"showVal"),
        category: on(b"showCatName"),
        series: on(b"showSerName"),
        percent: on(b"showPercent"),
        legend_key: on(b"showLegendKey"),
        position: val(element, b"dLblPos")
            .map_or(LabelPosition::Auto, |code| LabelPosition::from_code(&code)),
        separator: child(element, b"separator").map(Element::text),
        format: child(element, b"numFmt").and_then(|format| format.attr_text(b"formatCode")),
        look: text_look_of(element),
        deleted: flag(element, b"delete").unwrap_or(false),
    }
}

fn series_of(element: &Element) -> Series {
    let mut series = Series {
        index: number(element, b"idx").unwrap_or(0),
        order: number(element, b"order").unwrap_or(0),
        name: child(element, b"tx").and_then(|tx| chart_text(tx).0),
        shape: shape_of(element),
        marker: child(element, b"marker").map(marker_of),
        labels: child(element, b"dLbls").map(labels_of),
        smooth: flag(element, b"smooth"),
        invert_if_negative: flag(element, b"invertIfNegative").unwrap_or(false),
        explosion: number(element, b"explosion"),
        ..Series::default()
    };
    for inner in element.elements() {
        match inner.local() {
            b"cat" | b"xVal" => series.categories = Some(source_of(inner)),
            b"val" | b"yVal" => series.values = Some(source_of(inner)),
            b"dPt" => series.points.push(PointLook {
                index: number(inner, b"idx").unwrap_or(0),
                shape: shape_of(inner),
                marker: child(inner, b"marker").map(marker_of),
                explosion: number(inner, b"explosion"),
            }),
            _ => {}
        }
    }
    series
}

fn group_kind(element: &Element) -> Option<GroupKind> {
    let grouping =
        || val(element, b"grouping").map_or(Grouping::Standard, |code| Grouping::from_code(&code));
    let first_slice = || number(element, b"firstSliceAng").unwrap_or(0);
    Some(match element.local() {
        b"barChart" | b"bar3DChart" => GroupKind::Bar {
            horizontal: val(element, b"barDir").as_deref() == Some("bar"),
            grouping: val(element, b"grouping")
                .map_or(Grouping::Clustered, |code| Grouping::from_code(&code)),
            gap_width: number(element, b"gapWidth").unwrap_or(150),
            overlap: number(element, b"overlap").unwrap_or(0),
            three_d: element.local() == b"bar3DChart",
        },
        b"lineChart" | b"line3DChart" => GroupKind::Line {
            grouping: grouping(),
            markers: flag(element, b"marker").unwrap_or(false),
            three_d: element.local() == b"line3DChart",
        },
        b"areaChart" | b"area3DChart" => GroupKind::Area {
            grouping: grouping(),
            three_d: element.local() == b"area3DChart",
        },
        b"pieChart" | b"pie3DChart" => GroupKind::Pie {
            first_slice: first_slice(),
            three_d: element.local() == b"pie3DChart",
        },
        b"doughnutChart" => GroupKind::Doughnut {
            first_slice: first_slice(),
            hole: number(element, b"holeSize").unwrap_or(10),
        },
        b"scatterChart" => GroupKind::Scatter {
            style: val(element, b"scatterStyle")
                .map_or(ScatterStyle::Marker, |code| ScatterStyle::from_code(&code)),
        },
        b"radarChart" => GroupKind::Radar,
        b"bubbleChart" => GroupKind::Bubble,
        b"stockChart" => GroupKind::Stock,
        b"surfaceChart" | b"surface3DChart" => GroupKind::Surface,
        b"ofPieChart" => GroupKind::OfPie,
        _ => return None,
    })
}

pub(crate) fn group_of(element: &Element) -> Option<ChartGroup> {
    let kind = group_kind(element)?;
    Some(ChartGroup {
        vary_colors: flag(element, b"varyColors").unwrap_or(false),
        series: element
            .elements()
            .filter(|child| child.local() == b"ser")
            .map(series_of)
            .collect(),
        axes: element
            .elements()
            .filter(|child| child.local() == b"axId")
            .filter_map(|axis| axis.attr_text(b"val")?.trim().parse().ok())
            .collect(),
        labels: child(element, b"dLbls").map(labels_of),
        kind,
    })
}

fn gridlines(element: &Element, name: &[u8]) -> Option<Gridlines> {
    child(element, name).map(|lines| Gridlines {
        shape: shape_of(lines),
    })
}

pub(crate) fn axis_of(element: &Element) -> Option<ChartAxis> {
    let kind = match element.local() {
        b"catAx" => AxisKind::Category,
        b"valAx" => AxisKind::Value,
        b"dateAx" => AxisKind::Date,
        b"serAx" => AxisKind::Series,
        _ => return None,
    };
    let scaling = child(element, b"scaling");
    let scale = |name: &[u8]| scaling.and_then(|scaling| number::<f64>(scaling, name));
    let crosses = if let Some(at) = number::<f64>(element, b"crossesAt") {
        Crosses::At(at)
    } else {
        match val(element, b"crosses").as_deref() {
            Some("min") => Crosses::Min,
            Some("max") => Crosses::Max,
            _ => Crosses::AutoZero,
        }
    };
    Some(ChartAxis {
        id: number(element, b"axId").unwrap_or(0),
        kind,
        position: val(element, b"axPos")
            .map_or(AxisPosition::Bottom, |code| AxisPosition::from_code(&code)),
        deleted: flag(element, b"delete").unwrap_or(false),
        cross_axis: number(element, b"crossAx").unwrap_or(0),
        crosses,
        min: scale(b"min"),
        max: scale(b"max"),
        major_unit: number(element, b"majorUnit"),
        minor_unit: number(element, b"minorUnit"),
        log_base: scale(b"logBase"),
        reversed: scaling
            .and_then(|scaling| val(scaling, b"orientation"))
            .as_deref()
            == Some("maxMin"),
        format: child(element, b"numFmt").and_then(|format| {
            let code = format.attr_text(b"formatCode")?;
            let linked = format
                .attr_text(b"sourceLinked")
                .is_some_and(|text| text == "1" || text == "true");
            Some((code, linked))
        }),
        major_gridlines: gridlines(element, b"majorGridlines"),
        minor_gridlines: gridlines(element, b"minorGridlines"),
        title: child(element, b"title").map(title_of),
        major_tick: val(element, b"majorTickMark")
            .map_or(TickMark::Cross, |code| TickMark::from_code(&code)),
        minor_tick: val(element, b"minorTickMark")
            .map_or(TickMark::Cross, |code| TickMark::from_code(&code)),
        labels: val(element, b"tickLblPos")
            .map_or(LabelPlace::NextTo, |code| LabelPlace::from_code(&code)),
        between: val(element, b"crossBetween").map(|code| code == "between"),
        label_skip: number(element, b"tickLblSkip"),
        mark_skip: number(element, b"tickMarkSkip"),
        shape: shape_of(element),
        look: text_look_of(element),
    })
}

fn legend_of(element: &Element) -> Legend {
    Legend {
        position: val(element, b"legendPos").map_or(LegendPosition::Right, |code| {
            LegendPosition::from_code(&code)
        }),
        overlay: flag(element, b"overlay").unwrap_or(false),
        hidden: element
            .elements()
            .filter(|child| child.local() == b"legendEntry")
            .filter(|entry| flag(entry, b"delete").unwrap_or(false))
            .filter_map(|entry| number(entry, b"idx"))
            .collect(),
        look: text_look_of(element),
        shape: shape_of(element),
    }
}

fn manual_layout(plot: &Element) -> Option<[f64; 4]> {
    let layout = child(child(plot, b"layout")?, b"manualLayout")?;
    let at = |name: &[u8]| number::<f64>(layout, name);
    Some([at(b"x")?, at(b"y")?, at(b"w")?, at(b"h")?])
}

/// The chart a `c:chartSpace` describes.
pub fn read_chart_tree(space: &Element) -> Chart {
    let mut out = Chart {
        // `c14:style` is the 2007 style plus 100.
        style: child_any(space, b"style")
            .and_then(|style| style.attr_text(b"val")?.trim().parse::<u16>().ok())
            .map_or(2, |value| {
                (if value > 100 { value - 100 } else { value }).clamp(1, 48) as u8
            }),
        rounded_corners: flag(space, b"roundedCorners").unwrap_or(true),
        shape: shape_of(space),
        look: text_look_of(space),
        pivot: child(space, b"pivotSource").is_some(),
        ..Chart::default()
    };
    let Some(chart) = child(space, b"chart") else {
        return out;
    };
    out.title = child(chart, b"title").map(title_of);
    out.auto_title_deleted = flag(chart, b"autoTitleDeleted").unwrap_or(false);
    out.legend = child(chart, b"legend").map(legend_of);
    out.plot_visible_only = flag(chart, b"plotVisOnly").unwrap_or(true);
    out.blanks = val(chart, b"dispBlanksAs").map_or(Blanks::Zero, |code| Blanks::from_code(&code));
    out.three_d = child(chart, b"view3D").is_some();
    if let Some(plot) = child(chart, b"plotArea") {
        out.plot_shape = shape_of(plot);
        out.plot_layout = manual_layout(plot);
        for element in plot.elements() {
            if let Some(group) = group_of(element) {
                out.groups.push(group);
            } else if let Some(axis) = axis_of(element) {
                out.axes.push(axis);
            }
        }
    }
    out
}

/// What a chart part holds. `extended` for a `chartEx` part.
pub fn read_chart(xml: &[u8], extended: bool) -> ChartBody {
    let Ok(nodes) = parse_tree(xml) else {
        return ChartBody::Unreadable;
    };
    let Some(space) = root(&nodes) else {
        return ChartBody::Unreadable;
    };
    if extended {
        return ChartBody::Extended(extended_kind(space).unwrap_or_else(|| "chart".to_string()));
    }
    if space.local() != b"chartSpace" {
        return ChartBody::Unreadable;
    }
    ChartBody::Classic(Box::new(read_chart_tree(space)))
}

/// A `cx:chartSpace`'s first series' `layoutId`, such as `waterfall` or `sunburst`.
fn extended_kind(space: &Element) -> Option<String> {
    fn find(element: &Element) -> Option<String> {
        if element.local() == b"series" {
            if let Some(id) = element.attr_text(b"layoutId") {
                return Some(id);
            }
        }
        element.elements().find_map(find)
    }
    find(space)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLUMN: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><c:date1904 val="0"/><c:lang val="en-US"/><c:roundedCorners val="0"/><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="c14" xmlns:c14="http://schemas.microsoft.com/office/drawing/2007/8/2/chart"><c14:style val="102"/></mc:Choice><mc:Fallback><c:style val="2"/></mc:Fallback></mc:AlternateContent><c:chart><c:title><c:tx><c:rich><a:bodyPr/><a:p><a:pPr><a:defRPr sz="1400" b="0"><a:solidFill><a:schemeClr val="tx1"><a:lumMod val="65000"/><a:lumOff val="35000"/></a:schemeClr></a:solidFill></a:defRPr></a:pPr><a:r><a:rPr lang="en-US"/><a:t>Sales</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title><c:autoTitleDeleted val="0"/><c:plotArea><c:layout/><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:varyColors val="0"/><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:f>Sheet1!$B$1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>2024</c:v></c:pt></c:strCache></c:strRef></c:tx><c:spPr><a:solidFill><a:schemeClr val="accent1"/></a:solidFill><a:ln><a:noFill/></a:ln></c:spPr><c:invertIfNegative val="0"/><c:cat><c:strRef><c:f>Sheet1!$A$2:$A$4</c:f><c:strCache><c:ptCount val="3"/><c:pt idx="0"><c:v>North</c:v></c:pt><c:pt idx="1"><c:v>South</c:v></c:pt><c:pt idx="2"><c:v>East</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>Sheet1!$B$2:$B$4</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="3"/><c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="2"><c:v>30</c:v></c:pt></c:numCache></c:numRef></c:val><c:extLst><c:ext uri="{C3380CC4-5D6E-409C-BE32-E72D297353CC}" xmlns:c16="http://schemas.microsoft.com/office/drawing/2014/chart"><c16:uniqueId val="{00000000-0001-0000-0000-000000000000}"/></c:ext></c:extLst></c:ser><c:dLbls><c:showLegendKey val="0"/><c:showVal val="0"/><c:showCatName val="0"/><c:showSerName val="0"/><c:showPercent val="0"/><c:showBubbleSize val="0"/></c:dLbls><c:gapWidth val="219"/><c:overlap val="-27"/><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="b"/><c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="none"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crossAx val="20"/><c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/><c:noMultiLvlLbl val="0"/></c:catAx><c:valAx><c:axId val="20"/><c:scaling><c:orientation val="minMax"/><c:max val="40"/></c:scaling><c:delete val="0"/><c:axPos val="l"/><c:majorGridlines><c:spPr><a:ln w="9525"><a:solidFill><a:srgbClr val="D9D9D9"/></a:solidFill></a:ln></c:spPr></c:majorGridlines><c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="none"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crossAx val="10"/><c:crosses val="autoZero"/><c:crossBetween val="between"/><c:majorUnit val="10"/></c:valAx></c:plotArea><c:legend><c:legendPos val="b"/><c:overlay val="0"/></c:legend><c:plotVisOnly val="1"/><c:dispBlanksAs val="gap"/></c:chart></c:chartSpace>"#;

    #[test]
    fn reads_a_column_chart_as_excel_writes_it() {
        let ChartBody::Classic(chart) = read_chart(COLUMN.as_bytes(), false) else {
            panic!("not read");
        };
        assert_eq!(chart.style, 2);
        assert!(!chart.rounded_corners);
        let title = chart.title.as_ref().unwrap();
        assert_eq!(title.text, Some(ChartText::Rich("Sales".into())));
        assert_eq!(title.look.size, Some(1400));
        assert_eq!(
            title.look.color,
            Some(
                DmlColor::scheme("tx1")
                    .with(ColorModKind::LumMod, 65000)
                    .with(ColorModKind::LumOff, 35000)
            )
        );
        assert_eq!(chart.groups.len(), 1);
        let group = &chart.groups[0];
        assert_eq!(
            group.kind,
            GroupKind::Bar {
                horizontal: false,
                grouping: Grouping::Clustered,
                gap_width: 219,
                overlap: -27,
                three_d: false
            }
        );
        assert_eq!(group.axes, vec![10, 20]);
        let series = &group.series[0];
        assert_eq!(
            series.name,
            Some(ChartText::Cell {
                formula: "Sheet1!$B$1".into(),
                cache: "2024".into()
            })
        );
        let values = series.values.as_ref().unwrap();
        assert_eq!(values.formula.as_deref(), Some("Sheet1!$B$2:$B$4"));
        assert_eq!(values.count, 3);
        assert_eq!(values.cache.len(), 2);
        assert_eq!(values.cache[1].index, 2);
        assert_eq!(series.categories.as_ref().unwrap().kind, SourceKind::Text);
        assert_eq!(
            series.shape.fill,
            Some(ChartFill::Solid(DmlColor::scheme("accent1")))
        );
        assert_eq!(
            series.shape.line.as_ref().unwrap().fill,
            Some(ChartFill::None)
        );
        assert_eq!(chart.axes.len(), 2);
        let value_axis = chart.axis(20).unwrap();
        assert_eq!(value_axis.kind, AxisKind::Value);
        assert_eq!(value_axis.max, Some(40.0));
        assert_eq!(value_axis.major_unit, Some(10.0));
        assert!(value_axis.major_gridlines.is_some());
        assert_eq!(value_axis.position, AxisPosition::Left);
        assert_eq!(
            chart.legend.as_ref().unwrap().position,
            LegendPosition::Bottom
        );
        assert_eq!(chart.blanks, Blanks::Gap);
        assert!(chart.editable());
        assert_eq!(
            chart.formulas(),
            vec!["Sheet1!$B$1", "Sheet1!$A$2:$A$4", "Sheet1!$B$2:$B$4"]
        );
    }

    #[test]
    fn reads_an_extended_chart_by_its_layout() {
        let xml = br#"<cx:chartSpace xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex"><cx:chart><cx:plotArea><cx:plotAreaRegion><cx:series layoutId="waterfall" uniqueId="{1}"/></cx:plotAreaRegion></cx:plotArea></cx:chart></cx:chartSpace>"#;
        assert_eq!(
            read_chart(xml, true),
            ChartBody::Extended("waterfall".into())
        );
    }

    #[test]
    fn something_else_is_unreadable() {
        assert_eq!(read_chart(b"<oops", false), ChartBody::Unreadable);
    }
}
