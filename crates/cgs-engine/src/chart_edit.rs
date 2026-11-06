//! Making and changing charts and pictures: moving and sizing drawings, inserting a chart from the
//! selection as Excel 2016 makes one, and the Chart Design and Format tabs' changes. Each command
//! replaces a sheet's drawings or a chart part's model, and undo puts the old one back.

use cgs_calc::{CalcEngine, CalcValue, CellSink, Rect as CalcRect, SheetRect};
use cgs_model::chart::*;
use cgs_model::{Anchor, AnchorPoint, ChartBody, ChartPart, Drawing, DrawingKind, SheetInfo};
use cgs_refs::col_to_letters;
use serde::Deserialize;

use crate::command::{Applied, Area, Op};
use crate::tile::{sheet_x, sheet_y};
use crate::Session;

/// EMU in one CSS pixel at 100%.
const EMU_PER_PX: i64 = 9525;
/// A new chart's size, as Excel makes one: 5 by 3 inches.
pub const NEW_CHART_PX: (i64, i64) = (480, 288);

/// What the Insert tab's chart buttons and Change Chart Type make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChartType {
    ColumnClustered,
    ColumnStacked,
    ColumnPercent,
    BarClustered,
    BarStacked,
    BarPercent,
    Line,
    LineStacked,
    LinePercent,
    LineMarkers,
    LineMarkersStacked,
    LineMarkersPercent,
    Pie,
    Doughnut,
    Area,
    AreaStacked,
    AreaPercent,
    Scatter,
    ScatterSmoothMarkers,
    ScatterSmooth,
    ScatterLinesMarkers,
    ScatterLines,
    /// Clustered columns, and a line for the last series.
    ComboColumnLine,
    /// As above, with the line on a secondary axis.
    ComboColumnLineSecondary,
    /// A stacked area, and clustered columns for the last series.
    ComboAreaColumn,
}

impl ChartType {
    fn kind(self) -> GroupKind {
        use ChartType::*;
        let bar = |horizontal, grouping: Grouping| GroupKind::Bar {
            horizontal,
            grouping,
            gap_width: if grouping.stacked() {
                150
            } else if horizontal {
                182
            } else {
                219
            },
            overlap: if grouping.stacked() { 100 } else { -27 },
            three_d: false,
        };
        let line = |grouping, markers| GroupKind::Line {
            grouping,
            markers,
            three_d: false,
        };
        let area = |grouping| GroupKind::Area {
            grouping,
            three_d: false,
        };
        let scatter = |style| GroupKind::Scatter { style };
        match self {
            ColumnClustered | ComboColumnLine | ComboColumnLineSecondary => {
                bar(false, Grouping::Clustered)
            }
            ColumnStacked => bar(false, Grouping::Stacked),
            ColumnPercent => bar(false, Grouping::PercentStacked),
            BarClustered => bar(true, Grouping::Clustered),
            BarStacked => bar(true, Grouping::Stacked),
            BarPercent => bar(true, Grouping::PercentStacked),
            Line => line(Grouping::Standard, false),
            LineStacked => line(Grouping::Stacked, false),
            LinePercent => line(Grouping::PercentStacked, false),
            LineMarkers => line(Grouping::Standard, true),
            LineMarkersStacked => line(Grouping::Stacked, true),
            LineMarkersPercent => line(Grouping::PercentStacked, true),
            Pie => GroupKind::Pie {
                first_slice: 0,
                three_d: false,
            },
            Doughnut => GroupKind::Doughnut {
                first_slice: 0,
                hole: 75,
            },
            Area => area(Grouping::Standard),
            AreaStacked | ComboAreaColumn => area(Grouping::Stacked),
            AreaPercent => area(Grouping::PercentStacked),
            // Markers alone; Excel writes this as `lineMarker` with no line, which reads the same.
            Scatter => scatter(ScatterStyle::Marker),
            ScatterSmoothMarkers => scatter(ScatterStyle::SmoothMarker),
            ScatterSmooth => scatter(ScatterStyle::Smooth),
            ScatterLinesMarkers => scatter(ScatterStyle::LineMarker),
            ScatterLines => scatter(ScatterStyle::Line),
        }
    }

    fn combo(self) -> Option<(GroupKind, bool)> {
        match self {
            Self::ComboColumnLine => Some((Self::Line.kind(), false)),
            Self::ComboColumnLineSecondary => Some((Self::Line.kind(), true)),
            Self::ComboAreaColumn => Some((Self::ColumnClustered.kind(), false)),
            _ => None,
        }
    }

    fn scatter(self) -> bool {
        matches!(self.kind(), GroupKind::Scatter { .. })
    }
}

/// The part of a chart a Format command changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChartElement {
    ChartArea,
    PlotArea,
    Title,
    Legend,
    Axis { id: u32 },
    AxisTitle { id: u32 },
    Gridlines { id: u32 },
    Series { index: u32 },
    Point { index: u32, point: u32 },
    DataLabels { index: u32 },
}

/// A colour as the UI picks it: `#rrggbb`, or a theme colour by name with a luminance change.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartColor {
    #[serde(default)]
    pub rgb: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub lum_mod: Option<i32>,
    #[serde(default)]
    pub lum_off: Option<i32>,
}

impl ChartColor {
    fn dml(&self) -> Option<DmlColor> {
        let mut color = match (&self.rgb, &self.theme) {
            (Some(rgb), _) => {
                DmlColor::rgb(u32::from_str_radix(rgb.trim_start_matches('#'), 16).ok()?)
            }
            (None, Some(name)) => DmlColor::scheme(name),
            _ => return None,
        };
        if let Some(value) = self.lum_mod {
            color = color.with(ColorModKind::LumMod, value);
        }
        if let Some(value) = self.lum_off {
            color = color.with(ColorModKind::LumOff, value);
        }
        Some(color)
    }
}

/// One series as Select Data edits it: formulas without `=`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesSpec {
    /// A cell reference, or typed text in quotes.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub categories: Option<String>,
    pub values: String,
}

/// A change from the Chart Design or Format tab, or the format pane.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChartEdit {
    /// Change Chart Type, for the whole chart.
    Type {
        chart: ChartType,
    },
    /// One series' type in a combination chart.
    SeriesType {
        index: u32,
        chart: ChartType,
        #[serde(default)]
        secondary: bool,
    },
    /// `None` removes the title; an empty text makes it automatic.
    Title {
        #[serde(default)]
        text: Option<String>,
    },
    AxisTitle {
        id: u32,
        #[serde(default)]
        text: Option<String>,
    },
    /// `None` removes the legend.
    Legend {
        #[serde(default)]
        position: Option<String>,
    },
    /// Data labels for one series, or all with none.
    DataLabels {
        #[serde(default)]
        index: Option<u32>,
        show: bool,
        #[serde(default)]
        position: Option<String>,
        #[serde(default)]
        value: Option<bool>,
        #[serde(default)]
        category: Option<bool>,
        #[serde(default)]
        series: Option<bool>,
        #[serde(default)]
        percent: Option<bool>,
    },
    Gridlines {
        id: u32,
        major: bool,
        minor: bool,
    },
    AxisVisible {
        id: u32,
        visible: bool,
    },
    AxisScale {
        id: u32,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default)]
        major: Option<f64>,
        #[serde(default)]
        minor: Option<f64>,
        #[serde(default)]
        reversed: bool,
        /// A number format code, or `None` to follow the cells.
        #[serde(default)]
        format: Option<String>,
    },
    SwitchRowCol,
    /// Select Data's chart range: series made again from these cells.
    Range {
        sheet: u32,
        area: Area,
    },
    /// Select Data's series list.
    Series {
        list: Vec<SeriesSpec>,
    },
    /// Chart Styles: eight looks, as the first eight of Excel 2016's gallery give a chart.
    Style {
        style: u8,
    },
    /// Change Colors: 1 to 4 colourful, 5 to 17 one accent.
    Colors {
        palette: u8,
    },
    QuickLayout {
        layout: u8,
    },
    Fill {
        element: ChartElement,
        #[serde(default)]
        color: Option<ChartColor>,
        /// No fill at all, where `color` is none.
        #[serde(default)]
        none: bool,
    },
    Outline {
        element: ChartElement,
        #[serde(default)]
        color: Option<ChartColor>,
        #[serde(default)]
        none: bool,
        /// Points.
        #[serde(default)]
        width: Option<f64>,
        #[serde(default)]
        dash: Option<String>,
    },
    Font {
        element: ChartElement,
        #[serde(default)]
        size: Option<f64>,
        #[serde(default)]
        bold: Option<bool>,
        #[serde(default)]
        italic: Option<bool>,
        #[serde(default)]
        color: Option<ChartColor>,
    },
    GapWidth {
        group: usize,
        gap: u16,
        #[serde(default)]
        overlap: Option<i16>,
    },
}

// ---------------------------------------------------------------------------------------------
// Excel 2016's look for a new chart

fn grey(lum_mod: i32, lum_off: i32) -> DmlColor {
    DmlColor::scheme("tx1")
        .with(ColorModKind::LumMod, lum_mod)
        .with(ColorModKind::LumOff, lum_off)
}

fn text_look(size: u32) -> TextLook {
    TextLook {
        size: Some(size),
        bold: Some(false),
        italic: Some(false),
        color: Some(grey(65_000, 35_000)),
        ..TextLook::default()
    }
}

fn thin(color: DmlColor) -> ChartLine {
    ChartLine {
        fill: Some(ChartFill::Solid(color)),
        width: Some(9525),
        dash: None,
    }
}

fn no_line() -> ChartLine {
    ChartLine {
        fill: Some(ChartFill::None),
        ..ChartLine::default()
    }
}

/// An accent as Excel 2016's first colour set uses it for the nth series.
fn accent(index: usize) -> DmlColor {
    crate::charts::auto_color(2, index, index + 1)
}

/// A series' look in a group of this kind.
fn series_look(kind: &GroupKind, index: usize, series: &mut Series) {
    let color = accent(index);
    series.points.clear();
    match kind {
        GroupKind::Bar { .. } | GroupKind::Area { .. } => {
            series.shape = ShapeLook {
                fill: Some(ChartFill::Solid(color)),
                line: Some(no_line()),
            };
            series.marker = None;
        }
        GroupKind::Line { markers, .. } => {
            series.shape = ShapeLook {
                fill: None,
                line: Some(ChartLine {
                    fill: Some(ChartFill::Solid(color.clone())),
                    width: Some(28575),
                    dash: None,
                }),
            };
            series.marker = Some(if *markers {
                Marker {
                    symbol: MarkerSymbol::Circle,
                    size: Some(5),
                    shape: ShapeLook {
                        fill: Some(ChartFill::Solid(color.clone())),
                        line: Some(thin(color)),
                    },
                }
            } else {
                Marker {
                    symbol: MarkerSymbol::None,
                    ..Marker::default()
                }
            });
            series.smooth = Some(false);
        }
        GroupKind::Scatter { style } => {
            let lines = matches!(
                style,
                ScatterStyle::LineMarker
                    | ScatterStyle::SmoothMarker
                    | ScatterStyle::Line
                    | ScatterStyle::Smooth
            );
            series.shape = ShapeLook {
                fill: None,
                line: Some(if lines {
                    ChartLine {
                        fill: Some(ChartFill::Solid(color.clone())),
                        width: Some(19050),
                        dash: None,
                    }
                } else {
                    ChartLine {
                        width: Some(25400),
                        ..no_line()
                    }
                }),
            };
            series.marker = Some(
                if matches!(style, ScatterStyle::Line | ScatterStyle::Smooth) {
                    Marker {
                        symbol: MarkerSymbol::None,
                        ..Marker::default()
                    }
                } else {
                    Marker {
                        symbol: MarkerSymbol::Circle,
                        size: Some(5),
                        shape: ShapeLook {
                            fill: Some(ChartFill::Solid(color.clone())),
                            line: Some(thin(color)),
                        },
                    }
                },
            );
            series.smooth = Some(matches!(
                style,
                ScatterStyle::Smooth | ScatterStyle::SmoothMarker
            ));
        }
        GroupKind::Pie { .. } | GroupKind::Doughnut { .. } => {
            series.shape = ShapeLook::default();
            series.marker = None;
            let count = series
                .values
                .as_ref()
                .map_or(0, |values| values.count.max(values.cache.len() as u32));
            series.points = (0..count)
                .map(|point| PointLook {
                    index: point,
                    shape: ShapeLook {
                        fill: Some(ChartFill::Solid(accent(point as usize))),
                        line: Some(ChartLine {
                            fill: Some(ChartFill::Solid(DmlColor::scheme("bg1"))),
                            width: Some(19050),
                            dash: None,
                        }),
                    },
                    ..PointLook::default()
                })
                .collect();
        }
        _ => {}
    }
}

fn category_axis(id: u32, cross: u32, horizontal: bool) -> ChartAxis {
    ChartAxis {
        id,
        kind: AxisKind::Category,
        position: if horizontal {
            AxisPosition::Left
        } else {
            AxisPosition::Bottom
        },
        cross_axis: cross,
        format: Some(("General".to_string(), true)),
        major_tick: TickMark::None,
        minor_tick: TickMark::None,
        labels: LabelPlace::NextTo,
        shape: ShapeLook {
            fill: Some(ChartFill::None),
            line: Some(ChartLine {
                fill: Some(ChartFill::Solid(grey(15_000, 85_000))),
                width: Some(9525),
                dash: None,
            }),
        },
        look: text_look(900),
        ..ChartAxis::default()
    }
}

fn value_axis(id: u32, cross: u32, position: AxisPosition, between: bool) -> ChartAxis {
    ChartAxis {
        id,
        kind: AxisKind::Value,
        position,
        cross_axis: cross,
        format: Some(("General".to_string(), true)),
        major_gridlines: Some(Gridlines {
            shape: ShapeLook {
                fill: None,
                line: Some(thin(grey(15_000, 85_000))),
            },
        }),
        major_tick: TickMark::None,
        minor_tick: TickMark::None,
        labels: LabelPlace::NextTo,
        between: Some(between),
        shape: ShapeLook {
            fill: Some(ChartFill::None),
            line: Some(no_line()),
        },
        look: text_look(900),
        ..ChartAxis::default()
    }
}

/// Axis ids as Excel picks them: large and unlike any other chart's.
fn axis_ids(seed: u32) -> (u32, u32, u32, u32) {
    let base = 100_000_000 + (seed.wrapping_mul(2_654_435_761) % 800_000_000);
    (base, base + 1, base + 2, base + 3)
}

/// The axes a set of groups needs, keeping ids the chart already has.
fn axes_for(groups: &mut [ChartGroup], old: &[ChartAxis], seed: u32) -> Vec<ChartAxis> {
    let (a, b, c, d) = axis_ids(seed);
    // The chart's first pair of axes keep their ids; a scatter chart's x axis is a value axis.
    let old_cat = old
        .iter()
        .find(|axis| matches!(axis.kind, AxisKind::Category | AxisKind::Date))
        .or_else(|| old.first())
        .map(|axis| axis.id);
    let old_val = old
        .iter()
        .find(|axis| axis.kind == AxisKind::Value && Some(axis.id) != old_cat)
        .map(|axis| axis.id);
    let cat = old_cat.unwrap_or(a);
    let val = old_val.unwrap_or(b);
    let mut axes = Vec::new();
    let Some(first) = groups.first() else {
        return axes;
    };
    if first.kind.round() {
        for group in groups.iter_mut() {
            group.axes.clear();
        }
        return axes;
    }
    let horizontal = matches!(
        first.kind,
        GroupKind::Bar {
            horizontal: true,
            ..
        }
    );
    let scatter = matches!(first.kind, GroupKind::Scatter { .. });
    // Excel 2016 puts categories between tick marks for bars and lines, and on them for areas.
    let between = !matches!(
        first.kind,
        GroupKind::Area { .. } | GroupKind::Scatter { .. }
    );
    let keep = |axis: ChartAxis| -> ChartAxis {
        // An axis the chart had keeps its titles, scale and look.
        match old.iter().find(|o| o.id == axis.id && o.kind == axis.kind) {
            Some(o) => ChartAxis {
                position: axis.position,
                between: axis.between,
                cross_axis: axis.cross_axis,
                ..o.clone()
            },
            None => axis,
        }
    };
    if scatter {
        let mut x = value_axis(cat, val, AxisPosition::Bottom, false);
        x.id = cat;
        axes.push(keep(x));
        axes.push(keep(value_axis(val, cat, AxisPosition::Left, false)));
    } else {
        axes.push(keep(category_axis(cat, val, horizontal)));
        axes.push(keep(value_axis(
            val,
            cat,
            if horizontal {
                AxisPosition::Bottom
            } else {
                AxisPosition::Left
            },
            between,
        )));
    }
    let mut secondary = false;
    for (index, group) in groups.iter_mut().enumerate() {
        let on_secondary = index > 0 && group.axes.get(1).is_some_and(|id| *id != val && *id != 0);
        group.axes = if on_secondary {
            secondary = true;
            vec![c, d]
        } else {
            vec![cat, val]
        };
    }
    if secondary {
        let mut hidden = category_axis(c, d, horizontal);
        hidden.deleted = true;
        axes.push(keep(hidden));
        let mut right = value_axis(
            d,
            c,
            if horizontal {
                AxisPosition::Top
            } else {
                AxisPosition::Right
            },
            between,
        );
        right.major_gridlines = None;
        right.crosses = Crosses::Max;
        axes.push(keep(right));
    }
    axes
}

fn title_look() -> TextLook {
    text_look(1400)
}

fn legend() -> Legend {
    Legend {
        position: LegendPosition::Bottom,
        overlay: false,
        hidden: Vec::new(),
        look: text_look(900),
        shape: ShapeLook {
            fill: Some(ChartFill::None),
            line: Some(no_line()),
        },
    }
}

fn automatic_title() -> ChartTitle {
    ChartTitle {
        text: None,
        overlay: false,
        look: title_look(),
        shape: ShapeLook {
            fill: Some(ChartFill::None),
            line: Some(no_line()),
        },
    }
}

/// A chart of this type over these series, as Excel 2016 makes one.
pub fn new_chart(chart_type: ChartType, mut series: Vec<Series>, seed: u32) -> Chart {
    let kind = chart_type.kind();
    let round = kind.round();
    if round {
        series.truncate(1);
    }
    let count = series.len();
    let mut groups = vec![ChartGroup {
        kind: kind.clone(),
        vary_colors: round,
        series: Vec::new(),
        axes: vec![0, 0],
        labels: None,
    }];
    let combo = chart_type.combo();
    for (index, mut one) in series.into_iter().enumerate() {
        let target = match &combo {
            Some((other, secondary)) if index + 1 == count && count > 1 => {
                if groups.len() == 1 {
                    groups.push(ChartGroup {
                        kind: other.clone(),
                        vary_colors: false,
                        series: Vec::new(),
                        axes: if *secondary { vec![1, 1] } else { vec![0, 0] },
                        labels: None,
                    });
                }
                1
            }
            _ => 0,
        };
        let group_kind = groups[target].kind.clone();
        series_look(&group_kind, index, &mut one);
        groups[target].series.push(one);
    }
    // Secondary axes are marked by a different id until `axes_for` names them.
    if let Some(group) = groups.get_mut(1) {
        if group.axes == vec![1, 1] {
            group.axes = vec![u32::MAX - 1, u32::MAX];
        }
    }
    let axes = axes_for(&mut groups, &[], seed);
    Chart {
        groups,
        axes,
        title: Some(automatic_title()),
        auto_title_deleted: false,
        legend: (count > 1 || round).then(legend),
        style: 2,
        rounded_corners: false,
        plot_visible_only: true,
        blanks: Blanks::Gap,
        shape: ShapeLook {
            fill: Some(ChartFill::Solid(DmlColor::scheme("bg1"))),
            line: Some(thin(grey(15_000, 85_000))),
        },
        plot_shape: ShapeLook {
            fill: Some(ChartFill::None),
            line: Some(no_line()),
        },
        look: TextLook::default(),
        plot_layout: None,
        three_d: false,
        pivot: false,
    }
}

// ---------------------------------------------------------------------------------------------
// Series from a selection, as Excel reads one

/// What a cell of the selection holds, for telling headings from data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Empty,
    Number,
    /// A number shown as a date or time, which Excel takes for a heading.
    Date,
    Text,
}

fn quote_sheet(name: &str) -> String {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.')
        && !name.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        && cgs_refs::parse_range(name).is_none();
    if plain {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

fn reference(sheet: &str, top: u32, left: u32, bottom: u32, right: u32) -> String {
    let cell = |row: u32, col: u32| format!("${}${}", col_to_letters(i64::from(col)), row);
    if top == bottom && left == right {
        format!("{}!{}", quote_sheet(sheet), cell(top, left))
    } else {
        format!(
            "{}!{}:{}",
            quote_sheet(sheet),
            cell(top, left),
            cell(bottom, right)
        )
    }
}

/// How a selection becomes series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// Series run down columns.
    pub by_columns: bool,
    /// The first row holds headings.
    pub header_row: bool,
    /// The first column holds headings.
    pub header_col: bool,
}

fn plan(cells: &[Cell], rows: u32, cols: u32, by_columns: Option<bool>) -> Plan {
    let at = |row: u32, col: u32| cells[(row * cols + col) as usize];
    let numeric = |cell: Cell| cell == Cell::Number;
    let corner_blank = at(0, 0) == Cell::Empty;
    // A first row with no plain numbers past its first cell is headings, as is a first column.
    let header_row = rows > 1 && (corner_blank || (0..cols).all(|col| !numeric(at(0, col))));
    let header_col = cols > 1 && (corner_blank || (0..rows).all(|row| !numeric(at(row, 0))));
    let data_rows = rows - u32::from(header_row);
    let data_cols = cols - u32::from(header_col);
    Plan {
        by_columns: by_columns.unwrap_or(data_rows >= data_cols),
        header_row,
        header_col,
    }
}

/// Series over an area of a sheet. `scatter` takes the first data column or row as x values.
pub fn series_from(sheet_name: &str, area: Area, plan: Plan, scatter: bool) -> Vec<Series> {
    let first_row = area.top + u32::from(plan.header_row);
    let first_col = area.left + u32::from(plan.header_col);
    let mut out = Vec::new();
    let text_source = |formula: String| DataSource {
        formula: Some(formula),
        kind: SourceKind::Text,
        ..DataSource::default()
    };
    let number_source = |formula: String| DataSource {
        formula: Some(formula),
        kind: SourceKind::Number,
        format: Some("General".to_string()),
        ..DataSource::default()
    };
    if plan.by_columns {
        let mut categories = plan
            .header_col
            .then(|| reference(sheet_name, first_row, area.left, area.bottom, area.left));
        let mut start = first_col;
        if scatter && categories.is_none() && area.right > first_col {
            categories = Some(reference(
                sheet_name,
                first_row,
                first_col,
                area.bottom,
                first_col,
            ));
            start += 1;
        }
        for col in start..=area.right {
            out.push(Series {
                index: out.len() as u32,
                order: out.len() as u32,
                name: plan.header_row.then(|| ChartText::Cell {
                    formula: reference(sheet_name, area.top, col, area.top, col),
                    cache: String::new(),
                }),
                categories: categories.clone().map(|f| {
                    if scatter {
                        number_source(f)
                    } else {
                        text_source(f)
                    }
                }),
                values: Some(number_source(reference(
                    sheet_name,
                    first_row,
                    col,
                    area.bottom,
                    col,
                ))),
                ..Series::default()
            });
        }
    } else {
        let mut categories = plan
            .header_row
            .then(|| reference(sheet_name, area.top, first_col, area.top, area.right));
        let mut start = first_row;
        if scatter && categories.is_none() && area.bottom > first_row {
            categories = Some(reference(
                sheet_name, first_row, first_col, first_row, area.right,
            ));
            start += 1;
        }
        for row in start..=area.bottom {
            out.push(Series {
                index: out.len() as u32,
                order: out.len() as u32,
                name: plan.header_col.then(|| ChartText::Cell {
                    formula: reference(sheet_name, row, area.left, row, area.left),
                    cache: String::new(),
                }),
                categories: categories.clone().map(|f| {
                    if scatter {
                        number_source(f)
                    } else {
                        text_source(f)
                    }
                }),
                values: Some(number_source(reference(
                    sheet_name, row, first_col, row, area.right,
                ))),
                ..Series::default()
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Placing drawings

fn col_at(info: &SheetInfo, x: i64) -> (u32, i64) {
    let (mut low, mut high) = (1u32, cgs_model::MAX_COLS);
    while low < high {
        let mid = (low + high).div_ceil(2);
        if sheet_x(info, mid) <= x {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    (low, x - sheet_x(info, low))
}

fn row_at(info: &SheetInfo, y: i64) -> (u32, i64) {
    let (mut low, mut high) = (1u32, cgs_model::MAX_ROWS);
    while low < high {
        let mid = (low + high).div_ceil(2);
        if sheet_y(info, mid) <= y {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    (low, y - sheet_y(info, low))
}

/// The point of the grid at this place, in CSS pixels at 100%.
pub fn point_at(info: &SheetInfo, x: i64, y: i64) -> AnchorPoint {
    let (col, col_px) = col_at(info, x.max(0));
    let (row, row_px) = row_at(info, y.max(0));
    AnchorPoint {
        row,
        col,
        row_off: row_px * EMU_PER_PX,
        col_off: col_px * EMU_PER_PX,
    }
}

/// The anchor of the same kind at a new place and size.
pub fn placed(info: &SheetInfo, anchor: Anchor, x: i64, y: i64, width: i64, height: i64) -> Anchor {
    let (width, height) = (width.max(1), height.max(1));
    match anchor {
        Anchor::TwoCell { .. } => Anchor::TwoCell {
            from: point_at(info, x, y),
            to: point_at(info, x + width, y + height),
        },
        Anchor::OneCell { .. } => Anchor::OneCell {
            from: point_at(info, x, y),
            width: width * EMU_PER_PX,
            height: height * EMU_PER_PX,
        },
        Anchor::Absolute { .. } => Anchor::Absolute {
            x: x * EMU_PER_PX,
            y: y * EMU_PER_PX,
            width: width * EMU_PER_PX,
            height: height * EMU_PER_PX,
        },
    }
}

fn kind_of(element: &ChartElement) -> &'static str {
    match element {
        ChartElement::ChartArea => "chart area",
        ChartElement::PlotArea => "plot area",
        ChartElement::Title => "title",
        ChartElement::Legend => "legend",
        ChartElement::Axis { .. } => "axis",
        ChartElement::AxisTitle { .. } => "axis title",
        ChartElement::Gridlines { .. } => "gridlines",
        ChartElement::Series { .. } => "series",
        ChartElement::Point { .. } => "point",
        ChartElement::DataLabels { .. } => "data labels",
    }
}

fn series_mut(chart: &mut Chart, index: u32) -> Result<&mut Series, String> {
    chart
        .groups
        .iter_mut()
        .flat_map(|group| group.series.iter_mut())
        .find(|series| series.index == index)
        .ok_or_else(|| "That series isn't in the chart any more.".to_string())
}

fn shape_mut<'a>(
    chart: &'a mut Chart,
    element: &ChartElement,
) -> Result<&'a mut ShapeLook, String> {
    Ok(match element {
        ChartElement::ChartArea => &mut chart.shape,
        ChartElement::PlotArea => &mut chart.plot_shape,
        ChartElement::Title => &mut chart.title.as_mut().ok_or("The chart has no title.")?.shape,
        ChartElement::Legend => {
            &mut chart
                .legend
                .as_mut()
                .ok_or("The chart has no legend.")?
                .shape
        }
        ChartElement::Axis { id } => {
            &mut chart
                .axis_mut(*id)
                .ok_or("That axis isn't in the chart.")?
                .shape
        }
        ChartElement::AxisTitle { id } => {
            &mut chart
                .axis_mut(*id)
                .and_then(|axis| axis.title.as_mut())
                .ok_or("That axis has no title.")?
                .shape
        }
        ChartElement::Gridlines { id } => {
            &mut chart
                .axis_mut(*id)
                .and_then(|axis| axis.major_gridlines.as_mut())
                .ok_or("That axis has no gridlines.")?
                .shape
        }
        ChartElement::Series { index } => &mut series_mut(chart, *index)?.shape,
        ChartElement::Point { index, point } => {
            let series = series_mut(chart, *index)?;
            if !series.points.iter().any(|p| p.index == *point) {
                series.points.push(PointLook {
                    index: *point,
                    ..PointLook::default()
                });
                series.points.sort_by_key(|p| p.index);
            }
            &mut series
                .points
                .iter_mut()
                .find(|p| p.index == *point)
                .ok_or("That point isn't in the series.")?
                .shape
        }
        ChartElement::DataLabels { .. } => {
            return Err("Data labels take a font, not a fill.".to_string())
        }
    })
}

fn text_mut<'a>(chart: &'a mut Chart, element: &ChartElement) -> Result<&'a mut TextLook, String> {
    Ok(match element {
        ChartElement::ChartArea => &mut chart.look,
        ChartElement::Title => &mut chart.title.as_mut().ok_or("The chart has no title.")?.look,
        ChartElement::Legend => {
            &mut chart
                .legend
                .as_mut()
                .ok_or("The chart has no legend.")?
                .look
        }
        ChartElement::Axis { id } => {
            &mut chart
                .axis_mut(*id)
                .ok_or("That axis isn't in the chart.")?
                .look
        }
        ChartElement::AxisTitle { id } => {
            &mut chart
                .axis_mut(*id)
                .and_then(|axis| axis.title.as_mut())
                .ok_or("That axis has no title.")?
                .look
        }
        ChartElement::DataLabels { index } => {
            &mut series_mut(chart, *index)?
                .labels
                .as_mut()
                .ok_or("That series has no data labels.")?
                .look
        }
        other => return Err(format!("The {} has no text.", kind_of(other))),
    })
}

fn label_position(name: &str) -> LabelPosition {
    match name {
        "center" => LabelPosition::Center,
        "insideEnd" => LabelPosition::InsideEnd,
        "insideBase" => LabelPosition::InsideBase,
        "outsideEnd" => LabelPosition::OutsideEnd,
        "bestFit" => LabelPosition::BestFit,
        "left" => LabelPosition::Left,
        "right" => LabelPosition::Right,
        "above" => LabelPosition::Top,
        "below" => LabelPosition::Bottom,
        _ => LabelPosition::Auto,
    }
}

fn legend_position(name: &str) -> LegendPosition {
    match name {
        "left" => LegendPosition::Left,
        "top" => LegendPosition::Top,
        "bottom" => LegendPosition::Bottom,
        "topRight" => LegendPosition::TopRight,
        _ => LegendPosition::Right,
    }
}

fn series_name(text: &str) -> Option<ChartText> {
    let text = text.trim().trim_start_matches('=');
    if text.is_empty() {
        return None;
    }
    if let Some(inner) = text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        return Some(ChartText::Rich(inner.replace("\"\"", "\"")));
    }
    if text.contains('!') {
        return Some(ChartText::Cell {
            formula: text.to_string(),
            cache: String::new(),
        });
    }
    Some(ChartText::Rich(text.to_string()))
}

/// Changes a chart's model as an edit says. Errors say why it can't.
pub fn apply_edit(
    chart: &mut Chart,
    edit: &ChartEdit,
    book: &cgs_model::Book,
    seed: u32,
) -> Result<(), String> {
    match edit {
        ChartEdit::Type { chart: chart_type } => {
            let mut series: Vec<Series> = chart.series().cloned().collect();
            series.sort_by_key(|s| s.order);
            let mut next = new_chart(*chart_type, series, seed);
            // Keeps the chart's own title, legend and look.
            next.axes = {
                let mut groups = next.groups.clone();
                let axes = axes_for(&mut groups, &chart.axes, seed);
                next.groups = groups;
                axes
            };
            next.title = chart.title.clone();
            next.auto_title_deleted = chart.auto_title_deleted;
            next.legend = chart.legend.clone().or(next.legend);
            next.shape = chart.shape.clone();
            next.plot_shape = chart.plot_shape.clone();
            next.look = chart.look.clone();
            next.style = chart.style;
            next.rounded_corners = chart.rounded_corners;
            next.plot_visible_only = chart.plot_visible_only;
            next.blanks = chart.blanks;
            *chart = next;
        }
        ChartEdit::SeriesType {
            index,
            chart: chart_type,
            secondary,
        } => {
            let kind = chart_type.kind();
            if kind.round() || chart.groups.first().is_some_and(|g| g.kind.round()) {
                return Err("A pie or doughnut can't share a chart with other types.".to_string());
            }
            let Some((from, at)) = chart.groups.iter().enumerate().find_map(|(g, group)| {
                group
                    .series
                    .iter()
                    .position(|s| s.index == *index)
                    .map(|at| (g, at))
            }) else {
                return Err("That series isn't in the chart any more.".to_string());
            };
            let mut series = chart.groups[from].series.remove(at);
            let (_, primary_value) = chart
                .groups
                .first()
                .and_then(|g| g.axes.get(1).map(|v| (g.axes[0], *v)))
                .unwrap_or((0, 0));
            let target = chart.groups.iter().position(|group| {
                group.kind.tag() == kind.tag()
                    && group.kind == kind
                    && (group.axes.get(1).copied() != Some(primary_value)) == *secondary
            });
            series_look(&kind, series.index as usize, &mut series);
            match target {
                Some(target) => {
                    chart.groups[target].series.push(series);
                    chart.groups[target].series.sort_by_key(|s| s.order);
                }
                None => chart.groups.push(ChartGroup {
                    kind: kind.clone(),
                    vary_colors: false,
                    series: vec![series],
                    axes: if *secondary {
                        vec![u32::MAX - 1, u32::MAX]
                    } else {
                        vec![0, primary_value]
                    },
                    labels: None,
                }),
            }
            chart.groups.retain(|group| !group.series.is_empty());
            let axes = axes_for(&mut chart.groups, &chart.axes, seed);
            chart.axes = axes;
        }
        ChartEdit::Title { text } => match text {
            None => {
                chart.title = None;
                chart.auto_title_deleted = true;
            }
            Some(text) => {
                let mut title = chart.title.clone().unwrap_or_else(automatic_title);
                title.text = (!text.is_empty()).then(|| ChartText::Rich(text.clone()));
                chart.title = Some(title);
                chart.auto_title_deleted = false;
            }
        },
        ChartEdit::AxisTitle { id, text } => {
            let axis = chart.axis_mut(*id).ok_or("That axis isn't in the chart.")?;
            axis.title = text.as_ref().map(|text| {
                let vertical = matches!(axis.position, AxisPosition::Left | AxisPosition::Right);
                let mut title = axis.title.clone().unwrap_or(ChartTitle {
                    text: None,
                    overlay: false,
                    look: TextLook {
                        bold: Some(false),
                        rotation: vertical.then_some(-5_400_000),
                        ..text_look(1000)
                    },
                    shape: ShapeLook {
                        fill: Some(ChartFill::None),
                        line: Some(no_line()),
                    },
                });
                title.text = Some(ChartText::Rich(if text.is_empty() {
                    "Axis Title".to_string()
                } else {
                    text.clone()
                }));
                title
            });
        }
        ChartEdit::Legend { position } => {
            chart.legend = position.as_ref().map(|position| {
                let mut legend_now = chart.legend.clone().unwrap_or_else(legend);
                legend_now.position = legend_position(position);
                legend_now
            });
        }
        ChartEdit::DataLabels {
            index,
            show,
            position,
            value,
            category,
            series,
            percent,
        } => {
            let round = chart.groups.first().is_some_and(|g| g.kind.round());
            for group in &mut chart.groups {
                for one in &mut group.series {
                    if index.is_some_and(|index| index != one.index) {
                        continue;
                    }
                    if !show {
                        one.labels = None;
                        continue;
                    }
                    let mut labels = one.labels.clone().unwrap_or(DataLabels {
                        value: !round || percent.is_none(),
                        look: TextLook {
                            color: Some(grey(75_000, 25_000)),
                            ..text_look(900)
                        },
                        ..DataLabels::default()
                    });
                    labels.deleted = false;
                    if let Some(position) = position {
                        labels.position = label_position(position);
                    }
                    if let Some(on) = value {
                        labels.value = *on;
                    }
                    if let Some(on) = category {
                        labels.category = *on;
                    }
                    if let Some(on) = series {
                        labels.series = *on;
                    }
                    if let Some(on) = percent {
                        labels.percent = *on;
                    }
                    one.labels = Some(labels);
                }
            }
        }
        ChartEdit::Gridlines { id, major, minor } => {
            let axis = chart.axis_mut(*id).ok_or("That axis isn't in the chart.")?;
            axis.major_gridlines = major.then(|| {
                axis.major_gridlines.clone().unwrap_or(Gridlines {
                    shape: ShapeLook {
                        fill: None,
                        line: Some(thin(grey(15_000, 85_000))),
                    },
                })
            });
            axis.minor_gridlines = minor.then(|| {
                axis.minor_gridlines.clone().unwrap_or(Gridlines {
                    shape: ShapeLook {
                        fill: None,
                        line: Some(thin(grey(5_000, 95_000))),
                    },
                })
            });
        }
        ChartEdit::AxisVisible { id, visible } => {
            let axis = chart.axis_mut(*id).ok_or("That axis isn't in the chart.")?;
            axis.deleted = !visible;
        }
        ChartEdit::AxisScale {
            id,
            min,
            max,
            major,
            minor,
            reversed,
            format,
        } => {
            let axis = chart.axis_mut(*id).ok_or("That axis isn't in the chart.")?;
            if let (Some(min), Some(max)) = (min, max) {
                if min >= max {
                    return Err("The minimum must be less than the maximum.".to_string());
                }
            }
            if major.is_some_and(|u| u <= 0.0) || minor.is_some_and(|u| u <= 0.0) {
                return Err("Units must be greater than zero.".to_string());
            }
            if axis.kind == AxisKind::Value {
                axis.min = *min;
                axis.max = *max;
                axis.major_unit = *major;
                axis.minor_unit = *minor;
            }
            axis.reversed = *reversed;
            axis.format = Some(match format {
                Some(code) => (code.clone(), false),
                None => ("General".to_string(), true),
            });
        }
        ChartEdit::SwitchRowCol => switch_row_col(chart, book)?,
        ChartEdit::Range { .. } => unreachable!("handled by the session"),
        ChartEdit::Series { list } => {
            if list.is_empty() {
                return Err("A chart needs at least one series.".to_string());
            }
            let kind = chart
                .groups
                .first()
                .map(|g| g.kind.clone())
                .ok_or("The chart has no series.")?;
            let scatter = matches!(kind, GroupKind::Scatter { .. });
            let old: Vec<Series> = chart.series().cloned().collect();
            let mut series = Vec::new();
            for (at, spec) in list.iter().enumerate() {
                if crate::charts::resolve(book, &spec.values, 0).is_none() {
                    return Err(format!(
                        "{} isn't a reference to cells in this workbook.",
                        spec.values
                    ));
                }
                let mut one = old.get(at).cloned().unwrap_or_default();
                one.index = at as u32;
                one.order = at as u32;
                one.name = spec.name.as_deref().and_then(series_name);
                one.values = Some(DataSource {
                    formula: Some(spec.values.trim().trim_start_matches('=').to_string()),
                    kind: SourceKind::Number,
                    format: Some("General".to_string()),
                    ..DataSource::default()
                });
                one.categories = spec
                    .categories
                    .as_ref()
                    .filter(|c| !c.trim().is_empty())
                    .map(|c| DataSource {
                        formula: Some(c.trim().trim_start_matches('=').to_string()),
                        kind: if scatter {
                            SourceKind::Number
                        } else {
                            SourceKind::Text
                        },
                        ..DataSource::default()
                    });
                if at >= old.len() {
                    series_look(&kind, at, &mut one);
                }
                series.push(one);
            }
            // Series stay in the groups they were in; new ones join the first.
            let mut groups: Vec<ChartGroup> = chart
                .groups
                .iter()
                .map(|g| ChartGroup {
                    series: Vec::new(),
                    ..g.clone()
                })
                .collect();
            for one in series {
                let home = chart
                    .groups
                    .iter()
                    .position(|g| g.series.iter().any(|s| s.index == one.index))
                    .unwrap_or(0);
                groups[home].series.push(one);
            }
            groups.retain(|g| !g.series.is_empty());
            chart.groups = groups;
            if let Some(group) = chart.groups.first_mut() {
                if group.kind.round() {
                    group.series.truncate(1);
                    let kind = group.kind.clone();
                    if let Some(one) = group.series.first_mut() {
                        series_look(&kind, 0, one);
                    }
                }
            }
        }
        ChartEdit::Style { style } => chart_style(chart, *style)?,
        ChartEdit::Colors { palette } => recolor(chart, *palette)?,
        ChartEdit::QuickLayout { layout } => quick_layout(chart, *layout)?,
        ChartEdit::Fill {
            element,
            color,
            none,
        } => {
            let fill = match color.as_ref().and_then(ChartColor::dml) {
                Some(color) => Some(ChartFill::Solid(color)),
                None if *none => Some(ChartFill::None),
                None => None,
            };
            let kinds_with_lines = matches!(element, ChartElement::Series { .. })
                && chart.groups.iter().any(|g| {
                    matches!(g.kind, GroupKind::Line { .. } | GroupKind::Scatter { .. })
                        && g.series.iter().any(|s| matches!(element, ChartElement::Series { index } if *index == s.index))
                });
            if kinds_with_lines {
                // A line series' colour is its line's, and its markers'.
                if let ChartElement::Series { index } = element {
                    let series = series_mut(chart, *index)?;
                    if let Some(ChartFill::Solid(color)) = &fill {
                        let line = series.shape.line.get_or_insert_with(ChartLine::default);
                        line.fill = Some(ChartFill::Solid(color.clone()));
                        if let Some(marker) = series.marker.as_mut() {
                            marker.shape.fill = Some(ChartFill::Solid(color.clone()));
                            marker.shape.line = Some(thin(color.clone()));
                        }
                    }
                }
            } else {
                shape_mut(chart, element)?.fill = fill;
            }
        }
        ChartEdit::Outline {
            element,
            color,
            none,
            width,
            dash,
        } => {
            let shape = shape_mut(chart, element)?;
            if *none {
                shape.line = Some(no_line());
            } else {
                let line = shape.line.get_or_insert_with(ChartLine::default);
                if let Some(color) = color.as_ref().and_then(ChartColor::dml) {
                    line.fill = Some(ChartFill::Solid(color));
                }
                if let Some(width) = width {
                    line.width = Some((width * 12700.0).round() as i64);
                }
                if let Some(dash) = dash {
                    line.dash = Some(Dash::from_name(dash));
                }
            }
        }
        ChartEdit::Font {
            element,
            size,
            bold,
            italic,
            color,
        } => {
            let look = text_mut(chart, element)?;
            if let Some(size) = size {
                if !(1.0..=400.0).contains(size) {
                    return Err("Font sizes go from 1 to 400 points.".to_string());
                }
                look.size = Some((size * 100.0).round() as u32);
            }
            if let Some(bold) = bold {
                look.bold = Some(*bold);
            }
            if let Some(italic) = italic {
                look.italic = Some(*italic);
            }
            if let Some(color) = color.as_ref().and_then(ChartColor::dml) {
                look.color = Some(color);
            }
            // A typed title keeps its look in its runs, which the writer writes from this.
        }
        ChartEdit::GapWidth {
            group,
            gap,
            overlap,
        } => {
            let group = chart
                .groups
                .get_mut(*group)
                .ok_or("That chart group isn't there.")?;
            match &mut group.kind {
                GroupKind::Bar {
                    gap_width,
                    overlap: bar_overlap,
                    grouping,
                    ..
                } => {
                    *gap_width = (*gap).min(500);
                    if let Some(overlap) = overlap {
                        if !grouping.stacked() {
                            *bar_overlap = (*overlap).clamp(-100, 100);
                        }
                    }
                }
                _ => return Err("Only bars and columns have a gap width.".to_string()),
            }
        }
    }
    Ok(())
}

/// Swaps what is a series and what is a category, as Excel's Switch Row/Column does, for a chart
/// whose series read one rectangle of cells.
fn switch_row_col(chart: &mut Chart, book: &cgs_model::Book) -> Result<(), String> {
    let refusal = || {
        "This chart's data isn't one block of cells, so its rows and columns can't be switched."
            .to_string()
    };
    let series: Vec<&Series> = chart.series().collect();
    let mut top = u32::MAX;
    let mut left = u32::MAX;
    let mut bottom = 0;
    let mut right = 0;
    let mut sheet = None;
    let mut by_columns = None;
    let mut include = |formula: &str,
                       top: &mut u32,
                       left: &mut u32,
                       bottom: &mut u32,
                       right: &mut u32|
     -> Result<(), String> {
        let areas = crate::charts::resolve(book, formula, 0).ok_or_else(refusal)?;
        for (index, rect) in areas {
            if sheet.is_some_and(|s| s != index) {
                return Err(refusal());
            }
            sheet = Some(index);
            *top = (*top).min(rect.top);
            *left = (*left).min(rect.left);
            *bottom = (*bottom).max(rect.bottom);
            *right = (*right).max(rect.right);
        }
        Ok(())
    };
    for one in &series {
        let values = one
            .values
            .as_ref()
            .and_then(|v| v.formula.as_deref())
            .ok_or_else(refusal)?;
        let areas = crate::charts::resolve(book, values, 0).ok_or_else(refusal)?;
        if let Some((_, rect)) = areas.first() {
            by_columns.get_or_insert(rect.left == rect.right && rect.top != rect.bottom);
        }
        include(values, &mut top, &mut left, &mut bottom, &mut right)?;
        if let Some(ChartText::Cell { formula, .. }) = &one.name {
            include(formula, &mut top, &mut left, &mut bottom, &mut right)?;
        }
        if let Some(formula) = one.categories.as_ref().and_then(|c| c.formula.as_deref()) {
            include(formula, &mut top, &mut left, &mut bottom, &mut right)?;
        }
    }
    let sheet = sheet.ok_or_else(refusal)?;
    let first = series.first().ok_or_else(refusal)?;
    let by_columns = by_columns.unwrap_or(true);
    let plan = Plan {
        by_columns: !by_columns,
        header_row: if by_columns {
            first.name.is_some()
        } else {
            first.categories.is_some()
        },
        header_col: if by_columns {
            first.categories.is_some()
        } else {
            first.name.is_some()
        },
    };
    let area = Area {
        top,
        left,
        bottom,
        right,
    };
    let name = &book.sheets[sheet as usize].name;
    let scatter = chart
        .groups
        .first()
        .is_some_and(|g| matches!(g.kind, GroupKind::Scatter { .. }));
    let fresh = series_from(name, area, plan, scatter);
    replace_series(chart, fresh);
    Ok(())
}

/// New series in the chart's first group, each in that group's look, keeping the chart's type.
fn replace_series(chart: &mut Chart, fresh: Vec<Series>) {
    let Some(kind) = chart.groups.first().map(|g| g.kind.clone()) else {
        return;
    };
    let mut fresh = fresh;
    for (index, one) in fresh.iter_mut().enumerate() {
        series_look(&kind, index, one);
    }
    if kind.round() {
        fresh.truncate(1);
    }
    chart.groups.truncate(1);
    chart.groups[0].series = fresh;
    if chart.series_count() > 1 && chart.legend.is_none() {
        chart.legend = Some(legend());
    }
}

/// Change Colors: Excel 2016's colourful sets and one-accent sets, written into each series.
fn recolor(chart: &mut Chart, palette: u8) -> Result<(), String> {
    if !(1..=17).contains(&palette) {
        return Err("There is no such colour set.".to_string());
    }
    let color = |index: usize, count: usize| -> DmlColor {
        match palette {
            1 => accent(index),
            2 => DmlColor::scheme(&format!("accent{}", (index * 2) % 6 + 1)),
            3 => DmlColor::scheme(&format!("accent{}", (index * 2 + 1) % 6 + 1)),
            4 => DmlColor::scheme(&format!("accent{}", 6 - index % 6)),
            _ => {
                let accent_number = (palette - 5) % 6 + 1;
                let share = if count <= 1 {
                    0.0
                } else {
                    index as f64 / (count - 1) as f64
                };
                let base = DmlColor::scheme(&format!("accent{accent_number}"));
                if palette >= 11 {
                    base.with(ColorModKind::Tint, (100_000.0 - share * 60_000.0) as i32)
                } else {
                    base.with(ColorModKind::Shade, (100_000.0 - share * 50_000.0) as i32)
                }
            }
        }
    };
    let count = chart.series_count();
    let mut number = 0;
    for group in &mut chart.groups {
        let round = group.kind.round();
        let line = matches!(
            group.kind,
            GroupKind::Line { .. } | GroupKind::Scatter { .. }
        );
        for one in &mut group.series {
            if round {
                let points = one.points.len();
                for (at, point) in one.points.iter_mut().enumerate() {
                    point.shape.fill = Some(ChartFill::Solid(color(at, points)));
                }
            } else if line {
                let c = color(number, count);
                if let Some(line) = one.shape.line.as_mut() {
                    line.fill = Some(ChartFill::Solid(c.clone()));
                }
                if let Some(marker) = one
                    .marker
                    .as_mut()
                    .filter(|m| m.symbol != MarkerSymbol::None)
                {
                    marker.shape.fill = Some(ChartFill::Solid(c.clone()));
                    marker.shape.line = Some(thin(c));
                }
            } else {
                one.shape.fill = Some(ChartFill::Solid(color(number, count)));
            }
            number += 1;
        }
    }
    Ok(())
}

/// Chart Styles: the looks of the gallery's first eight, written into the chart's own elements.
fn chart_style(chart: &mut Chart, style: u8) -> Result<(), String> {
    if !(1..=8).contains(&style) {
        return Err("Chart styles go from 1 to 8.".to_string());
    }
    let dark = matches!(style, 6 | 8);
    let text = if dark {
        DmlColor::scheme("bg1")
    } else {
        grey(65_000, 35_000)
    };
    let grid = match style {
        5 => DmlColor::scheme("bg1"),
        6 => grey(75_000, 25_000).with(ColorModKind::Alpha, 50_000),
        8 => grey(85_000, 15_000),
        _ => grey(15_000, 85_000),
    };
    chart.shape = ShapeLook {
        fill: Some(ChartFill::Solid(match style {
            6 => grey(65_000, 35_000),
            7 => DmlColor::scheme("accent1")
                .with(ColorModKind::LumMod, 20_000)
                .with(ColorModKind::LumOff, 80_000),
            8 => DmlColor::scheme("tx1"),
            _ => DmlColor::scheme("bg1"),
        })),
        line: Some(if style == 4 {
            no_line()
        } else {
            thin(grey(15_000, 85_000))
        }),
    };
    chart.plot_shape = ShapeLook {
        fill: Some(if style == 5 {
            ChartFill::Solid(DmlColor::scheme("bg1").with(ColorModKind::LumMod, 95_000))
        } else {
            ChartFill::None
        }),
        line: Some(no_line()),
    };
    let recolor = |look: &mut TextLook| look.color = Some(text.clone());
    if let Some(title) = chart.title.as_mut() {
        recolor(&mut title.look);
        title.look.bold = Some(style == 3);
    }
    if let Some(legend) = chart.legend.as_mut() {
        recolor(&mut legend.look);
    }
    let labels_on = matches!(style, 2 | 4);
    for axis in &mut chart.axes {
        recolor(&mut axis.look);
        if let Some(title) = axis.title.as_mut() {
            recolor(&mut title.look);
        }
        if axis.kind == AxisKind::Value {
            axis.major_gridlines = (!matches!(style, 2 | 3)).then(|| Gridlines {
                shape: ShapeLook {
                    fill: None,
                    line: Some(ChartLine {
                        dash: (style == 7).then_some(Dash::SysDash),
                        ..thin(grid.clone())
                    }),
                },
            });
            // Style 2 labels the bars instead of the axis.
            axis.deleted = style == 2 && !axis.deleted && axis.position != AxisPosition::Right
                || (axis.deleted && axis.position == AxisPosition::Right);
        }
    }
    for group in &mut chart.groups {
        if let GroupKind::Bar {
            gap_width,
            grouping,
            ..
        } = &mut group.kind
        {
            *gap_width = match style {
                3 => 50,
                _ if grouping.stacked() => 150,
                _ => 219,
            };
        }
        for series in &mut group.series {
            series.labels = labels_on.then(|| DataLabels {
                value: true,
                position: if style == 4 {
                    LabelPosition::InsideEnd
                } else {
                    LabelPosition::Auto
                },
                look: TextLook {
                    color: Some(if style == 4 {
                        DmlColor::scheme("bg1")
                    } else {
                        text.clone()
                    }),
                    ..text_look(900)
                },
                ..DataLabels::default()
            });
        }
    }
    chart.style = 2;
    Ok(())
}

/// Quick Layout's eleven layouts: which of title, legend, axis titles, labels and gridlines show.
fn quick_layout(chart: &mut Chart, layout: u8) -> Result<(), String> {
    if !(1..=11).contains(&layout) {
        return Err("Quick layouts go from 1 to 11.".to_string());
    }
    // (title, legend, axis titles, value labels, major gridlines)
    let (title, legend_at, axis_titles, labels, gridlines) = match layout {
        1 => (true, Some("right"), false, false, true),
        2 => (true, Some("top"), false, true, false),
        3 => (true, Some("bottom"), false, false, true),
        4 => (false, Some("bottom"), false, true, false),
        5 => (true, None, true, false, true),
        6 => (true, None, true, false, true),
        7 => (false, Some("right"), true, false, true),
        8 => (true, None, true, false, false),
        9 => (true, Some("right"), true, false, true),
        10 => (true, Some("right"), false, true, true),
        _ => (false, Some("right"), false, false, true),
    };
    let book = cgs_model::Book::default();
    apply_edit(
        chart,
        &ChartEdit::Title {
            text: title.then(String::new),
        },
        &book,
        0,
    )?;
    apply_edit(
        chart,
        &ChartEdit::Legend {
            position: legend_at.map(str::to_string),
        },
        &book,
        0,
    )?;
    apply_edit(
        chart,
        &ChartEdit::DataLabels {
            index: None,
            show: labels,
            position: None,
            value: Some(true),
            category: None,
            series: None,
            percent: None,
        },
        &book,
        0,
    )?;
    let ids: Vec<(u32, AxisKind, bool)> = chart
        .axes
        .iter()
        .map(|a| (a.id, a.kind, a.deleted))
        .collect();
    for (id, kind, deleted) in ids {
        if deleted {
            continue;
        }
        apply_edit(
            chart,
            &ChartEdit::AxisTitle {
                id,
                text: axis_titles.then(String::new),
            },
            &book,
            0,
        )?;
        if kind == AxisKind::Value {
            apply_edit(
                chart,
                &ChartEdit::Gridlines {
                    id,
                    major: gridlines,
                    minor: false,
                },
                &book,
                0,
            )?;
        }
    }
    Ok(())
}

/// Rewrites every formula a chart reads. Returns whether any changed.
pub fn rewrite_formulas(chart: &mut Chart, change: &dyn Fn(&str) -> Option<String>) -> bool {
    let mut any = false;
    let mut text = |value: &mut Option<ChartText>| {
        if let Some(ChartText::Cell { formula, .. }) = value {
            if let Some(next) = change(formula) {
                *formula = next;
                any = true;
            }
        }
    };
    if let Some(title) = chart.title.as_mut() {
        text(&mut title.text);
    }
    for axis in &mut chart.axes {
        if let Some(title) = axis.title.as_mut() {
            text(&mut title.text);
        }
    }
    for group in &mut chart.groups {
        for series in &mut group.series {
            text(&mut series.name);
        }
    }
    let sources = |source: &mut Option<DataSource>| {
        if let Some(formula) = source.as_mut().and_then(|s| s.formula.as_mut()) {
            if let Some(next) = change(formula) {
                *formula = next;
                return true;
            }
        }
        false
    };
    for group in &mut chart.groups {
        for series in &mut group.series {
            any |= sources(&mut series.categories);
            any |= sources(&mut series.values);
        }
    }
    any
}

// ---------------------------------------------------------------------------------------------
// Commands

/// A source's saved copy, from the points its cells hold now.
fn cache_of(points: &[crate::charts::Point], number: bool) -> (Vec<CachePoint>, Option<String>) {
    let mut cache = Vec::new();
    for (index, point) in points.iter().enumerate() {
        let value = if number {
            match &point.value {
                CalcValue::Number(value) => cgs_xlsx::patch_sheet::number_text(*value),
                _ => continue,
            }
        } else {
            if point.text.is_empty() {
                continue;
            }
            point.text.clone()
        };
        cache.push(CachePoint {
            index: index as u32,
            value,
        });
    }
    let format = number.then(|| {
        points
            .first()
            .map_or_else(|| "General".to_string(), |p| p.format.clone())
    });
    (cache, format)
}

impl Session {
    /// Brings each chart's saved copy of its data up to what its cells hold, as Excel writes it,
    /// so that programs that read the copy show the same. A chart whose copy is current is left
    /// alone, and so is one whose cells aren't in this workbook.
    pub(crate) fn refresh_chart_caches(&mut self, options: &cgs_numfmt::Options<'_>) {
        let mut updates: Vec<(u32, Box<Chart>)> = Vec::new();
        {
            let reader = crate::charts::Reader {
                book: &self.book,
                calc: &self.calc,
                options,
            };
            for (index, part) in self.book.charts.iter().enumerate() {
                let ChartBody::Classic(chart) = &part.body else {
                    continue;
                };
                let mut next = (**chart).clone();
                let refresh = |source: &mut DataSource| {
                    let Some(formula) = source.formula.clone() else {
                        return;
                    };
                    if source.kind == SourceKind::MultiLevel {
                        return;
                    }
                    let Some(points) = reader.read(&formula, false) else {
                        return;
                    };
                    let number = source.kind == SourceKind::Number;
                    let (cache, format) = cache_of(&points, number);
                    source.count = points.len() as u32;
                    source.cache = cache;
                    if number {
                        source.format = format;
                    }
                };
                for group in &mut next.groups {
                    for series in &mut group.series {
                        if let Some(source) = series.categories.as_mut() {
                            refresh(source);
                        }
                        if let Some(source) = series.values.as_mut() {
                            refresh(source);
                        }
                        if let Some(ChartText::Cell { formula, cache }) = series.name.as_mut() {
                            if let Some(points) = reader.read(formula, false) {
                                let text: Vec<&str> = points
                                    .iter()
                                    .map(|p| p.text.as_str())
                                    .filter(|t| !t.is_empty())
                                    .collect();
                                *cache = text.join(" ");
                            }
                        }
                    }
                }
                if next != **chart {
                    updates.push((index as u32, Box::new(next)));
                }
            }
        }
        for (index, chart) in updates {
            if let Some(part) = self.book.charts.get_mut(index as usize) {
                part.body = ChartBody::Classic(chart);
            }
            self.mark_chart(index);
        }
    }

    fn drawings_op(
        &mut self,
        sheet: u32,
        drawings: Vec<Drawing>,
        label: &'static str,
    ) -> Result<Applied, String> {
        self.perform(label, Op::SheetDrawings { sheet, drawings }, None)
    }

    /// Moves or sizes a drawing, in CSS pixels at 100%.
    pub(crate) fn set_drawing_box(
        &mut self,
        sheet: u32,
        index: u32,
        (x, y, width, height): (i64, i64, i64, i64),
    ) -> Result<Applied, String> {
        let info = self.sheet_for_edit(sheet)?;
        let mut drawings = info.drawings.clone();
        let drawing = drawings
            .get_mut(index as usize)
            .ok_or("That picture or chart isn't on the sheet any more.")?;
        drawing.anchor = placed(info, drawing.anchor, x, y, width, height);
        self.drawings_op(sheet, drawings, "move")
    }

    pub(crate) fn delete_drawings(
        &mut self,
        sheet: u32,
        indexes: &[u32],
    ) -> Result<Applied, String> {
        let info = self.sheet_for_edit(sheet)?;
        let mut drawings = info.drawings.clone();
        let mut gone: Vec<usize> = indexes
            .iter()
            .map(|&i| i as usize)
            .filter(|&i| i < drawings.len())
            .collect();
        if gone.is_empty() {
            return Ok(Applied::default());
        }
        gone.sort_unstable();
        gone.dedup();
        for at in gone.into_iter().rev() {
            drawings.remove(at);
        }
        self.drawings_op(sheet, drawings, "delete")
    }

    /// A new name for a drawing, as Excel numbers them: `Chart 1`, `Picture 2`, by the next id.
    fn new_drawing(info: &SheetInfo, kind: DrawingKind, anchor: Anchor) -> Drawing {
        let id = info.drawings.iter().map(|d| d.id).max().unwrap_or(1) + 1;
        let label = match kind {
            DrawingKind::Chart => "Chart",
            DrawingKind::Picture => "Picture",
            _ => "Shape",
        };
        let mut drawing = Drawing::new(kind, format!("{label} {}", id - 1), anchor);
        drawing.id = id;
        drawing
    }

    /// Kinds of cell over an area, for telling headings from data.
    fn cell_kinds(&self, sheet: u32, area: Area) -> Vec<Cell> {
        struct Kinds<'a> {
            area: Area,
            info: &'a SheetInfo,
            styles: &'a cgs_model::StyleTable,
            out: Vec<Cell>,
        }
        impl CellSink for Kinds<'_> {
            fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
                let at =
                    ((row - self.area.top) * self.area.cols() + (col - self.area.left)) as usize;
                let kind = match value {
                    CalcValue::Empty => Cell::Empty,
                    CalcValue::Number(_) => {
                        let id = self.styles.xf(self.info.style_at(row, col)).num_fmt;
                        let date = self
                            .styles
                            .num_fmts
                            .get(&id)
                            .map(|code| cgs_numfmt::NumFmt::parse(code))
                            .or_else(|| cgs_numfmt::NumFmt::builtin(id))
                            .is_some_and(|format| format.is_date());
                        if date {
                            Cell::Date
                        } else {
                            Cell::Number
                        }
                    }
                    _ => Cell::Text,
                };
                if let Some(slot) = self.out.get_mut(at) {
                    *slot = kind;
                }
            }
        }
        let info = &self.book.sheets[sheet as usize];
        let mut kinds = Kinds {
            area,
            info,
            styles: &self.book.styles,
            out: vec![Cell::Empty; (area.rows() * area.cols()) as usize],
        };
        self.calc.read_block(
            SheetRect {
                sheet,
                rect: CalcRect {
                    top: area.top,
                    left: area.left,
                    bottom: area.bottom,
                    right: area.right,
                },
            },
            &mut kinds,
        );
        kinds.out
    }

    /// Series for a chart over an area, as Excel reads the selection.
    fn series_for(
        &self,
        sheet: u32,
        area: Area,
        scatter: bool,
        by_columns: Option<bool>,
    ) -> Result<Vec<Series>, String> {
        if !area.valid() {
            return Err("Select the cells to chart first.".to_string());
        }
        if u64::from(area.rows()) * u64::from(area.cols()) > 4_000_000 {
            return Err("That's too many cells for one chart.".to_string());
        }
        let cells = self.cell_kinds(sheet, area);
        if cells.iter().all(|cell| *cell == Cell::Empty) {
            return Err("The selected cells are empty, so there's nothing to chart.".to_string());
        }
        let plan = plan(&cells, area.rows(), area.cols(), by_columns);
        let name = self.book.sheets[sheet as usize].name.clone();
        let series = series_from(&name, area, plan, scatter);
        if series.is_empty() {
            return Err("The selection has headings but no data to chart.".to_string());
        }
        if series.len() > 255 {
            return Err("A chart can hold at most 255 series.".to_string());
        }
        Ok(series)
    }

    /// Insert › Chart: a chart of the selection at a place on the sheet, in CSS pixels at 100%.
    pub(crate) fn insert_chart(
        &mut self,
        sheet: u32,
        area: Area,
        chart_type: ChartType,
        place: (i64, i64),
    ) -> Result<Applied, String> {
        let info = self.sheet_for_edit(sheet)?;
        if info.kind != cgs_model::SheetKind::Worksheet {
            return Err("Charts go on worksheets.".to_string());
        }
        let area = if area.rows() == 1 && area.cols() == 1 {
            self.current_region(sheet, area.top, area.left)
        } else {
            area
        };
        let series = self.series_for(sheet, area, chart_type.scatter(), None)?;
        let seed = self.book.charts.len() as u32 * 7 + sheet * 131 + area.top * 31 + area.left;
        let chart = new_chart(chart_type, series, seed);
        let info = &self.book.sheets[sheet as usize];
        let (x, y) = place;
        let anchor = placed(
            info,
            Anchor::TwoCell {
                from: AnchorPoint::default(),
                to: AnchorPoint::default(),
            },
            x,
            y,
            NEW_CHART_PX.0,
            NEW_CHART_PX.1,
        );
        let mut drawing = Self::new_drawing(info, DrawingKind::Chart, anchor);
        let index = self.book.charts.len() as u32;
        drawing.chart = Some(index);
        let mut drawings = info.drawings.clone();
        drawings.push(drawing);
        let part = ChartPart {
            part: None,
            body: ChartBody::Classic(Box::new(chart)),
            changed: true,
        };
        self.perform(
            "chart",
            Op::Many(vec![
                Op::ChartPart {
                    index,
                    part: Some(Box::new(part)),
                },
                Op::SheetDrawings { sheet, drawings },
            ]),
            None,
        )
    }

    /// Insert › Pictures: a picture already added with `add_media`, at its own size.
    pub(crate) fn insert_picture(
        &mut self,
        sheet: u32,
        part: &str,
        (x, y, width, height): (i64, i64, i64, i64),
        name: &str,
    ) -> Result<Applied, String> {
        let info = self.sheet_for_edit(sheet)?;
        if !self.edits.media.contains_key(part) {
            return Err("That picture couldn't be read.".to_string());
        }
        let anchor = placed(
            info,
            Anchor::OneCell {
                from: AnchorPoint::default(),
                width: 0,
                height: 0,
            },
            x,
            y,
            width,
            height,
        );
        let mut drawing = Self::new_drawing(info, DrawingKind::Picture, anchor);
        drawing.image = Some(part.to_string());
        drawing.description = name.to_string();
        let mut drawings = info.drawings.clone();
        drawings.push(drawing);
        self.drawings_op(sheet, drawings, "picture")
    }

    /// Move Chart: the drawing goes to another worksheet, at the same place.
    pub(crate) fn move_drawing_to(
        &mut self,
        sheet: u32,
        index: u32,
        to: u32,
    ) -> Result<Applied, String> {
        if sheet == to {
            return Ok(Applied::default());
        }
        let target = self.sheet_for_edit(to)?;
        if target.deleted || target.kind != cgs_model::SheetKind::Worksheet {
            return Err("A chart can only move to a worksheet.".to_string());
        }
        let mut target_drawings = target.drawings.clone();
        let next_id = target_drawings.iter().map(|d| d.id).max().unwrap_or(1) + 1;
        let mut from = self.sheet_for_edit(sheet)?.drawings.clone();
        if index as usize >= from.len() {
            return Err("That chart isn't on the sheet any more.".to_string());
        }
        let mut drawing = from.remove(index as usize);
        drawing.source = None;
        drawing.id = next_id;
        target_drawings.push(drawing);
        self.perform(
            "move chart",
            Op::Many(vec![
                Op::SheetDrawings {
                    sheet,
                    drawings: from,
                },
                Op::SheetDrawings {
                    sheet: to,
                    drawings: target_drawings,
                },
            ]),
            None,
        )
    }

    /// A Chart Design or Format change to a chart on a sheet.
    pub(crate) fn edit_chart(
        &mut self,
        sheet: u32,
        drawing: u32,
        edit: &ChartEdit,
    ) -> Result<Applied, String> {
        let info = self.sheet_for_edit(sheet)?;
        let index = info
            .drawings
            .get(drawing as usize)
            .and_then(|d| d.chart)
            .ok_or("That chart isn't on the sheet any more.")?;
        let part = self
            .book
            .charts
            .get(index as usize)
            .ok_or("That chart can't be found.")?;
        let ChartBody::Classic(chart) = &part.body else {
            return Err(
                "This chart is from a newer Excel, so it can be moved and sized but not edited."
                    .to_string(),
            );
        };
        let mut chart = (**chart).clone();
        if !chart.editable() && !matches!(edit, ChartEdit::Type { .. }) {
            return Err(
                "This kind of chart is drawn but can't be edited. Change its type to edit it."
                    .to_string(),
            );
        }
        let seed = index * 7 + sheet * 131;
        match edit {
            ChartEdit::Range {
                sheet: data_sheet,
                area,
            } => {
                let scatter = chart
                    .groups
                    .first()
                    .is_some_and(|g| matches!(g.kind, GroupKind::Scatter { .. }));
                let fresh = self.series_for(*data_sheet, *area, scatter, None)?;
                replace_series(&mut chart, fresh);
            }
            ChartEdit::Series { list } => {
                // References typed without a sheet are the chart's own sheet's, as in Excel; a
                // chart part must name the sheet.
                let home = self.book.sheets[sheet as usize].name.clone();
                let qualify = |text: &str| {
                    let text = text.trim().trim_start_matches('=');
                    if text.starts_with('"') {
                        text.to_string()
                    } else {
                        cgs_refs::qualify_refs(text, &home).into_owned()
                    }
                };
                let list = list
                    .iter()
                    .map(|spec| SeriesSpec {
                        name: spec.name.as_deref().map(qualify),
                        categories: spec.categories.as_deref().map(qualify),
                        values: qualify(&spec.values),
                    })
                    .collect();
                apply_edit(&mut chart, &ChartEdit::Series { list }, &self.book, seed)?
            }
            other => apply_edit(&mut chart, other, &self.book, seed)?,
        }
        let part = ChartPart {
            part: part.part.clone(),
            body: ChartBody::Classic(Box::new(chart)),
            changed: true,
        };
        self.perform(
            "chart",
            Op::ChartPart {
                index,
                part: Some(Box::new(part)),
            },
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(rows: &[&str]) -> (Vec<Cell>, u32, u32) {
        let cols = rows[0].len() as u32;
        let cells = rows
            .iter()
            .flat_map(|row| {
                row.chars().map(|ch| match ch {
                    '.' => Cell::Empty,
                    'n' => Cell::Number,
                    'd' => Cell::Date,
                    _ => Cell::Text,
                })
            })
            .collect();
        (cells, rows.len() as u32, cols)
    }

    #[test]
    fn headings_are_told_from_data() {
        // A blank corner: both headings.
        let (cells, rows, cols) = kinds(&[".tt", "tnn", "tnn", "tnn"]);
        assert_eq!(
            plan(&cells, rows, cols, None),
            Plan {
                by_columns: true,
                header_row: true,
                header_col: true
            }
        );
        // Years as numbers under a text corner: still a heading row, as no number is in it.
        let (cells, rows, cols) = kinds(&["ttt", "tnn"]);
        let found = plan(&cells, rows, cols, None);
        assert!(found.header_row && found.header_col);
        // Numbers only: no headings, series down the longer side.
        let (cells, rows, cols) = kinds(&["nnn", "nnn", "nnn", "nnn"]);
        assert_eq!(
            plan(&cells, rows, cols, None),
            Plan {
                by_columns: true,
                header_row: false,
                header_col: false
            }
        );
        let (cells, rows, cols) = kinds(&["nnnnn", "nnnnn"]);
        assert!(!plan(&cells, rows, cols, None).by_columns);
        // Dates down the first column are its headings.
        let (cells, rows, cols) = kinds(&[".t", "dn", "dn"]);
        assert!(plan(&cells, rows, cols, None).header_col);
    }

    #[test]
    fn series_name_their_cells() {
        let area = Area {
            top: 1,
            left: 1,
            bottom: 4,
            right: 3,
        };
        let series = series_from(
            "My Sheet",
            area,
            Plan {
                by_columns: true,
                header_row: true,
                header_col: true,
            },
            false,
        );
        assert_eq!(series.len(), 2);
        assert_eq!(
            series[0].values.as_ref().unwrap().formula.as_deref(),
            Some("'My Sheet'!$B$2:$B$4")
        );
        assert_eq!(
            series[1].categories.as_ref().unwrap().formula.as_deref(),
            Some("'My Sheet'!$A$2:$A$4")
        );
        assert_eq!(
            series[1].name,
            Some(ChartText::Cell {
                formula: "'My Sheet'!$C$1".into(),
                cache: String::new()
            })
        );
    }

    #[test]
    fn a_new_chart_looks_as_excel_2016_makes_it() {
        let area = Area {
            top: 1,
            left: 1,
            bottom: 4,
            right: 3,
        };
        let series = series_from(
            "Sheet1",
            area,
            Plan {
                by_columns: true,
                header_row: true,
                header_col: true,
            },
            false,
        );
        let chart = new_chart(ChartType::ColumnClustered, series, 1);
        assert_eq!(chart.groups.len(), 1);
        assert_eq!(chart.axes.len(), 2);
        assert_eq!(
            chart.groups[0].axes,
            vec![chart.axes[0].id, chart.axes[1].id]
        );
        assert!(chart.legend.is_some());
        assert!(chart.editable());
        let combo = new_chart(
            ChartType::ComboColumnLineSecondary,
            series_from(
                "Sheet1",
                area,
                Plan {
                    by_columns: true,
                    header_row: true,
                    header_col: true,
                },
                false,
            ),
            1,
        );
        assert_eq!(combo.groups.len(), 2);
        assert_eq!(combo.axes.len(), 4);
        assert!(combo.axes[2].deleted);
        assert_eq!(combo.axes[3].position, AxisPosition::Right);
        // Written and read back, it is the same chart.
        let xml = cgs_xlsx::chart_write::write_chart(&combo);
        let ChartBody::Classic(back) = cgs_xlsx::chart::read_chart(&xml, false) else {
            panic!()
        };
        assert_eq!(back.groups, combo.groups);
        assert_eq!(back.axes, combo.axes);
    }

    #[test]
    fn axis_ids_stay_apart_through_every_type() {
        let area = Area {
            top: 1,
            left: 1,
            bottom: 4,
            right: 3,
        };
        let plan = Plan {
            by_columns: true,
            header_row: true,
            header_col: true,
        };
        let mut chart = new_chart(
            ChartType::ColumnClustered,
            series_from("Sheet1", area, plan, false),
            1,
        );
        let book = cgs_model::Book::default();
        for chart_type in [
            ChartType::Pie,
            ChartType::ScatterLines,
            ChartType::BarStacked,
            ChartType::Scatter,
            ChartType::ComboColumnLineSecondary,
            ChartType::Doughnut,
            ChartType::AreaPercent,
        ] {
            apply_edit(&mut chart, &ChartEdit::Type { chart: chart_type }, &book, 1).unwrap();
            let mut ids: Vec<u32> = chart.axes.iter().map(|axis| axis.id).collect();
            let count = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), count, "{chart_type:?}: {:?}", chart.axes);
            for group in &chart.groups {
                for id in &group.axes {
                    assert!(chart.axis(*id).is_some(), "{chart_type:?}: group axis {id}");
                }
            }
        }
    }

    #[test]
    fn change_type_and_back_keeps_the_series() {
        let area = Area {
            top: 1,
            left: 1,
            bottom: 4,
            right: 3,
        };
        let plan = Plan {
            by_columns: true,
            header_row: true,
            header_col: true,
        };
        let mut chart = new_chart(
            ChartType::ColumnClustered,
            series_from("Sheet1", area, plan, false),
            1,
        );
        let book = cgs_model::Book::default();
        apply_edit(
            &mut chart,
            &ChartEdit::Type {
                chart: ChartType::Pie,
            },
            &book,
            1,
        )
        .unwrap();
        assert!(chart.axes.is_empty());
        assert_eq!(chart.series_count(), 1);
        apply_edit(
            &mut chart,
            &ChartEdit::Type {
                chart: ChartType::LineMarkers,
            },
            &book,
            1,
        )
        .unwrap();
        assert_eq!(chart.axes.len(), 2);
        apply_edit(
            &mut chart,
            &ChartEdit::Title {
                text: Some("Sales".into()),
            },
            &book,
            1,
        )
        .unwrap();
        assert_eq!(
            chart.title.as_ref().unwrap().text,
            Some(ChartText::Rich("Sales".into()))
        );
        apply_edit(&mut chart, &ChartEdit::Title { text: None }, &book, 1).unwrap();
        assert!(chart.title.is_none() && chart.auto_title_deleted);
        apply_edit(&mut chart, &ChartEdit::QuickLayout { layout: 9 }, &book, 1).unwrap();
        assert!(chart.axes.iter().all(|a| a.title.is_some()));
    }
}
