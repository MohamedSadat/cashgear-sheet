//! Making and changing conditional formats: the rules the Conditional Formatting menu and the
//! rules manager make, written as Excel 2016 writes them. Extended rules stay as the file has them.

use cgs_model::{
    CfBlock, CfKind, CfOperator, CfRule, CfValueKind, Cfvo, Color, ColorKind, DataBar, Dxf,
    DxfFill, IconSet, Line, LineStyle, Pattern, Rect,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::command::{Applied, Area, Op};
use crate::Session;

/// What a rule's format sets, as the UI gives it. Colours are `RRGGBB`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FormatSpec {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub strike: Option<bool>,
    pub underline: Option<bool>,
    pub color: Option<String>,
    pub fill: Option<String>,
    /// A thin outline in this colour.
    pub border: Option<String>,
    pub number_format: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointSpec {
    /// `min`, `max`, `num`, `percent`, `percentile`, `formula`, `autoMin`, `autoMax`.
    pub kind: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "yes")]
    pub gte: bool,
}

fn yes() -> bool {
    true
}

/// A rule as the UI asks for it.
#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RuleSpec {
    CellIs {
        op: String,
        formulas: Vec<String>,
        #[serde(default)]
        format: FormatSpec,
    },
    Expression {
        formula: String,
        #[serde(default)]
        format: FormatSpec,
    },
    /// `contains`, `notContains`, `beginsWith` or `endsWith`.
    Text {
        op: String,
        text: String,
        #[serde(default)]
        format: FormatSpec,
    },
    Blanks {
        #[serde(default)]
        not: bool,
        #[serde(default)]
        format: FormatSpec,
    },
    Errors {
        #[serde(default)]
        not: bool,
        #[serde(default)]
        format: FormatSpec,
    },
    TimePeriod {
        period: String,
        #[serde(default)]
        format: FormatSpec,
    },
    Top10 {
        #[serde(default)]
        bottom: bool,
        #[serde(default)]
        percent: bool,
        rank: u32,
        #[serde(default)]
        format: FormatSpec,
    },
    Average {
        #[serde(default = "yes")]
        above: bool,
        #[serde(default)]
        equal: bool,
        #[serde(default)]
        std_dev: u32,
        #[serde(default)]
        format: FormatSpec,
    },
    Duplicates {
        #[serde(default)]
        unique: bool,
        #[serde(default)]
        format: FormatSpec,
    },
    ColorScale {
        points: Vec<PointSpec>,
    },
    DataBar {
        min: PointSpec,
        max: PointSpec,
        color: String,
        #[serde(default = "yes")]
        gradient: bool,
        #[serde(default = "yes")]
        show_value: bool,
    },
    IconSet {
        set: String,
        #[serde(default)]
        points: Vec<PointSpec>,
        #[serde(default)]
        reverse: bool,
        #[serde(default = "yes")]
        show_value: bool,
    },
}

/// A rule of the manager's list, in priority order: an existing one by its place, maybe changed,
/// or a new one.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRule {
    #[serde(default)]
    pub block: Option<usize>,
    #[serde(default)]
    pub rule: Option<usize>,
    pub areas: Vec<Area>,
    #[serde(default)]
    pub spec: Option<RuleSpec>,
    #[serde(default)]
    pub stop: bool,
}

fn rgb(text: &str) -> Color {
    Color {
        kind: u32::from_str_radix(text.trim_start_matches('#'), 16)
            .map_or(ColorKind::Auto, ColorKind::Rgb),
        tint: 0.0,
    }
}

fn point(spec: &PointSpec) -> Cfvo {
    Cfvo {
        kind: CfValueKind::from_xml(&spec.kind),
        value: spec.value.clone(),
        gte: spec.gte,
    }
}

fn area_rect(area: &Area) -> Rect {
    Rect {
        top: area.top,
        left: area.left,
        bottom: area.bottom,
        right: area.right,
    }
}

/// `rect` with `cut` taken out, as up to four pieces.
fn subtract(rect: Rect, cut: Rect) -> Vec<Rect> {
    if cut.top > rect.bottom
        || cut.bottom < rect.top
        || cut.left > rect.right
        || cut.right < rect.left
    {
        return vec![rect];
    }
    let mut out = Vec::new();
    if cut.top > rect.top {
        out.push(Rect {
            bottom: cut.top - 1,
            ..rect
        });
    }
    if cut.bottom < rect.bottom {
        out.push(Rect {
            top: cut.bottom + 1,
            ..rect
        });
    }
    let (top, bottom) = (rect.top.max(cut.top), rect.bottom.min(cut.bottom));
    if cut.left > rect.left {
        out.push(Rect {
            top,
            bottom,
            right: cut.left - 1,
            ..rect
        });
    }
    if cut.right < rect.right {
        out.push(Rect {
            top,
            bottom,
            left: cut.right + 1,
            ..rect
        });
    }
    out
}

/// A cell's address for a rule's formula, relative as Excel writes it.
fn anchor_ref(areas: &[Area]) -> String {
    let first = areas.first().copied().unwrap_or(Area::cell(1, 1));
    cgs_xlsx::patch_sheet::cell_name(first.top, first.left)
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

/// An id for an extended partner, as a GUID in braces.
fn partner_id(seed: u64) -> String {
    let mut state = seed ^ 0x9e37_79b9_7f4a_7c15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let (a, b) = (next(), next());
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}",
        a >> 32,
        (a >> 16) & 0xFFFF,
        (a & 0x0FFF) | 0x4000,
        ((b >> 48) & 0x3FFF) | 0x8000,
        b & 0xFFFF_FFFF_FFFF
    )
}

impl Session {
    fn format_dxf(&mut self, spec: &FormatSpec) -> Option<u32> {
        let mut dxf = Dxf::default();
        dxf.font.bold = spec.bold;
        dxf.font.italic = spec.italic;
        dxf.font.strike = spec.strike;
        dxf.font.underline = spec.underline.map(|on| {
            if on {
                cgs_model::Underline::Single
            } else {
                cgs_model::Underline::None
            }
        });
        dxf.font.color = spec.color.as_deref().map(rgb);
        dxf.fill = spec.fill.as_deref().map(|fill| DxfFill {
            pattern: Pattern::Solid,
            color: rgb(fill),
            pattern_color: Color::AUTO,
        });
        if let Some(border) = spec.border.as_deref() {
            let line = Some(Line {
                style: LineStyle::Thin,
                color: rgb(border),
            });
            dxf.border.left = line;
            dxf.border.right = line;
            dxf.border.top = line;
            dxf.border.bottom = line;
        }
        dxf.num_fmt = spec.number_format.clone();
        Some(self.add_dxf(dxf))
    }

    /// A rule from what the UI asked for, its formulas written for `areas`' first cell.
    fn rule_of(&mut self, spec: &RuleSpec, areas: &[Area], seed: u64) -> Result<CfRule, String> {
        let at = anchor_ref(areas);
        let mut rule = CfRule {
            kind: CfKind::Expression,
            priority: 1,
            stop: false,
            dxf: None,
            own_dxf: None,
            formulas: Vec::new(),
            x14_id: None,
        };
        let check = |session: &Self, formula: &str| -> Result<String, String> {
            let text = formula.trim().trim_start_matches('=');
            session.calc_check(text)
        };
        match spec {
            RuleSpec::CellIs {
                op,
                formulas,
                format,
            } => {
                let op =
                    CfOperator::from_xml(op).ok_or_else(|| format!("{op} isn't a comparison."))?;
                let wanted = if matches!(op, CfOperator::Between | CfOperator::NotBetween) {
                    2
                } else {
                    1
                };
                if formulas.len() != wanted {
                    return Err("Enter a value for each box.".to_string());
                }
                rule.kind = CfKind::CellIs(op);
                rule.formulas = formulas
                    .iter()
                    .map(|f| check(self, f))
                    .collect::<Result<_, _>>()?;
                rule.dxf = self.format_dxf(format);
            }
            RuleSpec::Expression { formula, format } => {
                rule.formulas = vec![check(self, formula)?];
                rule.dxf = self.format_dxf(format);
            }
            RuleSpec::Text { op, text, format } => {
                let search = format!("SEARCH({},{at})", quoted(text));
                let (kind, formula) = match op.as_str() {
                    "contains" => (
                        CfKind::ContainsText(text.clone()),
                        format!("NOT(ISERROR({search}))"),
                    ),
                    "notContains" => (
                        CfKind::NotContainsText(text.clone()),
                        format!("ISERROR({search})"),
                    ),
                    "beginsWith" => (
                        CfKind::BeginsWith(text.clone()),
                        format!("LEFT({at},LEN({}))={}", quoted(text), quoted(text)),
                    ),
                    "endsWith" => (
                        CfKind::EndsWith(text.clone()),
                        format!("RIGHT({at},LEN({}))={}", quoted(text), quoted(text)),
                    ),
                    other => return Err(format!("{other} isn't a text test.")),
                };
                rule.kind = kind;
                rule.formulas = vec![formula];
                rule.dxf = self.format_dxf(format);
            }
            RuleSpec::Blanks { not, format } => {
                rule.kind = if *not {
                    CfKind::NotContainsBlanks
                } else {
                    CfKind::ContainsBlanks
                };
                rule.formulas = vec![if *not {
                    format!("LEN(TRIM({at}))>0")
                } else {
                    format!("LEN(TRIM({at}))=0")
                }];
                rule.dxf = self.format_dxf(format);
            }
            RuleSpec::Errors { not, format } => {
                rule.kind = if *not {
                    CfKind::NotContainsErrors
                } else {
                    CfKind::ContainsErrors
                };
                rule.formulas = vec![if *not {
                    format!("NOT(ISERROR({at}))")
                } else {
                    format!("ISERROR({at})")
                }];
                rule.dxf = self.format_dxf(format);
            }
            RuleSpec::TimePeriod { period, format } => {
                let floor = format!("FLOOR({at},1)");
                let formula = match period.as_str() {
                    "today" => format!("{floor}=TODAY()"),
                    "yesterday" => format!("{floor}=TODAY()-1"),
                    "tomorrow" => format!("{floor}=TODAY()+1"),
                    "last7Days" => format!("AND(TODAY()-{floor}<=6,{floor}<=TODAY())"),
                    "thisWeek" => format!("AND(TODAY()-ROUNDDOWN({at},0)<=WEEKDAY(TODAY())-1,ROUNDDOWN({at},0)-TODAY()<=7-WEEKDAY(TODAY()))"),
                    "lastWeek" => format!("AND(TODAY()-ROUNDDOWN({at},0)>=(WEEKDAY(TODAY())),TODAY()-ROUNDDOWN({at},0)<(WEEKDAY(TODAY())+7))"),
                    "nextWeek" => format!("AND(ROUNDDOWN({at},0)-TODAY()>(7-WEEKDAY(TODAY())),ROUNDDOWN({at},0)-TODAY()<(15-WEEKDAY(TODAY())))"),
                    "thisMonth" => format!("AND(MONTH({at})=MONTH(TODAY()),YEAR({at})=YEAR(TODAY()))"),
                    "lastMonth" => format!("AND(MONTH({at})=MONTH(EDATE(TODAY(),0-1)),YEAR({at})=YEAR(EDATE(TODAY(),0-1)))"),
                    "nextMonth" => format!("AND(MONTH({at})=MONTH(EDATE(TODAY(),0+1)),YEAR({at})=YEAR(EDATE(TODAY(),0+1)))"),
                    other => return Err(format!("{other} isn't a time period.")),
                };
                rule.kind = CfKind::TimePeriod(period.clone());
                rule.formulas = vec![formula];
                rule.dxf = self.format_dxf(format);
            }
            RuleSpec::Top10 {
                bottom,
                percent,
                rank,
                format,
            } => {
                if *rank == 0 || *rank > 1000 {
                    return Err("Enter a whole number from 1 to 1000.".to_string());
                }
                rule.kind = CfKind::Top10 {
                    bottom: *bottom,
                    percent: *percent,
                    rank: *rank,
                };
                rule.dxf = self.format_dxf(format);
            }
            RuleSpec::Average {
                above,
                equal,
                std_dev,
                format,
            } => {
                rule.kind = CfKind::AboveAverage {
                    above: *above,
                    equal: *equal,
                    std_dev: *std_dev,
                };
                rule.dxf = self.format_dxf(format);
            }
            RuleSpec::Duplicates { unique, format } => {
                rule.kind = if *unique {
                    CfKind::Uniques
                } else {
                    CfKind::Duplicates
                };
                rule.dxf = self.format_dxf(format);
            }
            RuleSpec::ColorScale { points } => {
                if !(2..=3).contains(&points.len()) {
                    return Err("A colour scale has two or three colours.".to_string());
                }
                rule.kind = CfKind::ColorScale {
                    cfvos: points.iter().map(point).collect(),
                    colors: points
                        .iter()
                        .map(|p| rgb(p.color.as_deref().unwrap_or("000000")))
                        .collect(),
                };
            }
            RuleSpec::DataBar {
                min,
                max,
                color,
                gradient,
                show_value,
            } => {
                // Excel 2016's bars: the classic one, with an extended partner for its looks.
                let color = rgb(color);
                rule.kind = CfKind::DataBar(Box::new(DataBar {
                    min: point(min),
                    max: point(max),
                    color,
                    show_value: *show_value,
                    min_length: 0,
                    max_length: 100,
                    gradient: *gradient,
                    border: gradient.then_some(color),
                    negative_color: Some(rgb("FF0000")),
                    axis_color: Some(rgb("000000")),
                    right_to_left: false,
                }));
                rule.x14_id = Some(partner_id(seed));
            }
            RuleSpec::IconSet {
                set,
                points,
                reverse,
                show_value,
            } => {
                let size = IconSet::size(set);
                let cfvos: Vec<Cfvo> = if points.is_empty() {
                    (0..size)
                        .map(|at| {
                            Cfvo::new(CfValueKind::Percent, Some(&((at * 100 / size).to_string())))
                        })
                        .collect()
                } else {
                    points.iter().map(point).collect()
                };
                if cfvos.len() != size {
                    return Err(format!("{set} needs {size} thresholds."));
                }
                rule.kind = CfKind::IconSet(Box::new(IconSet {
                    set: set.clone(),
                    cfvos,
                    reverse: *reverse,
                    show_value: *show_value,
                    custom: Vec::new(),
                }));
            }
        }
        Ok(rule)
    }

    fn calc_check(&self, text: &str) -> Result<String, String> {
        use cgs_calc::CalcEngine;
        self.calc.check_formula(text).map_err(|error| error.0)?;
        Ok(text.to_string())
    }

    fn cf_op(&self, sheet: u32, blocks: Vec<CfBlock>) -> Op {
        Op::SheetCf { sheet, blocks }
    }

    /// Adds a rule over `areas`, above every rule the sheet has, as Excel does.
    pub(crate) fn add_cf_command(
        &mut self,
        sheet: u32,
        areas: &[Area],
        spec: &RuleSpec,
    ) -> Result<Applied, String> {
        self.sheet_for_edit(sheet)?;
        if areas.is_empty() || areas.iter().any(|area| !area.valid()) {
            return Err("Select the cells to format first.".to_string());
        }
        let seed = (u64::from(sheet) << 32)
            ^ self.history.state
            ^ (self.book.sheets[sheet as usize].cf.len() as u64);
        let rule = self.rule_of(spec, areas, seed)?;
        let mut blocks = self.book.sheets[sheet as usize].cf.clone();
        for block in &mut blocks {
            for existing in &mut block.rules {
                existing.priority += 1;
            }
            block.raw = None;
        }
        blocks.push(CfBlock {
            areas: areas.iter().map(area_rect).collect(),
            rules: vec![CfRule {
                priority: 1,
                ..rule
            }],
            extended: false,
            raw: None,
        });
        let op = self.cf_op(sheet, blocks);
        self.perform("conditional format", op, Some((sheet, areas[0])))
    }

    /// Takes rules away from these cells, or from the whole sheet with none.
    pub(crate) fn clear_cf_command(
        &mut self,
        sheet: u32,
        areas: Option<&[Area]>,
    ) -> Result<Applied, String> {
        self.sheet_for_edit(sheet)?;
        let blocks = self.book.sheets[sheet as usize].cf.clone();
        let next: Vec<CfBlock> = match areas {
            None => Vec::new(),
            Some(areas) => blocks
                .into_iter()
                .filter_map(|mut block| {
                    let mut kept = block.areas.clone();
                    for cut in areas.iter().map(area_rect) {
                        kept = kept
                            .into_iter()
                            .flat_map(|rect| subtract(rect, cut))
                            .collect();
                    }
                    if kept.is_empty() {
                        return None;
                    }
                    if kept != block.areas {
                        block.areas = kept;
                        block.raw = None;
                    }
                    Some(block)
                })
                .collect(),
        };
        let op = self.cf_op(sheet, next);
        self.perform("clearing rules", op, None)
    }

    /// The rules manager's list, in priority order, put in place of the sheet's classic rules.
    pub(crate) fn set_cf_command(
        &mut self,
        sheet: u32,
        list: &[ManagedRule],
    ) -> Result<Applied, String> {
        self.sheet_for_edit(sheet)?;
        let old = self.book.sheets[sheet as usize].cf.clone();
        let mut blocks: Vec<CfBlock> = old.iter().filter(|block| block.extended).cloned().collect();
        // Extended rules keep their places among the priorities after the classic ones.
        let mut priority = 1;
        for (at, entry) in list.iter().enumerate() {
            if entry.areas.is_empty() {
                return Err("Each rule needs cells to apply to.".to_string());
            }
            let found = entry
                .block
                .zip(entry.rule)
                .and_then(|(block, rule)| old.get(block).and_then(|b| b.rules.get(rule)).cloned());
            let mut rule = match (&entry.spec, found) {
                (Some(spec), _) => {
                    self.rule_of(spec, &entry.areas, at as u64 + self.history.state)?
                }
                (None, Some(rule)) => rule,
                (None, None) => return Err("A new rule needs a format.".to_string()),
            };
            rule.priority = priority;
            rule.stop = entry.stop;
            priority += 1;
            blocks.push(CfBlock {
                areas: entry.areas.iter().map(area_rect).collect(),
                rules: vec![rule],
                extended: false,
                raw: None,
            });
        }
        for block in blocks.iter_mut().filter(|block| block.extended) {
            for rule in &mut block.rules {
                rule.priority = rule.priority.max(priority);
            }
        }
        let op = self.cf_op(sheet, blocks);
        self.perform("conditional formats", op, None)
    }

    /// The sheet's rules for the manager, in priority order.
    pub(crate) fn cf_rules(&self, sheet: u32) -> Value {
        let Some(info) = self.book.sheets.get(sheet as usize) else {
            return Value::Array(Vec::new());
        };
        let mut list: Vec<(i32, Value)> = Vec::new();
        for (b, block) in info.cf.iter().enumerate() {
            for (r, rule) in block.rules.iter().enumerate() {
                let dxf = rule.own_dxf.clone().or_else(|| {
                    rule.dxf
                        .and_then(|index| self.book.styles.dxfs.get(index as usize).cloned())
                });
                let color = |color: Option<Color>| {
                    color
                        .and_then(|color| self.book.styles.colors.resolve(color, None))
                        .map(|rgb| format!("{rgb:06X}"))
                };
                let (kind, describe) = describe(&rule.kind, &rule.formulas);
                list.push((
                    rule.priority,
                    json!({
                        "block": b,
                        "rule": r,
                        "priority": rule.priority,
                        "stop": rule.stop,
                        "extended": block.extended,
                        "kind": kind,
                        "description": describe,
                        "areas": cgs_xlsx::cf::sqref_text(&block.areas),
                        "fill": color(dxf.as_ref().and_then(|d| d.fill.map(|f| f.color))),
                        "color": color(dxf.as_ref().and_then(|d| d.font.color)),
                        "bold": dxf.as_ref().and_then(|d| d.font.bold),
                    }),
                ));
            }
        }
        list.sort_by_key(|(priority, _)| *priority);
        Value::Array(list.into_iter().map(|(_, value)| value).collect())
    }
}

/// A rule's kind and a line about it, as the manager shows it.
fn describe(kind: &CfKind, formulas: &[String]) -> (&'static str, String) {
    let first = formulas.first().map(String::as_str).unwrap_or_default();
    match kind {
        CfKind::CellIs(op) => {
            let words = match op {
                CfOperator::LessThan => "Cell Value < ",
                CfOperator::LessThanOrEqual => "Cell Value <= ",
                CfOperator::Equal => "Cell Value = ",
                CfOperator::NotEqual => "Cell Value <> ",
                CfOperator::GreaterThanOrEqual => "Cell Value >= ",
                CfOperator::GreaterThan => "Cell Value > ",
                CfOperator::Between => "Cell Value between ",
                CfOperator::NotBetween => "Cell Value not between ",
            };
            let text = match formulas {
                [a, b] => format!("{words}{a} and {b}"),
                _ => format!("{words}{first}"),
            };
            ("cellIs", text)
        }
        CfKind::Expression => ("expression", format!("Formula: ={first}")),
        CfKind::ContainsText(text) => ("text", format!("Cell Value contains \"{text}\"")),
        CfKind::NotContainsText(text) => {
            ("text", format!("Cell Value does not contain \"{text}\""))
        }
        CfKind::BeginsWith(text) => ("text", format!("Cell Value begins with \"{text}\"")),
        CfKind::EndsWith(text) => ("text", format!("Cell Value ends with \"{text}\"")),
        CfKind::ContainsBlanks => ("blanks", "Blanks".to_string()),
        CfKind::NotContainsBlanks => ("blanks", "No Blanks".to_string()),
        CfKind::ContainsErrors => ("errors", "Errors".to_string()),
        CfKind::NotContainsErrors => ("errors", "No Errors".to_string()),
        CfKind::TimePeriod(period) => ("timePeriod", format!("A date occurring: {period}")),
        CfKind::Top10 {
            bottom,
            percent,
            rank,
        } => (
            "top10",
            format!(
                "{} {rank}{}",
                if *bottom { "Bottom" } else { "Top" },
                if *percent { "%" } else { "" }
            ),
        ),
        CfKind::AboveAverage { above, .. } => (
            "average",
            if *above {
                "Above Average"
            } else {
                "Below Average"
            }
            .to_string(),
        ),
        CfKind::Duplicates => ("duplicates", "Duplicate Values".to_string()),
        CfKind::Uniques => ("duplicates", "Unique Values".to_string()),
        CfKind::ColorScale { colors, .. } => {
            ("colorScale", format!("{}-Color Scale", colors.len()))
        }
        CfKind::DataBar(_) => ("dataBar", "Data Bar".to_string()),
        CfKind::IconSet(set) => ("iconSet", format!("Icon Set: {}", set.set)),
        CfKind::Other(kind) => ("other", kind.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taking_cells_out_leaves_the_rest_in_pieces() {
        let rect = Rect {
            top: 1,
            left: 1,
            bottom: 10,
            right: 5,
        };
        let cut = Rect {
            top: 3,
            left: 2,
            bottom: 4,
            right: 3,
        };
        let pieces = subtract(rect, cut);
        let cells: u32 = pieces
            .iter()
            .map(|r| (r.bottom - r.top + 1) * (r.right - r.left + 1))
            .sum();
        assert_eq!(cells, 50 - 4);
        assert_eq!(subtract(rect, rect), Vec::new());
    }

    #[test]
    fn partner_ids_look_like_guids() {
        let id = partner_id(7);
        assert_eq!(id.len(), 38);
        assert!(id.starts_with('{') && id.ends_with('}'));
        assert_ne!(partner_id(7), partner_id(8));
    }
}
