//! Pivot tables: refreshing them from their source, rearranging them, and creating them. Their
//! output is ordinary cells, written through the same steps as any edit so that undo takes it back.
//! A cache's records are read the first time a pivot table needs them: from the file when it kept
//! them, else from the source.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use cgs_calc::{CalcEngine, CalcValue, CellSink, Rect, SheetRect};
use cgs_model::pivot::{
    BaseItem, CacheField, CacheRecords, CacheSource, CacheValue, DataField, FieldSort, ItemType,
    PageField, PivotAxis, PivotCache, PivotField, PivotItem, PivotLocation, PivotOptions,
    PivotStyleInfo, PivotTable, RecordColumn, ShowAs, Summary, VALUES_FIELD,
};
use cgs_numfmt::{Locale, NumFmt, Options, Value};
use cgs_pivot::{CacheView, Captions, CellValue, Layout, Role};
use cgs_xlsx::pivot::ValueKey;
use serde::Deserialize;

use crate::command::{Applied, Area, Op};
use crate::edit::{EditCell, EditInput, NewStyle};
use crate::Session;

/// Names a pivot table: its sheet and its name.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PivotRef {
    pub sheet: u32,
    pub name: String,
}

/// A value field as the field list asks for it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueSpec {
    pub field: u32,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub show_as: Option<String>,
    #[serde(default)]
    pub base_field: Option<u32>,
    /// An item index, or "previous" or "next".
    #[serde(default)]
    pub base_item: Option<serde_json::Value>,
    #[serde(default)]
    pub name: Option<String>,
    /// A number format code.
    #[serde(default)]
    pub number_format: Option<String>,
}

/// Where each field goes, as the field list says.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSpec {
    #[serde(default)]
    pub rows: Vec<i32>,
    #[serde(default)]
    pub cols: Vec<i32>,
    #[serde(default)]
    pub pages: Vec<u32>,
    #[serde(default)]
    pub values: Vec<ValueSpec>,
}

/// Options from the PivotTable Analyze and Design tabs, and PivotTable Options.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionSpec {
    /// "compact", "outline" or "tabular".
    pub form: Option<String>,
    /// "none", "top" or "bottom".
    pub subtotals: Option<String>,
    /// "off", "both", "rows" or "cols".
    pub grand_totals: Option<String>,
    pub blank_rows: Option<bool>,
    pub repeat_labels: Option<bool>,
    pub style: Option<String>,
    pub row_headers: Option<bool>,
    pub col_headers: Option<bool>,
    pub row_stripes: Option<bool>,
    pub col_stripes: Option<bool>,
    pub name: Option<String>,
    /// What empty cells show; "" for nothing.
    pub empty_text: Option<String>,
    /// What errors show; `None` within shows the errors themselves.
    pub error_text: Option<Option<String>>,
    pub headers: Option<bool>,
}

/// A change to one field: its filter, sort, subtotals, or which items show their details.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    /// Item indexes to hide; the rest show.
    pub hidden: Option<Vec<u32>>,
    /// A report filter's one item, or `None` within for (All).
    pub page: Option<Option<u32>>,
    pub multiple: Option<bool>,
    /// "manual", "ascending" or "descending".
    pub sort: Option<String>,
    /// Sort by a value field's results, by its index.
    pub sort_by: Option<Option<u32>>,
    /// An item to expand or collapse.
    pub expand: Option<(u32, bool)>,
    pub expand_all: Option<bool>,
    /// "automatic", "none", or summaries such as "sum".
    pub subtotals: Option<Vec<String>>,
    pub caption: Option<String>,
    pub show_all: Option<bool>,
    pub blank_row: Option<bool>,
}

/// Where Insert › PivotTable puts a new pivot table.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Destination {
    NewSheet,
    Existing { sheet: u32, row: u32, col: u32 },
}

struct Block(Vec<((u32, u32), CalcValue)>);

impl CellSink for Block {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        self.0.push(((row, col), value.clone()));
    }
}

fn parse_summary(text: &str) -> Option<Summary> {
    Summary::from_xml(text).or(match text {
        "stdDevP" => Some(Summary::StdDevP),
        "varP" => Some(Summary::VarP),
        _ => None,
    })
}

/// A cache value as a cell holds it.
fn cell_of(value: &CacheValue) -> CalcValue {
    match value {
        CacheValue::Missing => CalcValue::Empty,
        CacheValue::Number(n) | CacheValue::Date(n) => CalcValue::Number(*n),
        CacheValue::Text(t) => CalcValue::Text(t.clone()),
        CacheValue::Bool(b) => CalcValue::Bool(*b),
        CacheValue::Error(e) => CalcValue::Error(e.clone()),
    }
}

impl Session {
    fn num_fmt(&self, id: u32) -> NumFmt {
        match self.book.styles.num_fmts.get(&id) {
            Some(code) => NumFmt::parse(code),
            None => NumFmt::builtin(id).unwrap_or_else(NumFmt::general),
        }
    }

    /// The pivot table a cell is part of, report filters included.
    pub(crate) fn pivot_at(&self, sheet: u32, row: u32, col: u32) -> Option<usize> {
        self.book.pivot_tables.iter().position(|table| {
            let area = pivot_area(table);
            table.sheet == sheet
                && row >= area.top
                && row <= area.bottom
                && col >= area.left
                && col <= area.right
        })
    }

    pub(crate) fn pivot_named(&self, pivot: &PivotRef) -> Result<usize, String> {
        self.book
            .pivot_tables
            .iter()
            .position(|t| t.sheet == pivot.sheet && t.name.eq_ignore_ascii_case(&pivot.name))
            .ok_or_else(|| format!("There's no PivotTable named {}.", pivot.name))
    }

    fn records_slot(&mut self, cache: usize) -> &mut Option<Rc<CacheRecords>> {
        if self.pivot_records.len() < self.book.pivot_caches.len() {
            self.pivot_records
                .resize(self.book.pivot_caches.len(), None);
        }
        &mut self.pivot_records[cache]
    }

    /// The records a cache holds now: read from the file the first time, or from the source if
    /// the file kept none.
    fn cache_records(&mut self, cache: usize) -> Result<Rc<CacheRecords>, String> {
        if let Some(records) = self.records_slot(cache).clone() {
            return Ok(records);
        }
        let def = &self.book.pivot_caches[cache];
        let from_file = def
            .records_part
            .as_ref()
            .and_then(|part| self.package.read(part))
            .map(|xml| cgs_xlsx::pivot::read_records(&xml, def, self.book.date1904));
        let records = match from_file {
            Some(records) => records,
            None => {
                let (fields, records, _) = self.source_records(cache)?;
                self.book.pivot_caches[cache].fields = fields;
                records
            }
        };
        let records = Rc::new(records);
        *self.records_slot(cache) = Some(records.clone());
        Ok(records)
    }

    /// The sheet and cells a cache reads.
    fn source_area(&self, cache: &PivotCache) -> Result<(u32, Area), String> {
        let CacheSource::Worksheet { sheet, range, name } = &cache.source else {
            return Err(cache
                .no_refresh
                .clone()
                .map(|why| format!("This PivotTable can't be refreshed here: {why}."))
                .unwrap_or_else(|| "This PivotTable's source can't be read.".to_string()));
        };
        if let Some(why) = &cache.no_refresh {
            return Err(format!("This PivotTable can't be refreshed here: {why}."));
        }
        let find_sheet = |name: &str| {
            let key = cgs_refs::sheet_key(name);
            self.book
                .sheets
                .iter()
                .position(|s| !s.deleted && cgs_refs::sheet_key(&s.name) == key)
                .map(|i| i as u32)
        };
        if let (Some(sheet), Some(range)) = (sheet, range) {
            let at = find_sheet(sheet).ok_or("The PivotTable's source sheet is gone.")?;
            let ((top, left), (bottom, right)) =
                cgs_refs::parse_range(range).ok_or("The PivotTable's source isn't a range.")?;
            return Ok((
                at,
                Area {
                    top,
                    left,
                    bottom,
                    right,
                },
            ));
        }
        if let Some(name) = name {
            // A table, or a defined name.
            if let Some(table) = self
                .calc
                .tables()
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(name))
            {
                return Ok((
                    table.sheet,
                    Area {
                        top: table.range.top,
                        left: table.range.left,
                        bottom: table.range.bottom - u32::from(table.totals_row),
                        right: table.range.right,
                    },
                ));
            }
            let key = name.to_lowercase();
            if let Some(defined) = self
                .book
                .names
                .iter()
                .find(|n| n.name.to_lowercase() == key)
            {
                let text = defined.formula.trim_start_matches('=');
                if let Some(at) = text.rfind('!') {
                    let sheet_name = text[..at].trim_matches('\'').replace("''", "'");
                    if let (Some(sheet), Some(((top, left), (bottom, right)))) = (
                        find_sheet(&sheet_name),
                        cgs_refs::parse_range(&text[at + 1..]),
                    ) {
                        return Ok((
                            sheet,
                            Area {
                                top,
                                left,
                                bottom,
                                right,
                            },
                        ));
                    }
                }
            }
        }
        Err("The PivotTable's source can't be found.".to_string())
    }

    fn is_date_style(&self, style: u32) -> bool {
        let xf = self.book.styles.xf(style);
        self.num_fmt(xf.num_fmt).is_date()
    }

    /// Reads a cache's source cells again: its fields, and its records by column. Items the cache
    /// already had keep their indexes, so the file's records and pivot items still point at the
    /// same values; new values go after them.
    pub(crate) fn source_records(
        &mut self,
        cache: usize,
    ) -> Result<(Vec<CacheField>, CacheRecords, Area), String> {
        let def = self.book.pivot_caches[cache].clone();
        let (sheet, area) = self.source_area(&def)?;
        let info = self
            .book
            .sheets
            .get(sheet as usize)
            .ok_or("The PivotTable's source sheet is gone.")?;
        if !info.loaded {
            return Err("The PivotTable's source sheet is still being read.".to_string());
        }
        if area.bottom <= area.top {
            return Err(
                "A PivotTable's source needs a row of headings and a row of data.".to_string(),
            );
        }
        let rect = Rect {
            top: area.top,
            left: area.left,
            bottom: area.bottom,
            right: area.right,
        };
        // Formulas in the source finish calculating first.
        let _ = self.calc.calc_slice(0, &[SheetRect { sheet, rect }]);
        self.absorb_results();
        let mut block = Block(Vec::new());
        self.calc.read_block(SheetRect { sheet, rect }, &mut block);
        let mut cells: HashMap<(u32, u32), CalcValue> = block.0.into_iter().collect();
        let width = (area.right - area.left + 1) as usize;
        let count = (area.bottom - area.top) as usize;
        let info = &self.book.sheets[sheet as usize];
        let mut headings = Vec::with_capacity(width);
        for c in 0..width {
            let col = area.left + c as u32;
            let heading = match cells.remove(&(area.top, col)) {
                Some(CalcValue::Text(t)) if !t.trim().is_empty() => t,
                Some(CalcValue::Number(n)) => {
                    let mut out = String::new();
                    let _ = NumFmt::general().format(
                        Value::Number(n),
                        &Options {
                            locale: &Locale::en_us(),
                            date1904: self.book.date1904,
                        },
                        &mut out,
                    );
                    out
                }
                Some(CalcValue::Bool(b)) => if b { "TRUE" } else { "FALSE" }.to_string(),
                _ => {
                    return Err(
                        "The PivotTable field name is not valid. Each column of the source needs a heading."
                            .to_string(),
                    )
                }
            };
            headings.push(heading);
        }
        let mut fields: Vec<CacheField> = Vec::with_capacity(width);
        let mut columns: Vec<RecordColumn> = Vec::with_capacity(width);
        for (c, heading) in headings.iter().enumerate() {
            let col = area.left + c as u32;
            let old = def
                .fields
                .iter()
                .find(|f| f.database && f.name.eq_ignore_ascii_case(heading));
            let mut values: Vec<CacheValue> = old.map(|f| f.shared.clone()).unwrap_or_default();
            let mut keys: HashMap<ValueKey, u32> = values
                .iter()
                .enumerate()
                .map(|(i, v)| (ValueKey::of(v), i as u32))
                .collect();
            let mut index = Vec::with_capacity(count);
            let mut texts = false;
            let mut num_fmt = None;
            for r in 0..count {
                let row = area.top + 1 + r as u32;
                let value = match cells.remove(&(row, col)) {
                    None | Some(CalcValue::Empty) => CacheValue::Missing,
                    Some(CalcValue::Number(n)) => {
                        let style = info.style_at(row, col);
                        if num_fmt.is_none() {
                            num_fmt = Some(self.book.styles.xf(style).num_fmt);
                        }
                        if self.is_date_style(style) {
                            CacheValue::Date(n)
                        } else {
                            CacheValue::Number(n)
                        }
                    }
                    Some(CalcValue::Text(t)) => {
                        texts = true;
                        CacheValue::Text(t)
                    }
                    Some(CalcValue::Bool(b)) => CacheValue::Bool(b),
                    Some(CalcValue::Error(e)) => CacheValue::Error(e),
                };
                let key = ValueKey::of(&value);
                let at = match keys.get(&key) {
                    Some(&at) => at,
                    None => {
                        let at = values.len() as u32;
                        values.push(value);
                        keys.insert(key, at);
                        at
                    }
                };
                index.push(at);
            }
            let used_on_axis = old.is_some_and(|f| f.listed);
            let listed = texts || used_on_axis;
            fields.push(CacheField {
                name: heading.clone(),
                num_fmt_id: num_fmt.unwrap_or(0),
                shared: if listed { values.clone() } else { Vec::new() },
                listed,
                formula: None,
                database: true,
                group: old.and_then(|f| f.group.clone()),
            });
            columns.push(RecordColumn { values, index });
        }
        // Groups and other fields the source doesn't hold keep their place after the source's.
        for old in def.fields.iter().filter(|f| !f.database) {
            let mut field = old.clone();
            if let Some(group) = &mut field.group {
                let base_name = &def.fields.get(group.base as usize).map(|f| f.name.clone());
                if let Some(base_name) = base_name {
                    if let Some(at) = fields.iter().position(|f| &f.name == base_name) {
                        group.base = at as u32;
                    }
                }
                if let Some(column) = columns.get(group.base as usize) {
                    regroup_dates(group, column, self.book.date1904);
                }
            }
            fields.push(field);
            columns.push(RecordColumn::default());
        }
        // A field grouped in place follows its data too.
        for (i, field) in fields.iter_mut().enumerate() {
            if let (true, Some(group)) = (field.database, field.group.as_mut()) {
                regroup_dates(group, &columns[i], self.book.date1904);
            }
        }
        Ok((
            fields,
            CacheRecords {
                columns,
                count: count as u32,
            },
            area,
        ))
    }

    /// The text a number shows in a format, for labels such as "1/5/2024 Total".
    fn format_number(&self, n: f64, id: Option<u32>) -> String {
        let format = id.map_or_else(NumFmt::general, |id| self.num_fmt(id));
        let mut out = String::new();
        let _ = format.format(
            Value::Number(n),
            &Options {
                locale: &Locale::en_us(),
                date1904: self.book.date1904,
            },
            &mut out,
        );
        out
    }

    fn lay_out(&mut self, table: &PivotTable) -> Result<Layout, String> {
        let cache = table.cache as usize;
        let records = self.cache_records(cache)?;
        let def = &self.book.pivot_caches[cache];
        let view = CacheView::new(def, &records, self.book.date1904);
        let format = |n: f64, id: Option<u32>| self.format_number(n, id);
        cgs_pivot::layout(table, &view, &self.pivot_captions, &format)
    }

    /// A cell format for an output cell, made once per look.
    fn pivot_style(
        &mut self,
        cell: &cgs_pivot::OutCell,
        compact: bool,
        made: &mut HashMap<(u8, Option<u32>), u32>,
    ) -> u32 {
        let indent = match cell.role {
            Role::RowLabel { indent } if compact => Some(indent),
            _ => None,
        };
        let key = (indent.unwrap_or(255), cell.num_fmt);
        if let Some(&style) = made.get(&key) {
            return style;
        }
        let mut new = NewStyle::default();
        if let Some(indent) = indent {
            new.alignment = Some(if indent == 0 {
                " horizontal=\"left\"".to_string()
            } else {
                format!(" horizontal=\"left\" indent=\"{indent}\"")
            });
        }
        if let Some(id) = cell.num_fmt {
            new.number_format_id = Some(id);
        }
        let style = if new == NewStyle::default() {
            0
        } else {
            self.add_style(0, &new)
        };
        made.insert(key, style);
        style
    }

    /// The step that puts a pivot table's new layout in its cells and the model, with what it
    /// replaces. `before` is the area the old output covered.
    fn output_op(
        &mut self,
        index: usize,
        mut table: PivotTable,
        layout: Layout,
        before: Option<cgs_model::Rect>,
        restyle: bool,
    ) -> Result<Op, String> {
        let sheet = table.sheet;
        let area = layout.area;
        // The cells it newly covers must be free: no other pivot table or table, and nothing
        // typed there.
        for (other, t) in self.book.pivot_tables.iter().enumerate() {
            if other != index && t.sheet == sheet && overlaps(&pivot_area(t), &area) {
                return Err(
                    "A PivotTable report cannot overlap another PivotTable report.".to_string(),
                );
            }
        }
        if self.calc.tables().iter().any(|t| {
            t.sheet == sheet
                && overlaps(
                    &cgs_model::Rect {
                        top: t.range.top,
                        left: t.range.left,
                        bottom: t.range.bottom,
                        right: t.range.right,
                    },
                    &area,
                )
        }) {
            return Err("A PivotTable can't overlap a table.".to_string());
        }
        if area.bottom > cgs_model::MAX_ROWS || area.right > cgs_model::MAX_COLS {
            return Err("The PivotTable doesn't fit on the sheet.".to_string());
        }
        let mut block = Block(Vec::new());
        self.calc.read_block(
            SheetRect {
                sheet,
                rect: Rect {
                    top: area.top,
                    left: area.left,
                    bottom: area.bottom,
                    right: area.right,
                },
            },
            &mut block,
        );
        let inside_before = |row: u32, col: u32| {
            before
                .is_some_and(|b| row >= b.top && row <= b.bottom && col >= b.left && col <= b.right)
        };
        if block
            .0
            .iter()
            .any(|((row, col), v)| *v != CalcValue::Empty && !inside_before(*row, *col))
        {
            return Err(
                "There's already data where the PivotTable would go. Make room for it, or move it."
                    .to_string(),
            );
        }
        let compact = table.options.compact;
        let mut made = HashMap::new();
        let mut written: HashSet<(u32, u32)> = HashSet::new();
        let mut cells = Vec::with_capacity(layout.cells.len());
        for cell in &layout.cells {
            written.insert((cell.row, cell.col));
            let value = match &cell.value {
                CellValue::Text(t) => CalcValue::Text(t.clone()),
                CellValue::Number(n) => CalcValue::Number(*n),
                CellValue::Bool(b) => CalcValue::Bool(*b),
                CellValue::Error(e) => CalcValue::Error(e.clone()),
            };
            let style = restyle.then(|| self.pivot_style(cell, compact, &mut made));
            cells.push(EditCell {
                sheet,
                row: cell.row,
                col: cell.col,
                input: Some(EditInput::Value(value)),
                style,
            });
        }
        // What the old output covered and the new doesn't comes away, its look too.
        let mut cleared: BTreeMap<(u32, u32), ()> = BTreeMap::new();
        if let Some(b) = before {
            for row in b.top..=b.bottom {
                for col in b.left..=b.right {
                    let inside_new = row >= area.top
                        && row <= area.bottom
                        && col >= area.left
                        && col <= area.right;
                    if !written.contains(&(row, col)) && (!inside_new || restyle || true) {
                        cleared.insert((row, col), ());
                    }
                }
            }
        }
        for (row, col) in cleared.into_keys() {
            cells.push(EditCell {
                sheet,
                row,
                col,
                input: Some(EditInput::Clear),
                style: restyle.then_some(0),
            });
        }
        table.location = layout.location;
        table.row_items = layout.row_items;
        table.col_items = layout.col_items;
        for (f, items) in layout.items.into_iter().enumerate() {
            if let Some(items) = items {
                table.fields[f].items = items;
                table.fields[f].items_listed = true;
            }
        }
        table.changed = true;
        let mut tables = self.book.pivot_tables.clone();
        tables[index] = table;
        Ok(Op::Many(vec![
            Op::Cells(cells),
            Op::BookPivots {
                tables,
                caches: self.book.pivot_caches.clone(),
                records: self.pivot_records.clone(),
            },
        ]))
    }

    /// Lays a changed pivot table out again and makes the step that shows it.
    fn relayout(&mut self, index: usize, table: PivotTable) -> Result<Op, String> {
        if let Some(why) = &table.unsupported {
            return Err(format!("This PivotTable can't be changed here: {why}."));
        }
        if let Some(why) = &self.book.pivot_caches[table.cache as usize].unsupported {
            return Err(format!("This PivotTable can't be changed here: {why}."));
        }
        let current = &self.book.pivot_tables[index];
        // A new pivot table with no fields yet holds no cells.
        let before = (!current.row_items.is_empty() || !current.col_items.is_empty())
            .then(|| pivot_area(current));
        let layout = self.lay_out(&table)?;
        let restyle = layout.row_items != self.book.pivot_tables[index].row_items
            || layout.col_items != self.book.pivot_tables[index].col_items
            || layout.location != self.book.pivot_tables[index].location;
        self.output_op(index, table, layout, before, restyle)
    }

    /// Refresh: reads the source again, then lays out every pivot table on the cache.
    fn refresh_op(&mut self, cache: usize) -> Result<Op, String> {
        let (fields, records, area) = self.source_records(cache)?;
        let old = self.book.pivot_caches[cache].clone();
        // Fields move with their names; a field the source no longer has leaves its tables.
        let remap: Vec<Option<usize>> = old
            .fields
            .iter()
            .map(|f| {
                fields
                    .iter()
                    .position(|n| n.name.eq_ignore_ascii_case(&f.name))
            })
            .collect();
        let mut cache_def = old.clone();
        cache_def.fields = fields;
        cache_def.record_count = records.count;
        cache_def.changed = true;
        cache_def.refresh_on_load = true;
        if let CacheSource::Worksheet { range, .. } = &mut cache_def.source {
            if range.is_some() {
                *range = Some(area_text(area));
            }
        }
        let records = Rc::new(records);
        let mut caches = self.book.pivot_caches.clone();
        caches[cache] = cache_def;
        let mut slots = self.pivot_records.clone();
        slots.resize(caches.len(), None);
        slots[cache] = Some(records.clone());
        let saved_caches = std::mem::replace(&mut self.book.pivot_caches, caches);
        let saved_records = std::mem::replace(&mut self.pivot_records, slots);
        let saved_tables = self.book.pivot_tables.clone();
        let result = (|| {
            let mut ops = Vec::new();
            let indexes: Vec<usize> = (0..self.book.pivot_tables.len())
                .filter(|&i| self.book.pivot_tables[i].cache as usize == cache)
                .collect();
            for index in indexes {
                let table = remap_table(
                    &self.book.pivot_tables[index],
                    &remap,
                    &saved_caches[cache],
                    &self.book.pivot_caches[cache],
                    &records,
                );
                if table.unsupported.is_some() {
                    continue;
                }
                let before = pivot_area(&self.book.pivot_tables[index]);
                let layout = self.lay_out(&table)?;
                let restyle = layout.row_items != self.book.pivot_tables[index].row_items
                    || layout.col_items != self.book.pivot_tables[index].col_items
                    || layout.location != self.book.pivot_tables[index].location;
                let op = self.output_op(index, table, layout, Some(before), restyle)?;
                // Each table's step carries the lists as they stand after it.
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
        let new_records = std::mem::replace(&mut self.pivot_records, saved_records);
        let tables_after = std::mem::replace(&mut self.book.pivot_tables, saved_tables);
        let mut ops = result?;
        ops.push(Op::BookPivots {
            tables: tables_after,
            caches: new_caches,
            records: new_records,
        });
        Ok(Op::Many(ops))
    }

    pub(crate) fn refresh_pivot_command(
        &mut self,
        pivot: Option<PivotRef>,
    ) -> Result<Applied, String> {
        let caches: Vec<usize> = match &pivot {
            Some(pivot) => {
                let index = self.pivot_named(pivot)?;
                vec![self.book.pivot_tables[index].cache as usize]
            }
            None => {
                let mut list: Vec<usize> = self
                    .book
                    .pivot_tables
                    .iter()
                    .filter(|t| t.unsupported.is_none())
                    .map(|t| t.cache as usize)
                    .filter(|&c| self.book.pivot_caches[c].no_refresh.is_none())
                    .collect();
                list.sort_unstable();
                list.dedup();
                if list.is_empty() {
                    return Err("There's no PivotTable here that can be refreshed.".to_string());
                }
                list
            }
        };
        let snapshot = std::rc::Rc::new(std::cell::RefCell::new(caches));
        self.perform_steps("refreshing PivotTables", None, move |session, step| {
            let list = snapshot.borrow();
            match list.get(step) {
                Some(&cache) => session.refresh_op(cache).map(Some),
                None => Ok(None),
            }
        })
    }

    pub(crate) fn pivot_layout_command(
        &mut self,
        pivot: &PivotRef,
        spec: LayoutSpec,
    ) -> Result<Applied, String> {
        let index = self.pivot_named(pivot)?;
        let mut table = self.book.pivot_tables[index].clone();
        let cache = self.book.pivot_caches[table.cache as usize].clone();
        let cache = &cache;
        let count = table.fields.len() as i32;
        let valid = |f: i32| f == VALUES_FIELD || (0..count).contains(&f);
        if !spec.rows.iter().chain(&spec.cols).all(|&f| valid(f))
            || spec.pages.iter().any(|&f| f as i32 >= count)
            || spec.values.iter().any(|v| v.field as i32 >= count)
        {
            return Err("That field isn't in this PivotTable.".to_string());
        }
        let mut seen = HashSet::new();
        for &f in spec.rows.iter().chain(&spec.cols) {
            if f != VALUES_FIELD && !seen.insert(f) {
                return Err(
                    "A field can be in the rows, the columns or the filters once.".to_string(),
                );
            }
        }
        for &f in &spec.pages {
            if !seen.insert(f as i32) {
                return Err(
                    "A field can be in the rows, the columns or the filters once.".to_string(),
                );
            }
        }
        // Old value fields keep their names and settings where they stay.
        let old_values = table.data_fields.clone();
        let mut data_fields = Vec::new();
        let mut taken: HashSet<String> = HashSet::new();
        for spec in &spec.values {
            let f = spec.field as usize;
            let summary = spec
                .summary
                .as_deref()
                .and_then(parse_summary)
                .unwrap_or_else(|| default_summary(cache, f));
            let existing = old_values
                .iter()
                .find(|d| d.field == spec.field && d.summary == summary);
            let mut name = spec
                .name
                .clone()
                .or_else(|| existing.map(|d| d.name.clone()))
                .unwrap_or_else(|| {
                    let field_name = table.fields[f]
                        .name
                        .clone()
                        .unwrap_or_else(|| cache.fields[f].name.clone());
                    format!("{} of {}", summary.caption(), field_name)
                });
            // Two value fields can't share a name: Excel adds a number.
            let base = name.clone();
            let mut n = 2;
            while !taken.insert(name.to_lowercase()) {
                name = format!("{base}{n}");
                n += 1;
            }
            let show_as = spec
                .show_as
                .as_deref()
                .and_then(ShowAs::from_xml)
                .or_else(|| existing.map(|d| d.show_as))
                .unwrap_or_default();
            let base_item = match &spec.base_item {
                Some(serde_json::Value::String(s)) if s == "next" => BaseItem::Next,
                Some(serde_json::Value::String(s)) if s == "previous" => BaseItem::Previous,
                Some(serde_json::Value::Number(n)) => {
                    BaseItem::Item(n.as_u64().unwrap_or(0) as u32)
                }
                _ => existing.map_or(BaseItem::Item(0), |d| d.base_item),
            };
            let num_fmt_id = match &spec.number_format {
                Some(code) if code.is_empty() || code == "General" => None,
                Some(code) => Some(match cgs_numfmt::builtin_id(code) {
                    Some(id) => id,
                    None => self.styles_xml().add_num_fmt(code),
                }),
                None => existing.and_then(|d| d.num_fmt_id).or(match show_as {
                    ShowAs::Normal
                    | ShowAs::Difference
                    | ShowAs::RunTotal
                    | ShowAs::Index
                    | ShowAs::RankAscending
                    | ShowAs::RankDescending => None,
                    _ => Some(10),
                }),
            };
            data_fields.push(DataField {
                name,
                field: spec.field,
                summary,
                show_as,
                base_field: spec
                    .base_field
                    .or_else(|| existing.map(|d| d.base_field))
                    .unwrap_or(0),
                base_item,
                num_fmt_id,
            });
        }
        self.refresh_styles();
        let mut rows = spec.rows.clone();
        let mut cols = spec.cols.clone();
        // The values field is there only when two or more values are.
        let want_values = data_fields.len() > 1;
        if !want_values {
            rows.retain(|&f| f != VALUES_FIELD);
            cols.retain(|&f| f != VALUES_FIELD);
        } else if !rows.contains(&VALUES_FIELD) && !cols.contains(&VALUES_FIELD) {
            if table.options.data_on_rows {
                rows.push(VALUES_FIELD);
            } else {
                cols.push(VALUES_FIELD);
            }
        }
        table.options.data_on_rows = rows.contains(&VALUES_FIELD);
        table.options.data_position = None;
        for (f, field) in table.fields.iter_mut().enumerate() {
            let fi = f as i32;
            let axis = if rows.contains(&fi) {
                Some(PivotAxis::Row)
            } else if cols.contains(&fi) {
                Some(PivotAxis::Col)
            } else if spec.pages.contains(&(f as u32)) {
                Some(PivotAxis::Page)
            } else {
                None
            };
            if field.axis.is_some() && axis.is_none() {
                // A field taken out forgets its filter, as in Excel.
                for item in &mut field.items {
                    item.hidden = false;
                }
            }
            field.axis = axis;
            field.data_field = data_fields.iter().any(|d| d.field as usize == f);
        }
        let old_pages = table.page_fields.clone();
        table.page_fields = spec
            .pages
            .iter()
            .map(|&f| {
                old_pages
                    .iter()
                    .find(|p| p.field == f)
                    .cloned()
                    .unwrap_or(PageField {
                        field: f,
                        item: None,
                        name: None,
                    })
            })
            .collect();
        table.row_fields = rows;
        table.col_fields = cols;
        table.data_fields = data_fields;
        // A sort by a value field that went falls back to the labels.
        let nd = table.data_fields.len() as u32;
        for field in &mut table.fields {
            if field.sort_by_data.is_some_and(|d| d >= nd) {
                field.sort_by_data = None;
            }
        }
        let op = self.relayout(index, table)?;
        self.perform("PivotTable layout", op, None)
    }

    pub(crate) fn pivot_options_command(
        &mut self,
        pivot: &PivotRef,
        spec: OptionSpec,
    ) -> Result<Applied, String> {
        let index = self.pivot_named(pivot)?;
        let mut table = self.book.pivot_tables[index].clone();
        if let Some(form) = &spec.form {
            let (compact, outline) = match form.as_str() {
                "compact" => (true, true),
                "outline" => (false, true),
                "tabular" => (false, false),
                _ => return Err("That's not a report layout.".to_string()),
            };
            table.options.compact = compact;
            table.options.compact_data = compact;
            table.options.outline = outline;
            table.options.outline_data = outline;
            for field in &mut table.fields {
                field.compact = compact;
                field.outline = outline;
            }
        }
        if let Some(subtotals) = &spec.subtotals {
            for field in &mut table.fields {
                match subtotals.as_str() {
                    "none" => {
                        field.default_subtotal = false;
                        field.subtotals.clear();
                    }
                    "top" => {
                        field.default_subtotal = true;
                        field.subtotal_top = true;
                    }
                    "bottom" => {
                        field.default_subtotal = true;
                        field.subtotal_top = false;
                    }
                    _ => return Err("That's not a subtotal setting.".to_string()),
                }
                sync_subtotal_item(field);
            }
        }
        if let Some(grand) = &spec.grand_totals {
            let (rows, cols) = match grand.as_str() {
                "off" => (false, false),
                "both" => (true, true),
                "rows" => (true, false),
                "cols" => (false, true),
                _ => return Err("That's not a grand total setting.".to_string()),
            };
            table.options.row_grand_totals = rows;
            table.options.col_grand_totals = cols;
        }
        if let Some(blank) = spec.blank_rows {
            let last_row = table
                .row_fields
                .iter()
                .rev()
                .find(|&&f| f != VALUES_FIELD)
                .copied();
            for &f in &table.row_fields.clone() {
                if f != VALUES_FIELD {
                    table.fields[f as usize].insert_blank_row = blank && Some(f) != last_row;
                }
            }
        }
        if let Some(repeat) = spec.repeat_labels {
            for field in &mut table.fields {
                field.repeat_labels = repeat;
            }
        }
        let style = &mut table.style;
        if let Some(name) = &spec.style {
            style.name = (!name.is_empty()).then(|| name.clone());
        }
        let set = |flag: Option<bool>, field: &mut bool| {
            if let Some(flag) = flag {
                *field = flag;
            }
        };
        set(spec.row_headers, &mut style.row_headers);
        set(spec.col_headers, &mut style.col_headers);
        set(spec.row_stripes, &mut style.row_stripes);
        set(spec.col_stripes, &mut style.col_stripes);
        if let Some(name) = &spec.name {
            let name = name.trim();
            if name.is_empty() {
                return Err("A PivotTable needs a name.".to_string());
            }
            if self.book.pivot_tables.iter().enumerate().any(|(i, t)| {
                i != index && t.sheet == table.sheet && t.name.eq_ignore_ascii_case(name)
            }) {
                return Err("A PivotTable with that name is already on this sheet.".to_string());
            }
            table.name = name.to_string();
        }
        if let Some(text) = &spec.empty_text {
            table.options.missing_caption = Some(text.clone());
            table.options.show_missing = true;
        }
        if let Some(error) = &spec.error_text {
            table.options.show_error = error.is_some();
            table.options.error_caption = error.clone();
        }
        if let Some(headers) = spec.headers {
            table.options.show_headers = headers;
        }
        let only_look = spec.form.is_none()
            && spec.subtotals.is_none()
            && spec.grand_totals.is_none()
            && spec.blank_rows.is_none()
            && spec.repeat_labels.is_none()
            && spec.empty_text.is_none()
            && spec.error_text.is_none()
            && spec.headers.is_none();
        if only_look {
            table.changed = true;
            let mut tables = self.book.pivot_tables.clone();
            tables[index] = table;
            let op = Op::BookPivots {
                tables,
                caches: self.book.pivot_caches.clone(),
                records: self.pivot_records.clone(),
            };
            return self.perform("PivotTable style", op, None);
        }
        let op = self.relayout(index, table)?;
        self.perform("PivotTable options", op, None)
    }

    pub(crate) fn pivot_field_command(
        &mut self,
        pivot: &PivotRef,
        field: u32,
        change: FieldChange,
    ) -> Result<Applied, String> {
        let index = self.pivot_named(pivot)?;
        let mut table = self.book.pivot_tables[index].clone();
        let f = field as usize;
        if f >= table.fields.len() {
            return Err("That field isn't in this PivotTable.".to_string());
        }
        // A field's items, made if it never had any.
        if !table.fields[f]
            .items
            .iter()
            .any(|i| i.kind == ItemType::Data)
        {
            let records = self.cache_records(table.cache as usize)?;
            let def = &self.book.pivot_caches[table.cache as usize];
            let view = CacheView::new(def, &records, self.book.date1904);
            table.fields[f].items = cgs_pivot::default_items(view.items(f));
        }
        let data_count = table.fields[f]
            .items
            .iter()
            .filter(|i| i.kind == ItemType::Data)
            .count();
        let field_ref = &mut table.fields[f];
        if let Some(hidden) = &change.hidden {
            let hide: HashSet<u32> = hidden.iter().copied().collect();
            if hide.len() >= data_count && data_count > 0 {
                return Err("At least one item must show.".to_string());
            }
            for (pos, item) in field_ref.items.iter_mut().enumerate() {
                if item.kind == ItemType::Data {
                    item.hidden = hide.contains(&(pos as u32));
                }
            }
        }
        if let Some(multiple) = change.multiple {
            field_ref.multiple_item_selection = multiple;
        }
        if let Some(sort) = &change.sort {
            field_ref.sort = match sort.as_str() {
                "ascending" => FieldSort::Ascending,
                "descending" => FieldSort::Descending,
                _ => FieldSort::Manual,
            };
            if field_ref.sort == FieldSort::Manual {
                field_ref.sort_by_data = None;
            }
        }
        if let Some(by) = change.sort_by {
            field_ref.sort_by_data = by;
        }
        if let Some((pos, expanded)) = change.expand {
            if let Some(item) = field_ref.items.get_mut(pos as usize) {
                item.show_details = expanded;
            }
        }
        if let Some(expanded) = change.expand_all {
            for item in &mut field_ref.items {
                if item.kind == ItemType::Data {
                    item.show_details = expanded;
                }
            }
        }
        if let Some(list) = &change.subtotals {
            field_ref.subtotals.clear();
            field_ref.default_subtotal = false;
            for kind in list {
                match kind.as_str() {
                    "automatic" => field_ref.default_subtotal = true,
                    "none" => {}
                    other => {
                        let summary = parse_summary(other).ok_or("That's not a summary.")?;
                        let item = subtotal_item(summary);
                        field_ref.subtotals.push(item);
                        field_ref.default_subtotal = true;
                    }
                }
            }
            sync_subtotal_item(field_ref);
        }
        if let Some(caption) = &change.caption {
            field_ref.name = (!caption.is_empty()).then(|| caption.clone());
        }
        if let Some(show_all) = change.show_all {
            field_ref.show_all = show_all;
        }
        if let Some(blank) = change.blank_row {
            field_ref.insert_blank_row = blank;
        }
        if let Some(page) = change.page {
            match table.page_fields.iter_mut().find(|p| p.field == field) {
                Some(entry) => entry.item = page,
                None => return Err("That field isn't a report filter.".to_string()),
            }
            if page.is_some() {
                for item in &mut table.fields[f].items {
                    item.hidden = false;
                }
            }
        }
        let op = self.relayout(index, table)?;
        self.perform("PivotTable field", op, None)
    }

    /// Insert › PivotTable: a new cache over the cells, and an empty pivot table where it goes.
    pub(crate) fn create_pivot_command(
        &mut self,
        sheet: u32,
        area: Area,
        to: Destination,
    ) -> Result<Applied, String> {
        let info = self.sheet_for_edit(sheet)?;
        if info.kind != cgs_model::SheetKind::Worksheet || !info.loaded {
            return Err("This sheet has no cells to summarize.".to_string());
        }
        let area = if area.rows() == 1 && area.cols() == 1 {
            self.current_region(sheet, area.top, area.left)
        } else {
            area
        };
        if area.rows() < 2 {
            return Err(
                "A PivotTable's source needs a row of headings and a row of data.".to_string(),
            );
        }
        let source_name = info.name.clone();
        // A table's cells go by its name, as Excel does it.
        let table_name = self
            .calc
            .tables()
            .iter()
            .find(|t| {
                t.sheet == sheet
                    && t.range.top == area.top
                    && t.range.left == area.left
                    && t.range.bottom == area.bottom
                    && t.range.right == area.right
            })
            .map(|t| t.name.clone());
        let cache = PivotCache {
            id: self.next_cache_id(),
            source: match table_name {
                Some(name) => CacheSource::Worksheet {
                    sheet: None,
                    range: None,
                    name: Some(name),
                },
                None => CacheSource::Worksheet {
                    sheet: Some(source_name),
                    range: Some(area_text(area)),
                    name: None,
                },
            },
            save_data: true,
            refresh_on_load: true,
            changed: true,
            ..PivotCache::default()
        };
        let cache_index = self.book.pivot_caches.len();
        let name = self.next_pivot_name();
        let new_sheet = matches!(to, Destination::NewSheet);
        let mut slot: Option<(u32, u32, u32)> = match to {
            Destination::Existing { sheet, row, col } => {
                let target = self.sheet_for_edit(sheet)?;
                if target.kind != cgs_model::SheetKind::Worksheet {
                    return Err("A PivotTable goes on a worksheet.".to_string());
                }
                Some((sheet, row.max(1), col.max(1)))
            }
            Destination::NewSheet => None,
        };
        self.perform_steps("making a PivotTable", None, move |session, step| {
            if new_sheet && step == 0 {
                // A new sheet before the source's, as Excel puts it.
                let name = crate::command::next_sheet_name(&session.book);
                let position = session
                    .book
                    .tab_order()
                    .iter()
                    .position(|&s| s == sheet)
                    .unwrap_or(0) as u32;
                slot = Some((session.book.sheets.len() as u32, 3, 1));
                return Ok(Some(Op::AddSheet { name, position }));
            }
            if step != usize::from(new_sheet) {
                return Ok(None);
            }
            let (at_sheet, row, col) = slot.ok_or("There's nowhere to put the PivotTable.")?;
            let mut caches = session.book.pivot_caches.clone();
            caches.push(cache.clone());
            let mut slots = session.pivot_records.clone();
            slots.resize(caches.len(), None);
            let saved_caches = std::mem::replace(&mut session.book.pivot_caches, caches);
            let saved_slots = std::mem::replace(&mut session.pivot_records, slots);
            let made = session.source_records(cache_index);
            let (fields, records) = match made {
                Ok((fields, records, _)) => (fields, records),
                Err(problem) => {
                    session.book.pivot_caches = saved_caches;
                    session.pivot_records = saved_slots;
                    return Err(problem);
                }
            };
            {
                let def = &mut session.book.pivot_caches[cache_index];
                def.fields = fields;
                def.record_count = records.count;
            }
            session.pivot_records[cache_index] = Some(Rc::new(records));
            let table = new_table(
                &name,
                at_sheet,
                cache_index as u32,
                row,
                col,
                &session.book.pivot_caches[cache_index],
            );
            let new_caches = std::mem::replace(&mut session.book.pivot_caches, saved_caches);
            let new_slots = std::mem::replace(&mut session.pivot_records, saved_slots);
            let mut tables = session.book.pivot_tables.clone();
            tables.push(table);
            Ok(Some(Op::BookPivots {
                tables,
                caches: new_caches,
                records: new_slots,
            }))
        })
    }

    pub(crate) fn delete_pivot_command(&mut self, pivot: &PivotRef) -> Result<Applied, String> {
        let index = self.pivot_named(pivot)?;
        let table = &self.book.pivot_tables[index];
        let sheet = table.sheet;
        let area = pivot_area(table);
        let cells: Vec<EditCell> = (area.top..=area.bottom)
            .flat_map(|row| {
                (area.left..=area.right).map(move |col| EditCell {
                    sheet,
                    row,
                    col,
                    input: Some(EditInput::Clear),
                    style: Some(0),
                })
            })
            .collect();
        let mut tables = self.book.pivot_tables.clone();
        tables.remove(index);
        let op = Op::Many(vec![
            Op::Cells(cells),
            Op::BookPivots {
                tables,
                caches: self.book.pivot_caches.clone(),
                records: self.pivot_records.clone(),
            },
        ]);
        self.perform("deleting a PivotTable", op, None)
    }

    fn next_cache_id(&self) -> u32 {
        self.book
            .pivot_caches
            .iter()
            .map(|c| c.id + 1)
            .max()
            .unwrap_or(1)
    }

    fn next_pivot_name(&self) -> String {
        let taken: HashSet<String> = self
            .book
            .pivot_tables
            .iter()
            .map(|t| t.name.to_lowercase())
            .collect();
        (1..)
            .map(|n| format!("PivotTable{n}"))
            .find(|name| !taken.contains(&name.to_lowercase()))
            .expect("a free name")
    }

    /// Applies a whole-list step for pivot tables, returning the lists as they were.
    pub(crate) fn apply_book_pivots(
        &mut self,
        tables: Vec<PivotTable>,
        caches: Vec<PivotCache>,
        records: Vec<Option<Rc<CacheRecords>>>,
    ) -> Op {
        for table in tables.iter().chain(self.book.pivot_tables.iter()) {
            self.dirty.sheets.insert(table.sheet);
        }
        self.edits.pivots_changed = true;
        Op::BookPivots {
            tables: std::mem::replace(&mut self.book.pivot_tables, tables),
            caches: std::mem::replace(&mut self.book.pivot_caches, caches),
            records: std::mem::replace(&mut self.pivot_records, records),
        }
    }

    /// Moves pivot tables and their sources with rows and columns put in or taken out.
    pub(crate) fn move_pivots(&mut self, sheet: u32, edit: &cgs_refs::AxisEdit) {
        let name = self.book.sheets[sheet as usize].name.clone();
        for table in &mut self.book.pivot_tables {
            if table.sheet != sheet {
                continue;
            }
            let r = table.location.range;
            match cgs_refs::edit_rect((r.top, r.left, r.bottom, r.right), edit) {
                Some((top, left, bottom, right)) => {
                    table.location.range = cgs_model::Rect {
                        top,
                        left,
                        bottom,
                        right,
                    };
                }
                None => {
                    table.unsupported = Some("its cells were deleted".to_string());
                }
            }
        }
        for cache in &mut self.book.pivot_caches {
            if let CacheSource::Worksheet {
                sheet: Some(on),
                range: Some(range),
                ..
            } = &mut cache.source
            {
                if cgs_refs::sheet_key(on) != cgs_refs::sheet_key(&name) {
                    continue;
                }
                if let Some(((top, left), (bottom, right))) = cgs_refs::parse_range(range) {
                    match cgs_refs::edit_rect((top, left, bottom, right), edit) {
                        Some((top, left, bottom, right)) => {
                            *range = area_text(Area {
                                top,
                                left,
                                bottom,
                                right,
                            });
                        }
                        None => cache.no_refresh = Some("its source was deleted".to_string()),
                    }
                }
            }
        }
    }

    /// A sheet renamed: caches that read it follow.
    pub(crate) fn rename_pivot_sources(&mut self, old: &str, new: &str) {
        for cache in &mut self.book.pivot_caches {
            if let CacheSource::Worksheet {
                sheet: Some(on), ..
            } = &mut cache.source
            {
                if cgs_refs::sheet_key(on) == cgs_refs::sheet_key(old) {
                    *on = new.to_string();
                }
            }
        }
    }
}

/// `m/d/yyyy`, as Excel labels a date group's ends.
fn short_date(serial: f64, date1904: bool) -> String {
    let (y, m, d) = cgs_pivot::view::civil(serial, date1904);
    format!("{m}/{d}/{y}")
}

/// A date group whose ends are automatic takes them from the data again: from the first date to
/// the day after the last, with "<start" and ">end" items and, by years, a year each.
fn regroup_dates(group: &mut cgs_model::pivot::FieldGroup, column: &RecordColumn, date1904: bool) {
    let Some(cgs_model::pivot::GroupRange::Dates {
        by,
        start,
        end,
        auto_start,
        auto_end,
        ..
    }) = group.range.as_mut()
    else {
        return;
    };
    let used: HashSet<u32> = column.index.iter().copied().collect();
    let dates: Vec<f64> = column
        .values
        .iter()
        .enumerate()
        .filter(|(i, _)| used.contains(&(*i as u32)))
        .filter_map(|(_, v)| match v {
            CacheValue::Date(d) | CacheValue::Number(d) => Some(*d),
            _ => None,
        })
        .collect();
    if dates.is_empty() {
        return;
    }
    let min = dates.iter().copied().fold(f64::INFINITY, f64::min).floor();
    let max = dates
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max)
        .floor();
    if *auto_start {
        *start = min;
    }
    if *auto_end {
        *end = max + 1.0;
    }
    let n = group.items.len();
    if n < 2 {
        return;
    }
    let first = CacheValue::Text(format!("<{}", short_date(*start, date1904)));
    let last = CacheValue::Text(format!(">{}", short_date(*end, date1904)));
    if *by == cgs_model::pivot::DateGrouping::Years {
        let (y0, _, _) = cgs_pivot::view::civil(*start, date1904);
        let (y1, _, _) = cgs_pivot::view::civil(max.min(*end), date1904);
        let mut items = vec![first];
        for y in y0..=y1 {
            items.push(CacheValue::Text(y.to_string()));
        }
        items.push(last);
        group.items = items;
    } else {
        group.items[0] = first;
        group.items[n - 1] = last;
    }
}

fn subtotal_item(summary: Summary) -> ItemType {
    match summary {
        Summary::Sum => ItemType::Sum,
        Summary::Count => ItemType::CountA,
        Summary::Average => ItemType::Avg,
        Summary::Max => ItemType::Max,
        Summary::Min => ItemType::Min,
        Summary::Product => ItemType::Product,
        Summary::CountNums => ItemType::Count,
        Summary::StdDev => ItemType::StdDev,
        Summary::StdDevP => ItemType::StdDevP,
        Summary::Var => ItemType::Var,
        Summary::VarP => ItemType::VarP,
    }
}

/// A field's subtotal items at the end of its items, as its subtotals say.
fn sync_subtotal_item(field: &mut PivotField) {
    if !field.items.iter().any(|i| i.kind == ItemType::Data) {
        return;
    }
    field.items.retain(|i| i.kind == ItemType::Data);
    if field.subtotals.is_empty() {
        if field.default_subtotal {
            field.items.push(PivotItem::total(ItemType::Default));
        }
    } else {
        for kind in field.subtotals.clone() {
            field.items.push(PivotItem::total(kind));
        }
    }
}

/// Excel's starting summary: Sum for numbers, Count for anything else.
fn default_summary(cache: &PivotCache, field: usize) -> Summary {
    let Some(f) = cache.fields.get(field) else {
        return Summary::Count;
    };
    let numeric = !f.shared.is_empty()
        && f.shared
            .iter()
            .all(|v| matches!(v, CacheValue::Number(_) | CacheValue::Missing));
    if numeric || (!f.listed && f.database) {
        Summary::Sum
    } else {
        Summary::Count
    }
}

/// Everything a pivot table covers on its sheet, report filters included.
pub(crate) fn pivot_area(table: &PivotTable) -> cgs_model::Rect {
    let r = table.location.range;
    let pages = table.page_fields.len() as u32;
    if pages == 0 {
        return r;
    }
    cgs_model::Rect {
        top: r.top.saturating_sub(pages + 1).max(1),
        left: r.left,
        bottom: r.bottom,
        right: r.right.max(r.left + 1),
    }
}

fn overlaps(a: &cgs_model::Rect, b: &cgs_model::Rect) -> bool {
    a.top <= b.bottom && b.top <= a.bottom && a.left <= b.right && b.left <= a.right
}

pub(crate) fn area_text(area: Area) -> String {
    let cell = |row: u32, col: u32| format!("{}{}", cgs_refs::col_to_letters(i64::from(col)), row);
    if area.top == area.bottom && area.left == area.right {
        cell(area.top, area.left)
    } else {
        format!(
            "{}:{}",
            cell(area.top, area.left),
            cell(area.bottom, area.right)
        )
    }
}

/// A pivot table's fields after its cache was read again: fields follow their names, and items
/// follow their values.
fn remap_table(
    table: &PivotTable,
    remap: &[Option<usize>],
    old: &PivotCache,
    new: &PivotCache,
    records: &CacheRecords,
) -> PivotTable {
    let mut out = table.clone();
    let count = new.fields.len();
    let mut fields = vec![PivotField::default(); count];
    for (f, field) in fields.iter_mut().enumerate() {
        field.compact = table.options.compact;
        field.outline = table.options.outline || table.options.compact;
        let _ = f;
    }
    let old_values = |f: usize| -> Vec<CacheValue> {
        match &old.fields[f].group {
            Some(g) if !g.items.is_empty() => g.items.clone(),
            _ => old.fields[f].shared.clone(),
        }
    };
    let mut lost = false;
    for (f, target) in remap.iter().enumerate() {
        let Some(t) = *target else {
            if table.fields[f].axis.is_some() || table.fields[f].data_field {
                lost = true;
            }
            continue;
        };
        let mut field = table.fields[f].clone();
        let new_values: &[CacheValue] = match &new.fields[t].group {
            Some(g) if !g.items.is_empty() => &g.items,
            _ => records
                .columns
                .get(t)
                .map_or(&[][..], |c| c.values.as_slice()),
        };
        if new.fields[t]
            .group
            .as_ref()
            .is_some_and(|g| !g.items.is_empty())
        {
            // A group's items are all its group items, in their order.
            let before = old_values(f);
            let was: HashMap<ValueKey, &PivotItem> = field
                .items
                .iter()
                .filter(|i| i.kind == ItemType::Data)
                .filter_map(|i| Some((ValueKey::of(before.get(i.x? as usize)?), i)))
                .collect();
            let mut items: Vec<PivotItem> = (0..new_values.len() as u32)
                .map(|x| {
                    let mut item = PivotItem::data(x);
                    if let Some(old) = was.get(&ValueKey::of(&new_values[x as usize])) {
                        item.hidden = old.hidden;
                        item.show_details = old.show_details;
                        item.name = old.name.clone();
                    }
                    item
                })
                .collect();
            items.extend(
                field
                    .items
                    .iter()
                    .filter(|i| i.kind != ItemType::Data)
                    .cloned(),
            );
            if field.items.iter().any(|i| i.kind == ItemType::Data) {
                field.items = items;
            }
        } else if field.items.iter().any(|i| i.kind == ItemType::Data) {
            let index: HashMap<ValueKey, u32> = new_values
                .iter()
                .enumerate()
                .map(|(i, v)| (ValueKey::of(v), i as u32))
                .collect();
            let before = old_values(f);
            let mut kept: Vec<PivotItem> = Vec::new();
            let mut seen: HashSet<u32> = HashSet::new();
            for item in field.items.iter().filter(|i| i.kind == ItemType::Data) {
                let Some(value) = item.x.and_then(|x| before.get(x as usize)) else {
                    continue;
                };
                if let Some(&x) = index.get(&ValueKey::of(value)) {
                    if seen.insert(x) {
                        let mut moved = item.clone();
                        moved.x = Some(x);
                        kept.push(moved);
                    }
                }
            }
            // New values go at the end, in the order the cache has them.
            let used: HashSet<u32> = records
                .columns
                .get(t)
                .map(|c| c.index.iter().copied().collect())
                .unwrap_or_default();
            // A field whose filter hides items lets new ones in hidden, unless it says otherwise.
            let filtered = field
                .items
                .iter()
                .any(|i| i.kind == ItemType::Data && i.hidden)
                && !field.include_new_items;
            for x in 0..new_values.len() as u32 {
                if !seen.contains(&x) && used.contains(&x) {
                    let mut item = PivotItem::data(x);
                    item.hidden = filtered;
                    kept.push(item);
                }
            }
            // A sorted field keeps its items in the order it shows them.
            if field.sort != FieldSort::Manual && field.sort_by_data.is_none() {
                kept.sort_by(|a, b| {
                    let va = a.x.and_then(|x| new_values.get(x as usize));
                    let vb = b.x.and_then(|x| new_values.get(x as usize));
                    let ord = match (va, vb) {
                        (Some(x), Some(y)) => cgs_pivot::compare_values(x, y),
                        _ => std::cmp::Ordering::Equal,
                    };
                    if field.sort == FieldSort::Descending {
                        ord.reverse()
                    } else {
                        ord
                    }
                });
            }
            let totals: Vec<PivotItem> = field
                .items
                .iter()
                .filter(|i| i.kind != ItemType::Data)
                .cloned()
                .collect();
            kept.extend(totals);
            field.items = kept;
        }
        fields[t] = field;
    }
    let map = |f: i32| -> Option<i32> {
        if f == VALUES_FIELD {
            return Some(f);
        }
        remap.get(f as usize).copied().flatten().map(|t| t as i32)
    };
    out.fields = fields;
    out.row_fields = table.row_fields.iter().filter_map(|&f| map(f)).collect();
    out.col_fields = table.col_fields.iter().filter_map(|&f| map(f)).collect();
    out.page_fields = table
        .page_fields
        .iter()
        .filter_map(|p| {
            let field = map(p.field as i32)? as u32;
            Some(PageField { field, ..p.clone() })
        })
        .collect();
    out.data_fields = table
        .data_fields
        .iter()
        .filter_map(|d| {
            let field = map(d.field as i32)? as u32;
            let base_field = map(d.base_field as i32).unwrap_or(0) as u32;
            Some(DataField {
                field,
                base_field,
                ..d.clone()
            })
        })
        .collect();
    if lost && out.data_fields.is_empty() && out.row_fields.is_empty() && out.col_fields.is_empty()
    {
        out.unsupported = Some("its fields are gone from the source".to_string());
    }
    out
}

/// A new pivot table with no fields yet, as Insert › PivotTable makes it.
fn new_table(
    name: &str,
    sheet: u32,
    cache: u32,
    row: u32,
    col: u32,
    def: &PivotCache,
) -> PivotTable {
    let fields = def
        .fields
        .iter()
        .map(|_| PivotField {
            show_all: false,
            ..PivotField::default()
        })
        .collect();
    PivotTable {
        name: name.to_string(),
        sheet,
        part: None,
        cache,
        location: PivotLocation {
            range: cgs_model::Rect {
                top: row,
                left: col,
                bottom: row + 17,
                right: col + 2,
            },
            first_header_row: 1,
            first_data_row: 1,
            first_data_col: 1,
            row_page_count: 0,
            col_page_count: 0,
        },
        fields,
        row_fields: Vec::new(),
        col_fields: Vec::new(),
        page_fields: Vec::new(),
        data_fields: Vec::new(),
        row_items: Vec::new(),
        col_items: Vec::new(),
        options: PivotOptions {
            data_caption: "Values".to_string(),
            outline: true,
            outline_data: true,
            hide_values_row: true,
            indent: 0,
            created_version: 6,
            updated_version: 6,
            ..PivotOptions::default()
        },
        style: PivotStyleInfo::default(),
        unsupported: None,
        changed: true,
    }
}

/// The value a pivot cell's label stands for, for the field list's filter.
pub(crate) fn item_label(value: &CacheValue, blank: &str) -> String {
    match value {
        CacheValue::Missing => blank.to_string(),
        CacheValue::Text(t) => t.clone(),
        CacheValue::Number(n) | CacheValue::Date(n) => {
            let _ = cell_of(value);
            let mut out = String::new();
            let _ = NumFmt::general().format(
                Value::Number(*n),
                &Options {
                    locale: &Locale::en_us(),
                    date1904: false,
                },
                &mut out,
            );
            out
        }
        CacheValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        CacheValue::Error(e) => e.clone(),
    }
}

fn summary_name(summary: Summary) -> &'static str {
    summary.xml()
}

fn axis_name(axis: Option<PivotAxis>) -> Option<&'static str> {
    axis.map(|a| match a {
        PivotAxis::Row => "row",
        PivotAxis::Col => "col",
        PivotAxis::Page => "page",
        PivotAxis::Values => "values",
    })
}

/// Which column of the row area each row field's labels go in, as `cgs-pivot` lays them out.
fn row_columns(table: &PivotTable) -> Vec<u32> {
    let mut out = Vec::new();
    let mut col = 0u32;
    for i in 0..table.row_fields.len() {
        if i > 0 {
            let prev = table.row_fields[i - 1];
            let shares = if prev == VALUES_FIELD {
                table.options.compact_data && table.options.outline_data
            } else {
                let field = &table.fields[prev as usize];
                field.compact && field.outline
            };
            if !shares {
                col += 1;
            }
        }
        out.push(col);
    }
    out
}

/// A line's members in full, filling in what `repeat` leaves out from the lines before.
fn full_members(lines: &[cgs_model::pivot::AxisLine], index: usize) -> Vec<u32> {
    let mut current: Vec<u32> = Vec::new();
    let end = index.min(lines.len().saturating_sub(1));
    for line in &lines[..=end] {
        current.truncate(line.repeat as usize);
        current.extend_from_slice(&line.members);
    }
    current
}

fn field_json(
    f: usize,
    field: &PivotField,
    cache: &PivotCache,
    values: &[CacheValue],
    blank: &str,
) -> serde_json::Value {
    let numeric = !values.is_empty()
        && values
            .iter()
            .all(|v| matches!(v, CacheValue::Number(_) | CacheValue::Missing))
        && cache.fields[f].database
        && cache.fields[f].group.is_none();
    let label = |item: &PivotItem| {
        item.name.clone().unwrap_or_else(|| {
            item.x
                .and_then(|x| values.get(x as usize))
                .map_or(String::new(), |v| item_label(v, blank))
        })
    };
    let mut items: Vec<serde_json::Value> = Vec::new();
    let has_items = field.items.iter().any(|i| i.kind == ItemType::Data);
    let fresh;
    let list: &[PivotItem] = if has_items {
        &field.items
    } else {
        fresh = cgs_pivot::default_items(values);
        &fresh
    };
    for (pos, item) in list.iter().enumerate() {
        if item.kind != ItemType::Data {
            continue;
        }
        items.push(serde_json::json!({
            "index": pos,
            "label": label(item),
            "hidden": item.hidden,
            "expanded": item.show_details,
        }));
    }
    let subtotals: Vec<&str> = if field.subtotals.is_empty() {
        if field.default_subtotal {
            vec!["automatic"]
        } else {
            Vec::new()
        }
    } else {
        field
            .subtotals
            .iter()
            .filter_map(|k| k.function().map(summary_name))
            .collect()
    };
    let sort = match field.sort {
        FieldSort::Manual => "manual",
        FieldSort::Ascending => "ascending",
        FieldSort::Descending => "descending",
    };
    serde_json::json!({
        "index": f,
        "name": cache.fields[f].name,
        "caption": field.name.clone().unwrap_or_else(|| cache.fields[f].name.clone()),
        "axis": axis_name(field.axis),
        "numeric": numeric,
        "items": items,
        "sort": sort,
        "sortBy": field.sort_by_data,
        "subtotals": subtotals,
        "showAll": field.show_all,
        "blankRow": field.insert_blank_row,
        "multiple": field.multiple_item_selection,
        "grouped": cache.fields[f].group.is_some(),
    })
}

/// What a cell of a pivot table is: a label of which field and item, a value of which value
/// field, a report filter, a header or a total.
fn cell_json(table: &PivotTable, row: u32, col: u32) -> serde_json::Value {
    let l = &table.location;
    let r = l.range;
    let real = |fields: &[i32]| fields.iter().filter(|&&f| f != VALUES_FIELD).count();
    if row < r.top {
        let top = pivot_area(table).top;
        let i = (row - top) as usize;
        return match table.page_fields.get(i) {
            Some(p) => serde_json::json!({ "role": "page", "field": p.field }),
            None => serde_json::json!({ "role": "other" }),
        };
    }
    let data_top = r.top + l.first_data_row;
    let data_left = r.left + l.first_data_col;
    if row >= data_top && col < data_left {
        let k = (row - data_top) as usize;
        let Some(line) = table.row_items.get(k) else {
            return serde_json::json!({ "role": "other" });
        };
        if line.kind == ItemType::Grand {
            return serde_json::json!({ "role": "grand" });
        }
        let members = full_members(&table.row_items, k);
        let columns = row_columns(table);
        let c = col - r.left;
        let level = (0..members.len())
            .rev()
            .find(|&lv| columns.get(lv).is_some_and(|&x| x <= c));
        return match level {
            Some(level) if table.row_fields.get(level) != Some(&VALUES_FIELD) => {
                serde_json::json!({
                    "role": "item",
                    "axis": "row",
                    "field": table.row_fields[level],
                    "item": members[level],
                    "subtotal": line.kind != ItemType::Data,
                    "hasChildren": level + 1 < real(&table.row_fields),
                })
            }
            Some(level) => serde_json::json!({ "role": "value", "value": members[level] }),
            None => serde_json::json!({ "role": "other" }),
        };
    }
    if row >= r.top + l.first_header_row && row < data_top && col >= data_left {
        let k = (col - data_left) as usize;
        let first_item_row = data_top - table.col_fields.len() as u32;
        if row < first_item_row {
            return serde_json::json!({ "role": "header" });
        }
        let level = (row - first_item_row) as usize;
        let Some(line) = table.col_items.get(k) else {
            return serde_json::json!({ "role": "other" });
        };
        if line.kind == ItemType::Grand {
            return serde_json::json!({ "role": "grand" });
        }
        let members = full_members(&table.col_items, k);
        if level >= members.len() {
            return serde_json::json!({ "role": "other" });
        }
        if table.col_fields.get(level) == Some(&VALUES_FIELD) {
            return serde_json::json!({ "role": "value", "value": members[level] });
        }
        return serde_json::json!({
            "role": "item",
            "axis": "col",
            "field": table.col_fields[level],
            "item": members[level],
            "subtotal": line.kind != ItemType::Data,
            "hasChildren": level + 1 < real(&table.col_fields),
        });
    }
    if row >= data_top && col >= data_left {
        let data = if table.data_fields.len() <= 1 {
            0
        } else if table.row_fields.contains(&VALUES_FIELD) {
            table
                .row_items
                .get((row - data_top) as usize)
                .map_or(0, |line| line.data)
        } else {
            table
                .col_items
                .get((col - data_left) as usize)
                .map_or(0, |line| line.data)
        };
        return serde_json::json!({ "role": "data", "value": data });
    }
    serde_json::json!({ "role": "header" })
}

/// The filter buttons on a pivot table's captions, 1-based, with the fields each one filters.
fn buttons_json(table: &PivotTable) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let area = pivot_area(table);
    for (i, page) in table.page_fields.iter().enumerate() {
        out.push(serde_json::json!({ "row": area.top + i as u32, "col": area.left + 1, "fields": [page.field] }));
    }
    if !table.options.show_headers {
        return out;
    }
    let l = &table.location;
    let r = l.range;
    let rows: Vec<i32> = table
        .row_fields
        .iter()
        .copied()
        .filter(|&f| f != VALUES_FIELD)
        .collect();
    let cols: Vec<i32> = table
        .col_fields
        .iter()
        .copied()
        .filter(|&f| f != VALUES_FIELD)
        .collect();
    let caption_row = r.top + l.first_data_row.max(1) - 1;
    if !rows.is_empty() {
        if table.options.compact {
            out.push(serde_json::json!({ "row": caption_row, "col": r.left, "fields": rows }));
        } else {
            let columns = row_columns(table);
            for (level, &f) in table.row_fields.iter().enumerate() {
                if f == VALUES_FIELD || (level > 0 && columns[level] == columns[level - 1]) {
                    continue;
                }
                out.push(serde_json::json!({ "row": caption_row, "col": r.left + columns[level], "fields": [f] }));
            }
        }
    }
    if !cols.is_empty() {
        let row = r.top + l.first_data_row - table.col_fields.len() as u32 - 1;
        let compact = cols
            .iter()
            .all(|&f| table.fields[f as usize].compact && table.fields[f as usize].outline);
        if compact {
            out.push(
                serde_json::json!({ "row": row, "col": r.left + l.first_data_col, "fields": cols }),
            );
        } else {
            for (j, &f) in table.col_fields.iter().enumerate() {
                if f != VALUES_FIELD {
                    out.push(serde_json::json!({ "row": row, "col": r.left + l.first_data_col + j as u32, "fields": [f] }));
                }
            }
        }
    }
    out
}

impl crate::Engine {
    /// The words a pivot table uses that the file doesn't hold, in the UI's language.
    pub fn set_pivot_captions(&mut self, captions: Captions) {
        self.pivot_captions = captions.clone();
        if let Some(session) = self.session.as_mut() {
            session.pivot_captions = captions;
        }
    }

    /// Every pivot table on a sheet: its name and the cells it covers.
    pub fn pivot_list(&self, sheet: u32) -> String {
        let Some(session) = self.session.as_ref() else {
            return "[]".to_string();
        };
        let list: Vec<serde_json::Value> = session
            .book
            .pivot_tables
            .iter()
            .filter(|t| t.sheet == sheet)
            .map(|t| {
                let a = pivot_area(t);
                serde_json::json!({
                    "name": t.name,
                    "top": a.top,
                    "left": a.left,
                    "bottom": a.bottom,
                    "right": a.right,
                })
            })
            .collect();
        serde_json::Value::Array(list).to_string()
    }

    /// What the field list, the ribbon and the context menu need about the pivot table at a cell,
    /// or `None` if the cell isn't in one.
    pub fn pivot_info(&mut self, sheet: u32, row: u32, col: u32) -> Option<String> {
        let session = self.session.as_mut()?;
        let index = session.pivot_at(sheet, row, col)?;
        let table = session.book.pivot_tables[index].clone();
        let cache_index = table.cache as usize;
        let records = session.cache_records(cache_index).ok();
        let cache = session.book.pivot_caches[cache_index].clone();
        let blank = session.pivot_captions.blank.clone();
        let view = records
            .as_ref()
            .map(|r| CacheView::new(&cache, r, session.book.date1904));
        let fields: Vec<serde_json::Value> = table
            .fields
            .iter()
            .enumerate()
            .map(|(f, field)| {
                let values = view.as_ref().map_or(&[][..], |v| v.items(f));
                field_json(f, field, &cache, values, &blank)
            })
            .collect();
        let values: Vec<serde_json::Value> = table
            .data_fields
            .iter()
            .map(|d| {
                let code = d.num_fmt_id.map(|id| {
                    session
                        .book
                        .styles
                        .num_fmts
                        .get(&id)
                        .cloned()
                        .or_else(|| cgs_numfmt::builtin_code(id).map(str::to_string))
                        .unwrap_or_default()
                });
                let base_item = match d.base_item {
                    BaseItem::Previous => serde_json::json!("previous"),
                    BaseItem::Next => serde_json::json!("next"),
                    BaseItem::Item(i) => serde_json::json!(i),
                };
                serde_json::json!({
                    "field": d.field,
                    "name": d.name,
                    "summary": summary_name(d.summary),
                    "showAs": d.show_as.xml(),
                    "baseField": d.base_field,
                    "baseItem": base_item,
                    "numberFormat": code,
                })
            })
            .collect();
        let pages: Vec<serde_json::Value> = table
            .page_fields
            .iter()
            .map(|p| serde_json::json!({ "field": p.field, "item": p.item }))
            .collect();
        let form = if table.options.compact {
            "compact"
        } else if table.options.outline {
            "outline"
        } else {
            "tabular"
        };
        let first_row_field = table.row_fields.iter().find(|&&f| f != VALUES_FIELD);
        let subtotals = match first_row_field.map(|&f| &table.fields[f as usize]) {
            Some(f) if !f.default_subtotal && f.subtotals.is_empty() => "none",
            Some(f) if f.subtotal_top => "top",
            Some(_) => "bottom",
            None => "top",
        };
        let area = pivot_area(&table);
        let error_text = if table.options.show_error {
            table.options.error_caption.clone()
        } else {
            None
        };
        let info = serde_json::json!({
            "name": table.name,
            "sheet": table.sheet,
            "area": { "top": area.top, "left": area.left, "bottom": area.bottom, "right": area.right },
            "readOnly": table.unsupported.clone().or_else(|| cache.unsupported.clone()),
            "noRefresh": cache.no_refresh.clone(),
            "fields": fields,
            "rows": table.row_fields,
            "cols": table.col_fields,
            "pages": pages,
            "values": values,
            "options": {
                "form": form,
                "subtotals": subtotals,
                "rowGrand": table.options.row_grand_totals,
                "colGrand": table.options.col_grand_totals,
                "blankRows": table.fields.iter().any(|f| f.insert_blank_row),
                "repeatLabels": table.fields.iter().any(|f| f.repeat_labels),
                "emptyText": table.options.missing_caption,
                "errorText": error_text,
                "headers": table.options.show_headers,
                "style": {
                    "name": table.style.name,
                    "rowHeaders": table.style.row_headers,
                    "colHeaders": table.style.col_headers,
                    "rowStripes": table.style.row_stripes,
                    "colStripes": table.style.col_stripes,
                },
            },
            "cell": cell_json(&table, row, col),
            "buttons": buttons_json(&table),
        });
        Some(info.to_string())
    }
}

impl crate::Engine {
    /// A cell's value as the engine holds it now, for checks.
    pub fn cell_value(&self, sheet: u32, row: u32, col: u32) -> CalcValue {
        let Some(session) = self.session.as_ref() else {
            return CalcValue::Empty;
        };
        let mut block = Block(Vec::new());
        session.calc.read_block(
            SheetRect {
                sheet,
                rect: Rect {
                    top: row,
                    left: col,
                    bottom: row,
                    right: col,
                },
            },
            &mut block,
        );
        block.0.pop().map_or(CalcValue::Empty, |(_, v)| v)
    }
}

impl crate::Engine {
    /// What each cell of a sheet's pivot tables takes from its pivot style, as JSON, for checks:
    /// fill, font colour and weight, and each side's line, with colours as `RRGGBB`.
    pub fn pivot_look_json(&self, sheet: u32) -> String {
        let Some(session) = self.session.as_ref() else {
            return "[]".to_string();
        };
        let book = &session.book;
        let colors = &book.styles.colors;
        let rgb = |color: Option<cgs_model::Color>| -> serde_json::Value {
            color
                .and_then(|color| colors.resolve(color, None))
                .map_or(serde_json::Value::Null, |rgb| {
                    serde_json::json!(format!("{rgb:06X}"))
                })
        };
        let line = |line: Option<cgs_model::Line>| -> serde_json::Value {
            match line {
                Some(l) if l.style != cgs_model::LineStyle::None => {
                    serde_json::json!({ "style": format!("{:?}", l.style), "color": rgb(Some(l.color)) })
                }
                _ => serde_json::Value::Null,
            }
        };
        let mut out = Vec::new();
        for table in book.pivot_tables.iter().filter(|t| t.sheet == sheet) {
            let style = table
                .style
                .name
                .as_deref()
                .and_then(crate::pivot_style::find);
            let area = pivot_area(table);
            let mut cells = Vec::new();
            for row in area.top..=area.bottom {
                for col in area.left..=area.right {
                    let look = style.map(|style| {
                        crate::table_style::exact_look(
                            crate::pivot_style::cell_look(table, style, row, col),
                            &book.styles.colors.theme,
                        )
                    });
                    let look = look.unwrap_or_default();
                    cells.push(serde_json::json!({
                        "row": row,
                        "col": col,
                        "fill": rgb(look.fill.filter(|f| f.pattern != cgs_model::Pattern::None).map(|f| f.color)),
                        "color": rgb(look.font.color),
                        "bold": look.font.bold.unwrap_or(false),
                        "top": line(look.border.top),
                        "bottom": line(look.border.bottom),
                        "left": line(look.border.left),
                        "right": line(look.border.right),
                    }));
                }
            }
            out.push(serde_json::json!({
                "name": table.name,
                "style": table.style.name,
                "top": area.top,
                "left": area.left,
                "bottom": area.bottom,
                "right": area.right,
                "cells": cells,
            }));
        }
        serde_json::Value::Array(out).to_string()
    }
}
