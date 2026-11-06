//! A cache seen field by field: each field's items, and which item each record has, groups worked
//! out from the values they group.

use cgs_model::pivot::{CacheRecords, CacheValue, DateGrouping, GroupRange, PivotCache};

/// Each field's items and each record's item of it.
pub struct CacheView<'a> {
    pub cache: &'a PivotCache,
    pub records: &'a CacheRecords,
    /// Per field, its items: the group's items for a grouped field, else the values.
    items: Vec<Vec<CacheValue>>,
    /// Per field, the item of each record; empty for a field with no items.
    of_record: Vec<Vec<u32>>,
}

/// Civil date from a 1900-system serial: (year, month, day).
pub fn civil(serial: f64, date1904: bool) -> (i64, u32, u32) {
    let mut days = serial.floor() as i64;
    if date1904 {
        days += 1462;
    } else if (1..61).contains(&days) {
        days += 1;
    }
    let z = days - 25_569 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

const DAYS_BEFORE: [u32; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];

/// Which group item a value falls in. Item 0 is "<start" and the last is ">end".
pub fn group_index(range: &GroupRange, value: &CacheValue, count: usize, date1904: bool) -> u32 {
    let last = count.saturating_sub(1) as u32;
    match range {
        GroupRange::Discrete(_) => 0,
        GroupRange::Numbers {
            start,
            end,
            interval,
            ..
        } => {
            let Some(v) = value.number() else { return 0 };
            if v < *start {
                return 0;
            }
            if v > *end {
                return last;
            }
            let step = if *interval > 0.0 { *interval } else { 1.0 };
            let index = ((v - start) / step).floor() as i64 + 1;
            (index.max(1) as u32).min(last.saturating_sub(1).max(1))
        }
        GroupRange::Dates {
            by,
            start,
            end,
            interval,
            ..
        } => {
            let Some(v) = value.number() else { return 0 };
            if v.floor() < start.floor() {
                return 0;
            }
            if v.floor() > end.floor() {
                return last;
            }
            let (year, month, day) = civil(v, date1904);
            let fraction = v - v.floor();
            let seconds = (fraction * 86_400.0).round() as u32;
            let index = match by {
                DateGrouping::Years => {
                    let (first, _, _) = civil(*start, date1904);
                    (year - first) as u32 + 1
                }
                DateGrouping::Quarters => (month - 1) / 3 + 1,
                DateGrouping::Months => month,
                DateGrouping::Days if *interval > 1.0 => {
                    ((v.floor() - start.floor()) / interval).floor() as u32 + 1
                }
                DateGrouping::Days => DAYS_BEFORE[month as usize - 1] + day,
                DateGrouping::Hours => seconds / 3600 + 1,
                DateGrouping::Minutes => seconds / 60 % 60 + 1,
                DateGrouping::Seconds => seconds % 60 + 1,
            };
            index.min(last)
        }
    }
}

impl<'a> CacheView<'a> {
    pub fn new(cache: &'a PivotCache, records: &'a CacheRecords, date1904: bool) -> Self {
        let count = records.count as usize;
        let fields = cache.fields.len();
        let mut items = vec![Vec::new(); fields];
        let mut of_record = vec![Vec::new(); fields];
        for f in 0..fields {
            let field = &cache.fields[f];
            match &field.group {
                Some(group) if !group.items.is_empty() => {
                    items[f] = group.items.clone();
                }
                _ => {
                    if let Some(column) = records.columns.get(f) {
                        items[f] = column.values.clone();
                        of_record[f] = column.index.clone();
                    }
                }
            }
        }
        // Groups, from the values they group. A group of a group goes after its base.
        {
            for f in 0..fields {
                let Some(group) = &cache.fields[f].group else {
                    continue;
                };
                if group.items.is_empty() {
                    continue;
                }
                // Groups divide the values of the field they group, never its groups.
                let base = group.base as usize;
                let (base_items, base_records) = match records.columns.get(base) {
                    Some(column) if column.index.len() == count => (&column.values, &column.index),
                    _ => continue,
                };
                let mapped: Vec<u32> = match &group.range {
                    Some(GroupRange::Discrete(map)) => base_records
                        .iter()
                        .map(|&i| map.get(i as usize).copied().unwrap_or(0))
                        .collect(),
                    Some(range) => {
                        let per_item: Vec<u32> = base_items
                            .iter()
                            .map(|v| group_index(range, v, group.items.len(), date1904))
                            .collect();
                        base_records
                            .iter()
                            .map(|&i| per_item.get(i as usize).copied().unwrap_or(0))
                            .collect()
                    }
                    None => continue,
                };
                if mapped.len() == count {
                    of_record[f] = mapped;
                }
            }
        }
        Self {
            cache,
            records,
            items,
            of_record,
        }
    }

    pub fn count(&self) -> usize {
        self.records.count as usize
    }

    pub fn items(&self, field: usize) -> &[CacheValue] {
        self.items.get(field).map_or(&[], Vec::as_slice)
    }

    /// The item record `record` has in `field`.
    pub fn item(&self, field: usize, record: usize) -> Option<u32> {
        self.of_record.get(field)?.get(record).copied()
    }

    /// The value record `record` has in `field`, for summing.
    pub fn value(&self, field: usize, record: usize) -> &CacheValue {
        static MISSING: CacheValue = CacheValue::Missing;
        if let Some(column) = self
            .records
            .columns
            .get(field)
            .filter(|c| c.index.len() == self.count())
        {
            return column
                .index
                .get(record)
                .and_then(|&i| column.values.get(i as usize))
                .unwrap_or(&MISSING);
        }
        // A group's value is its label.
        self.item(field, record)
            .and_then(|i| self.items(field).get(i as usize))
            .unwrap_or(&MISSING)
    }

    /// Whether the field has an item for every record.
    pub fn has_items(&self, field: usize) -> bool {
        self.of_record
            .get(field)
            .is_some_and(|items| items.len() == self.count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_fall_in_their_groups() {
        let months = GroupRange::Dates {
            by: DateGrouping::Months,
            start: 44927.0,
            end: 45622.0,
            interval: 1.0,
            auto_start: true,
            auto_end: true,
        };
        // 2023-03-15
        assert_eq!(
            group_index(&months, &CacheValue::Date(44998.0), 14, false),
            3
        );
        assert_eq!(
            group_index(&months, &CacheValue::Date(44000.0), 14, false),
            0
        );
        assert_eq!(
            group_index(&months, &CacheValue::Date(46000.0), 14, false),
            13
        );
        let years = GroupRange::Dates {
            by: DateGrouping::Years,
            start: 44927.0,
            end: 45622.0,
            interval: 1.0,
            auto_start: true,
            auto_end: true,
        };
        assert_eq!(group_index(&years, &CacheValue::Date(45300.0), 4, false), 2);
        assert_eq!(civil(44927.0, false), (2023, 1, 1));
        let numbers = GroupRange::Numbers {
            start: 0.0,
            end: 1000.0,
            interval: 250.0,
            auto_start: false,
            auto_end: false,
        };
        assert_eq!(
            group_index(&numbers, &CacheValue::Number(260.0), 6, false),
            2
        );
        assert_eq!(
            group_index(&numbers, &CacheValue::Number(1000.0), 6, false),
            4
        );
        assert_eq!(group_index(&numbers, &CacheValue::Missing, 6, false), 0);
    }
}
