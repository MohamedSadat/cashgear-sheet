//! Writes chart parts. A new chart is written whole, as Excel 2016 writes one. An edited chart
//! keeps its file's bytes: only elements whose meaning changed are written again, each in its
//! place in the schema's order, and everything the model doesn't hold, such as extension lists,
//! trendlines and effects, stays as it was.

use std::io;

use cgs_model::chart::*;

use crate::chart::{axis_of, group_of, read_chart_tree};
use crate::scan::escape_attr;
use crate::structure::{parse_tree, write_tree, Element, Node};

const C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn text(value: &str) -> String {
    let mut out = Vec::new();
    crate::scan::escape_text(value, &mut out);
    String::from_utf8(out).unwrap_or_default()
}

fn number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

// ---------------------------------------------------------------------------------------------
// Pieces, written with `c:` and `a:`. `Prefixes::fix` renames them for a file that uses others.

fn color(color: &DmlColor) -> String {
    let mods: String = color
        .mods
        .iter()
        .map(|change| {
            let name = match change.kind {
                ColorModKind::LumMod => "lumMod",
                ColorModKind::LumOff => "lumOff",
                ColorModKind::Tint => "tint",
                ColorModKind::Shade => "shade",
                ColorModKind::Alpha => "alpha",
                ColorModKind::SatMod => "satMod",
            };
            format!("<a:{name} val=\"{}\"/>", change.value)
        })
        .collect();
    let (tag, attrs) = match &color.base {
        DmlBase::Rgb(value) | DmlBase::Preset(value) | DmlBase::System(value) => {
            ("srgbClr", format!("val=\"{value:06X}\""))
        }
        DmlBase::Scheme(name) => ("schemeClr", format!("val=\"{}\"", escape_attr(name))),
    };
    if mods.is_empty() {
        format!("<a:{tag} {attrs}/>")
    } else {
        format!("<a:{tag} {attrs}>{mods}</a:{tag}>")
    }
}

fn fill(fill: &ChartFill) -> String {
    match fill {
        ChartFill::None => "<a:noFill/>".to_string(),
        ChartFill::Solid(value) | ChartFill::Gradient(value) | ChartFill::Other(Some(value)) => {
            format!("<a:solidFill>{}</a:solidFill>", color(value))
        }
        ChartFill::Other(None) => "<a:noFill/>".to_string(),
    }
}

fn line(line: &ChartLine) -> String {
    let width = line
        .width
        .map(|w| format!(" w=\"{w}\""))
        .unwrap_or_default();
    let body = format!(
        "{}{}",
        line.fill.as_ref().map(fill).unwrap_or_default(),
        line.dash
            .map(|dash| format!("<a:prstDash val=\"{}\"/>", dash.name()))
            .unwrap_or_default()
    );
    if body.is_empty() {
        format!("<a:ln{width}/>")
    } else {
        format!("<a:ln{width}>{body}</a:ln>")
    }
}

fn shape(look: &ShapeLook) -> String {
    if look.is_empty() {
        return String::new();
    }
    format!(
        "<c:spPr>{}{}</c:spPr>",
        look.fill.as_ref().map(fill).unwrap_or_default(),
        look.line.as_ref().map(line).unwrap_or_default()
    )
}

fn run_props(name: &str, look: &TextLook) -> String {
    let mut attrs = String::new();
    if let Some(size) = look.size {
        attrs.push_str(&format!(" sz=\"{size}\""));
    }
    if let Some(bold) = look.bold {
        attrs.push_str(if bold { " b=\"1\"" } else { " b=\"0\"" });
    }
    if let Some(italic) = look.italic {
        attrs.push_str(if italic { " i=\"1\"" } else { " i=\"0\"" });
    }
    let mut body = String::new();
    if let Some(value) = &look.color {
        body.push_str(&format!("<a:solidFill>{}</a:solidFill>", color(value)));
    }
    if let Some(font) = &look.font {
        body.push_str(&format!("<a:latin typeface=\"{}\"/>", escape_attr(font)));
    }
    if body.is_empty() {
        format!("<a:{name}{attrs}/>")
    } else {
        format!("<a:{name}{attrs}>{body}</a:{name}>")
    }
}

fn body_props(look: &TextLook) -> String {
    match look.rotation {
        Some(rotation) => format!("<a:bodyPr rot=\"{rotation}\" vert=\"horz\"/>"),
        None => "<a:bodyPr/>".to_string(),
    }
}

fn text_props(look: &TextLook) -> String {
    if look.is_empty() {
        return String::new();
    }
    format!(
        "<c:txPr>{}<a:lstStyle/><a:p><a:pPr>{}</a:pPr><a:endParaRPr lang=\"en-US\"/></a:p></c:txPr>",
        body_props(look),
        run_props("defRPr", look)
    )
}

fn rich(value: &str, look: &TextLook) -> String {
    let paragraphs: String = value
        .split('\n')
        .map(|line| {
            format!(
                "<a:p><a:pPr>{}</a:pPr><a:r><a:rPr lang=\"en-US\"/><a:t>{}</a:t></a:r></a:p>",
                run_props("defRPr", look),
                text(line)
            )
        })
        .collect();
    format!(
        "<c:rich>{}<a:lstStyle/>{paragraphs}</c:rich>",
        body_props(look)
    )
}

fn string_ref(formula: &str, cache: &[&str]) -> String {
    let points: String = cache
        .iter()
        .enumerate()
        .map(|(at, value)| format!("<c:pt idx=\"{at}\"><c:v>{}</c:v></c:pt>", text(value)))
        .collect();
    format!(
        "<c:strRef><c:f>{}</c:f><c:strCache><c:ptCount val=\"{}\"/>{points}</c:strCache></c:strRef>",
        text(formula),
        cache.len()
    )
}

fn series_text(value: &ChartText) -> String {
    match value {
        ChartText::Rich(value) => format!("<c:tx><c:v>{}</c:v></c:tx>", text(value)),
        ChartText::Cell { formula, cache } => {
            format!("<c:tx>{}</c:tx>", string_ref(formula, &[cache]))
        }
    }
}

fn title(title: &ChartTitle) -> String {
    let tx = match &title.text {
        Some(ChartText::Rich(value)) => format!("<c:tx>{}</c:tx>", rich(value, &title.look)),
        Some(ChartText::Cell { formula, cache }) => {
            format!("<c:tx>{}</c:tx>", string_ref(formula, &[cache]))
        }
        None => String::new(),
    };
    // A typed title's look is in its runs; an automatic or linked one's in `txPr`.
    let props = if matches!(title.text, Some(ChartText::Rich(_))) {
        String::new()
    } else {
        text_props(&title.look)
    };
    format!(
        "<c:title>{tx}<c:overlay val=\"{}\"/>{}{props}</c:title>",
        u8::from(title.overlay),
        shape(&title.shape)
    )
}

fn source(name: &str, source: &DataSource) -> String {
    let points: String = source
        .cache
        .iter()
        .map(|point| {
            format!(
                "<c:pt idx=\"{}\"><c:v>{}</c:v></c:pt>",
                point.index,
                text(&point.value)
            )
        })
        .collect();
    let format = |source: &DataSource| {
        format!(
            "<c:formatCode>{}</c:formatCode>",
            text(source.format.as_deref().unwrap_or("General"))
        )
    };
    let count = format!("<c:ptCount val=\"{}\"/>", source.count);
    let inner = match (&source.formula, source.kind) {
        (Some(formula), SourceKind::Number) => format!(
            "<c:numRef><c:f>{}</c:f><c:numCache>{}{count}{points}</c:numCache></c:numRef>",
            text(formula),
            format(source)
        ),
        (Some(formula), _) => format!(
            "<c:strRef><c:f>{}</c:f><c:strCache>{count}{points}</c:strCache></c:strRef>",
            text(formula)
        ),
        (None, SourceKind::Number) => {
            format!("<c:numLit>{}{count}{points}</c:numLit>", format(source))
        }
        (None, _) => format!("<c:strLit>{count}{points}</c:strLit>"),
    };
    format!("<c:{name}>{inner}</c:{name}>")
}

fn marker(marker: &Marker) -> String {
    format!(
        "<c:marker><c:symbol val=\"{}\"/>{}{}</c:marker>",
        marker.symbol.code(),
        marker
            .size
            .map(|size| format!("<c:size val=\"{size}\"/>"))
            .unwrap_or_default(),
        shape(&marker.shape)
    )
}

fn labels(labels: &DataLabels) -> String {
    if labels.deleted {
        return "<c:dLbls><c:delete val=\"1\"/></c:dLbls>".to_string();
    }
    let flag = |name: &str, on: bool| format!("<c:{name} val=\"{}\"/>", u8::from(on));
    format!(
        "<c:dLbls>{}{}{}{}{}{}{}{}{}{}</c:dLbls>",
        labels
            .format
            .as_ref()
            .map(|code| format!(
                "<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>",
                escape_attr(code)
            ))
            .unwrap_or_default(),
        text_props(&labels.look),
        labels
            .position
            .code()
            .map(|code| format!("<c:dLblPos val=\"{code}\"/>"))
            .unwrap_or_default(),
        flag("showLegendKey", labels.legend_key),
        flag("showVal", labels.value),
        flag("showCatName", labels.category),
        flag("showSerName", labels.series),
        flag("showPercent", labels.percent),
        flag("showBubbleSize", false),
        labels
            .separator
            .as_ref()
            .map(|separator| format!("<c:separator>{}</c:separator>", text(separator)))
            .unwrap_or_default()
    )
}

fn point_look(kind: &GroupKind, point: &PointLook) -> String {
    let bars = matches!(kind, GroupKind::Bar { .. });
    format!(
        "<c:dPt><c:idx val=\"{}\"/>{}{}{}{}</c:dPt>",
        point.index,
        if bars {
            "<c:invertIfNegative val=\"0\"/>"
        } else {
            ""
        },
        point.marker.as_ref().map(marker).unwrap_or_default(),
        if matches!(kind, GroupKind::Pie { .. } | GroupKind::Doughnut { .. }) {
            "<c:bubble3D val=\"0\"/>"
        } else {
            ""
        },
        point
            .explosion
            .map(|explosion| format!("<c:explosion val=\"{explosion}\"/>"))
            .unwrap_or_default()
            + &shape(&point.shape)
    )
}

/// A series' children, by name, as this group kind takes them.
/// A trendline as Excel writes one, in the schema's order.
fn trendline(trend: &Trendline) -> String {
    let mut out = String::from("<c:trendline>");
    if let Some(name) = &trend.name {
        out.push_str(&format!("<c:name>{}</c:name>", text(name)));
    }
    out.push_str(&shape(&trend.shape));
    out.push_str(&format!("<c:trendlineType val=\"{}\"/>", trend.kind.code()));
    match trend.kind {
        TrendKind::Polynomial(order) => out.push_str(&format!("<c:order val=\"{order}\"/>")),
        TrendKind::MovingAverage(period) => out.push_str(&format!("<c:period val=\"{period}\"/>")),
        _ => {}
    }
    if trend.forward != 0.0 {
        out.push_str(&format!("<c:forward val=\"{}\"/>", number(trend.forward)));
    }
    if trend.backward != 0.0 {
        out.push_str(&format!("<c:backward val=\"{}\"/>", number(trend.backward)));
    }
    if let Some(intercept) = trend.intercept {
        out.push_str(&format!("<c:intercept val=\"{}\"/>", number(intercept)));
    }
    out.push_str(&format!(
        "<c:dispRSqr val=\"{}\"/><c:dispEq val=\"{}\"/></c:trendline>",
        u8::from(trend.show_r2),
        u8::from(trend.show_equation)
    ));
    out
}

fn series_parts(kind: &GroupKind, series: &Series) -> Vec<(&'static str, String)> {
    let scatter = matches!(kind, GroupKind::Scatter { .. } | GroupKind::Bubble);
    let mut parts = vec![
        ("idx", format!("<c:idx val=\"{}\"/>", series.index)),
        ("order", format!("<c:order val=\"{}\"/>", series.order)),
        (
            "tx",
            series.name.as_ref().map(series_text).unwrap_or_default(),
        ),
        ("spPr", shape(&series.shape)),
    ];
    match kind {
        GroupKind::Bar { .. } => parts.push((
            "invertIfNegative",
            format!(
                "<c:invertIfNegative val=\"{}\"/>",
                u8::from(series.invert_if_negative)
            ),
        )),
        GroupKind::Line { .. }
        | GroupKind::Scatter { .. }
        | GroupKind::Radar
        | GroupKind::Stock => parts.push((
            "marker",
            series.marker.as_ref().map(marker).unwrap_or_default(),
        )),
        GroupKind::Pie { .. } | GroupKind::Doughnut { .. } | GroupKind::OfPie => parts.push((
            "explosion",
            series
                .explosion
                .map(|explosion| format!("<c:explosion val=\"{explosion}\"/>"))
                .unwrap_or_default(),
        )),
        _ => {}
    }
    let points: String = series
        .points
        .iter()
        .map(|point| point_look(kind, point))
        .collect();
    parts.push(("dPt", points));
    parts.push((
        "dLbls",
        series.labels.as_ref().map(labels).unwrap_or_default(),
    ));
    if matches!(
        kind,
        GroupKind::Bar { .. } | GroupKind::Line { .. } | GroupKind::Scatter { .. }
    ) {
        parts.push((
            "trendline",
            series.trendlines.iter().map(trendline).collect(),
        ));
    }
    let (cat, val) = if scatter {
        ("xVal", "yVal")
    } else {
        ("cat", "val")
    };
    parts.push((
        cat,
        series
            .categories
            .as_ref()
            .map(|s| source(cat, s))
            .unwrap_or_default(),
    ));
    parts.push((
        val,
        series
            .values
            .as_ref()
            .map(|s| source(val, s))
            .unwrap_or_default(),
    ));
    if matches!(kind, GroupKind::Line { .. } | GroupKind::Scatter { .. }) {
        parts.push((
            "smooth",
            format!(
                "<c:smooth val=\"{}\"/>",
                u8::from(series.smooth.unwrap_or(false))
            ),
        ));
    }
    parts
}

fn series(kind: &GroupKind, series: &Series) -> String {
    let body: String = series_parts(kind, series)
        .into_iter()
        .map(|(_, xml)| xml)
        .collect();
    format!("<c:ser>{body}</c:ser>")
}

/// A group's own settings, by name, without its series.
fn group_parts(group: &ChartGroup) -> Vec<(&'static str, String)> {
    let vary = (
        "varyColors",
        format!("<c:varyColors val=\"{}\"/>", u8::from(group.vary_colors)),
    );
    let mut parts = Vec::new();
    match &group.kind {
        GroupKind::Bar {
            horizontal,
            grouping,
            gap_width,
            overlap,
            three_d,
        } => {
            parts.push((
                "barDir",
                format!(
                    "<c:barDir val=\"{}\"/>",
                    if *horizontal { "bar" } else { "col" }
                ),
            ));
            parts.push((
                "grouping",
                format!("<c:grouping val=\"{}\"/>", grouping.code()),
            ));
            parts.push(vary);
            parts.push(("gapWidth", format!("<c:gapWidth val=\"{gap_width}\"/>")));
            if !three_d {
                // Stacked bars always overlap fully.
                let overlap = if grouping.stacked() { 100 } else { *overlap };
                parts.push(("overlap", format!("<c:overlap val=\"{overlap}\"/>")));
            }
        }
        GroupKind::Line {
            grouping, markers, ..
        } => {
            parts.push((
                "grouping",
                format!("<c:grouping val=\"{}\"/>", grouping.code()),
            ));
            parts.push(vary);
            parts.push((
                "marker",
                format!("<c:marker val=\"{}\"/>", u8::from(*markers)),
            ));
        }
        GroupKind::Area { grouping, .. } => {
            parts.push((
                "grouping",
                format!("<c:grouping val=\"{}\"/>", grouping.code()),
            ));
            parts.push(vary);
        }
        GroupKind::Pie { first_slice, .. } => {
            parts.push(vary);
            parts.push((
                "firstSliceAng",
                format!("<c:firstSliceAng val=\"{first_slice}\"/>"),
            ));
        }
        GroupKind::Doughnut { first_slice, hole } => {
            parts.push(vary);
            parts.push((
                "firstSliceAng",
                format!("<c:firstSliceAng val=\"{first_slice}\"/>"),
            ));
            parts.push(("holeSize", format!("<c:holeSize val=\"{hole}\"/>")));
        }
        GroupKind::Scatter { style } => {
            parts.push((
                "scatterStyle",
                format!("<c:scatterStyle val=\"{}\"/>", style.code()),
            ));
            parts.push(vary);
        }
        _ => parts.push(vary),
    }
    parts.push((
        "dLbls",
        group.labels.as_ref().map(labels).unwrap_or_default(),
    ));
    if !group.kind.round() {
        let ids: String = group
            .axes
            .iter()
            .map(|id| format!("<c:axId val=\"{id}\"/>"))
            .collect();
        parts.push(("axId", ids));
    }
    parts
}

fn group(group: &ChartGroup) -> String {
    let tag = group.kind.tag();
    let parts = group_parts(group);
    let order = group_order(&group.kind);
    let mut body = String::new();
    let mut series_written = false;
    for name in order {
        if *name == "ser" {
            let mut list: Vec<&Series> = group.series.iter().collect();
            list.sort_by_key(|s| s.order);
            for one in list {
                body.push_str(&series(&group.kind, one));
            }
            series_written = true;
            continue;
        }
        for (part, xml) in &parts {
            if part == name {
                body.push_str(xml);
            }
        }
    }
    debug_assert!(series_written);
    format!("<c:{tag}>{body}</c:{tag}>")
}

/// The schema's order of a group's children.
fn group_order(kind: &GroupKind) -> &'static [&'static str] {
    match kind {
        GroupKind::Bar { .. } => &[
            "barDir",
            "grouping",
            "varyColors",
            "ser",
            "dLbls",
            "gapWidth",
            "gapDepth",
            "shape",
            "overlap",
            "serLines",
            "axId",
            "extLst",
        ],
        GroupKind::Line { .. } => &[
            "grouping",
            "varyColors",
            "ser",
            "dLbls",
            "dropLines",
            "gapDepth",
            "hiLowLines",
            "upDownBars",
            "marker",
            "smooth",
            "axId",
            "extLst",
        ],
        GroupKind::Area { .. } => &[
            "grouping",
            "varyColors",
            "ser",
            "dLbls",
            "dropLines",
            "gapDepth",
            "axId",
            "extLst",
        ],
        GroupKind::Pie { .. } => &["varyColors", "ser", "dLbls", "firstSliceAng", "extLst"],
        GroupKind::Doughnut { .. } => &[
            "varyColors",
            "ser",
            "dLbls",
            "firstSliceAng",
            "holeSize",
            "extLst",
        ],
        GroupKind::Scatter { .. } => &[
            "scatterStyle",
            "varyColors",
            "ser",
            "dLbls",
            "axId",
            "extLst",
        ],
        _ => &["varyColors", "ser", "dLbls", "axId", "extLst"],
    }
}

fn series_order(kind: &GroupKind) -> &'static [&'static str] {
    match kind {
        GroupKind::Bar { .. } => &[
            "idx",
            "order",
            "tx",
            "spPr",
            "invertIfNegative",
            "pictureOptions",
            "dPt",
            "dLbls",
            "trendline",
            "errBars",
            "cat",
            "val",
            "shape",
            "extLst",
        ],
        GroupKind::Line { .. } => &[
            "idx",
            "order",
            "tx",
            "spPr",
            "marker",
            "dPt",
            "dLbls",
            "trendline",
            "errBars",
            "cat",
            "val",
            "smooth",
            "extLst",
        ],
        GroupKind::Area { .. } => &[
            "idx",
            "order",
            "tx",
            "spPr",
            "pictureOptions",
            "dPt",
            "dLbls",
            "trendline",
            "errBars",
            "cat",
            "val",
            "extLst",
        ],
        GroupKind::Pie { .. } | GroupKind::Doughnut { .. } | GroupKind::OfPie => &[
            "idx",
            "order",
            "tx",
            "spPr",
            "explosion",
            "dPt",
            "dLbls",
            "cat",
            "val",
            "extLst",
        ],
        GroupKind::Scatter { .. } => &[
            "idx",
            "order",
            "tx",
            "spPr",
            "marker",
            "dPt",
            "dLbls",
            "trendline",
            "errBars",
            "xVal",
            "yVal",
            "smooth",
            "extLst",
        ],
        _ => &[
            "idx",
            "order",
            "tx",
            "spPr",
            "marker",
            "dPt",
            "dLbls",
            "trendline",
            "errBars",
            "cat",
            "val",
            "xVal",
            "yVal",
            "smooth",
            "extLst",
        ],
    }
}

/// What a series kind can't hold, among children carried over from another kind's series.
fn series_allows(kind: &GroupKind, name: &[u8]) -> bool {
    let order = series_order(kind);
    order.iter().any(|allowed| allowed.as_bytes() == name)
}

const AXIS_ORDER: &[&str] = &[
    "axId",
    "scaling",
    "delete",
    "axPos",
    "majorGridlines",
    "minorGridlines",
    "title",
    "numFmt",
    "majorTickMark",
    "minorTickMark",
    "tickLblPos",
    "spPr",
    "txPr",
    "crossAx",
    "crosses",
    "crossesAt",
    "crossBetween",
    "auto",
    "lblAlgn",
    "lblOffset",
    "baseTimeUnit",
    "majorUnit",
    "majorTimeUnit",
    "minorUnit",
    "minorTimeUnit",
    "tickLblSkip",
    "tickMarkSkip",
    "noMultiLvlLbl",
    "dispUnits",
    "extLst",
];

fn axis_parts(axis: &ChartAxis) -> Vec<(&'static str, String)> {
    let scaling = format!(
        "<c:scaling>{}<c:orientation val=\"{}\"/>{}{}</c:scaling>",
        axis.log_base
            .map(|base| format!("<c:logBase val=\"{}\"/>", number(base)))
            .unwrap_or_default(),
        if axis.reversed { "maxMin" } else { "minMax" },
        axis.max
            .map(|max| format!("<c:max val=\"{}\"/>", number(max)))
            .unwrap_or_default(),
        axis.min
            .map(|min| format!("<c:min val=\"{}\"/>", number(min)))
            .unwrap_or_default(),
    );
    let gridlines = |name: &str, lines: &Option<Gridlines>| {
        lines
            .as_ref()
            .map(|lines| {
                let look = shape(&lines.shape);
                if look.is_empty() {
                    format!("<c:{name}/>")
                } else {
                    format!("<c:{name}>{look}</c:{name}>")
                }
            })
            .unwrap_or_default()
    };
    let mut parts = vec![
        ("axId", format!("<c:axId val=\"{}\"/>", axis.id)),
        ("scaling", scaling),
        (
            "delete",
            format!("<c:delete val=\"{}\"/>", u8::from(axis.deleted)),
        ),
        (
            "axPos",
            format!("<c:axPos val=\"{}\"/>", axis.position.code()),
        ),
        (
            "majorGridlines",
            gridlines("majorGridlines", &axis.major_gridlines),
        ),
        (
            "minorGridlines",
            gridlines("minorGridlines", &axis.minor_gridlines),
        ),
        ("title", axis.title.as_ref().map(title).unwrap_or_default()),
        (
            "numFmt",
            axis.format
                .as_ref()
                .map(|(code, linked)| {
                    format!(
                        "<c:numFmt formatCode=\"{}\" sourceLinked=\"{}\"/>",
                        escape_attr(code),
                        u8::from(*linked)
                    )
                })
                .unwrap_or_default(),
        ),
        (
            "majorTickMark",
            format!("<c:majorTickMark val=\"{}\"/>", axis.major_tick.code()),
        ),
        (
            "minorTickMark",
            format!("<c:minorTickMark val=\"{}\"/>", axis.minor_tick.code()),
        ),
        (
            "tickLblPos",
            format!("<c:tickLblPos val=\"{}\"/>", axis.labels.code()),
        ),
        ("spPr", shape(&axis.shape)),
        ("txPr", text_props(&axis.look)),
        (
            "crossAx",
            format!("<c:crossAx val=\"{}\"/>", axis.cross_axis),
        ),
    ];
    match axis.crosses {
        Crosses::At(value) => parts.push((
            "crossesAt",
            format!("<c:crossesAt val=\"{}\"/>", number(value)),
        )),
        other => parts.push((
            "crosses",
            format!(
                "<c:crosses val=\"{}\"/>",
                match other {
                    Crosses::Min => "min",
                    Crosses::Max => "max",
                    _ => "autoZero",
                }
            ),
        )),
    }
    match axis.kind {
        AxisKind::Value => {
            if let Some(between) = axis.between {
                parts.push((
                    "crossBetween",
                    format!(
                        "<c:crossBetween val=\"{}\"/>",
                        if between { "between" } else { "midCat" }
                    ),
                ));
            }
            if let Some(unit) = axis.major_unit {
                parts.push((
                    "majorUnit",
                    format!("<c:majorUnit val=\"{}\"/>", number(unit)),
                ));
            }
            if let Some(unit) = axis.minor_unit {
                parts.push((
                    "minorUnit",
                    format!("<c:minorUnit val=\"{}\"/>", number(unit)),
                ));
            }
        }
        AxisKind::Category => {
            parts.push(("auto", "<c:auto val=\"1\"/>".to_string()));
            parts.push(("lblAlgn", "<c:lblAlgn val=\"ctr\"/>".to_string()));
            parts.push(("lblOffset", "<c:lblOffset val=\"100\"/>".to_string()));
            if let Some(skip) = axis.label_skip {
                parts.push(("tickLblSkip", format!("<c:tickLblSkip val=\"{skip}\"/>")));
            }
            if let Some(skip) = axis.mark_skip {
                parts.push(("tickMarkSkip", format!("<c:tickMarkSkip val=\"{skip}\"/>")));
            }
            parts.push(("noMultiLvlLbl", "<c:noMultiLvlLbl val=\"0\"/>".to_string()));
        }
        AxisKind::Date => {
            parts.push(("auto", "<c:auto val=\"1\"/>".to_string()));
            parts.push(("lblOffset", "<c:lblOffset val=\"100\"/>".to_string()));
            if let Some(unit) = axis.major_unit {
                parts.push((
                    "majorUnit",
                    format!("<c:majorUnit val=\"{}\"/>", number(unit)),
                ));
            }
        }
        AxisKind::Series => {}
    }
    parts
}

fn axis(axis: &ChartAxis) -> String {
    let tag = axis.kind.tag();
    let mut parts = axis_parts(axis);
    parts.sort_by_key(|(name, _)| {
        AXIS_ORDER
            .iter()
            .position(|n| n == name)
            .unwrap_or(usize::MAX)
    });
    let body: String = parts.into_iter().map(|(_, xml)| xml).collect();
    format!("<c:{tag}>{body}</c:{tag}>")
}

fn legend(legend: &Legend) -> String {
    let entries: String = legend
        .hidden
        .iter()
        .map(|index| {
            format!("<c:legendEntry><c:idx val=\"{index}\"/><c:delete val=\"1\"/></c:legendEntry>")
        })
        .collect();
    format!(
        "<c:legend><c:legendPos val=\"{}\"/>{entries}<c:overlay val=\"{}\"/>{}{}</c:legend>",
        legend.position.code(),
        u8::from(legend.overlay),
        shape(&legend.shape),
        text_props(&legend.look)
    )
}

fn plot_area(chart: &Chart) -> String {
    let groups: String = chart.groups.iter().map(group).collect();
    let axes: String = chart.axes.iter().map(axis).collect();
    format!(
        "<c:plotArea><c:layout/>{groups}{axes}{}</c:plotArea>",
        shape(&chart.plot_shape)
    )
}

/// A whole chart part, as Excel 2016 writes a new one.
pub fn write_chart(chart: &Chart) -> Vec<u8> {
    let titles = format!(
        "{}<c:autoTitleDeleted val=\"{}\"/>",
        chart.title.as_ref().map(title).unwrap_or_default(),
        u8::from(chart.auto_title_deleted)
    );
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n\
<c:chartSpace xmlns:c=\"{C}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\" xmlns:c16r2=\"http://schemas.microsoft.com/office/drawing/2015/06/chart\">\
<c:date1904 val=\"0\"/><c:lang val=\"en-US\"/><c:roundedCorners val=\"{}\"/>\
<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\">\
<mc:Choice Requires=\"c14\" xmlns:c14=\"http://schemas.microsoft.com/office/drawing/2007/8/2/chart\"><c14:style val=\"{}\"/></mc:Choice>\
<mc:Fallback><c:style val=\"{}\"/></mc:Fallback></mc:AlternateContent>\
<c:chart>{titles}{}{}<c:plotVisOnly val=\"{}\"/><c:dispBlanksAs val=\"{}\"/></c:chart>{}{}\
<c:printSettings><c:headerFooter/><c:pageMargins b=\"0.75\" l=\"0.7\" r=\"0.7\" t=\"0.75\" header=\"0.3\" footer=\"0.3\"/><c:pageSetup/></c:printSettings>\
</c:chartSpace>",
        u8::from(chart.rounded_corners),
        u16::from(chart.style) + 100,
        chart.style,
        plot_area(chart),
        chart.legend.as_ref().map(legend).unwrap_or_default(),
        u8::from(chart.plot_visible_only),
        chart.blanks.code(),
        shape(&chart.shape),
        text_props(&chart.look),
    );
    xml.into_bytes()
}

// ---------------------------------------------------------------------------------------------
// Bringing a file's chart part in line with an edited model

/// The file's own prefixes for the chart and drawing namespaces.
struct Prefixes {
    chart: String,
    drawing: String,
    /// The drawing namespace has to be declared on the root first.
    declare_drawing: bool,
}

impl Prefixes {
    fn of(root: &Element) -> Self {
        let tag = String::from_utf8_lossy(&root.tag).to_string();
        let name = tag
            .trim_start_matches('<')
            .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .next()
            .unwrap_or("");
        let chart = name
            .split_once(':')
            .map(|(prefix, _)| prefix.to_string())
            .unwrap_or_default();
        let mut drawing = None;
        for (at, _) in tag.match_indices("xmlns:") {
            let rest = &tag[at + 6..];
            if let Some((prefix, value)) = rest.split_once('=') {
                let value = value.trim_start_matches(['"', '\'']);
                if value.starts_with(A) && value[A.len()..].starts_with(['"', '\'']) {
                    drawing = Some(prefix.to_string());
                }
            }
        }
        Self {
            chart,
            declare_drawing: drawing.is_none(),
            drawing: drawing.unwrap_or_else(|| "a".to_string()),
        }
    }

    /// `c:` and `a:` in written XML, renamed to the file's prefixes.
    fn fix(&self, xml: &str) -> String {
        let chart = if self.chart.is_empty() {
            String::new()
        } else {
            format!("{}:", self.chart)
        };
        let drawing = format!("{}:", self.drawing);
        if chart == "c:" && drawing == "a:" {
            return xml.to_string();
        }
        xml.replace("<c:", "<\u{1}")
            .replace("</c:", "</\u{1}")
            .replace("<a:", "<\u{2}")
            .replace("</a:", "</\u{2}")
            .replace('\u{1}', &chart)
            .replace('\u{2}', &drawing)
    }
}

fn nodes(prefixes: &Prefixes, xml: &str) -> Vec<Node> {
    if xml.is_empty() {
        return Vec::new();
    }
    parse_tree(prefixes.fix(xml).as_bytes()).unwrap_or_default()
}

fn child_mut<'a>(element: &'a mut Element, name: &[u8]) -> Option<&'a mut Element> {
    element.elements_mut().find(|child| child.local() == name)
}

/// Puts `new` in place of `parent`'s children called `name`, at its place in `order`. Empty
/// `new` removes them.
fn replace_children(parent: &mut Element, order: &[&str], name: &str, new: Vec<Node>) {
    let rank = |local: &[u8]| order.iter().position(|n| n.as_bytes() == local);
    let wanted = rank(name.as_bytes());
    let mut at = None;
    let mut index = 0;
    parent.children.retain(|node| {
        let keep = match node {
            Node::Element(element) if element.local() == name.as_bytes() => {
                at.get_or_insert(index);
                false
            }
            _ => true,
        };
        if keep {
            index += 1;
        }
        keep
    });
    let position = at.unwrap_or_else(|| {
        // After the last child that comes before it, or before the first that comes after.
        let mut position = 0;
        for (index, node) in parent.children.iter().enumerate() {
            if let Node::Element(element) = node {
                match (rank(element.local()), wanted) {
                    (Some(rank), Some(wanted)) if rank < wanted => position = index + 1,
                    (Some(_), Some(_)) => break,
                    _ => {}
                }
            }
        }
        position
    });
    if parent.empty && !new.is_empty() {
        open_element(parent);
    }
    for (offset, node) in new.into_iter().enumerate() {
        parent.children.insert(position + offset, node);
    }
}

/// Turns `<x/>` into `<x></x>` so it can hold children.
fn open_element(element: &mut Element) {
    if !element.empty {
        return;
    }
    let tag = String::from_utf8_lossy(&element.tag).to_string();
    let open = tag
        .trim_end_matches('>')
        .trim_end()
        .trim_end_matches('/')
        .trim_end()
        .to_string()
        + ">";
    let name = open
        .trim_start_matches('<')
        .split(|c: char| c.is_whitespace() || c == '>')
        .next()
        .unwrap_or("")
        .to_string();
    element.tag = open.into_bytes();
    element.end = format!("</{name}>").into_bytes();
    element.empty = false;
}

/// Writes the named parts that differ, each in its place.
fn sync_parts(
    element: &mut Element,
    prefixes: &Prefixes,
    order: &[&str],
    before: &[(&'static str, String)],
    after: &[(&'static str, String)],
) {
    let mut names: Vec<&str> = after.iter().map(|(name, _)| *name).collect();
    for (name, _) in before {
        if !names.contains(name) {
            names.push(name);
        }
    }
    for name in names {
        let old: String = before
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, x)| x.as_str())
            .collect();
        let new: String = after
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, x)| x.as_str())
            .collect();
        if old != new {
            replace_children(element, order, name, nodes(prefixes, &new));
        }
    }
}

fn sync_series(
    element: &mut Element,
    prefixes: &Prefixes,
    kind: &GroupKind,
    before: &Series,
    after: &Series,
) {
    let order = series_order(kind);
    sync_parts(
        element,
        prefixes,
        order,
        &series_parts(kind, before),
        &series_parts(kind, after),
    );
}

/// A group element written from the model, keeping what each series held beyond the model.
fn rebuilt_group(prefixes: &Prefixes, old: Option<&Element>, after: &ChartGroup) -> Vec<Node> {
    let mut built = nodes(prefixes, &group(after));
    let Some(old) = old else {
        return built;
    };
    let Some(Node::Element(new_group)) = built.first_mut() else {
        return built;
    };
    // The old group's own extras: series lines, drop lines, extension lists.
    let order = group_order(&after.kind);
    for extra in old.elements() {
        let name = String::from_utf8_lossy(extra.local()).to_string();
        let managed = matches!(
            name.as_str(),
            "barDir"
                | "grouping"
                | "varyColors"
                | "ser"
                | "dLbls"
                | "gapWidth"
                | "overlap"
                | "axId"
                | "marker"
                | "firstSliceAng"
                | "holeSize"
                | "scatterStyle"
        );
        if !managed && order.contains(&name.as_str()) {
            let mut raw = Vec::new();
            write_tree(&[Node::Element(clone_element(extra))], &mut raw);
            let kept = parse_tree(&raw).unwrap_or_default();
            if !new_group
                .elements()
                .any(|element| element.local() == extra.local())
            {
                replace_children(new_group, order, &name, kept);
            }
        }
    }
    // Each series' extras that its new kind can hold.
    let old_series: Vec<&Element> = old.elements().filter(|e| e.local() == b"ser").collect();
    let series_order = series_order(&after.kind);
    for node in new_group.children.iter_mut() {
        let Node::Element(series) = node else {
            continue;
        };
        if series.local() != b"ser" {
            continue;
        }
        let index = crate::chart::val(series, b"idx");
        let Some(source) = old_series
            .iter()
            .find(|s| crate::chart::val(s, b"idx") == index)
        else {
            continue;
        };
        for extra in source.elements() {
            let name = String::from_utf8_lossy(extra.local()).to_string();
            let present = series.elements().any(|e| e.local() == extra.local());
            if present || !series_allows(&after.kind, extra.local()) {
                continue;
            }
            let managed = matches!(
                name.as_str(),
                "idx"
                    | "order"
                    | "tx"
                    | "spPr"
                    | "marker"
                    | "dPt"
                    | "dLbls"
                    | "cat"
                    | "val"
                    | "xVal"
                    | "yVal"
                    | "smooth"
                    | "invertIfNegative"
                    | "explosion"
            );
            if managed {
                continue;
            }
            let mut raw = Vec::new();
            write_tree(&[Node::Element(clone_element(extra))], &mut raw);
            replace_children(
                series,
                series_order,
                &name,
                parse_tree(&raw).unwrap_or_default(),
            );
        }
    }
    built
}

fn clone_element(element: &Element) -> Element {
    Element {
        tag: element.tag.clone(),
        empty: element.empty,
        children: element.children.iter().map(clone_node).collect(),
        end: element.end.clone(),
    }
}

fn clone_node(node: &Node) -> Node {
    match node {
        Node::Text(text) => Node::Text(text.clone()),
        Node::Element(element) => Node::Element(clone_element(element)),
    }
}

const PLOT_ORDER: &[&str] = &[
    "layout",
    "areaChart",
    "area3DChart",
    "lineChart",
    "line3DChart",
    "stockChart",
    "radarChart",
    "scatterChart",
    "pieChart",
    "pie3DChart",
    "doughnutChart",
    "barChart",
    "bar3DChart",
    "ofPieChart",
    "surfaceChart",
    "surface3DChart",
    "bubbleChart",
    "valAx",
    "catAx",
    "dateAx",
    "serAx",
    "dTable",
    "spPr",
    "extLst",
];
const CHART_ORDER: &[&str] = &[
    "title",
    "autoTitleDeleted",
    "pivotFmts",
    "view3D",
    "floor",
    "sideWall",
    "backWall",
    "plotArea",
    "legend",
    "plotVisOnly",
    "dispBlanksAs",
    "showDLblsOverMax",
    "extLst",
];
const SPACE_ORDER: &[&str] = &[
    "date1904",
    "lang",
    "roundedCorners",
    "AlternateContent",
    "style",
    "clrMapOvr",
    "pivotSource",
    "protection",
    "chart",
    "spPr",
    "txPr",
    "externalData",
    "printSettings",
    "userShapes",
    "extLst",
];

fn sync_plot(plot: &mut Element, prefixes: &Prefixes, before: &Chart, after: &Chart) {
    // Groups: kept where their kind and series stay, patched where only settings or series
    // changed, and written again where the kind changed.
    let same_shape = before.groups.len() == after.groups.len()
        && before
            .groups
            .iter()
            .zip(&after.groups)
            .all(|(a, b)| a.kind.tag() == b.kind.tag());
    if same_shape {
        let mut index = 0;
        for node in plot.children.iter_mut() {
            let Node::Element(element) = node else {
                continue;
            };
            if group_of(element).is_none() {
                continue;
            }
            let (Some(old), Some(new)) = (before.groups.get(index), after.groups.get(index)) else {
                break;
            };
            index += 1;
            if old == new {
                continue;
            }
            let order = group_order(&new.kind);
            sync_parts(
                element,
                prefixes,
                order,
                &group_parts(old),
                &group_parts(new),
            );
            // Series by their index: patched, added or removed.
            let old_indexes: Vec<u32> = old.series.iter().map(|s| s.index).collect();
            let new_indexes: Vec<u32> = new.series.iter().map(|s| s.index).collect();
            if old_indexes == new_indexes {
                for child in element.children.iter_mut() {
                    let Node::Element(series) = child else {
                        continue;
                    };
                    if series.local() != b"ser" {
                        continue;
                    }
                    let index =
                        crate::chart::val(series, b"idx").and_then(|t| t.parse::<u32>().ok());
                    let before_series = old.series.iter().find(|s| Some(s.index) == index);
                    let after_series = new.series.iter().find(|s| Some(s.index) == index);
                    if let (Some(a), Some(b)) = (before_series, after_series) {
                        if a != b {
                            sync_series(series, prefixes, &new.kind, a, b);
                        }
                    }
                }
            } else {
                let rebuilt = rebuilt_group(prefixes, Some(element), new);
                if let Some(Node::Element(rebuilt)) = rebuilt.into_iter().next() {
                    *element = rebuilt;
                }
            }
        }
    } else {
        // Every group written again, in the model's order, where the first group was.
        let olds: Vec<Element> = plot
            .elements()
            .filter(|element| group_of(element).is_some())
            .map(clone_element)
            .collect();
        let first = plot
            .children
            .iter()
            .position(|node| matches!(node, Node::Element(e) if group_of(e).is_some()));
        plot.children
            .retain(|node| !matches!(node, Node::Element(e) if group_of(e).is_some()));
        let mut built = Vec::new();
        for (index, group) in after.groups.iter().enumerate() {
            let old = olds
                .iter()
                .find(|old| old.local() == group.kind.tag().as_bytes())
                .or_else(|| olds.get(index));
            built.extend(rebuilt_group(prefixes, old, group));
        }
        let at = first.unwrap_or_else(|| {
            plot.children
                .iter()
                .position(|node| matches!(node, Node::Element(e) if e.local() != b"layout"))
                .unwrap_or(plot.children.len())
        });
        for (offset, node) in built.into_iter().enumerate() {
            plot.children.insert(at + offset, node);
        }
    }
    // Axes by id.
    if before.axes != after.axes {
        let ids: Vec<u32> = after.axes.iter().map(|axis| axis.id).collect();
        // Gone ones.
        plot.children.retain(|node| match node {
            Node::Element(element) => match axis_of(element) {
                Some(axis) => {
                    ids.contains(&axis.id)
                        && after.axis(axis.id).is_some_and(|a| a.kind == axis.kind)
                }
                None => true,
            },
            Node::Text(_) => true,
        });
        for axis_after in &after.axes {
            let existing = plot.elements_mut().find(|element| {
                axis_of(element)
                    .is_some_and(|axis| axis.id == axis_after.id && axis.kind == axis_after.kind)
            });
            match existing {
                Some(element) => {
                    let old = before
                        .axis(axis_after.id)
                        .cloned()
                        .unwrap_or_else(|| axis_of(element).unwrap_or_default());
                    if old != *axis_after {
                        let mut old_parts = axis_parts(&old);
                        let mut new_parts = axis_parts(axis_after);
                        // `auto` and the label settings are written only for new axes.
                        old_parts.retain(|(name, _)| {
                            !matches!(*name, "auto" | "lblAlgn" | "lblOffset" | "noMultiLvlLbl")
                        });
                        new_parts.retain(|(name, _)| {
                            !matches!(*name, "auto" | "lblAlgn" | "lblOffset" | "noMultiLvlLbl")
                        });
                        sync_parts(element, prefixes, AXIS_ORDER, &old_parts, &new_parts);
                    }
                }
                None => {
                    let built = nodes(prefixes, &axis(axis_after));
                    // After the last group or axis.
                    let at = plot
                        .children
                        .iter()
                        .rposition(|node| {
                            matches!(node, Node::Element(e) if group_of(e).is_some() || axis_of(e).is_some())
                        })
                        .map_or(plot.children.len(), |at| at + 1);
                    for (offset, node) in built.into_iter().enumerate() {
                        plot.children.insert(at + offset, node);
                    }
                }
            }
        }
    }
    if before.plot_shape != after.plot_shape {
        replace_children(
            plot,
            PLOT_ORDER,
            "spPr",
            nodes(prefixes, &shape(&after.plot_shape)),
        );
    }
}

/// Brings a chart part's XML in line with `after`, where it was `before`. What didn't change
/// keeps its bytes.
pub fn sync_chart(xml: &[u8], after: &Chart) -> io::Result<Vec<u8>> {
    let mut tree = parse_tree(xml)?;
    let Some(space) = tree.iter_mut().find_map(|node| match node {
        Node::Element(element) if element.local() == b"chartSpace" => Some(element),
        _ => None,
    }) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a chart part",
        ));
    };
    let before = read_chart_tree(space);
    if before == *after {
        return Ok(xml.to_vec());
    }
    let prefixes = Prefixes::of(space);
    if prefixes.declare_drawing {
        let declaration = format!("xmlns:{}", prefixes.drawing);
        space.set_attr(&declaration, Some(A));
    }
    if before.style != after.style {
        // The 2007 style, and its Excel 2010 stand-in.
        let style = format!(
            "<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice Requires=\"c14\" xmlns:c14=\"http://schemas.microsoft.com/office/drawing/2007/8/2/chart\"><c14:style val=\"{}\"/></mc:Choice><mc:Fallback><c:style val=\"{}\"/></mc:Fallback></mc:AlternateContent>",
            u16::from(after.style) + 100,
            after.style
        );
        replace_children(space, SPACE_ORDER, "style", Vec::new());
        replace_children(
            space,
            SPACE_ORDER,
            "AlternateContent",
            nodes(&prefixes, &style),
        );
    }
    if before.rounded_corners != after.rounded_corners {
        replace_children(
            space,
            SPACE_ORDER,
            "roundedCorners",
            nodes(
                &prefixes,
                &format!(
                    "<c:roundedCorners val=\"{}\"/>",
                    u8::from(after.rounded_corners)
                ),
            ),
        );
    }
    if before.shape != after.shape {
        replace_children(
            space,
            SPACE_ORDER,
            "spPr",
            nodes(&prefixes, &shape(&after.shape)),
        );
    }
    if before.look != after.look {
        replace_children(
            space,
            SPACE_ORDER,
            "txPr",
            nodes(&prefixes, &text_props(&after.look)),
        );
    }
    if let Some(chart) = child_mut(space, b"chart") {
        if before.title != after.title {
            let written = after.title.as_ref().map(title).unwrap_or_default();
            replace_children(chart, CHART_ORDER, "title", nodes(&prefixes, &written));
        }
        if before.auto_title_deleted != after.auto_title_deleted || before.title != after.title {
            replace_children(
                chart,
                CHART_ORDER,
                "autoTitleDeleted",
                nodes(
                    &prefixes,
                    &format!(
                        "<c:autoTitleDeleted val=\"{}\"/>",
                        u8::from(after.auto_title_deleted)
                    ),
                ),
            );
        }
        if before.legend != after.legend {
            let written = after.legend.as_ref().map(legend).unwrap_or_default();
            // A legend that stays keeps its layout and extensions.
            match (child_mut(chart, b"legend"), &after.legend, &before.legend) {
                (Some(element), Some(new), Some(old)) => {
                    let parts = |legend: &Legend| {
                        vec![
                            ("legendPos", format!("<c:legendPos val=\"{}\"/>", legend.position.code())),
                            (
                                "legendEntry",
                                legend
                                    .hidden
                                    .iter()
                                    .map(|i| format!("<c:legendEntry><c:idx val=\"{i}\"/><c:delete val=\"1\"/></c:legendEntry>"))
                                    .collect::<String>(),
                            ),
                            ("overlay", format!("<c:overlay val=\"{}\"/>", u8::from(legend.overlay))),
                            ("spPr", shape(&legend.shape)),
                            ("txPr", text_props(&legend.look)),
                        ]
                    };
                    let order = [
                        "legendPos",
                        "legendEntry",
                        "layout",
                        "overlay",
                        "spPr",
                        "txPr",
                        "extLst",
                    ];
                    sync_parts(element, &prefixes, &order, &parts(old), &parts(new));
                    // A moved legend leaves its old manual place.
                    if old.position != new.position {
                        replace_children(element, &order, "layout", Vec::new());
                    }
                }
                _ => replace_children(chart, CHART_ORDER, "legend", nodes(&prefixes, &written)),
            }
        }
        if before.plot_visible_only != after.plot_visible_only {
            replace_children(
                chart,
                CHART_ORDER,
                "plotVisOnly",
                nodes(
                    &prefixes,
                    &format!(
                        "<c:plotVisOnly val=\"{}\"/>",
                        u8::from(after.plot_visible_only)
                    ),
                ),
            );
        }
        if before.blanks != after.blanks {
            replace_children(
                chart,
                CHART_ORDER,
                "dispBlanksAs",
                nodes(
                    &prefixes,
                    &format!("<c:dispBlanksAs val=\"{}\"/>", after.blanks.code()),
                ),
            );
        }
        if let Some(plot) = child_mut(chart, b"plotArea") {
            sync_plot(plot, &prefixes, &before, after);
        }
    }
    let mut out = Vec::with_capacity(xml.len() + 256);
    write_tree(&tree, &mut out);
    Ok(out)
}

/// The labels' and shapes' look of an element, as `chart.rs` reads it. For tests.
#[cfg(test)]
fn look_of(xml: &str) -> (ShapeLook, TextLook, DataLabels) {
    use crate::chart::{labels_of, shape_of, text_look_of};
    let tree = parse_tree(xml.as_bytes()).unwrap();
    let Node::Element(element) = &tree[0] else {
        panic!()
    };
    (shape_of(element), text_look_of(element), labels_of(element))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::read_chart;

    fn sample() -> Chart {
        let source = |formula: &str, kind, values: &[&str]| DataSource {
            formula: Some(formula.to_string()),
            kind,
            count: values.len() as u32,
            cache: values
                .iter()
                .enumerate()
                .map(|(index, value)| CachePoint {
                    index: index as u32,
                    value: value.to_string(),
                })
                .collect(),
            format: (kind == SourceKind::Number).then(|| "General".to_string()),
        };
        Chart {
            groups: vec![ChartGroup {
                kind: GroupKind::Bar {
                    horizontal: false,
                    grouping: Grouping::Clustered,
                    gap_width: 219,
                    overlap: -27,
                    three_d: false,
                },
                vary_colors: false,
                series: vec![Series {
                    index: 0,
                    order: 0,
                    name: Some(ChartText::Cell {
                        formula: "Sheet1!$B$1".into(),
                        cache: "Sales".into(),
                    }),
                    categories: Some(source(
                        "Sheet1!$A$2:$A$3",
                        SourceKind::Text,
                        &["North", "South"],
                    )),
                    values: Some(source(
                        "Sheet1!$B$2:$B$3",
                        SourceKind::Number,
                        &["10", "20"],
                    )),
                    shape: ShapeLook {
                        fill: Some(ChartFill::Solid(DmlColor::scheme("accent1"))),
                        line: Some(ChartLine {
                            fill: Some(ChartFill::None),
                            ..ChartLine::default()
                        }),
                    },
                    ..Series::default()
                }],
                axes: vec![1, 2],
                labels: None,
            }],
            axes: vec![
                ChartAxis {
                    id: 1,
                    kind: AxisKind::Category,
                    position: AxisPosition::Bottom,
                    cross_axis: 2,
                    labels: LabelPlace::NextTo,
                    ..ChartAxis::default()
                },
                ChartAxis {
                    id: 2,
                    kind: AxisKind::Value,
                    position: AxisPosition::Left,
                    cross_axis: 1,
                    between: Some(true),
                    major_gridlines: Some(Gridlines::default()),
                    labels: LabelPlace::NextTo,
                    ..ChartAxis::default()
                },
            ],
            title: Some(ChartTitle::text("Sales")),
            legend: Some(Legend {
                position: LegendPosition::Bottom,
                ..Legend::default()
            }),
            style: 2,
            plot_visible_only: true,
            ..Chart::default()
        }
    }

    fn read(xml: &[u8]) -> Chart {
        match read_chart(xml, false) {
            ChartBody::Classic(chart) => *chart,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_new_chart_reads_back_as_written() {
        let chart = sample();
        let xml = write_chart(&chart);
        let back = read(&xml);
        assert_eq!(back.groups, chart.groups);
        assert_eq!(back.axes, chart.axes);
        assert_eq!(back.title, chart.title);
        assert_eq!(back.legend, chart.legend);
        assert_eq!(back.style, 2);
    }

    #[test]
    fn an_unchanged_chart_keeps_its_bytes() {
        let xml = write_chart(&sample());
        assert_eq!(sync_chart(&xml, &read(&xml)).unwrap(), xml);
    }

    #[test]
    fn edits_keep_what_the_model_does_not_hold() {
        let mut xml = String::from_utf8(write_chart(&sample())).unwrap();
        // Extensions Excel writes: a series id and a chart-level extension.
        xml = xml.replace(
            "</c:val></c:ser>",
            "</c:val><c:extLst><c:ext uri=\"{C3380CC4-5D6E-409C-BE32-E72D297353CC}\"><c16:uniqueId xmlns:c16=\"x\" val=\"{1}\"/></c:ext></c:extLst></c:ser>",
        );
        xml = xml.replace("</c:chart>", "<c:extLst><c:ext uri=\"{56B9EC1D-385E-4148-901F-78D8002777C0}\"/></c:extLst></c:chart>");
        let mut chart = read(xml.as_bytes());
        chart.title = Some(ChartTitle::text("Revenue"));
        chart.legend.as_mut().unwrap().position = LegendPosition::Right;
        chart.axes[1].max = Some(50.0);
        let out = String::from_utf8(sync_chart(xml.as_bytes(), &chart).unwrap()).unwrap();
        assert!(out.contains("c16:uniqueId"), "{out}");
        assert!(out.contains("56B9EC1D"), "{out}");
        let back = read(out.as_bytes());
        assert_eq!(back.title, Some(ChartTitle::text("Revenue")));
        assert_eq!(back.legend.unwrap().position, LegendPosition::Right);
        assert_eq!(back.axes[1].max, Some(50.0));
        // The schema's order: `max` inside scaling after orientation.
        assert!(
            out.contains("<c:orientation val=\"minMax\"/><c:max val=\"50\"/>"),
            "{out}"
        );
    }

    #[test]
    fn a_new_type_keeps_series_extensions() {
        let mut xml = String::from_utf8(write_chart(&sample())).unwrap();
        xml = xml.replace(
            "</c:val></c:ser>",
            "</c:val><c:extLst><c:ext uri=\"{C3380CC4}\"/></c:extLst></c:ser>",
        );
        let mut chart = read(xml.as_bytes());
        chart.groups[0].kind = GroupKind::Line {
            grouping: Grouping::Standard,
            markers: true,
            three_d: false,
        };
        let out = String::from_utf8(sync_chart(xml.as_bytes(), &chart).unwrap()).unwrap();
        assert!(out.contains("<c:lineChart>"), "{out}");
        assert!(!out.contains("barChart"), "{out}");
        assert!(out.contains("C3380CC4"), "{out}");
        assert!(!out.contains("invertIfNegative"), "{out}");
        let back = read(out.as_bytes());
        assert_eq!(back.groups[0].kind, chart.groups[0].kind);
        assert_eq!(
            back.groups[0].series[0].values,
            chart.groups[0].series[0].values
        );
    }

    #[test]
    fn other_prefixes_are_kept() {
        let xml = String::from_utf8(write_chart(&sample()))
            .unwrap()
            .replace("<c:", "<ch:")
            .replace("</c:", "</ch:")
            .replace("xmlns:c=", "xmlns:ch=");
        let mut chart = read(xml.as_bytes());
        chart.title = Some(ChartTitle::text("Other"));
        let out = String::from_utf8(sync_chart(xml.as_bytes(), &chart).unwrap()).unwrap();
        assert!(out.contains("<ch:title>"), "{out}");
        assert!(!out.contains("<c:"), "{out}");
    }

    #[test]
    fn looks_read_back() {
        let (shape, _, _) = look_of(
            "<c:x><c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></c:spPr></c:x>",
        );
        assert_eq!(shape.fill, Some(ChartFill::Solid(DmlColor::rgb(0xFF0000))));
    }
}
