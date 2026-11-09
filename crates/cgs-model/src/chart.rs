//! A chart as `c:chartSpace` describes it: groups of series of one type each, the axes they plot
//! on, a title and a legend. Only what the app draws or edits is here. Everything else in the
//! part stays in the file's own bytes, which the writer keeps.

/// A DrawingML colour: a base and the changes applied to it, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct DmlColor {
    pub base: DmlBase,
    pub mods: Vec<ColorMod>,
}

impl DmlColor {
    pub fn rgb(value: u32) -> Self {
        Self {
            base: DmlBase::Rgb(value),
            mods: Vec::new(),
        }
    }

    pub fn scheme(name: &str) -> Self {
        Self {
            base: DmlBase::Scheme(name.to_string()),
            mods: Vec::new(),
        }
    }

    pub fn with(mut self, kind: ColorModKind, value: i32) -> Self {
        self.mods.push(ColorMod { kind, value });
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DmlBase {
    /// `0xRRGGBB`.
    Rgb(u32),
    /// `accent1`, `tx1`, `bg1`, `dk1`, `lt1`, `phClr` and so on.
    Scheme(String),
    /// A system colour, as its last saved value.
    System(u32),
    /// A preset name such as `black`, as its value.
    Preset(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorModKind {
    LumMod,
    LumOff,
    Tint,
    Shade,
    Alpha,
    SatMod,
}

/// In 1000ths of a percent, as the file has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorMod {
    pub kind: ColorModKind,
    pub value: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChartFill {
    None,
    Solid(DmlColor),
    /// A gradient, drawn as its first stop's colour.
    Gradient(DmlColor),
    /// A pattern or picture, drawn as its foreground colour if it has one.
    Other(Option<DmlColor>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dash {
    #[default]
    Solid,
    Dot,
    Dash,
    LongDash,
    DashDot,
    LongDashDot,
    LongDashDotDot,
    SysDash,
    SysDot,
    SysDashDot,
    SysDashDotDot,
}

impl Dash {
    pub fn from_name(name: &str) -> Self {
        match name {
            "dot" => Self::Dot,
            "dash" => Self::Dash,
            "lgDash" => Self::LongDash,
            "dashDot" => Self::DashDot,
            "lgDashDot" => Self::LongDashDot,
            "lgDashDotDot" => Self::LongDashDotDot,
            "sysDash" => Self::SysDash,
            "sysDot" => Self::SysDot,
            "sysDashDot" => Self::SysDashDot,
            "sysDashDotDot" => Self::SysDashDotDot,
            _ => Self::Solid,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Dot => "dot",
            Self::Dash => "dash",
            Self::LongDash => "lgDash",
            Self::DashDot => "dashDot",
            Self::LongDashDot => "lgDashDot",
            Self::LongDashDotDot => "lgDashDotDot",
            Self::SysDash => "sysDash",
            Self::SysDot => "sysDot",
            Self::SysDashDot => "sysDashDot",
            Self::SysDashDotDot => "sysDashDotDot",
        }
    }
}

/// An outline: `a:ln`. `None` fields weren't stated, so the chart style decides.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartLine {
    pub fill: Option<ChartFill>,
    /// In EMU.
    pub width: Option<i64>,
    pub dash: Option<Dash>,
}

/// `c:spPr`: a shape's fill and outline. `None` means the file says nothing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShapeLook {
    pub fill: Option<ChartFill>,
    pub line: Option<ChartLine>,
}

impl ShapeLook {
    pub fn is_empty(&self) -> bool {
        self.fill.is_none() && self.line.is_none()
    }
}

/// The font of a text element: `c:txPr`'s first `a:defRPr`, or a rich text's first run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextLook {
    /// In hundredths of a point.
    pub size: Option<u32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub color: Option<DmlColor>,
    pub font: Option<String>,
    /// In 60,000ths of a degree, from `a:bodyPr rot`.
    pub rotation: Option<i32>,
}

impl TextLook {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A title's or label's text.
#[derive(Debug, Clone, PartialEq)]
pub enum ChartText {
    /// Typed text, as runs joined.
    Rich(String),
    /// A cell's text, with the value saved beside it.
    Cell { formula: String, cache: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChartTitle {
    /// `None` makes the text automatic: the series' name for a chart of one series.
    pub text: Option<ChartText>,
    pub overlay: bool,
    pub look: TextLook,
    pub shape: ShapeLook,
}

impl ChartTitle {
    pub fn automatic() -> Self {
        Self {
            text: None,
            overlay: false,
            look: TextLook::default(),
            shape: ShapeLook::default(),
        }
    }

    pub fn text(text: &str) -> Self {
        Self {
            text: Some(ChartText::Rich(text.to_string())),
            ..Self::automatic()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LegendPosition {
    #[default]
    Right,
    Left,
    Top,
    Bottom,
    TopRight,
}

impl LegendPosition {
    pub fn from_code(code: &str) -> Self {
        match code {
            "l" => Self::Left,
            "t" => Self::Top,
            "b" => Self::Bottom,
            "tr" => Self::TopRight,
            _ => Self::Right,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Right => "r",
            Self::Left => "l",
            Self::Top => "t",
            Self::Bottom => "b",
            Self::TopRight => "tr",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Legend {
    pub position: LegendPosition,
    pub overlay: bool,
    /// Entries deleted from the legend, by series or point index.
    pub hidden: Vec<u32>,
    pub look: TextLook,
    pub shape: ShapeLook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Grouping {
    #[default]
    Clustered,
    Standard,
    Stacked,
    PercentStacked,
}

impl Grouping {
    pub fn from_code(code: &str) -> Self {
        match code {
            "standard" => Self::Standard,
            "stacked" => Self::Stacked,
            "percentStacked" => Self::PercentStacked,
            _ => Self::Clustered,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Clustered => "clustered",
            Self::Standard => "standard",
            Self::Stacked => "stacked",
            Self::PercentStacked => "percentStacked",
        }
    }

    pub fn stacked(self) -> bool {
        matches!(self, Self::Stacked | Self::PercentStacked)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScatterStyle {
    #[default]
    LineMarker,
    Line,
    Marker,
    Smooth,
    SmoothMarker,
    None,
}

impl ScatterStyle {
    pub fn from_code(code: &str) -> Self {
        match code {
            "line" => Self::Line,
            "marker" => Self::Marker,
            "smooth" => Self::Smooth,
            "smoothMarker" => Self::SmoothMarker,
            "none" => Self::None,
            _ => Self::LineMarker,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::LineMarker => "lineMarker",
            Self::Line => "line",
            Self::Marker => "marker",
            Self::Smooth => "smooth",
            Self::SmoothMarker => "smoothMarker",
            Self::None => "none",
        }
    }
}

/// What a plot area's chart element is. The 3-D and rarer kinds are drawn as the nearest 2-D
/// kind, and edited as it once their type is changed.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupKind {
    Bar {
        horizontal: bool,
        grouping: Grouping,
        /// Percent of a bar's width, 0 to 500.
        gap_width: u16,
        /// -100 to 100.
        overlap: i16,
        three_d: bool,
    },
    Line {
        grouping: Grouping,
        markers: bool,
        three_d: bool,
    },
    Area {
        grouping: Grouping,
        three_d: bool,
    },
    Pie {
        /// Degrees clockwise from 12 o'clock.
        first_slice: u16,
        three_d: bool,
    },
    Doughnut {
        first_slice: u16,
        /// Percent of the radius, 10 to 90.
        hole: u8,
    },
    Scatter {
        style: ScatterStyle,
    },
    Radar,
    Bubble,
    Stock,
    Surface,
    /// Pie of pie, or bar of pie.
    OfPie,
}

impl GroupKind {
    /// The element's name in the file, without its prefix.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Bar { three_d: false, .. } => "barChart",
            Self::Bar { three_d: true, .. } => "bar3DChart",
            Self::Line { three_d: false, .. } => "lineChart",
            Self::Line { three_d: true, .. } => "line3DChart",
            Self::Area { three_d: false, .. } => "areaChart",
            Self::Area { three_d: true, .. } => "area3DChart",
            Self::Pie { three_d: false, .. } => "pieChart",
            Self::Pie { three_d: true, .. } => "pie3DChart",
            Self::Doughnut { .. } => "doughnutChart",
            Self::Scatter { .. } => "scatterChart",
            Self::Radar => "radarChart",
            Self::Bubble => "bubbleChart",
            Self::Stock => "stockChart",
            Self::Surface => "surfaceChart",
            Self::OfPie => "ofPieChart",
        }
    }

    /// Pie and doughnut charts have no axes.
    pub fn round(&self) -> bool {
        matches!(self, Self::Pie { .. } | Self::Doughnut { .. } | Self::OfPie)
    }

    /// One of the six kinds the app creates and edits.
    pub fn editable(&self) -> bool {
        matches!(
            self,
            Self::Bar { three_d: false, .. }
                | Self::Line { three_d: false, .. }
                | Self::Area { three_d: false, .. }
                | Self::Pie { three_d: false, .. }
                | Self::Doughnut { .. }
                | Self::Scatter { .. }
        )
    }
}

/// One point of a cache: its index and the text the file saved.
#[derive(Debug, Clone, PartialEq)]
pub struct CachePoint {
    pub index: u32,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceKind {
    #[default]
    Number,
    Text,
    /// Several rows or columns of labels.
    MultiLevel,
}

/// Where a series' names or values come from: cells, with a saved copy, or only the copy.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DataSource {
    /// `Sheet1!$B$2:$B$5`, without `=`. `None` for values typed into the chart.
    pub formula: Option<String>,
    pub kind: SourceKind,
    /// The saved values' count.
    pub count: u32,
    pub cache: Vec<CachePoint>,
    /// The number format the cache was saved with.
    pub format: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkerSymbol {
    #[default]
    Auto,
    None,
    Circle,
    Square,
    Diamond,
    Triangle,
    X,
    Star,
    Dot,
    Dash,
    Plus,
    Picture,
}

impl MarkerSymbol {
    pub fn from_code(code: &str) -> Self {
        match code {
            "none" => Self::None,
            "circle" => Self::Circle,
            "square" => Self::Square,
            "diamond" => Self::Diamond,
            "triangle" => Self::Triangle,
            "x" => Self::X,
            "star" => Self::Star,
            "dot" => Self::Dot,
            "dash" => Self::Dash,
            "plus" => Self::Plus,
            "picture" => Self::Picture,
            _ => Self::Auto,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Diamond => "diamond",
            Self::Triangle => "triangle",
            Self::X => "x",
            Self::Star => "star",
            Self::Dot => "dot",
            Self::Dash => "dash",
            Self::Plus => "plus",
            Self::Picture => "picture",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Marker {
    pub symbol: MarkerSymbol,
    /// 2 to 72 points.
    pub size: Option<u8>,
    pub shape: ShapeLook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelPosition {
    #[default]
    Auto,
    BestFit,
    Bottom,
    Center,
    InsideBase,
    InsideEnd,
    Left,
    OutsideEnd,
    Right,
    Top,
}

impl LabelPosition {
    pub fn from_code(code: &str) -> Self {
        match code {
            "bestFit" => Self::BestFit,
            "b" => Self::Bottom,
            "ctr" => Self::Center,
            "inBase" => Self::InsideBase,
            "inEnd" => Self::InsideEnd,
            "l" => Self::Left,
            "outEnd" => Self::OutsideEnd,
            "r" => Self::Right,
            "t" => Self::Top,
            _ => Self::Auto,
        }
    }

    pub fn code(self) -> Option<&'static str> {
        Some(match self {
            Self::Auto => return None,
            Self::BestFit => "bestFit",
            Self::Bottom => "b",
            Self::Center => "ctr",
            Self::InsideBase => "inBase",
            Self::InsideEnd => "inEnd",
            Self::Left => "l",
            Self::OutsideEnd => "outEnd",
            Self::Right => "r",
            Self::Top => "t",
        })
    }
}

/// `c:dLbls`: what each label shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DataLabels {
    pub value: bool,
    pub category: bool,
    pub series: bool,
    pub percent: bool,
    pub legend_key: bool,
    pub position: LabelPosition,
    pub separator: Option<String>,
    pub format: Option<String>,
    pub look: TextLook,
    /// `c:delete val="1"`: labels switched off here, whatever the group says.
    pub deleted: bool,
}

impl DataLabels {
    pub fn shows_anything(&self) -> bool {
        !self.deleted && (self.value || self.category || self.series || self.percent)
    }
}

/// One series' own look for one point: `c:dPt`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PointLook {
    pub index: u32,
    pub shape: ShapeLook,
    pub marker: Option<Marker>,
    /// Pie slices only: percent of the radius.
    pub explosion: Option<u32>,
}

/// How a trendline fits its series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendKind {
    Linear,
    Exponential,
    Logarithmic,
    /// Of this order, 2 to 6.
    Polynomial(u8),
    Power,
    /// Over this many points.
    MovingAverage(u32),
}

impl TrendKind {
    /// `trendlineType`'s value.
    pub fn code(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Exponential => "exp",
            Self::Logarithmic => "log",
            Self::Polynomial(_) => "poly",
            Self::Power => "power",
            Self::MovingAverage(_) => "movingAvg",
        }
    }

    /// The name Excel gives a trendline of a series named `series`, as its legend shows it.
    pub fn default_name(self, series: &str) -> String {
        match self {
            Self::Linear => format!("Linear ({series})"),
            Self::Exponential => format!("Expon. ({series})"),
            Self::Logarithmic => format!("Log. ({series})"),
            Self::Polynomial(_) => format!("Poly. ({series})"),
            Self::Power => format!("Power ({series})"),
            Self::MovingAverage(period) => format!("{period} per. Mov. Avg. ({series})"),
        }
    }
}

/// `c:trendline`.
#[derive(Debug, Clone, PartialEq)]
pub struct Trendline {
    pub kind: TrendKind,
    /// Its own name; Excel's default when `None`.
    pub name: Option<String>,
    /// Periods it runs on past the last point and before the first.
    pub forward: f64,
    pub backward: f64,
    pub intercept: Option<f64>,
    pub show_equation: bool,
    pub show_r2: bool,
    pub shape: ShapeLook,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Series {
    pub index: u32,
    pub order: u32,
    pub name: Option<ChartText>,
    /// Categories, or a scatter chart's x values.
    pub categories: Option<DataSource>,
    /// Values, or a scatter chart's y values.
    pub values: Option<DataSource>,
    pub shape: ShapeLook,
    pub marker: Option<Marker>,
    pub points: Vec<PointLook>,
    pub labels: Option<DataLabels>,
    pub smooth: Option<bool>,
    pub invert_if_negative: bool,
    pub explosion: Option<u32>,
    pub trendlines: Vec<Trendline>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChartGroup {
    pub kind: GroupKind,
    pub vary_colors: bool,
    pub series: Vec<Series>,
    /// The axes it plots on, by id: category or x first, then value or y.
    pub axes: Vec<u32>,
    pub labels: Option<DataLabels>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AxisKind {
    #[default]
    Category,
    Value,
    Date,
    Series,
}

impl AxisKind {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Category => "catAx",
            Self::Value => "valAx",
            Self::Date => "dateAx",
            Self::Series => "serAx",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AxisPosition {
    #[default]
    Bottom,
    Left,
    Right,
    Top,
}

impl AxisPosition {
    pub fn from_code(code: &str) -> Self {
        match code {
            "l" => Self::Left,
            "r" => Self::Right,
            "t" => Self::Top,
            _ => Self::Bottom,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Bottom => "b",
            Self::Left => "l",
            Self::Right => "r",
            Self::Top => "t",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Crosses {
    #[default]
    AutoZero,
    Min,
    Max,
    At(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TickMark {
    #[default]
    None,
    Inside,
    Outside,
    Cross,
}

impl TickMark {
    pub fn from_code(code: &str) -> Self {
        match code {
            "in" => Self::Inside,
            "out" => Self::Outside,
            "cross" => Self::Cross,
            _ => Self::None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Inside => "in",
            Self::Outside => "out",
            Self::Cross => "cross",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelPlace {
    #[default]
    NextTo,
    Low,
    High,
    None,
}

impl LabelPlace {
    pub fn from_code(code: &str) -> Self {
        match code {
            "low" => Self::Low,
            "high" => Self::High,
            "none" => Self::None,
            _ => Self::NextTo,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::NextTo => "nextTo",
            Self::Low => "low",
            Self::High => "high",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gridlines {
    pub shape: ShapeLook,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartAxis {
    pub id: u32,
    pub kind: AxisKind,
    pub position: AxisPosition,
    pub deleted: bool,
    pub cross_axis: u32,
    pub crosses: Crosses,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub major_unit: Option<f64>,
    pub minor_unit: Option<f64>,
    pub log_base: Option<f64>,
    /// `maxMin`: values run down, or categories right to left.
    pub reversed: bool,
    /// The number format, and whether it follows the cells.
    pub format: Option<(String, bool)>,
    pub major_gridlines: Option<Gridlines>,
    pub minor_gridlines: Option<Gridlines>,
    pub title: Option<ChartTitle>,
    pub major_tick: TickMark,
    pub minor_tick: TickMark,
    pub labels: LabelPlace,
    /// Value axes: whether the category axis crosses between categories (`between`) or on them.
    pub between: Option<bool>,
    pub label_skip: Option<u32>,
    pub mark_skip: Option<u32>,
    pub shape: ShapeLook,
    pub look: TextLook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Blanks {
    #[default]
    Gap,
    Zero,
    Span,
}

impl Blanks {
    pub fn from_code(code: &str) -> Self {
        match code {
            "zero" => Self::Zero,
            "span" => Self::Span,
            _ => Self::Gap,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Gap => "gap",
            Self::Zero => "zero",
            Self::Span => "span",
        }
    }
}

/// A chart part's contents.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chart {
    pub groups: Vec<ChartGroup>,
    pub axes: Vec<ChartAxis>,
    /// `None`: no title. A chart of one series shows its name unless `auto_title_deleted`.
    pub title: Option<ChartTitle>,
    pub auto_title_deleted: bool,
    pub legend: Option<Legend>,
    /// 1 to 48, Excel 2007's chart styles; 2 when the file doesn't say.
    pub style: u8,
    pub rounded_corners: bool,
    pub plot_visible_only: bool,
    pub blanks: Blanks,
    pub shape: ShapeLook,
    pub plot_shape: ShapeLook,
    pub look: TextLook,
    /// The plot area's own place, as fractions of the chart, when the file fixes it.
    pub plot_layout: Option<[f64; 4]>,
    /// A 3-D view: the chart is drawn flat.
    pub three_d: bool,
    /// The chart's data comes from a pivot table.
    pub pivot: bool,
}

impl Chart {
    pub fn axis(&self, id: u32) -> Option<&ChartAxis> {
        self.axes.iter().find(|axis| axis.id == id)
    }

    pub fn axis_mut(&mut self, id: u32) -> Option<&mut ChartAxis> {
        self.axes.iter_mut().find(|axis| axis.id == id)
    }

    pub fn series(&self) -> impl Iterator<Item = &Series> {
        self.groups.iter().flat_map(|group| group.series.iter())
    }

    pub fn series_count(&self) -> usize {
        self.groups.iter().map(|group| group.series.len()).sum()
    }

    /// Every cell formula the chart reads, for rewriting and for knowing when to redraw.
    pub fn formulas(&self) -> Vec<&str> {
        fn text<'a>(text: &'a Option<ChartText>, out: &mut Vec<&'a str>) {
            if let Some(ChartText::Cell { formula, .. }) = text {
                out.push(formula.as_str());
            }
        }
        let mut out = Vec::new();
        if let Some(title) = &self.title {
            text(&title.text, &mut out);
        }
        for series in self.series() {
            text(&series.name, &mut out);
            for source in [&series.categories, &series.values].into_iter().flatten() {
                if let Some(formula) = &source.formula {
                    out.push(formula);
                }
            }
        }
        out
    }

    /// Whether the app can edit it: only the six kinds, flat.
    pub fn editable(&self) -> bool {
        !self.groups.is_empty() && self.groups.iter().all(|group| group.kind.editable())
    }
}

/// What a chart part is.
#[derive(Debug, Clone, PartialEq)]
pub enum ChartBody {
    Classic(Box<Chart>),
    /// A `cx:` chart from Excel 2016 and later, by its layout's name, such as `waterfall`.
    Extended(String),
    /// A chart part that couldn't be read.
    Unreadable,
}

/// A chart part in the book.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartPart {
    /// The part's name, such as `xl/charts/chart1.xml`; `None` for a new chart.
    pub part: Option<String>,
    pub body: ChartBody,
    /// Changed since it was read, so the writer brings the file's part in line.
    pub changed: bool,
}
