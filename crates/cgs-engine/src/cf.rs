//! What conditional formats do to each cell: the formats of the rules that hold, a colour scale's
//! fill, a data bar and an icon. Rules apply in priority order across the sheet, a higher one's
//! format winning where two set the same thing, and one that stops stops those after it.

use std::cell::RefCell;
use std::collections::HashMap;

use cgs_calc::{CalcEngine, CalcValue, CellSink, Rect as CalcRect, SheetRect};
use cgs_model::{
    CfBlock, CfKind, CfOperator, CfRule, CfValueKind, Cfvo, Color, ColorKind, Dxf, DxfFill,
    Pattern, Rgb, StyleTable,
};

/// A data bar, as a tile draws it: shares of the cell's width, from the left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub start: f32,
    pub end: f32,
    pub color: Rgb,
    pub gradient: bool,
    pub border: Option<Rgb>,
    /// Where the axis is, for bars that go both ways.
    pub axis: Option<f32>,
}

/// What conditional formats make of one cell.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellCf {
    pub dxf: Option<Dxf>,
    pub bar: Option<Bar>,
    /// The icon set's name and the icon's place in it, from 0.
    pub icon: Option<(String, u32)>,
    pub hide_value: bool,
}

/// What a block's values come to as a whole, worked out once.
#[derive(Debug, Default)]
struct Stats {
    numbers: Vec<f64>,
    sorted: Vec<f64>,
    counts: HashMap<String, u32>,
}

/// Values of the cells a block covers.
#[derive(Default)]
struct Values(HashMap<(u32, u32), CalcValue>);

impl CellSink for Values {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.insert((row, col), value.clone());
    }
    fn take(&mut self, row: u32, col: u32, value: CalcValue) {
        self.0.insert((row, col), value);
    }
}

fn number(value: Option<&CalcValue>) -> Option<f64> {
    match value {
        Some(CalcValue::Number(number)) => Some(*number),
        _ => None,
    }
}

fn truthy(value: &CalcValue) -> bool {
    match value {
        CalcValue::Number(number) => *number != 0.0,
        CalcValue::Bool(flag) => *flag,
        _ => false,
    }
}

/// A value's text, as SEARCH sees it.
fn as_text(value: Option<&CalcValue>) -> String {
    match value {
        Some(CalcValue::Text(text)) => text.clone(),
        Some(CalcValue::Number(number)) => cgs_xlsx::patch_sheet::number_text(*number),
        Some(CalcValue::Bool(flag)) => if *flag { "TRUE" } else { "FALSE" }.to_string(),
        Some(CalcValue::Error(text)) => text.clone(),
        _ => String::new(),
    }
}

fn duplicate_key(value: &CalcValue) -> Option<String> {
    match value {
        CalcValue::Empty => None,
        CalcValue::Text(text) if text.is_empty() => None,
        CalcValue::Text(text) => Some(format!("t{}", text.to_lowercase())),
        CalcValue::Number(number) => Some(format!("n{number}")),
        CalcValue::Bool(flag) => Some(format!("b{flag}")),
        CalcValue::Error(text) => Some(format!("e{text}")),
    }
}

fn percentile(sorted: &[f64], share: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    // Excel's PERCENTILE: linear between ranks.
    let rank = share.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let low = rank.floor() as usize;
    let high = rank.ceil() as usize;
    Some(sorted[low] + (sorted[high] - sorted[low]) * (rank - low as f64))
}

fn lerp(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let channel = |shift: u32| {
        let x = f64::from((a >> shift) & 0xFF);
        let y = f64::from((b >> shift) & 0xFF);
        ((x + (y - x) * t).round() as u32).min(255) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// Works out conditional formats for a sheet, with each block's whole-range figures read once.
pub struct CfReader<'a> {
    pub calc: &'a cgs_calc::formualizer::FormualizerEngine,
    pub styles: &'a StyleTable,
    pub sheet: u32,
    pub sheet_name: &'a str,
    pub blocks: &'a [CfBlock],
    pub used: (u32, u32),
    pub today: f64,
    stats: RefCell<HashMap<usize, std::rc::Rc<Stats>>>,
}

impl<'a> CfReader<'a> {
    pub fn new(
        calc: &'a cgs_calc::formualizer::FormualizerEngine,
        styles: &'a StyleTable,
        sheet: u32,
        sheet_name: &'a str,
        blocks: &'a [CfBlock],
        used: (u32, u32),
        today: f64,
    ) -> Self {
        Self {
            calc,
            styles,
            sheet,
            sheet_name,
            blocks,
            used,
            today,
            stats: RefCell::new(HashMap::new()),
        }
    }

    fn value(&self, row: u32, col: u32) -> CalcValue {
        let mut values = Values::default();
        self.calc.read_block(
            SheetRect {
                sheet: self.sheet,
                rect: CalcRect {
                    top: row,
                    left: col,
                    bottom: row,
                    right: col,
                },
            },
            &mut values,
        );
        values.0.remove(&(row, col)).unwrap_or(CalcValue::Empty)
    }

    fn stats(&self, index: usize) -> std::rc::Rc<Stats> {
        if let Some(stats) = self.stats.borrow().get(&index) {
            return stats.clone();
        }
        let block = &self.blocks[index];
        let mut stats = Stats::default();
        for area in &block.areas {
            let bottom = area.bottom.min(self.used.0);
            let right = area.right.min(self.used.1);
            if area.top > bottom || area.left > right {
                continue;
            }
            let mut values = Values::default();
            self.calc.read_block(
                SheetRect {
                    sheet: self.sheet,
                    rect: CalcRect {
                        top: area.top,
                        left: area.left,
                        bottom,
                        right,
                    },
                },
                &mut values,
            );
            for value in values.0.values() {
                if let CalcValue::Number(number) = value {
                    stats.numbers.push(*number);
                }
                if let Some(key) = duplicate_key(value) {
                    *stats.counts.entry(key).or_default() += 1;
                }
            }
        }
        stats.sorted = stats.numbers.clone();
        stats
            .sorted
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let stats = std::rc::Rc::new(stats);
        self.stats.borrow_mut().insert(index, stats.clone());
        stats
    }

    fn formula(&self, block: &CfBlock, text: &str, row: u32, col: u32) -> CalcValue {
        self.calc
            .evaluate_at(self.sheet_name, block.anchor(), row, col, text)
    }

    /// A threshold's value, given the block's numbers.
    fn threshold(
        &self,
        block: &CfBlock,
        cfvo: &Cfvo,
        stats: &Stats,
        row: u32,
        col: u32,
    ) -> Option<f64> {
        let (min, max) = (stats.sorted.first().copied(), stats.sorted.last().copied());
        let given = || {
            cfvo.value.as_deref().and_then(|text| {
                text.trim().parse::<f64>().ok().or_else(|| {
                    number(Some(&self.formula(
                        block,
                        text.trim_start_matches('='),
                        row,
                        col,
                    )))
                })
            })
        };
        match cfvo.kind {
            CfValueKind::Min => min,
            CfValueKind::Max => max,
            // Excel 2010's automatic ends start from zero when every value is on one side.
            CfValueKind::AutoMin => min.map(|min| min.min(0.0)),
            CfValueKind::AutoMax => max.map(|max| max.max(0.0)),
            CfValueKind::Number | CfValueKind::Formula => given(),
            CfValueKind::Percent => {
                let (min, max) = (min?, max?);
                Some(min + (max - min) * given()? / 100.0)
            }
            CfValueKind::Percentile => percentile(&stats.sorted, given()? / 100.0),
        }
    }

    fn holds(&self, index: usize, rule: &CfRule, row: u32, col: u32, value: &CalcValue) -> bool {
        let block = &self.blocks[index];
        let text = || as_text(Some(value)).to_lowercase();
        match &rule.kind {
            CfKind::CellIs(op) => {
                let operand = |at: usize| {
                    rule.formulas
                        .get(at)
                        .map(|formula| self.formula(block, formula, row, col))
                };
                let compare = |a: &CalcValue, b: &CalcValue| -> Option<std::cmp::Ordering> {
                    match (a, b) {
                        (CalcValue::Text(x), CalcValue::Text(y)) => {
                            Some(x.to_lowercase().cmp(&y.to_lowercase()))
                        }
                        // Blanks count as 0 against a number, as in Excel.
                        (CalcValue::Empty, CalcValue::Number(y)) => 0.0f64.partial_cmp(y),
                        (CalcValue::Number(x), CalcValue::Number(y)) => x.partial_cmp(y),
                        (CalcValue::Number(_), CalcValue::Text(_)) => {
                            Some(std::cmp::Ordering::Less)
                        }
                        (CalcValue::Text(_), CalcValue::Number(_)) => {
                            Some(std::cmp::Ordering::Greater)
                        }
                        (CalcValue::Empty, CalcValue::Text(y)) => {
                            Some("".cmp(y.to_lowercase().as_str()))
                        }
                        _ => None,
                    }
                };
                if matches!(value, CalcValue::Error(_)) {
                    return false;
                }
                let Some(first) = operand(0) else {
                    return false;
                };
                let first = compare(value, &first);
                use std::cmp::Ordering::*;
                match op {
                    CfOperator::LessThan => first == Some(Less),
                    CfOperator::LessThanOrEqual => matches!(first, Some(Less | Equal)),
                    CfOperator::Equal => first == Some(Equal),
                    CfOperator::NotEqual => first != Some(Equal),
                    CfOperator::GreaterThanOrEqual => matches!(first, Some(Greater | Equal)),
                    CfOperator::GreaterThan => first == Some(Greater),
                    CfOperator::Between | CfOperator::NotBetween => {
                        let Some(second) = operand(1) else {
                            return false;
                        };
                        let second = compare(value, &second);
                        // Either way round, as Excel takes the two bounds.
                        let inside = (matches!(first, Some(Greater | Equal))
                            && matches!(second, Some(Less | Equal)))
                            || (matches!(first, Some(Less | Equal))
                                && matches!(second, Some(Greater | Equal)));
                        inside == (*op == CfOperator::Between)
                    }
                }
            }
            CfKind::Expression => rule
                .formulas
                .first()
                .is_some_and(|formula| truthy(&self.formula(block, formula, row, col))),
            CfKind::ContainsText(wanted) => text().contains(&wanted.to_lowercase()),
            CfKind::NotContainsText(wanted) => !text().contains(&wanted.to_lowercase()),
            CfKind::BeginsWith(wanted) => text().starts_with(&wanted.to_lowercase()),
            CfKind::EndsWith(wanted) => text().ends_with(&wanted.to_lowercase()),
            CfKind::ContainsBlanks => {
                as_text(Some(value)).trim().is_empty() && !matches!(value, CalcValue::Error(_))
            }
            CfKind::NotContainsBlanks => {
                !as_text(Some(value)).trim().is_empty() || matches!(value, CalcValue::Error(_))
            }
            CfKind::ContainsErrors => matches!(value, CalcValue::Error(_)),
            CfKind::NotContainsErrors => !matches!(value, CalcValue::Error(_)),
            CfKind::TimePeriod(period) => {
                let Some(day) = number(Some(value)).map(f64::floor) else {
                    return false;
                };
                let today = self.today.floor();
                // Weeks start on Sunday; serial 1 was a Sunday.
                let week = today - ((today as i64 - 1).rem_euclid(7)) as f64;
                let month_of = |serial: f64| {
                    cgs_numfmt::serial_to_datetime(serial, false)
                        .map(|parts| (parts.year, parts.month))
                };
                let (this_year, this_month) = month_of(today).unwrap_or((1900, 1));
                let shift = |months: i32| {
                    let total = this_year * 12 + this_month as i32 - 1 + months;
                    (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32)
                };
                match period.as_str() {
                    "today" => day == today,
                    "yesterday" => day == today - 1.0,
                    "tomorrow" => day == today + 1.0,
                    "last7Days" => day > today - 7.0 && day <= today,
                    "thisWeek" => day >= week && day < week + 7.0,
                    "lastWeek" => day >= week - 7.0 && day < week,
                    "nextWeek" => day >= week + 7.0 && day < week + 14.0,
                    "thisMonth" => month_of(day) == Some(shift(0)),
                    "lastMonth" => month_of(day) == Some(shift(-1)),
                    "nextMonth" => month_of(day) == Some(shift(1)),
                    _ => false,
                }
            }
            CfKind::Top10 {
                bottom,
                percent,
                rank,
            } => {
                let Some(number) = number(Some(value)) else {
                    return false;
                };
                let stats = self.stats(index);
                let count = if *percent {
                    ((stats.sorted.len() as f64) * f64::from(*rank) / 100.0)
                        .floor()
                        .max(1.0) as usize
                } else {
                    (*rank).max(1) as usize
                }
                .min(stats.sorted.len());
                if count == 0 {
                    return false;
                }
                if *bottom {
                    number <= stats.sorted[count - 1]
                } else {
                    number >= stats.sorted[stats.sorted.len() - count]
                }
            }
            CfKind::AboveAverage {
                above,
                equal,
                std_dev,
            } => {
                let Some(number) = number(Some(value)) else {
                    return false;
                };
                let stats = self.stats(index);
                if stats.numbers.is_empty() {
                    return false;
                }
                let n = stats.numbers.len() as f64;
                let mean = stats.numbers.iter().sum::<f64>() / n;
                let spread = if *std_dev > 0 && n > 1.0 {
                    let var = stats
                        .numbers
                        .iter()
                        .map(|x| (x - mean).powi(2))
                        .sum::<f64>()
                        / (n - 1.0);
                    var.sqrt() * f64::from(*std_dev)
                } else {
                    0.0
                };
                match (above, equal) {
                    (true, false) => number > mean + spread,
                    (true, true) => number >= mean + spread,
                    (false, false) => number < mean - spread,
                    (false, true) => number <= mean - spread,
                }
            }
            CfKind::Duplicates | CfKind::Uniques => {
                let Some(key) = duplicate_key(value) else {
                    return false;
                };
                let count = self.stats(index).counts.get(&key).copied().unwrap_or(0);
                (count > 1) == matches!(rule.kind, CfKind::Duplicates)
            }
            CfKind::ColorScale { .. } | CfKind::DataBar(_) | CfKind::IconSet(_) => {
                matches!(value, CalcValue::Number(_))
            }
            CfKind::Other(_) => false,
        }
    }

    fn resolve(&self, color: &Color) -> Rgb {
        self.styles.colors.resolve(*color, None).unwrap_or(0)
    }

    /// What conditional formats make of a cell.
    pub fn cell(&self, row: u32, col: u32) -> Option<CellCf> {
        let mut rules: Vec<(usize, &CfRule)> = self
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| block.covers(row, col))
            .flat_map(|(index, block)| block.rules.iter().map(move |rule| (index, rule)))
            .collect();
        if rules.is_empty() {
            return None;
        }
        rules.sort_by_key(|(_, rule)| rule.priority);
        let value = self.value(row, col);
        let mut out = CellCf::default();
        let mut dxf = Dxf::default();
        for (index, rule) in rules {
            if !self.holds(index, rule, row, col, &value) {
                continue;
            }
            let block = &self.blocks[index];
            match &rule.kind {
                CfKind::ColorScale { cfvos, colors } => {
                    if dxf.fill.is_none() && colors.len() >= 2 && cfvos.len() == colors.len() {
                        let stats = self.stats(index);
                        let points: Vec<Option<f64>> = cfvos
                            .iter()
                            .map(|cfvo| self.threshold(block, cfvo, &stats, row, col))
                            .collect();
                        if let (Some(v), true) =
                            (number(Some(&value)), points.iter().all(Option::is_some))
                        {
                            let points: Vec<f64> = points.into_iter().flatten().collect();
                            let colors: Vec<Rgb> =
                                colors.iter().map(|color| self.resolve(color)).collect();
                            let shown = if v <= points[0] {
                                colors[0]
                            } else if v >= points[points.len() - 1] {
                                colors[colors.len() - 1]
                            } else {
                                let at =
                                    points.windows(2).position(|pair| v <= pair[1]).unwrap_or(0);
                                let (a, b) = (points[at], points[at + 1]);
                                let t = if b > a { (v - a) / (b - a) } else { 0.0 };
                                lerp(colors[at], colors[at + 1], t)
                            };
                            dxf.fill = Some(DxfFill {
                                pattern: Pattern::Solid,
                                color: Color {
                                    kind: ColorKind::Rgb(shown),
                                    tint: 0.0,
                                },
                                pattern_color: Color::AUTO,
                            });
                        }
                    }
                }
                CfKind::DataBar(bar) => {
                    if out.bar.is_none() {
                        let stats = self.stats(index);
                        let low = self.threshold(block, &bar.min, &stats, row, col);
                        let high = self.threshold(block, &bar.max, &stats, row, col);
                        if let (Some(v), Some(low), Some(high)) = (number(Some(&value)), low, high)
                        {
                            let (min_len, max_len) = (
                                f64::from(bar.min_length) / 100.0,
                                f64::from(bar.max_length) / 100.0,
                            );
                            let span = (high - low).max(f64::EPSILON);
                            let share = ((v - low) / span).clamp(0.0, 1.0);
                            let both_ways = low < 0.0 && high > 0.0;
                            let (start, end, axis) = if both_ways {
                                // The axis sits where zero falls.
                                let axis = (-low / span) as f32;
                                let here = share as f32;
                                (axis.min(here), axis.max(here), Some(axis))
                            } else {
                                (0.0, (min_len + share * (max_len - min_len)) as f32, None)
                            };
                            let negative = v < 0.0;
                            let color = match (negative, &bar.negative_color) {
                                (true, Some(color)) => self.resolve(color),
                                _ => self.resolve(&bar.color),
                            };
                            out.bar = Some(Bar {
                                start,
                                end,
                                color,
                                gradient: bar.gradient,
                                border: bar.border.map(|border| match border.kind {
                                    ColorKind::Auto => color,
                                    _ => self.resolve(&border),
                                }),
                                axis,
                            });
                            out.hide_value |= !bar.show_value;
                        }
                    }
                }
                CfKind::IconSet(set) => {
                    if out.icon.is_none() {
                        let stats = self.stats(index);
                        if let Some(v) = number(Some(&value)) {
                            let mut place = 0;
                            for (at, cfvo) in set.cfvos.iter().enumerate().skip(1) {
                                let Some(limit) = self.threshold(block, cfvo, &stats, row, col)
                                else {
                                    continue;
                                };
                                if (cfvo.gte && v >= limit) || (!cfvo.gte && v > limit) {
                                    place = at;
                                }
                            }
                            let size = set.cfvos.len().max(1);
                            let place = if set.reverse { size - 1 - place } else { place };
                            let icon = match set.custom.get(place) {
                                Some(Some((name, id))) => Some((name.clone(), *id)),
                                Some(None) => None,
                                None => Some((set.set.clone(), place as u32)),
                            };
                            out.icon = icon;
                            out.hide_value |= !set.show_value;
                        }
                    }
                }
                _ => {
                    let format = rule.own_dxf.clone().or_else(|| {
                        rule.dxf
                            .and_then(|index| self.styles.dxfs.get(index as usize).cloned())
                    });
                    if let Some(format) = format {
                        // The higher rule's settings stand; this one fills in what it left.
                        let mut under = format;
                        under.under(&dxf);
                        dxf = under;
                    }
                }
            }
            if rule.stop {
                break;
            }
        }
        if !dxf.is_empty() {
            out.dxf = Some(dxf);
        }
        (out != CellCf::default()).then_some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_and_colours_go_between_as_excels_do() {
        let sorted = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&sorted, 0.5), Some(3.0));
        assert_eq!(percentile(&sorted, 0.25), Some(2.0));
        assert_eq!(lerp(0xF8696B, 0x63BE7B, 0.0), 0xF8696B);
        assert_eq!(lerp(0x000000, 0xFFFFFF, 0.5), 0x808080);
    }
}
