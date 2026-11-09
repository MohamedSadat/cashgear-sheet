//! Grouping in pivot tables, made here as Excel 2016 makes it over COM: dates by period into new
//! fields named "Months (Date)" and so on, numbers by steps on the field itself, and items picked
//! by hand into a new field named after the one they group ("Product2"). Grouping belongs to the
//! cache, so every table on it gets the new fields; the table grouped from puts them where the
//! field was. Ungroup takes a grouping away again.

use std::collections::HashSet;

use cgs_model::pivot::{
    CacheField, CacheValue, DateGrouping, FieldGroup, GroupRange, ItemType, PivotAxis, PivotCache,
    PivotItem, PivotTable,
};
use cgs_pivot::CacheView;
use serde::Deserialize;

use crate::command::{Applied, Op};
use crate::pivots::{pivot_area, remap_table, PivotRef};
use crate::Session;

/// What the Grouping dialog, or Group Selection on items, asks for.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GroupSpec {
    /// Dates by the periods named: "days", "months", "quarters" or "years". A start or end left
    /// out is the data's own, as the dialog's Auto boxes give it.
    Dates {
        parts: Vec<String>,
        #[serde(default)]
        start: Option<f64>,
        #[serde(default)]
        end: Option<f64>,
    },
    /// Numbers in steps of `by`.
    Numbers {
        #[serde(default)]
        start: Option<f64>,
        #[serde(default)]
        end: Option<f64>,
        by: f64,
    },
    /// The field's items at these positions, as the pivot table lists them, as one group.
    Items { items: Vec<u32> },
}

const CANT_GROUP: &str = "Cannot group that selection.";
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
/// Days in each month of a leap year, as Excel's day groups list every day of one.
const MONTH_DAYS: [u32; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn part_of(name: &str) -> Option<DateGrouping> {
    match name {
        "days" => Some(DateGrouping::Days),
        "months" => Some(DateGrouping::Months),
        "quarters" => Some(DateGrouping::Quarters),
        "years" => Some(DateGrouping::Years),
        _ => None,
    }
}

fn part_name(by: DateGrouping) -> &'static str {
    match by {
        DateGrouping::Seconds => "Seconds",
        DateGrouping::Minutes => "Minutes",
        DateGrouping::Hours => "Hours",
        DateGrouping::Days => "Days",
        DateGrouping::Months => "Months",
        DateGrouping::Quarters => "Quarters",
        DateGrouping::Years => "Years",
    }
}

/// Finest first, as Excel's list has them.
fn rank(by: DateGrouping) -> u8 {
    match by {
        DateGrouping::Seconds => 0,
        DateGrouping::Minutes => 1,
        DateGrouping::Hours => 2,
        DateGrouping::Days => 3,
        DateGrouping::Months => 4,
        DateGrouping::Quarters => 5,
        DateGrouping::Years => 6,
    }
}

fn text(value: &str) -> CacheValue {
    CacheValue::Text(value.to_string())
}

/// A name no field of the cache has yet.
fn unused_name(cache: &PivotCache, wanted: &str) -> String {
    let taken = |name: &str| {
        cache
            .fields
            .iter()
            .any(|field| field.name.eq_ignore_ascii_case(name))
    };
    if !taken(wanted) {
        return wanted.to_string();
    }
    (2..)
        .map(|n| format!("{wanted}{n}"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| wanted.to_string())
}

fn group_field(name: String, group: FieldGroup) -> CacheField {
    CacheField {
        name,
        num_fmt_id: 0,
        database: false,
        group: Some(group),
        ..CacheField::default()
    }
}

impl Session {
    /// The year of a date serial, through the Years group's own reckoning.
    fn year_of(&self, serial: f64) -> i64 {
        self.format_number(serial.floor(), Some(14))
            .rsplit('/')
            .next()
            .and_then(|year| year.parse().ok())
            .unwrap_or(1900)
    }

    /// A date group's items: "<start", each period, ">end", as Excel labels them in English.
    fn date_items(&self, by: DateGrouping, start: f64, end: f64) -> Vec<CacheValue> {
        let mut items = vec![text(&format!("<{}", self.format_number(start, Some(14))))];
        match by {
            DateGrouping::Days => {
                for (month, days) in MONTH_DAYS.iter().enumerate() {
                    for day in 1..=*days {
                        items.push(text(&format!("{day}-{}", MONTHS[month])));
                    }
                }
            }
            DateGrouping::Months => items.extend(MONTHS.iter().map(|month| text(month))),
            DateGrouping::Quarters => items.extend((1..=4).map(|q| text(&format!("Qtr{q}")))),
            DateGrouping::Years => {
                let (first, last) = (self.year_of(start), self.year_of(end));
                items.extend((first..=last.max(first)).map(|year| text(&year.to_string())));
            }
            DateGrouping::Seconds | DateGrouping::Minutes | DateGrouping::Hours => {}
        }
        items.push(text(&format!(">{}", self.format_number(end, Some(14)))));
        items
    }

    /// A number group's items: "<start", "0-49" and on to "200-250", ">end", as Excel labels them.
    fn number_items(&self, start: f64, end: f64, by: f64) -> Vec<CacheValue> {
        let general = |n: f64| self.format_number(n, None);
        let whole = start.fract() == 0.0 && by.fract() == 0.0;
        let count = (((end - start) / by).ceil() as u64).max(1);
        let mut items = vec![text(&format!("<{}", general(start)))];
        for k in 0..count {
            let from = start + by * k as f64;
            let last = k + 1 == count;
            let to = if last {
                end
            } else if whole {
                from + by - 1.0
            } else {
                from + by
            };
            items.push(text(&format!("{}-{}", general(from), general(to))));
        }
        items.push(text(&format!(">{}", general(end))));
        items
    }

    /// The smallest and largest number or date a field holds.
    fn field_bounds(values: &[CacheValue], dates: bool) -> Option<(f64, f64)> {
        let numbers: Vec<f64> = values
            .iter()
            .filter_map(|value| match value {
                CacheValue::Date(n) => Some(*n),
                CacheValue::Number(n) if !dates => Some(*n),
                _ => None,
            })
            .collect();
        let low = numbers.iter().copied().fold(f64::INFINITY, f64::min);
        let high = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        low.is_finite().then_some((low, high))
    }

    pub(crate) fn pivot_group_command(
        &mut self,
        pivot: &PivotRef,
        field: u32,
        spec: GroupSpec,
    ) -> Result<Applied, String> {
        let index = self.pivot_named(pivot)?;
        let cache = self.book.pivot_tables[index].cache as usize;
        let records = self.cache_records(cache)?;
        let old = self.book.pivot_caches[cache].clone();
        let f = field as usize;
        let def = old.fields.get(f).ok_or(CANT_GROUP)?;
        let values: Vec<CacheValue> = records
            .columns
            .get(f)
            .map(|column| column.values.clone())
            .unwrap_or_default();
        let mut new = old.clone();
        new.changed = true;
        // Fields to put where the grouped one was on its axis, outermost first, and whether it
        // leaves that axis.
        let mut placed: Vec<u32> = Vec::new();
        let mut replaces = false;
        match spec {
            GroupSpec::Dates { parts, start, end } => {
                if def.group.is_some() || !def.database {
                    return Err(CANT_GROUP.to_string());
                }
                let (low, high) = Self::field_bounds(&values, true).ok_or(CANT_GROUP)?;
                let mut periods: Vec<DateGrouping> = parts
                    .iter()
                    .map(|part| part_of(part).ok_or(CANT_GROUP))
                    .collect::<Result<_, _>>()?;
                periods.sort_by_key(|by| rank(*by));
                periods.dedup();
                if periods.is_empty() {
                    return Err("Select at least one way to group.".to_string());
                }
                let from = start.unwrap_or(low.floor());
                let to = end.unwrap_or(high.floor() + 1.0);
                if to < from {
                    return Err("The ending date must be after the starting date.".to_string());
                }
                for by in &periods {
                    let name = unused_name(&new, &format!("{} ({})", part_name(*by), def.name));
                    let items = self.date_items(*by, from, to);
                    new.fields.push(group_field(
                        name,
                        FieldGroup {
                            base: field,
                            base_named: true,
                            parent: None,
                            range: Some(GroupRange::Dates {
                                by: *by,
                                start: from,
                                end: to,
                                interval: 1.0,
                                auto_start: start.is_none(),
                                auto_end: end.is_none(),
                            }),
                            items,
                        },
                    ));
                    placed.insert(0, new.fields.len() as u32 - 1);
                }
                new.fields[f].group = Some(FieldGroup {
                    base: field,
                    base_named: false,
                    parent: Some(new.fields.len() as u32 - 1),
                    range: None,
                    items: Vec::new(),
                });
                replaces = true;
            }
            GroupSpec::Numbers { start, end, by } => {
                if def.group.is_some() || !def.database {
                    return Err(CANT_GROUP.to_string());
                }
                if by.is_nan() || by <= 0.0 {
                    return Err("Enter a number greater than zero in By.".to_string());
                }
                if values
                    .iter()
                    .any(|value| matches!(value, CacheValue::Date(_)))
                {
                    return Err(CANT_GROUP.to_string());
                }
                let (low, high) = Self::field_bounds(&values, false).ok_or(CANT_GROUP)?;
                let from = start.unwrap_or(low);
                let to = end.unwrap_or(high);
                if to < from {
                    return Err(
                        "The ending number must be greater than the starting number.".to_string(),
                    );
                }
                let items = self.number_items(from, to, by);
                new.fields[f].group = Some(FieldGroup {
                    base: field,
                    base_named: true,
                    parent: None,
                    range: Some(GroupRange::Numbers {
                        start: from,
                        end: to,
                        interval: by,
                        auto_start: start.is_none(),
                        auto_end: end.is_none(),
                    }),
                    items,
                });
            }
            GroupSpec::Items { items } => {
                let listed = &self.book.pivot_tables[index].fields[f].items;
                let picked: HashSet<u32> = items
                    .into_iter()
                    .filter_map(|pos| listed.get(pos as usize)?.x)
                    .filter(|&x| (x as usize) < values.len())
                    .collect();
                if picked.is_empty() || !def.database {
                    return Err(CANT_GROUP.to_string());
                }
                let label = |value: &CacheValue| {
                    CacheValue::Text(crate::pivots::item_label(value, "(blank)"))
                };
                match def.group.as_ref().and_then(|group| group.parent) {
                    // A field grouped by hand already: the picked items leave their groups for
                    // a new one, and groups left empty go.
                    Some(parent) => {
                        let p = parent as usize;
                        let Some(FieldGroup {
                            range: Some(GroupRange::Discrete(map)),
                            items: labels,
                            ..
                        }) = new.fields[p].group.clone()
                        else {
                            return Err(CANT_GROUP.to_string());
                        };
                        let taken: HashSet<String> = labels
                            .iter()
                            .map(|label| crate::pivots::item_label(label, ""))
                            .collect();
                        let name = (1..)
                            .map(|n| format!("Group{n}"))
                            .find(|name| !taken.contains(name))
                            .unwrap_or_default();
                        let mut next_labels: Vec<CacheValue> = Vec::new();
                        let mut moved: Vec<Option<u32>> = vec![None; labels.len()];
                        let mut next_map = map.clone();
                        for (old_at, label_value) in labels.iter().enumerate() {
                            let members = (0..map.len()).any(|x| {
                                map[x] as usize == old_at && !picked.contains(&(x as u32))
                            });
                            if members {
                                moved[old_at] = Some(next_labels.len() as u32);
                                next_labels.push(label_value.clone());
                            }
                        }
                        let group_at = next_labels.len() as u32;
                        next_labels.push(text(&name));
                        for (x, slot) in next_map.iter_mut().enumerate() {
                            *slot = if picked.contains(&(x as u32)) {
                                group_at
                            } else {
                                moved[*slot as usize].unwrap_or(group_at)
                            };
                        }
                        if let Some(group) = new.fields[p].group.as_mut() {
                            group.range = Some(GroupRange::Discrete(next_map));
                            group.items = next_labels;
                        }
                    }
                    None if def.group.is_some() => return Err(CANT_GROUP.to_string()),
                    None => {
                        // Items left out keep their own names, in the field's order, and the
                        // group comes after them.
                        let mut labels: Vec<CacheValue> = Vec::new();
                        let mut map = vec![0u32; values.len()];
                        for (x, value) in values.iter().enumerate() {
                            if !picked.contains(&(x as u32)) {
                                map[x] = labels.len() as u32;
                                labels.push(label(value));
                            }
                        }
                        let group_at = labels.len() as u32;
                        labels.push(text("Group1"));
                        for &x in &picked {
                            map[x as usize] = group_at;
                        }
                        let name = unused_name(&new, &format!("{}2", def.name));
                        new.fields.push(group_field(
                            name,
                            FieldGroup {
                                base: field,
                                base_named: true,
                                parent: None,
                                range: Some(GroupRange::Discrete(map)),
                                items: labels,
                            },
                        ));
                        let added = new.fields.len() as u32 - 1;
                        new.fields[f].group = Some(FieldGroup {
                            base: field,
                            base_named: false,
                            parent: Some(added),
                            range: None,
                            items: Vec::new(),
                        });
                        placed.push(added);
                    }
                }
            }
        }
        let remap: Vec<Option<usize>> = (0..old.fields.len()).map(Some).collect();
        let op = self.regroup_op(cache, new, &remap, index, field, &placed, replaces)?;
        self.perform("Group", op, None)
    }

    pub(crate) fn pivot_ungroup_command(
        &mut self,
        pivot: &PivotRef,
        field: u32,
    ) -> Result<Applied, String> {
        const CANT: &str = "Cannot ungroup that selection.";
        let index = self.pivot_named(pivot)?;
        let cache = self.book.pivot_tables[index].cache as usize;
        let old = self.book.pivot_caches[cache].clone();
        let f = field as usize;
        let group = old
            .fields
            .get(f)
            .and_then(|def| def.group.clone())
            .ok_or(CANT)?;
        let mut new = old.clone();
        new.changed = true;
        let mut removed: HashSet<usize> = HashSet::new();
        // The field that goes back where the removed ones were.
        let mut back: Option<u32> = None;
        match &group.range {
            Some(GroupRange::Numbers { .. }) if group.base == field => {
                new.fields[f].group = None;
            }
            Some(GroupRange::Discrete(_)) => {
                removed.insert(f);
                let base = group.base as usize;
                new.fields[base].group = None;
                back = Some(group.base);
            }
            Some(GroupRange::Dates { .. }) if group.base != field => {
                let base = group.base;
                for (at, other) in old.fields.iter().enumerate() {
                    if other.group.as_ref().is_some_and(|g| {
                        g.base == base && matches!(g.range, Some(GroupRange::Dates { .. }))
                    }) && at != base as usize
                    {
                        removed.insert(at);
                    }
                }
                new.fields[base as usize].group = None;
                back = Some(base);
            }
            None => {
                // The base of groups made from it: ungrouping it takes them all away.
                if group.parent.is_none() {
                    return Err(CANT.to_string());
                }
                for (at, other) in old.fields.iter().enumerate() {
                    if other.group.as_ref().is_some_and(|g| g.base == field) && at != f {
                        removed.insert(at);
                    }
                }
                new.fields[f].group = None;
                back = Some(field);
            }
            _ => return Err(CANT.to_string()),
        }
        // Later fields move up over the ones taken away.
        let mut remap: Vec<Option<usize>> = Vec::with_capacity(old.fields.len());
        let mut next = 0;
        for at in 0..old.fields.len() {
            if removed.contains(&at) {
                remap.push(None);
            } else {
                remap.push(Some(next));
                next += 1;
            }
        }
        let kept: Vec<CacheField> = new
            .fields
            .iter()
            .enumerate()
            .filter(|(at, _)| !removed.contains(at))
            .map(|(_, def)| def.clone())
            .collect();
        new.fields = kept;
        for def in &mut new.fields {
            if let Some(group) = def.group.as_mut() {
                group.base = remap[group.base as usize].unwrap_or(0) as u32;
                group.parent = group
                    .parent
                    .and_then(|p| remap.get(p as usize).copied().flatten())
                    .map(|p| p as u32);
            }
        }
        // The base goes back on the axis where the first removed field sat.
        let table = &self.book.pivot_tables[index];
        let first_removed = table
            .row_fields
            .iter()
            .chain(table.col_fields.iter())
            .find(|&&x| x >= 0 && removed.contains(&(x as usize)))
            .copied();
        let op = self.ungroup_op(cache, new, &remap, index, back, first_removed)?;
        self.perform("Ungroup", op, None)
    }

    /// Lays every table on a cache out again over its new fields.
    #[allow(clippy::too_many_arguments)]
    fn regroup_op(
        &mut self,
        cache: usize,
        def: PivotCache,
        remap: &[Option<usize>],
        target: usize,
        field: u32,
        placed: &[u32],
        replaces: bool,
    ) -> Result<Op, String> {
        let added: Vec<u32> = placed.to_vec();
        // Numbers group on the field itself, whose items become its groups.
        let regrouped = !replaces && placed.is_empty();
        // Date and number groups list their items in the group's own order, as Excel does.
        let ranged: Vec<usize> = def
            .fields
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.group.as_ref().is_some_and(|g| {
                    matches!(
                        g.range,
                        Some(GroupRange::Dates { .. } | GroupRange::Numbers { .. })
                    )
                })
            })
            .map(|(at, _)| at)
            .collect();
        self.relayout_cache(cache, def, remap, |_, at, table, view| {
            for (f, pivot_field) in table.fields.iter_mut().enumerate() {
                let new_field = added.contains(&(f as u32));
                let reset = regrouped && f == field as usize;
                if new_field
                    || reset
                    || (pivot_field.axis.is_some() && pivot_field.items.is_empty())
                {
                    pivot_field.items = if ranged.contains(&f) {
                        in_order(view.items(f).len())
                    } else {
                        cgs_pivot::default_items(view.items(f))
                    };
                }
            }
            if at == target {
                place_fields(table, field, &added, replaces);
            }
        })
    }

    fn ungroup_op(
        &mut self,
        cache: usize,
        def: PivotCache,
        remap: &[Option<usize>],
        target: usize,
        back: Option<u32>,
        first_removed: Option<i32>,
    ) -> Result<Op, String> {
        let old_table = self.book.pivot_tables[target].clone();
        let back = back.and_then(|b| remap.get(b as usize).copied().flatten());
        self.relayout_cache(cache, def, remap, |_, at, table, view| {
            if let Some(b) = back {
                let field = &mut table.fields[b];
                if field.items.is_empty() || at == target {
                    field.items = cgs_pivot::default_items(view.items(b));
                }
            }
            for (f, pivot_field) in table.fields.iter_mut().enumerate() {
                if pivot_field.axis.is_some() && pivot_field.items.is_empty() {
                    pivot_field.items = cgs_pivot::default_items(view.items(f));
                }
            }
            if at != target {
                return;
            }
            let (Some(b), Some(first)) = (back, first_removed) else {
                return;
            };
            // Where the first removed field was in the old table, on its axis.
            let on_rows = old_table.row_fields.contains(&first);
            let list = if on_rows {
                &old_table.row_fields
            } else {
                &old_table.col_fields
            };
            let position = list.iter().position(|&x| x == first).unwrap_or(0);
            let target_list = if on_rows {
                &mut table.row_fields
            } else {
                &mut table.col_fields
            };
            if !target_list.contains(&(b as i32)) {
                // Fields before it that went away shift the place.
                let before = list[..position]
                    .iter()
                    .filter(|&&x| x >= 0 && remap.get(x as usize).copied().flatten().is_none())
                    .count();
                let at_place = (position - before).min(target_list.len());
                target_list.insert(at_place, b as i32);
                table.fields[b].axis = Some(if on_rows {
                    PivotAxis::Row
                } else {
                    PivotAxis::Col
                });
            }
        })
    }

    /// Puts a cache's new definition in place and lays out each of its tables, as Refresh does,
    /// with `adjust` given each table after its fields move.
    fn relayout_cache(
        &mut self,
        cache: usize,
        def: PivotCache,
        remap: &[Option<usize>],
        mut adjust: impl FnMut(&Self, usize, &mut PivotTable, &CacheView<'_>),
    ) -> Result<Op, String> {
        let records = self.cache_records(cache)?;
        let mut caches = self.book.pivot_caches.clone();
        caches[cache] = def;
        let saved_caches = std::mem::replace(&mut self.book.pivot_caches, caches);
        let saved_tables = self.book.pivot_tables.clone();
        let date1904 = self.book.date1904;
        let result = (|| {
            let mut ops = Vec::new();
            let indexes: Vec<usize> = (0..self.book.pivot_tables.len())
                .filter(|&i| self.book.pivot_tables[i].cache as usize == cache)
                .collect();
            for at in indexes {
                let mut table = remap_table(
                    &self.book.pivot_tables[at],
                    remap,
                    &saved_caches[cache],
                    &self.book.pivot_caches[cache],
                    &records,
                );
                {
                    let def = self.book.pivot_caches[cache].clone();
                    let view = CacheView::new(&def, &records, date1904);
                    adjust(self, at, &mut table, &view);
                }
                if table.unsupported.is_some() {
                    continue;
                }
                let current = &self.book.pivot_tables[at];
                let before = (!current.row_items.is_empty() || !current.col_items.is_empty())
                    .then(|| pivot_area(current));
                let layout = self.lay_out(&table)?;
                let restyle = layout.row_items != self.book.pivot_tables[at].row_items
                    || layout.col_items != self.book.pivot_tables[at].col_items
                    || layout.location != self.book.pivot_tables[at].location;
                let op = self.output_op(at, table, layout, before, restyle)?;
                if let Op::Many(mut list) = op {
                    if let Some(Op::BookPivots { tables, .. }) = list.pop() {
                        self.book.pivot_tables = tables;
                    }
                    ops.extend(list);
                }
            }
            Ok::<_, String>(ops)
        })();
        let new_caches = std::mem::replace(&mut self.book.pivot_caches, saved_caches);
        let tables_after = std::mem::replace(&mut self.book.pivot_tables, saved_tables);
        let mut ops = result?;
        ops.push(Op::BookPivots {
            tables: tables_after,
            caches: new_caches,
            records: self.pivot_records.clone(),
        });
        Ok(Op::Many(ops))
    }
}

/// The grouped field's place on its axis goes to the new fields, outermost first: in place of it
/// for dates, before it for items grouped by hand.
fn in_order(count: usize) -> Vec<PivotItem> {
    let mut items: Vec<PivotItem> = (0..count as u32).map(PivotItem::data).collect();
    items.push(PivotItem::total(ItemType::Default));
    items
}

fn place_fields(table: &mut PivotTable, field: u32, added: &[u32], replaces: bool) {
    let f = field as i32;
    let axis = table.fields[field as usize].axis;
    let list = match axis {
        Some(PivotAxis::Row) => &mut table.row_fields,
        Some(PivotAxis::Col) => &mut table.col_fields,
        _ => return,
    };
    let Some(at) = list.iter().position(|&x| x == f) else {
        return;
    };
    let new: Vec<i32> = added.iter().map(|&x| x as i32).collect();
    if replaces {
        list.splice(at..=at, new);
        table.fields[field as usize].axis = None;
    } else {
        list.splice(at..at, new);
    }
    for &x in added {
        table.fields[x as usize].axis = axis;
    }
}
