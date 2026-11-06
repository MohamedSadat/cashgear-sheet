//! Autofilters, on a sheet or a table: what each column keeps, and the rows that hides. Read off
//! Excel 2016's own filters over COM.

use std::collections::{BTreeMap, HashMap};

use cgs_calc::{CalcEngine, CalcValue, CellSink, Rect as CalcRect, SheetRect};
use cgs_model::{AutoFilter, DateGroup, FilterColumn, FilterCriteria, FilterOp, LineInfo, Rect};
use cgs_numfmt::{NumFmt, Options};
use serde::Deserialize;

use crate::command::{Applied, Area, Op};
use crate::Session;

/// What a column should keep, as the UI asks for it.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FilterSpec {
    Values {
        #[serde(default)]
        values: Vec<String>,
        #[serde(default)]
        blank: bool,
        /// `[year]`, `[year, month]` or `[year, month, day]`.
        #[serde(default)]
        dates: Vec<Vec<u32>>,
    },
    Custom {
        #[serde(default)]
        and: bool,
        conditions: Vec<ConditionSpec>,
    },
    Top10 {
        #[serde(default = "yes")]
        top: bool,
        #[serde(default)]
        percent: bool,
        value: f64,
    },
    /// `aboveAverage`, `belowAverage`, `today`, `thisMonth` and the rest of Excel's names.
    Dynamic { name: String },
    /// Cells with this fill, or text in this colour: `RRGGBB`, or none for no fill.
    Color {
        #[serde(default = "yes")]
        cell: bool,
        #[serde(default)]
        color: Option<String>,
    },
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConditionSpec {
    /// `equal`, `notEqual`, `lessThan`, `lessThanOrEqual`, `greaterThan`, `greaterThanOrEqual`.
    pub op: String,
    pub value: String,
}

/// Which filter a command is about: a table's, or else the sheet's own.
#[derive(Debug, Clone, Copy)]
enum Which<'a> {
    Sheet(u32),
    Table(&'a str),
}

/// Values read from a block of cells, by place.
#[derive(Default)]
struct Block(HashMap<(u32, u32), CalcValue>);

impl CellSink for Block {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        if *value != CalcValue::Empty {
            self.0.insert((row, col), value.clone());
        }
    }
}

/// One column's values, by row from `first`.
struct Column {
    first: u32,
    values: Vec<Option<CalcValue>>,
}

impl CellSink for Column {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.take(row, col, value.clone());
    }

    fn take(&mut self, row: u32, _col: u32, value: CalcValue) {
        if value != CalcValue::Empty {
            if let Some(slot) = self.values.get_mut((row - self.first) as usize) {
                *slot = Some(value);
            }
        }
    }
}

/// What a test made of each value and format, looked up without copying the value.
#[derive(Default)]
struct Known {
    numbers: HashMap<(u32, u64), bool>,
    texts: HashMap<u32, HashMap<String, bool>>,
}

impl Known {
    fn get(&self, num_fmt: u32, value: &CalcValue) -> Option<bool> {
        match value {
            CalcValue::Number(number) => self.numbers.get(&(num_fmt, number.to_bits())).copied(),
            CalcValue::Text(text) => self.texts.get(&num_fmt)?.get(text.as_str()).copied(),
            _ => None,
        }
    }

    fn put(&mut self, num_fmt: u32, value: &CalcValue, answer: bool) {
        match value {
            CalcValue::Number(number) => {
                self.numbers.insert((num_fmt, number.to_bits()), answer);
            }
            CalcValue::Text(text) => {
                self.texts
                    .entry(num_fmt)
                    .or_default()
                    .insert(text.clone(), answer);
            }
            _ => {}
        }
    }
}

/// Excel's `*` and `?`, with `~` before either to mean itself. Case doesn't count.
pub(crate) fn wildcard(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    fn walk(p: &[char], t: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('*') => (0..=t.len()).any(|skip| walk(&p[1..], &t[skip..])),
            Some('?') => !t.is_empty() && walk(&p[1..], &t[1..]),
            Some('~') if matches!(p.get(1), Some('*' | '?' | '~')) => {
                t.first() == p.get(1) && walk(&p[2..], &t[1..])
            }
            Some(ch) => t.first() == Some(ch) && walk(&p[1..], &t[1..]),
        }
    }
    walk(&pattern, &text)
}

fn op_of(text: &str) -> FilterOp {
    FilterOp::from_xml(text)
}

/// A date serial's year, month and day, in the 1900 system.
fn ymd(serial: f64, date1904: bool) -> Option<(u32, u32, u32)> {
    let days = serial.floor() as i64 + if date1904 { 1462 } else { 0 };
    if days < 1 {
        return None;
    }
    // Excel's 1900 has a 29 February that never was.
    let days = if days >= 60 { days - 1 } else { days };
    let z = days + 693_594 + 306;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some((y as u32, m as u32, d as u32))
}

/// The serial of a date, in the 1900 system.
fn serial_of(year: i64, month: i64, day: i64) -> f64 {
    let (y, m) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 693_594 - 306;
    let days = if days >= 60 { days + 1 } else { days };
    days as f64
}

impl Session {
    fn which<'a>(&self, sheet: u32, table: &'a Option<String>) -> Which<'a> {
        match table {
            Some(name) => Which::Table(name),
            None => Which::Sheet(sheet),
        }
    }

    fn filter_of(&self, which: Which<'_>) -> Option<(u32, AutoFilter)> {
        match which {
            Which::Sheet(sheet) => {
                let info = self.book.sheets.get(sheet as usize)?;
                info.filter.clone().map(|filter| (sheet, filter))
            }
            Which::Table(name) => {
                let table = self
                    .book
                    .tables
                    .iter()
                    .find(|table| table.name.eq_ignore_ascii_case(name))?;
                table.filter.clone().map(|filter| (table.sheet, filter))
            }
        }
    }

    /// The op that puts a filter in place.
    fn set_filter_op(&self, which: Which<'_>, filter: Option<AutoFilter>) -> Result<Op, String> {
        match which {
            Which::Sheet(sheet) => Ok(Op::SheetFilter { sheet, filter }),
            Which::Table(name) => {
                let mut tables = self.book.tables.clone();
                let table = tables
                    .iter_mut()
                    .find(|table| table.name.eq_ignore_ascii_case(name))
                    .ok_or_else(|| format!("There is no table called {name}."))?;
                table.filter = filter;
                table.edited = true;
                Ok(Op::BookTables(tables))
            }
        }
    }

    /// Text a cell shows, as a value filter lists and matches it.
    fn shown(
        &self,
        value: &CalcValue,
        num_fmt: u32,
        formats: &mut HashMap<u32, NumFmt>,
        options: &Options<'_>,
    ) -> String {
        let format = formats.entry(num_fmt).or_insert_with(|| {
            match self.book.styles.num_fmts.get(&num_fmt) {
                Some(code) => NumFmt::parse(code),
                None => NumFmt::builtin(num_fmt).unwrap_or_else(NumFmt::general),
            }
        });
        let mut out = String::new();
        let shown = match value {
            CalcValue::Number(number) => cgs_numfmt::Value::Number(*number),
            CalcValue::Text(text) => cgs_numfmt::Value::Text(text),
            CalcValue::Bool(flag) => cgs_numfmt::Value::Bool(*flag),
            CalcValue::Error(text) => cgs_numfmt::Value::Error(text),
            CalcValue::Empty => return out,
        };
        let _ = format.format(shown, options, &mut out);
        out.trim().to_string()
    }

    fn is_date(&self, num_fmt: u32) -> bool {
        match self.book.styles.num_fmts.get(&num_fmt) {
            Some(code) => NumFmt::parse(code).is_date(),
            None => NumFmt::builtin(num_fmt).is_some_and(|format| format.is_date()),
        }
    }

    /// Which data rows of a filter show, given the values of its columns.
    fn rows_kept(
        &self,
        sheet: u32,
        filter: &AutoFilter,
        options: &Options<'_>,
    ) -> Vec<(u32, bool)> {
        let r = filter.range;
        let first = r.top + 1;
        if first > r.bottom {
            return Vec::new();
        }
        let info = &self.book.sheets[sheet as usize];
        let mut keep: Vec<bool> = vec![true; (r.bottom - first + 1) as usize];
        let mut formats = HashMap::new();
        for column in &filter.columns {
            let Some(criteria) = &column.criteria else {
                continue;
            };
            if matches!(
                criteria,
                FilterCriteria::Other(_) | FilterCriteria::Icon { .. }
            ) {
                continue;
            }
            let col = r.left + column.col_id;
            let mut block = Column {
                first,
                values: vec![None; (r.bottom - first + 1) as usize],
            };
            self.calc.read_block(
                SheetRect {
                    sheet,
                    rect: CalcRect {
                        top: first,
                        left: col,
                        bottom: r.bottom,
                        right: col,
                    },
                },
                &mut block,
            );
            let value_at = |row: u32| block.values[(row - first) as usize].as_ref();
            // What each value and format came to, for a test that only looks at them.
            let mut known = Known::default();
            let mut last_style = (u32::MAX, 0u32);
            // What a whole column decides, worked out once: a top ten's cut-off and an average.
            let numbers: Vec<f64> = (first..=r.bottom)
                .filter_map(|row| match value_at(row) {
                    Some(CalcValue::Number(number)) => Some(*number),
                    _ => None,
                })
                .collect();
            let cut = match criteria {
                FilterCriteria::Top10 {
                    top,
                    percent,
                    value,
                    ..
                } => {
                    let mut sorted = numbers.clone();
                    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
                    if !top {
                        sorted.reverse();
                    }
                    let count = if *percent {
                        ((sorted.len() as f64) * value / 100.0).floor().max(1.0) as usize
                    } else {
                        value.max(1.0) as usize
                    };
                    sorted
                        .get(count.min(sorted.len()).saturating_sub(1))
                        .copied()
                }
                _ => None,
            };
            let average = if numbers.is_empty() {
                None
            } else {
                Some(numbers.iter().sum::<f64>() / numbers.len() as f64)
            };
            let (date_from, date_to) = match criteria {
                FilterCriteria::Dynamic { kind, .. } => self.dynamic_dates(kind),
                _ => (None, None),
            };
            let by_value = matches!(
                criteria,
                FilterCriteria::Values { .. } | FilterCriteria::Custom { .. }
            );
            for (at, row) in (first..=r.bottom).enumerate() {
                if !keep[at] {
                    continue;
                }
                let value = value_at(row);
                let style = info.style_at(row, col);
                let num_fmt = if style == last_style.0 {
                    last_style.1
                } else {
                    let num_fmt = self.book.styles.xf(style).num_fmt;
                    last_style = (style, num_fmt);
                    num_fmt
                };
                if by_value {
                    if let Some(answer) = value.and_then(|value| known.get(num_fmt, value)) {
                        keep[at] = answer;
                        continue;
                    }
                }
                keep[at] = match criteria {
                    FilterCriteria::Values {
                        values,
                        blank,
                        dates,
                    } => match value {
                        None => *blank,
                        Some(CalcValue::Text(text)) if text.is_empty() => *blank,
                        Some(value) => {
                            let date_hit = match value {
                                CalcValue::Number(serial)
                                    if !dates.is_empty() && self.is_date(num_fmt) =>
                                {
                                    ymd(*serial, self.book.date1904).is_some_and(|(y, m, d)| {
                                        dates.iter().any(|group| {
                                            u32::from(group.year) == y
                                                && group
                                                    .month
                                                    .is_none_or(|month| u32::from(month) == m)
                                                && group.day.is_none_or(|day| u32::from(day) == d)
                                        })
                                    })
                                }
                                _ => false,
                            };
                            date_hit || {
                                let text = self.shown(value, num_fmt, &mut formats, options);
                                values.iter().any(|wanted| {
                                    wanted.eq_ignore_ascii_case(&text)
                                        || wanted.to_lowercase() == text.to_lowercase()
                                })
                            }
                        }
                    },
                    FilterCriteria::Custom { and, conditions } => {
                        let results = conditions.iter().map(|(op, wanted)| {
                            self.condition_holds(*op, wanted, value, num_fmt, &mut formats, options)
                        });
                        let results: Vec<bool> = results.collect();
                        if *and {
                            results.iter().all(|&hit| hit)
                        } else {
                            results.iter().any(|&hit| hit)
                        }
                    }
                    FilterCriteria::Top10 { top, .. } => match (value, cut) {
                        (Some(CalcValue::Number(number)), Some(cut)) => {
                            if *top {
                                *number >= cut
                            } else {
                                *number <= cut
                            }
                        }
                        _ => false,
                    },
                    FilterCriteria::Dynamic { kind, .. } => match (value, kind.as_str()) {
                        (Some(CalcValue::Number(number)), "aboveAverage") => {
                            average.is_some_and(|average| *number > average)
                        }
                        (Some(CalcValue::Number(number)), "belowAverage") => {
                            average.is_some_and(|average| *number < average)
                        }
                        (Some(CalcValue::Number(number)), _) => match (date_from, date_to) {
                            (Some(from), Some(to)) => *number >= from && *number < to,
                            _ => false,
                        },
                        _ => false,
                    },
                    FilterCriteria::Color { cell, dxf } => {
                        let wanted = dxf
                            .and_then(|index| self.book.styles.dxfs.get(index as usize))
                            .and_then(|dxf| {
                                if *cell {
                                    dxf.fill.and_then(|fill| {
                                        self.book.styles.colors.resolve(fill.color, None)
                                    })
                                } else {
                                    dxf.font.color.and_then(|color| {
                                        self.book.styles.colors.resolve(color, None)
                                    })
                                }
                            });
                        self.shown_color(sheet, row, col, !*cell) == wanted
                    }
                    FilterCriteria::Icon { .. } | FilterCriteria::Other(_) => true,
                };
                if by_value {
                    if let Some(value) = value {
                        known.put(num_fmt, value, keep[at]);
                    }
                }
            }
        }
        (first..=r.bottom).zip(keep).collect()
    }

    fn condition_holds(
        &self,
        op: FilterOp,
        wanted: &str,
        value: Option<&CalcValue>,
        num_fmt: u32,
        formats: &mut HashMap<u32, NumFmt>,
        options: &Options<'_>,
    ) -> bool {
        let number = wanted.trim().parse::<f64>().ok();
        if let (Some(target), Some(CalcValue::Number(actual))) = (number, value) {
            return match op {
                FilterOp::Equal => actual == &target,
                FilterOp::NotEqual => actual != &target,
                FilterOp::Less => *actual < target,
                FilterOp::LessOrEqual => *actual <= target,
                FilterOp::Greater => *actual > target,
                FilterOp::GreaterOrEqual => *actual >= target,
            };
        }
        let text = value.map_or_else(String::new, |value| {
            self.shown(value, num_fmt, formats, options)
        });
        match op {
            FilterOp::Equal => wildcard(wanted, &text),
            FilterOp::NotEqual => !wildcard(wanted, &text),
            // Text compares with text only, in sort order.
            _ => {
                if number.is_some() || matches!(value, Some(CalcValue::Number(_)) | None) {
                    return false;
                }
                let a = crate::sort::text_key(&text, false);
                let b = crate::sort::text_key(wanted, false);
                match op {
                    FilterOp::Less => a < b,
                    FilterOp::LessOrEqual => a <= b,
                    FilterOp::Greater => a > b,
                    _ => a >= b,
                }
            }
        }
    }

    /// The days a dynamic date filter keeps, as a serial range `[from, to)`.
    fn dynamic_dates(&self, kind: &str) -> (Option<f64>, Option<f64>) {
        let today = self.today.floor();
        let Some((y, m, _)) = ymd(today, false) else {
            return (None, None);
        };
        let (y, m) = (i64::from(y), i64::from(m));
        let month = |year: i64, month: i64| {
            let total = year * 12 + month - 1;
            serial_of(total.div_euclid(12), total.rem_euclid(12) + 1, 1)
        };
        // Weeks start on Sunday, as in Excel. Serial 1 was a Sunday.
        let week_start = today - ((today as i64 - 1).rem_euclid(7)) as f64;
        let quarter = (m - 1) / 3;
        let range = |from: f64, to: f64| (Some(from), Some(to));
        match kind {
            "today" => range(today, today + 1.0),
            "yesterday" => range(today - 1.0, today),
            "tomorrow" => range(today + 1.0, today + 2.0),
            "thisWeek" => range(week_start, week_start + 7.0),
            "lastWeek" => range(week_start - 7.0, week_start),
            "nextWeek" => range(week_start + 7.0, week_start + 14.0),
            "thisMonth" => range(month(y, m), month(y, m + 1)),
            "lastMonth" => range(month(y, m - 1), month(y, m)),
            "nextMonth" => range(month(y, m + 1), month(y, m + 2)),
            "thisQuarter" => range(month(y, quarter * 3 + 1), month(y, quarter * 3 + 4)),
            "lastQuarter" => range(month(y, quarter * 3 - 2), month(y, quarter * 3 + 1)),
            "nextQuarter" => range(month(y, quarter * 3 + 4), month(y, quarter * 3 + 7)),
            "thisYear" => range(month(y, 1), month(y + 1, 1)),
            "lastYear" => range(month(y - 1, 1), month(y, 1)),
            "nextYear" => range(month(y + 1, 1), month(y + 2, 1)),
            "yearToDate" => range(month(y, 1), today + 1.0),
            _ => (None, None),
        }
    }

    /// The rows a filter hides and shows, as a change to the sheet's rows.
    fn rows_op(&self, sheet: u32, filter: &AutoFilter, options: &Options<'_>) -> Option<Op> {
        let info = &self.book.sheets[sheet as usize];
        let mut rows = Vec::new();
        // The sheet's row records walked alongside, as both are in order.
        let records = &info.rows.rows;
        let mut at = records.partition_point(|&(row, _)| row < filter.range.top);
        for (row, keep) in self.rows_kept(sheet, filter, options) {
            while at < records.len() && records[at].0 < row {
                at += 1;
            }
            let line = match records.get(at) {
                Some(&(found, line)) if found == row => line,
                _ => LineInfo::default(),
            };
            if line.hidden == !keep {
                continue;
            }
            rows.push((
                row,
                LineInfo {
                    hidden: !keep,
                    ..line
                },
            ));
        }
        (!rows.is_empty()).then_some(Op::Lines {
            sheet,
            rows,
            cols: Vec::new(),
        })
    }

    /// A sheet filter's range, grown down over data added just below it, as Excel's reapply does.
    fn grown(&self, sheet: u32, mut filter: AutoFilter) -> AutoFilter {
        let info = &self.book.sheets[sheet as usize];
        let r = filter.range;
        let mut bottom = r.bottom;
        while bottom < info.used.0 {
            let mut block = Block::default();
            self.calc.read_block(
                SheetRect {
                    sheet,
                    rect: CalcRect {
                        top: bottom + 1,
                        left: r.left,
                        bottom: bottom + 1,
                        right: r.right,
                    },
                },
                &mut block,
            );
            if block.0.is_empty() {
                break;
            }
            bottom += 1;
        }
        filter.range.bottom = bottom;
        filter
    }

    /// Turns the sheet's filter buttons on over the block around a selection, or off. In a table it
    /// turns that table's buttons on or off instead.
    pub(crate) fn toggle_filter_command(
        &mut self,
        sheet: u32,
        area: Area,
    ) -> Result<Applied, String> {
        self.sheet_for_edit(sheet)?;
        if let Some(table) = self
            .table_at(sheet, area.top, area.left)
            .map(|table| table.name.clone())
        {
            let on = self
                .book
                .tables
                .iter()
                .find(|look| look.name.eq_ignore_ascii_case(&table))
                .is_some_and(|look| look.filter.is_some());
            let mut list = Vec::new();
            if on {
                if let Some((_, filter)) = self.filter_of(Which::Table(&table)) {
                    list.extend(self.show_all_op(sheet, &filter));
                }
            }
            let options = crate::tables::TableOptions {
                filter_button: Some(!on),
                ..Default::default()
            };
            if list.is_empty() {
                return self.table_options_command(&table, options);
            }
            let mut tables = self.book.tables.clone();
            if let Some(look) = tables
                .iter_mut()
                .find(|look| look.name.eq_ignore_ascii_case(&table))
            {
                look.filter = None;
                look.edited = true;
            }
            list.push(Op::BookTables(tables));
            return self.perform("filter", Op::Many(list), Some((sheet, area)));
        }
        let info = &self.book.sheets[sheet as usize];
        if let Some(filter) = info.filter.clone() {
            let mut list = self
                .show_all_op(sheet, &filter)
                .into_iter()
                .collect::<Vec<_>>();
            list.push(Op::SheetFilter {
                sheet,
                filter: None,
            });
            return self.perform("filter", Op::Many(list), Some((sheet, area)));
        }
        let region = if area.rows() == 1 && area.cols() == 1 {
            self.current_region(sheet, area.top, area.left)
        } else {
            area
        };
        let region = Area {
            bottom: region.bottom.min(info.used.0.max(region.top)),
            right: region.right.min(info.used.1.max(region.left)),
            ..region
        };
        let filter = AutoFilter::new(Rect {
            top: region.top,
            left: region.left,
            bottom: region.bottom,
            right: region.right,
        });
        self.perform(
            "filter",
            Op::SheetFilter {
                sheet,
                filter: Some(filter),
            },
            Some((sheet, region)),
        )
    }

    fn show_all_op(&self, sheet: u32, filter: &AutoFilter) -> Option<Op> {
        let info = &self.book.sheets[sheet as usize];
        let records = &info.rows.rows;
        let start = records.partition_point(|&(row, _)| row <= filter.range.top);
        let rows: Vec<(u32, LineInfo)> = records[start..]
            .iter()
            .take_while(|&&(row, _)| row <= filter.range.bottom)
            .filter(|(_, line)| line.hidden)
            .map(|&(row, line)| {
                (
                    row,
                    LineInfo {
                        hidden: false,
                        ..line
                    },
                )
            })
            .collect();
        (!rows.is_empty()).then_some(Op::Lines {
            sheet,
            rows,
            cols: Vec::new(),
        })
    }

    /// Sets what one column keeps, or nothing, and filters again.
    pub(crate) fn set_filter_command(
        &mut self,
        sheet: u32,
        table: &Option<String>,
        col: u32,
        spec: Option<FilterSpec>,
        locale: &cgs_numfmt::Locale,
    ) -> Result<Applied, String> {
        let which = self.which(sheet, table);
        let (sheet, filter) = self
            .filter_of(which)
            .ok_or_else(|| "There is no filter here.".to_string())?;
        let filter = match which {
            Which::Sheet(_) => self.grown(sheet, filter),
            Which::Table(_) => filter,
        };
        if col < filter.range.left || col > filter.range.right {
            return Err("That column isn't in the filter.".to_string());
        }
        let col_id = col - filter.range.left;
        let criteria = match spec {
            None => None,
            Some(spec) => Some(self.criteria(spec)?),
        };
        let mut next = filter.clone();
        next.columns.retain(|column| column.col_id != col_id);
        if criteria.is_some() {
            next.columns.push(FilterColumn {
                col_id,
                hidden_button: false,
                criteria,
            });
            next.columns.sort_by_key(|column| column.col_id);
        }
        self.apply_filter(which, sheet, next, locale)
    }

    fn apply_filter(
        &mut self,
        which: Which<'_>,
        sheet: u32,
        mut filter: AutoFilter,
        locale: &cgs_numfmt::Locale,
    ) -> Result<Applied, String> {
        let options = Options {
            locale,
            date1904: self.book.date1904,
        };
        for column in &mut filter.columns {
            if let Some(FilterCriteria::Dynamic {
                kind,
                value,
                max_value,
            }) = &mut column.criteria
            {
                if kind != "aboveAverage" && kind != "belowAverage" {
                    let (from, to) = self.dynamic_dates(kind);
                    *value = from;
                    *max_value = to;
                }
            }
        }
        let mut list = Vec::new();
        list.extend(self.rows_op(sheet, &filter, &options));
        let area = Area {
            top: filter.range.top,
            left: filter.range.left,
            bottom: filter.range.bottom,
            right: filter.range.right,
        };
        list.push(self.set_filter_op(which, Some(filter))?);
        self.perform("filter", Op::Many(list), Some((sheet, area)))
    }

    /// Takes every column's criteria away, and shows the rows.
    pub(crate) fn clear_filter_command(
        &mut self,
        sheet: u32,
        table: &Option<String>,
    ) -> Result<Applied, String> {
        let which = self.which(sheet, table);
        let (sheet, mut filter) = self
            .filter_of(which)
            .ok_or_else(|| "There is no filter here.".to_string())?;
        let mut list: Vec<Op> = self.show_all_op(sheet, &filter).into_iter().collect();
        filter.columns.clear();
        list.push(self.set_filter_op(which, Some(filter))?);
        self.perform("filter", Op::Many(list), None)
    }

    /// Filters again, for what changed since.
    pub(crate) fn reapply_filter_command(
        &mut self,
        sheet: u32,
        table: &Option<String>,
        locale: &cgs_numfmt::Locale,
    ) -> Result<Applied, String> {
        let which = self.which(sheet, table);
        let (sheet, filter) = self
            .filter_of(which)
            .ok_or_else(|| "There is no filter here.".to_string())?;
        let filter = match which {
            Which::Sheet(_) => self.grown(sheet, filter),
            Which::Table(_) => filter,
        };
        self.apply_filter(which, sheet, filter, locale)
    }

    fn criteria(&mut self, spec: FilterSpec) -> Result<FilterCriteria, String> {
        Ok(match spec {
            FilterSpec::Values {
                values,
                blank,
                dates,
            } => FilterCriteria::Values {
                values,
                blank,
                dates: dates
                    .iter()
                    .filter_map(|parts| {
                        Some(DateGroup {
                            year: *parts.first()? as u16,
                            month: parts.get(1).map(|&month| month as u8),
                            day: parts.get(2).map(|&day| day as u8),
                            hour: None,
                            minute: None,
                            second: None,
                        })
                    })
                    .collect(),
            },
            FilterSpec::Custom { and, conditions } => {
                if conditions.is_empty() || conditions.len() > 2 {
                    return Err("A custom filter has one or two conditions.".to_string());
                }
                FilterCriteria::Custom {
                    and,
                    conditions: conditions
                        .into_iter()
                        .map(|condition| (op_of(&condition.op), condition.value))
                        .collect(),
                }
            }
            FilterSpec::Top10 {
                top,
                percent,
                value,
            } => {
                if !(1.0..=500.0).contains(&value) {
                    return Err("Top 10 takes a number from 1 to 500.".to_string());
                }
                FilterCriteria::Top10 {
                    top,
                    percent,
                    value,
                    filter_value: None,
                }
            }
            FilterSpec::Dynamic { name } => FilterCriteria::Dynamic {
                kind: name,
                value: None,
                max_value: None,
            },
            FilterSpec::Color { cell, color } => {
                let dxf = self.color_dxf(cell, color.as_deref());
                FilterCriteria::Color {
                    cell,
                    dxf: Some(dxf),
                }
            }
        })
    }

    /// A differential format holding just a colour, for a filter or sort by colour to name.
    fn color_dxf(&mut self, cell: bool, color: Option<&str>) -> u32 {
        let rgb = color.and_then(|text| u32::from_str_radix(text.trim_start_matches('#'), 16).ok());
        let mut dxf = cgs_model::Dxf::default();
        let color = cgs_model::Color {
            kind: rgb.map_or(cgs_model::ColorKind::Auto, cgs_model::ColorKind::Rgb),
            tint: 0.0,
        };
        if cell {
            dxf.fill = Some(cgs_model::DxfFill {
                pattern: cgs_model::Pattern::Solid,
                color,
                pattern_color: cgs_model::Color::AUTO,
            });
        } else {
            dxf.font.color = Some(color);
        }
        self.add_dxf(dxf)
    }

    /// A filter's column list for its dropdown: each shown value with whether it's kept, blanks,
    /// and dates by year, month and day.
    pub(crate) fn filter_list(
        &self,
        sheet: u32,
        table: &Option<String>,
        col: u32,
        options: &Options<'_>,
    ) -> Result<serde_json::Value, String> {
        let which = self.which(sheet, table);
        let (sheet, filter) = self
            .filter_of(which)
            .ok_or_else(|| "There is no filter here.".to_string())?;
        if col < filter.range.left || col > filter.range.right {
            return Err("That column isn't in the filter.".to_string());
        }
        let col_id = col - filter.range.left;
        let criteria = filter
            .column(col_id)
            .and_then(|column| column.criteria.clone());
        // Other columns' filters decide which values are offered, as in Excel.
        let mut others = filter.clone();
        others.columns.retain(|column| column.col_id != col_id);
        let kept: HashMap<u32, bool> = self
            .rows_kept(sheet, &others, options)
            .into_iter()
            .collect();
        let info = &self.book.sheets[sheet as usize];
        let first = filter.range.top + 1;
        let mut block = Block::default();
        if first <= filter.range.bottom {
            self.calc.read_block(
                SheetRect {
                    sheet,
                    rect: CalcRect {
                        top: first,
                        left: col,
                        bottom: filter.range.bottom,
                        right: col,
                    },
                },
                &mut block,
            );
        }
        let mut formats = HashMap::new();
        let mut texts: BTreeMap<crate::sort::TextKey, String> = BTreeMap::new();
        let mut numbers: Vec<(f64, String)> = Vec::new();
        let mut dates: BTreeMap<(u32, u32, u32), ()> = BTreeMap::new();
        let mut blanks = false;
        for row in first..=filter.range.bottom {
            if !kept.get(&row).copied().unwrap_or(true) {
                continue;
            }
            let num_fmt = self.book.styles.xf(info.style_at(row, col)).num_fmt;
            match block.0.get(&(row, col)) {
                None => blanks = true,
                Some(CalcValue::Text(text)) if text.is_empty() => blanks = true,
                Some(CalcValue::Number(serial)) if self.is_date(num_fmt) => {
                    if let Some(day) = ymd(*serial, self.book.date1904) {
                        dates.insert(day, ());
                    }
                }
                Some(value) => {
                    let text = self.shown(value, num_fmt, &mut formats, options);
                    match value {
                        CalcValue::Number(number) => {
                            if !numbers
                                .iter()
                                .any(|(_, known)| known.eq_ignore_ascii_case(&text))
                            {
                                numbers.push((*number, text));
                            }
                        }
                        _ => {
                            texts
                                .entry(crate::sort::text_key(&text, false))
                                .or_insert(text);
                        }
                    }
                }
            }
        }
        numbers.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let checked = |text: &str| match &criteria {
            None => true,
            Some(FilterCriteria::Values { values, .. }) => values
                .iter()
                .any(|value| value.to_lowercase() == text.to_lowercase()),
            Some(_) => false,
        };
        let date_checked = |(y, m, d): (u32, u32, u32)| match &criteria {
            None => true,
            Some(FilterCriteria::Values { dates, .. }) => dates.iter().any(|group| {
                u32::from(group.year) == y
                    && group.month.is_none_or(|month| u32::from(month) == m)
                    && group.day.is_none_or(|day| u32::from(day) == d)
            }),
            Some(_) => false,
        };
        let values: Vec<serde_json::Value> = numbers
            .iter()
            .map(|(_, text)| text.clone())
            .chain(texts.into_values())
            .map(|text| serde_json::json!({ "text": text, "checked": checked(&text) }))
            .collect();
        let dates: Vec<serde_json::Value> = dates
            .keys()
            .map(|&(y, m, d)| serde_json::json!({ "date": [y, m, d], "checked": date_checked((y, m, d)) }))
            .collect();
        let blank_checked = match &criteria {
            None => true,
            Some(FilterCriteria::Values { blank, .. }) => *blank,
            Some(_) => false,
        };
        let kind = match &criteria {
            None => "none",
            Some(FilterCriteria::Values { .. }) => "values",
            Some(FilterCriteria::Custom { .. }) => "custom",
            Some(FilterCriteria::Top10 { .. }) => "top10",
            Some(FilterCriteria::Dynamic { .. }) => "dynamic",
            Some(FilterCriteria::Color { .. }) => "color",
            Some(FilterCriteria::Icon { .. }) => "icon",
            Some(FilterCriteria::Other(_)) => "other",
        };
        Ok(serde_json::json!({
            "values": values,
            "dates": dates,
            "blanks": blanks,
            "blanksChecked": blank_checked,
            "filtered": kind,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_match_as_excels_do() {
        assert!(wildcard("a*", "Apple"));
        assert!(wildcard("*ple", "apple"));
        assert!(wildcard("a?ple", "ample"));
        assert!(!wildcard("a?ple", "aple"));
        assert!(wildcard("~*", "*"));
        assert!(!wildcard("~*", "x"));
        assert!(wildcard("*", ""));
    }

    #[test]
    fn dates_go_both_ways() {
        assert_eq!(ymd(45306.0, false), Some((2024, 1, 15)));
        assert_eq!(serial_of(2024, 1, 15), 45306.0);
        assert_eq!(ymd(61.0, false), Some((1900, 3, 1)));
        assert_eq!(serial_of(1900, 3, 1), 61.0);
    }
}
