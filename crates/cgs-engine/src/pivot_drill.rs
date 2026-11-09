//! Drill Down and Show Report Filter Pages, as Excel 2016 does them over COM. Drill Down (Show
//! Details, or a double-click on a value) puts the records behind a value on a new sheet left of
//! the pivot table's, as a table in TableStyleMedium2. Show Report Filter Pages copies the pivot
//! table onto a sheet of its own for each item of a report filter, named after the item, with the
//! filter set to it, in the field's order before the pivot table's sheet.

use std::collections::HashMap;

use cgs_calc::pivot_data::{PivotItemKey, PivotLookup, PivotValueLookup};
use cgs_calc::{CalcValue, CellRef, TableInfo};
use cgs_model::pivot::{ItemType, PivotTable, VALUES_FIELD};
use cgs_model::{AutoFilter, Rect, Table};
use cgs_pivot::CacheView;

use crate::command::{Applied, Area, Op};
use crate::edit::{EditCell, EditInput, NewStyle};
use crate::pivots::{cell_of, item_label, pivot_area, PivotRef};
use crate::Session;

const NOT_DATA: &str = "Cannot change this part of a PivotTable report.";

/// A line's members in full, filling in what `repeat` leaves out from the lines before.
fn members(lines: &[cgs_model::pivot::AxisLine], index: usize) -> Vec<u32> {
    let mut current: Vec<u32> = Vec::new();
    for line in lines.iter().take(index + 1) {
        current.truncate(line.repeat as usize);
        current.extend_from_slice(&line.members);
    }
    current
}

/// The items a value cell sums, as (field, item position) pairs, or `None` off the values.
fn cell_items(table: &PivotTable, row: u32, col: u32) -> Option<Vec<(usize, u32)>> {
    let l = &table.location;
    let r = l.range;
    let data_top = r.top + l.first_data_row;
    let data_left = r.left + l.first_data_col;
    if row < data_top || col < data_left || row > r.bottom || col > r.right {
        return None;
    }
    let mut out = Vec::new();
    let mut axis = |lines: &[cgs_model::pivot::AxisLine], fields: &[i32], k: usize| -> bool {
        let Some(line) = lines.get(k) else {
            return lines.is_empty();
        };
        if line.kind == ItemType::Grand {
            return true;
        }
        let full = members(lines, k);
        for (level, &field) in fields.iter().enumerate() {
            let Some(&pos) = full.get(level) else { break };
            if field != VALUES_FIELD {
                out.push((field as usize, pos));
            }
        }
        true
    };
    if !axis(
        &table.row_items,
        &table.row_fields,
        (row - data_top) as usize,
    ) {
        return None;
    }
    if !axis(
        &table.col_items,
        &table.col_fields,
        (col - data_left) as usize,
    ) {
        return None;
    }
    Some(out)
}

/// A sheet name made from an item, as Excel makes one: no characters a name can't hold, at most
/// 31 long, and not one the book has.
fn page_name(book: &cgs_model::Book, label: &str) -> String {
    let clean: String = label
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .collect();
    let clean = clean.trim_matches('\'').trim();
    let base: String = if clean.is_empty() {
        "(blank)".to_string()
    } else {
        clean.chars().take(31).collect()
    };
    let taken = |name: &str| crate::command::sheet_name_problem(book, name, None).is_some();
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| {
            let tail = format!(" ({n})");
            let room = 31 - tail.chars().count();
            format!("{}{tail}", base.chars().take(room).collect::<String>())
        })
        .find(|name| !taken(name))
        .unwrap_or(base)
}

impl Session {
    /// Show Details on a value cell: its records on a new sheet, as a table.
    pub(crate) fn pivot_details_command(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
    ) -> Result<Applied, String> {
        let index = self.pivot_at(sheet, row, col).ok_or(NOT_DATA)?;
        let table = self.book.pivot_tables[index].clone();
        let wanted = cell_items(&table, row, col).ok_or(NOT_DATA)?;
        if table.data_fields.is_empty() {
            return Err(NOT_DATA.to_string());
        }
        let cache = table.cache as usize;
        let records = self.cache_records(cache)?;
        let def = self.book.pivot_caches[cache].clone();
        let view = CacheView::new(&def, &records, self.book.date1904);
        let x_of = |field: usize, pos: u32| table.fields[field].items.get(pos as usize)?.x;
        let constraints: Vec<(usize, u32)> = wanted
            .iter()
            .filter_map(|&(field, pos)| Some((field, x_of(field, pos)?)))
            .collect();
        // Report filters, and items hidden on the table's fields, leave records out too.
        let mut hidden: HashMap<usize, Vec<u32>> = HashMap::new();
        for (f, field) in table.fields.iter().enumerate() {
            let list: Vec<u32> = field
                .items
                .iter()
                .filter(|item| item.kind == ItemType::Data && item.hidden)
                .filter_map(|item| item.x)
                .collect();
            if !list.is_empty() && (field.axis.is_some()) {
                hidden.insert(f, list);
            }
        }
        let pages: Vec<(usize, u32)> = table
            .page_fields
            .iter()
            .filter_map(|page| {
                let pos = page.item?;
                Some((page.field as usize, x_of(page.field as usize, pos)?))
            })
            .collect();
        let chosen: Vec<usize> = (0..view.count())
            .filter(|&r| {
                constraints
                    .iter()
                    .chain(pages.iter())
                    .all(|&(f, x)| view.item(f, r) == Some(x))
                    && hidden
                        .iter()
                        .all(|(&f, list)| view.item(f, r).is_none_or(|x| !list.contains(&x)))
            })
            .collect();
        let columns: Vec<usize> = (0..def.fields.len())
            .filter(|&f| def.fields[f].database)
            .collect();
        let headings: Vec<String> = columns
            .iter()
            .map(|&f| def.fields[f].name.clone())
            .collect();
        // Each column keeps the source's number format, as Excel's details do.
        let styles: Vec<Option<u32>> = columns
            .iter()
            .map(|&f| {
                let id = def.fields[f].num_fmt_id;
                (id != 0).then(|| {
                    self.add_style(
                        0,
                        &NewStyle {
                            number_format_id: Some(id),
                            ..NewStyle::default()
                        },
                    )
                })
            })
            .collect();
        let rows = chosen.len() as u32 + 1;
        let cols = columns.len() as u32;
        let new_sheet = self.book.sheets.len() as u32;
        let position = self
            .book
            .tab_order()
            .iter()
            .position(|&s| s == sheet)
            .unwrap_or(0) as u32;
        let name = crate::command::next_sheet_name(&self.book);
        let right_to_left = self
            .book
            .sheets
            .get(sheet as usize)
            .is_some_and(|info| info.view.right_to_left);
        let mut cells: Vec<EditCell> = Vec::new();
        for (c, heading) in headings.iter().enumerate() {
            cells.push(EditCell {
                sheet: new_sheet,
                row: 1,
                col: c as u32 + 1,
                input: Some(EditInput::Value(CalcValue::Text(heading.clone()))),
                style: None,
            });
        }
        for (r, &record) in chosen.iter().enumerate() {
            for (c, &f) in columns.iter().enumerate() {
                cells.push(EditCell {
                    sheet: new_sheet,
                    row: r as u32 + 2,
                    col: c as u32 + 1,
                    input: Some(EditInput::Value(cell_of(view.value(f, record)))),
                    style: styles[c],
                });
            }
        }
        let table_name = self.next_table_name();
        let id = self.next_table_id();
        let area = Area {
            top: 1,
            left: 1,
            bottom: rows,
            right: cols.max(1),
        };
        let mut cells = Some(cells);
        self.perform_steps(
            "Show Details",
            Some((new_sheet, area)),
            move |session, step| match step {
                0 => Ok(Some(Op::AddSheet {
                    name: name.clone(),
                    position,
                    right_to_left,
                })),
                1 => Ok(cells.take().map(Op::Cells)),
                2 => {
                    let (mut calc, mut book) = session.table_lists();
                    let rect = Rect {
                        top: area.top,
                        left: area.left,
                        bottom: area.bottom,
                        right: area.right,
                    };
                    calc.push(TableInfo {
                        name: table_name.clone(),
                        sheet: new_sheet,
                        range: cgs_calc::Rect {
                            top: rect.top,
                            left: rect.left,
                            bottom: rect.bottom,
                            right: rect.right,
                        },
                        header_row: true,
                        totals_row: false,
                        columns: headings.clone(),
                    });
                    let mut table =
                        Table::new(table_name.clone(), new_sheet, rect, headings.clone());
                    table.id = id;
                    table.filter = Some(AutoFilter::new(rect));
                    book.push(table);
                    Ok(Some(Op::Many(vec![Op::BookTables(book), Op::Tables(calc)])))
                }
                _ => Ok(None),
            },
        )
    }

    /// Show Report Filter Pages: a copy of the pivot table per item of a report filter.
    pub(crate) fn pivot_filter_pages_command(
        &mut self,
        pivot: &PivotRef,
        field: u32,
    ) -> Result<Applied, String> {
        let index = self.pivot_named(pivot)?;
        let table = self.book.pivot_tables[index].clone();
        if !table.page_fields.iter().any(|page| page.field == field) {
            return Err("That field isn't a report filter.".to_string());
        }
        let cache = table.cache as usize;
        let records = self.cache_records(cache)?;
        let def = self.book.pivot_caches[cache].clone();
        let view = CacheView::new(&def, &records, self.book.date1904);
        let f = field as usize;
        let items: Vec<(u32, String)> = table.fields[f]
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.kind == ItemType::Data && !item.hidden)
            .filter_map(|(pos, item)| {
                let label = item.name.clone().or_else(|| {
                    item.x
                        .and_then(|x| view.items(f).get(x as usize))
                        .map(|value| item_label(value, "(blank)"))
                })?;
                Some((pos as u32, label))
            })
            .collect();
        if items.is_empty() {
            return Err("There are no items to show pages for.".to_string());
        }
        let sheet = table.sheet;
        let position = self
            .book
            .tab_order()
            .iter()
            .position(|&s| s == sheet)
            .unwrap_or(0) as u32;
        let right_to_left = self
            .book
            .sheets
            .get(sheet as usize)
            .is_some_and(|info| info.view.right_to_left);
        let first_new = self.book.sheets.len() as u32;
        // The last item's page is made first, as Excel makes them, so each goes before the last.
        let order: Vec<(u32, String)> = items.iter().rev().cloned().collect();
        let last = first_new + order.len() as u32 - 1;
        self.perform_steps(
            "Show Report Filter Pages",
            Some((last, Area::cell(1, 1))),
            move |session, step| {
                let k = step / 3;
                let Some((pos, label)) = order.get(k) else {
                    return Ok(None);
                };
                let at_sheet = first_new + k as u32;
                match step % 3 {
                    0 => Ok(Some(Op::AddSheet {
                        name: page_name(&session.book, label),
                        position,
                        right_to_left,
                    })),
                    1 => {
                        let mut copy = table.clone();
                        copy.name = session.next_pivot_name();
                        copy.sheet = at_sheet;
                        copy.part = None;
                        copy.changed = true;
                        for page in &mut copy.page_fields {
                            if page.field == field {
                                page.item = Some(*pos);
                            }
                        }
                        for item in &mut copy.fields[f].items {
                            item.hidden = false;
                        }
                        let mut tables = session.book.pivot_tables.clone();
                        tables.push(copy);
                        Ok(Some(Op::BookPivots {
                            tables,
                            caches: session.book.pivot_caches.clone(),
                            records: session.pivot_records.clone(),
                        }))
                    }
                    _ => {
                        let at = session.book.pivot_tables.len() - 1;
                        let copy = session.book.pivot_tables[at].clone();
                        let layout = session.lay_out(&copy)?;
                        session.output_op(at, copy, layout, None, true).map(Some)
                    }
                }
            },
        )
    }

    /// Which value field a value cell shows.
    fn data_at(table: &PivotTable, row: u32, col: u32) -> u32 {
        let l = &table.location;
        if table.data_fields.len() <= 1 {
            return 0;
        }
        if table.row_fields.contains(&VALUES_FIELD) {
            let k = row.saturating_sub(l.range.top + l.first_data_row) as usize;
            table.row_items.get(k).map_or(0, |line| line.data)
        } else {
            let k = col.saturating_sub(l.range.left + l.first_data_col) as usize;
            table.col_items.get(k).map_or(0, |line| line.data)
        }
    }

    /// Hands GETPIVOTDATA every pivot table's values as they stand, with the items each sums.
    pub(crate) fn rebuild_pivot_index(&mut self) {
        self.pivots_stale = false;
        let mut out = Vec::new();
        for t in 0..self.book.pivot_tables.len() {
            let table = self.book.pivot_tables[t].clone();
            if table.unsupported.is_some() || table.data_fields.is_empty() {
                continue;
            }
            let Some(info) = self.book.sheets.get(table.sheet as usize) else {
                continue;
            };
            if info.deleted {
                continue;
            }
            let sheet_name = info.name.clone();
            let cache = table.cache as usize;
            let Ok(records) = self.cache_records(cache) else {
                continue;
            };
            let def = self.book.pivot_caches[cache].clone();
            let view = CacheView::new(&def, &records, self.book.date1904);
            let key = |f: usize, pos: u32| -> Option<PivotItemKey> {
                let item = table.fields.get(f)?.items.get(pos as usize)?;
                let value = view.items(f).get(item.x? as usize)?;
                Some(PivotItemKey {
                    label: item
                        .name
                        .clone()
                        .unwrap_or_else(|| item_label(value, "(blank)")),
                    number: value.number(),
                    date: matches!(value, cgs_model::pivot::CacheValue::Date(_)),
                })
            };
            let name_of = |f: usize| def.fields.get(f).map_or(String::new(), |d| d.name.clone());
            let area = pivot_area(&table);
            let l = table.location;
            let mut values = Vec::new();
            for row in (l.range.top + l.first_data_row)..=l.range.bottom {
                for col in (l.range.left + l.first_data_col)..=l.range.right {
                    let Some(items) = cell_items(&table, row, col) else {
                        continue;
                    };
                    let CalcValue::Number(value) = self.value_of(CellRef {
                        sheet: table.sheet,
                        row,
                        col,
                    }) else {
                        continue;
                    };
                    let items: Option<Vec<(String, PivotItemKey)>> = items
                        .iter()
                        .map(|&(f, pos)| Some((name_of(f), key(f, pos)?)))
                        .collect();
                    if let Some(items) = items {
                        values.push(PivotValueLookup {
                            row,
                            col,
                            data: Self::data_at(&table, row, col),
                            items,
                            value,
                        });
                    }
                }
            }
            let pages = table
                .page_fields
                .iter()
                .filter_map(|page| {
                    let f = page.field as usize;
                    Some((name_of(f), key(f, page.item?)?))
                })
                .collect();
            let data_names = table
                .data_fields
                .iter()
                .map(|d| vec![d.name.clone(), name_of(d.field as usize)])
                .collect();
            out.push(PivotLookup {
                sheet: sheet_name,
                top: area.top,
                left: area.left,
                bottom: area.bottom,
                right: area.right,
                data_names,
                pages,
                values,
            });
        }
        self.calc.set_pivots(out);
    }

    /// The formula a click on a pivot table's value writes, as Excel's Generate GetPivotData does:
    /// `GETPIVOTDATA("Sales",$G$3,"Region","East")`. Null off the values.
    pub(crate) fn pivot_data_formula(
        &self,
        sheet: u32,
        row: u32,
        col: u32,
        from: u32,
    ) -> Option<String> {
        let index = self.pivot_at(sheet, row, col)?;
        let table = &self.book.pivot_tables[index];
        let name = &self.book.sheets.get(sheet as usize)?.name;
        let lookups = self.calc.pivots();
        let lookup = lookups.iter().find(|l| {
            l.sheet.eq_ignore_ascii_case(name)
                && (l.top..=l.bottom).contains(&row)
                && (l.left..=l.right).contains(&col)
        })?;
        let value = lookup
            .values
            .iter()
            .find(|v| v.row == row && v.col == col)?;
        let data = table.data_fields.get(value.data as usize)?;
        let source = &self.book.pivot_caches[table.cache as usize]
            .fields
            .get(data.field as usize)?
            .name;
        // A caption Excel made names the source field; one typed in names itself.
        let made = data
            .name
            .split_once(" of ")
            .is_some_and(|(_, rest)| rest == source);
        let data_name = if made { source } else { &data.name };
        let quoted = |text: &str| format!("\"{}\"", text.replace('"', "\"\""));
        let corner = format!(
            "${}${}",
            cgs_refs::col_to_letters(i64::from(table.location.range.left)),
            table.location.range.top
        );
        let place = if from == sheet {
            corner
        } else {
            format!("{}!{corner}", cgs_refs::quote_sheet(name))
        };
        let mut out = format!("GETPIVOTDATA({},{place}", quoted(data_name));
        for (field, key) in &value.items {
            let item = match key.number {
                Some(serial) if key.date => {
                    let (y, m, d) = cgs_pivot::view::civil(serial, self.book.date1904);
                    format!("DATE({y},{m},{d})")
                }
                Some(n) if key.label == self.format_number(n, None) => key.label.clone(),
                _ => quoted(&key.label),
            };
            out.push_str(&format!(",{},{item}", quoted(field)));
        }
        out.push(')');
        Some(out)
    }
}
