//! Editing ERP data (Phase 16, `docs/ERP_SHEET_INTEGRATION_PLAN.md` B.3 items 2, 3, 5, 6 and 7):
//! text and lookup fields keep what is typed as text on every input path; each record's editable
//! fields are compared with what the ERP sent as rows are written, so the status badge, the tiles
//! and the changeset read a dirty-record set rather than diffing every cell; Discard Changes,
//! Next Error, hover details and lookup dropdowns read the same states. Nothing here leaves the
//! sheet: Phase 17 sends the changeset.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cgs_bind::guard::{Content, TableArea};
use cgs_bind::state::{wire, FieldState, FORMULA, INVALID, MODIFIED};
use cgs_bind::{Binding, Bindings, FieldKind, Rule};
use cgs_calc::{CalcEngine, CalcValue, CellRef, Rect as CalcRect, SheetRect};
use serde::Serialize;
use serde_json::{json, Value};

use crate::command::{Applied, Area, Op};
use crate::edit::{EditCell, EditInput};
use crate::erp::content_of_input;
use crate::input::Typed;
use crate::view_block::{
    CELL_ERP_FORMULA, CELL_ERP_INVALID, CELL_ERP_LOCK, CELL_ERP_MODIFIED, CELL_ERP_READ_ONLY,
};
use crate::Session;

/// At most this many items in a lookup's dropdown, as a list rule's dropdown is capped.
pub(crate) const LIST_CAP: usize = 1000;

/// A number as a cell in General shows it when nothing is too narrow, in en-US: the text a bound
/// text field takes for a number written into it.
fn general_text(n: f64) -> String {
    let locale = cgs_numfmt::Locale::en_us();
    let options = cgs_numfmt::Options {
        locale: &locale,
        date1904: false,
    };
    let mut out = String::new();
    cgs_numfmt::NumFmt::general().format(cgs_numfmt::Value::Number(n), &options, &mut out);
    out
}

fn is_text_kind(kind: &FieldKind) -> bool {
    matches!(kind, FieldKind::Text | FieldKind::Lookup(_))
}

/// Where each binding's table is now, for checking many cells at once.
struct Places {
    list: Vec<(usize, u32, TableArea)>,
}

impl Places {
    fn of(session: &Session) -> Self {
        let list = session
            .bindings
            .iter()
            .enumerate()
            .filter_map(|(index, binding)| {
                Some((index, binding.sheet, session.bound_area(binding)?))
            })
            .collect();
        Self { list }
    }

    /// The binding and field of a data cell.
    fn field(&self, sheet: u32, row: u32, col: u32) -> Option<(usize, usize)> {
        self.list.iter().find_map(|&(index, on, table)| {
            (on == sheet
                && row > table.header
                && row <= table.bottom
                && col >= table.left
                && col <= table.right)
                .then(|| (index, (col - table.left) as usize))
        })
    }
}

/// What the tiles of one sheet show of its bound tables: each cell's state flags, and which cells
/// are text fields, whose numbers-as-text triangles are hidden.
pub(crate) struct TileStates {
    top: u32,
    left: u32,
    cols: u32,
    flags: Vec<u16>,
    text_fields: Vec<bool>,
}

impl TileStates {
    pub(crate) fn flags(&self, row: u32, col: u32) -> u16 {
        self.flags[((row - self.top) * self.cols + col - self.left) as usize]
    }

    pub(crate) fn text_field(&self, row: u32, col: u32) -> bool {
        self.text_fields[((row - self.top) * self.cols + col - self.left) as usize]
    }
}

/// The states a tile shows, or `None` when no bound table meets it. One read of the key column
/// for the tile's rows, then a lookup per row (plan B.3, item 6).
pub(crate) fn tile_states(
    bindings: &Bindings,
    book: &cgs_model::Book,
    calc: &dyn CalcEngine,
    sheet: u32,
    (top, left, bottom, right): (u32, u32, u32, u32),
) -> Option<TileStates> {
    let mut found: Option<TileStates> = None;
    for binding in bindings.on_sheet(sheet) {
        let Some(table) = book.tables.iter().find(|t| t.id == binding.table_id) else {
            continue;
        };
        let area = TableArea {
            header: table.range.top,
            bottom: table.range.bottom,
            left: table.range.left,
            right: table.range.right,
        };
        if !area.meets(top, left, bottom, right) {
            continue;
        }
        let cols = right - left + 1;
        let states = found.get_or_insert_with(|| TileStates {
            top,
            left,
            cols,
            flags: vec![0; ((bottom - top + 1) * cols) as usize],
            text_fields: vec![false; ((bottom - top + 1) * cols) as usize],
        });
        let first_col = area.left.max(left);
        let last_col = area.right.min(right);
        let at = |row: u32, col: u32| ((row - top) * cols + col - left) as usize;
        // The heading of a read-only column carries a lock.
        if (top..=bottom).contains(&area.header) {
            for col in first_col..=last_col {
                if !binding.editable_in_sheet((col - area.left) as usize) {
                    states.flags[at(area.header, col)] |= CELL_ERP_LOCK;
                }
            }
        }
        let first_row = area.first_data_row().max(top);
        let last_row = area.bottom.min(bottom);
        if first_row > last_row {
            continue;
        }
        let key_col = area.left + binding.key_index as u32;
        let mut keys = KeyColumn::new(first_row, last_row - first_row + 1);
        calc.read_block(
            SheetRect {
                sheet,
                rect: CalcRect {
                    top: first_row,
                    left: key_col,
                    bottom: last_row,
                    right: key_col,
                },
            },
            &mut keys,
        );
        let spills: Vec<CalcRect> = calc
            .spill_areas()
            .into_iter()
            .filter(|spill| spill.sheet == sheet)
            .map(|spill| spill.rect)
            .collect();
        for row in first_row..=last_row {
            let fields = keys.get(row).and_then(|key| binding.states.get(key));
            for col in first_col..=last_col {
                let field = (col - area.left) as usize;
                let index = at(row, col);
                states.text_fields[index] = is_text_kind(&binding.fields[field].kind);
                let Some(slot) = binding.slot(field) else {
                    states.flags[index] |= CELL_ERP_READ_ONLY;
                    continue;
                };
                let spilled = spills
                    .iter()
                    .any(|spill| spill.contains(row, col) && (spill.top, spill.left) != (row, col));
                let bits = fields.map_or(0, |fields| fields[slot].bits)
                    | if spilled { FORMULA } else { 0 };
                states.flags[index] |= flags_of(bits);
            }
        }
    }
    found
}

fn flags_of(bits: u8) -> u16 {
    if bits & FORMULA != 0 {
        return CELL_ERP_FORMULA;
    }
    let mut flags = 0;
    if bits & MODIFIED != 0 {
        flags |= CELL_ERP_MODIFIED;
    }
    if bits & INVALID != 0 {
        flags |= CELL_ERP_INVALID;
    }
    flags
}

/// A column of keys, as `read_block` gives them.
struct KeyColumn {
    top: u32,
    keys: Vec<Option<String>>,
}

impl KeyColumn {
    fn new(top: u32, rows: u32) -> Self {
        Self {
            top,
            keys: vec![None; rows as usize],
        }
    }

    fn get(&self, row: u32) -> Option<&str> {
        self.keys
            .get((row - self.top) as usize)
            .and_then(Option::as_deref)
    }
}

impl cgs_calc::CellSink for KeyColumn {
    fn cell(&mut self, row: u32, _col: u32, value: &CalcValue) {
        if let CalcValue::Text(text) = value {
            if let Some(slot) = self.keys.get_mut((row - self.top) as usize) {
                *slot = Some(text.clone());
            }
        }
    }
}

/// The counts as the changeset writes them.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CountsIn {
    changed: usize,
    changed_records: usize,
    invalid: usize,
    formulas: usize,
}

/// A bound cell's details, for the hover layer.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CellDetails<'a> {
    field: &'a str,
    label: &'a str,
    heading: bool,
    editable: bool,
    /// Why a column is read-only: `key`, or the schema's `editReason`.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    /// `modified`, `invalid` or `formula`.
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    original: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    now: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rule: Option<Value>,
}

/// What a rule needs said about it: its code and the field's limits.
fn rule_json(binding: &Binding, field: usize, rule: Rule, now: &Content) -> Value {
    let bound = &binding.fields[field];
    let scale = match bound.kind {
        FieldKind::Decimal(scale) => Some(scale),
        _ => None,
    };
    let kind = match &bound.kind {
        FieldKind::Text => "text",
        FieldKind::Lookup(_) => "lookup",
        FieldKind::Integer => "integer",
        FieldKind::Decimal(_) => "decimal",
        FieldKind::Boolean => "boolean",
        FieldKind::Date => "date",
        FieldKind::DateTime => "dateTime",
    };
    let value = match now {
        Content::Text(text) => Some(text.clone()),
        _ => None,
    };
    let inactive = matches!(rule, Rule::Lookup)
        && value.as_deref().is_some_and(|text| {
            binding
                .lookup_list(field)
                .unwrap_or_default()
                .iter()
                .any(|item| item.code == text && !item.active)
        });
    json!({
        "code": rule.code(),
        "type": kind,
        "maxLength": bound.max_length,
        "min": bound.min,
        "max": bound.max,
        "scale": scale,
        "value": value,
        "inactive": inactive,
    })
}

impl Session {
    /// Every bound table's data cell `(sheet, row, col)` is in: its binding and field.
    fn bound_field_at(&self, sheet: u32, row: u32, col: u32) -> Option<(&Binding, usize)> {
        if self.bindings.is_empty() {
            return None;
        }
        let (index, field) = Places::of(self).field(sheet, row, col)?;
        Some((&self.bindings.0[index], field))
    }

    /// What typing `raw` into a bound text or lookup cell writes: a formula stays a formula, and
    /// anything else is the text as typed, without a leading apostrophe, with a lookup's code
    /// spelled as its list spells it. So `0100`, `001`, `1-2` and `1,23` stay as typed (plan B.3,
    /// item 3). `None` for any other cell, which takes typing as Excel does.
    pub(crate) fn bound_text_typed(
        &self,
        sheet: u32,
        row: u32,
        col: u32,
        raw: &str,
        typed: &Typed,
    ) -> Option<Typed> {
        let (binding, field) = self.bound_field_at(sheet, row, col)?;
        if !is_text_kind(&binding.fields[field].kind) {
            return None;
        }
        Some(match typed {
            Typed::Formula(_) | Typed::Clear => typed.clone(),
            _ => {
                let text = raw.strip_prefix('\'').unwrap_or(raw);
                let text = binding.lookup_spelling(field, text).unwrap_or(text);
                if text.is_empty() {
                    Typed::Clear
                } else {
                    Typed::Value(CalcValue::Text(text.to_string()))
                }
            }
        })
    }

    /// Whether a data cell is a bound text or lookup field's, whose external paste is its text.
    pub(crate) fn bound_text_cell(&self, sheet: u32, row: u32, col: u32) -> bool {
        self.bound_field_at(sheet, row, col)
            .is_some_and(|(binding, field)| is_text_kind(&binding.fields[field].kind))
    }

    /// Values written into bound text and lookup fields become text, whatever path wrote them:
    /// paste, fill, Convert to Number, a dropdown pick. A number takes its General form, TRUE and
    /// FALSE their words, and a lookup's code its list's spelling. Typing, external paste and
    /// replace have already kept what was typed.
    pub(crate) fn coerce_bound_values(&self, edits: &mut [EditCell]) {
        if self.unguarded || self.bindings.is_empty() {
            return;
        }
        let places = Places::of(self);
        for edit in edits.iter_mut() {
            let Some(EditInput::Value(value)) = &edit.input else {
                continue;
            };
            let Some((index, field)) = places.field(edit.sheet, edit.row, edit.col) else {
                continue;
            };
            let binding = &self.bindings.0[index];
            if !is_text_kind(&binding.fields[field].kind) {
                continue;
            }
            let text = match value {
                CalcValue::Number(n) => general_text(*n),
                CalcValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
                CalcValue::Text(text) => match binding.lookup_spelling(field, text) {
                    Some(code) => code.to_string(),
                    None => continue,
                },
                _ => continue,
            };
            edit.input = Some(EditInput::Value(CalcValue::Text(text)));
        }
    }

    /// A row's cells now, in `editable`'s order, and its key.
    fn row_now(
        &self,
        binding: &Binding,
        table: &TableArea,
        row: u32,
    ) -> Option<(String, Vec<Content>)> {
        let read = |col: u32| {
            content_of_input(&self.calc.cell_input(CellRef {
                sheet: binding.sheet,
                row,
                col,
            }))
        };
        let Content::Text(key) = read(table.left + binding.key_index as u32) else {
            return None;
        };
        let now = binding
            .editable
            .iter()
            .map(|&field| read(table.left + field as u32))
            .collect();
        Some((key, now))
    }

    /// Compares the written rows of bound tables with their records again. Called for every
    /// write, undo and redo included, so the states never fall behind the cells.
    pub(crate) fn note_bound_writes(&mut self, edits: &[EditCell]) {
        if self.bindings.is_empty() {
            return;
        }
        let places = Places::of(self);
        let mut rows: BTreeMap<usize, BTreeSet<u32>> = BTreeMap::new();
        for edit in edits.iter().filter(|edit| edit.input.is_some()) {
            if let Some((index, _)) = places.field(edit.sheet, edit.row, edit.col) {
                rows.entry(index).or_default().insert(edit.row);
            }
        }
        for (index, rows) in rows {
            self.restate_rows(index, rows);
        }
    }

    fn restate_rows(&mut self, index: usize, rows: impl IntoIterator<Item = u32>) {
        let binding = &self.bindings.0[index];
        let Some(table) = self.bound_area(binding) else {
            return;
        };
        let updates: Vec<(String, Vec<Content>)> = rows
            .into_iter()
            .filter_map(|row| self.row_now(binding, &table, row))
            .collect();
        let binding = &mut self.bindings.0[index];
        for (key, now) in updates {
            binding.note_record(&key, &now);
        }
    }

    /// Compares every row of every bound table again, after the originals changed.
    pub(crate) fn restate_all(&mut self) {
        for index in 0..self.bindings.0.len() {
            let binding = &self.bindings.0[index];
            let Some(table) = self.bound_area(binding) else {
                continue;
            };
            let sheet = binding.sheet;
            self.bindings.0[index].states.clear();
            self.restate_rows(index, table.first_data_row()..=table.bottom);
            self.dirty.sheets.insert(sheet);
        }
    }

    /// Each data row of the first binding's table by key.
    fn rows_by_key(&self, binding: &Binding, table: &TableArea) -> HashMap<String, u32> {
        let mut keys = KeyColumn::new(table.first_data_row(), table.bottom - table.header);
        let key_col = table.left + binding.key_index as u32;
        self.calc.read_block(
            SheetRect {
                sheet: binding.sheet,
                rect: CalcRect {
                    top: table.first_data_row(),
                    left: key_col,
                    bottom: table.bottom,
                    right: key_col,
                },
            },
            &mut keys,
        );
        keys.keys
            .into_iter()
            .enumerate()
            .filter_map(|(offset, key)| Some((key?, table.first_data_row() + offset as u32)))
            .collect()
    }

    /// Spilled cells in a bound table's editable columns, which count as formulas.
    fn spilled_cells(&self, binding: &Binding, table: &TableArea) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for spill in self.calc.spill_areas() {
            if spill.sheet != binding.sheet {
                continue;
            }
            let rect = spill.rect;
            for row in rect.top.max(table.first_data_row())..=rect.bottom.min(table.bottom) {
                for col in rect.left.max(table.left)..=rect.right.min(table.right) {
                    if (row, col) != (rect.top, rect.left)
                        && binding.slot((col - table.left) as usize).is_some()
                    {
                        out.push((row, col));
                    }
                }
            }
        }
        out
    }

    /// The changeset (plan B.3, item 5; contract C.4): each changed record's id, key, version and
    /// changed fields from and to, in id order, with the row it is on now. Formulas and spilled
    /// values are listed apart and left out, and the changes local rules refuse are listed too.
    pub(crate) fn changeset_json(&self) -> Value {
        let Some(binding) = self.bindings.0.first() else {
            return Value::Null;
        };
        let Some(table) = self.bound_area(binding) else {
            return Value::Null;
        };
        let rows = self.rows_by_key(binding, &table);
        // Spills into empty editable cells come from formulas elsewhere, so no write noted them.
        let spilled: HashSet<(u32, u32)> =
            self.spilled_cells(binding, &table).into_iter().collect();
        let mut records = Vec::new();
        let mut formulas = Vec::new();
        let mut errors = Vec::new();
        let mut changed = 0;
        let mut keys: Vec<&str> = binding.states.keys().collect();
        keys.sort_unstable();
        for key in keys {
            let (Some(meta), Some(&row), Some(fields)) =
                (binding.record(key), rows.get(key), binding.states.get(key))
            else {
                continue;
            };
            let mut changes = serde_json::Map::new();
            for (slot, state) in fields.iter().enumerate() {
                let field = binding.editable[slot];
                let col = table.left + field as u32;
                let bound = &binding.fields[field];
                if state.bits & FORMULA != 0 || spilled.contains(&(row, col)) {
                    formulas
                        .push(json!({ "key": key, "field": bound.name, "row": row, "col": col }));
                    continue;
                }
                if state.bits & MODIFIED == 0 {
                    continue;
                }
                let now = content_of_input(&self.calc.cell_input(CellRef {
                    sheet: binding.sheet,
                    row,
                    col,
                }));
                if let Some(rule) = state.rule {
                    errors.push(json!({
                        "key": key, "field": bound.name, "code": rule.code(), "row": row, "col": col,
                    }));
                }
                changes.insert(
                    bound.name.clone(),
                    json!({
                        "from": wire(&bound.kind, &Content::of_cell(&meta.originals[slot])),
                        "to": wire(&bound.kind, &now),
                    }),
                );
            }
            if !changes.is_empty() {
                changed += changes.len();
                records.push((
                    meta.id,
                    json!({
                        "id": meta.id,
                        "key": key,
                        "version": binding.version_text(key),
                        "row": row,
                        "changes": changes,
                    }),
                ));
            }
        }
        records.sort_by_key(|(id, _)| *id);
        let by_row: HashMap<u32, &str> =
            rows.iter().map(|(key, &row)| (row, key.as_str())).collect();
        for &(row, col) in &spilled {
            let Some(&key) = by_row.get(&row) else {
                continue;
            };
            if binding.states.get(key).is_none() {
                let field = &binding.fields[(col - table.left) as usize];
                formulas.push(json!({ "key": key, "field": field.name, "row": row, "col": col }));
            }
        }
        let by_place = |a: &Value, b: &Value| {
            (a["row"].as_u64(), a["col"].as_u64()).cmp(&(b["row"].as_u64(), b["col"].as_u64()))
        };
        formulas.sort_by(by_place);
        errors.sort_by(by_place);
        let counts = cgs_bind::Counts {
            changed,
            changed_records: records.len(),
            invalid: errors.len(),
            formulas: formulas.len(),
        };
        json!({
            "company": binding.session.company,
            "schemaVersion": binding.schema_version,
            "records": records.into_iter().map(|(_, record)| record).collect::<Vec<_>>(),
            "formulas": formulas,
            "errors": errors,
            "counts": counts,
        })
    }

    /// What the status badge counts. The dirty-record set's own counts, unless a formula spills
    /// into the table, when the changeset counts them exactly.
    pub(crate) fn erp_counts(&self, binding: &Binding) -> cgs_bind::Counts {
        let spills_in = self
            .bound_area(binding)
            .is_some_and(|table| !self.spilled_cells(binding, &table).is_empty());
        if !spills_in {
            return binding.states.counts();
        }
        serde_json::from_value::<CountsIn>(self.changeset_json()["counts"].clone())
            .map(|c| cgs_bind::Counts {
                changed: c.changed,
                changed_records: c.changed_records,
                invalid: c.invalid,
                formulas: c.formulas,
            })
            .unwrap_or_else(|_| binding.states.counts())
    }

    /// What a cell shows of a bound value, in its column's format.
    fn erp_shown(
        &self,
        binding: &Binding,
        field: usize,
        value: &Content,
        locale: &cgs_numfmt::Locale,
    ) -> Option<String> {
        let options = cgs_numfmt::Options {
            locale,
            date1904: self.book.date1904,
        };
        let format = binding.fields[field]
            .kind
            .number_format()
            .map_or_else(cgs_numfmt::NumFmt::general, |code| {
                cgs_numfmt::NumFmt::parse(&code)
            });
        let mut out = String::new();
        match value {
            Content::Empty => return None,
            Content::Text(text) if text.is_empty() => return None,
            Content::Text(text) => return Some(text.clone()),
            Content::Number(bits) => {
                format.format(
                    cgs_numfmt::Value::Number(f64::from_bits(*bits)),
                    &options,
                    &mut out,
                );
            }
            Content::Bool(b) => return Some(if *b { "TRUE" } else { "FALSE" }.to_string()),
            Content::Error(error) => return Some(error.clone()),
            Content::Formula(text) => return Some(format!("={}", text.trim_start_matches('='))),
            Content::Spill => return None,
        }
        Some(out)
    }

    /// A bound cell's field, state, original and new values and broken rule, for hover details.
    pub(crate) fn erp_cell_json(
        &self,
        sheet: u32,
        row: u32,
        col: u32,
        locale: &cgs_numfmt::Locale,
    ) -> Value {
        let Some((binding, table)) = self.bindings.on_sheet(sheet).find_map(|binding| {
            let table = self.bound_area(binding)?;
            table.contains(row, col).then_some((binding, table))
        }) else {
            return Value::Null;
        };
        let field = (col - table.left) as usize;
        let bound = &binding.fields[field];
        let editable = binding.editable_in_sheet(field);
        let reason = if editable {
            None
        } else if field == binding.key_index {
            Some("key")
        } else {
            Some(bound.edit_reason.as_deref().unwrap_or("notEditable"))
        };
        let mut details = CellDetails {
            field: &bound.name,
            label: &bound.label,
            heading: row == table.header,
            editable,
            reason,
            key: None,
            state: None,
            original: None,
            now: None,
            rule: None,
        };
        if row == table.header {
            return serde_json::to_value(details).unwrap_or(Value::Null);
        }
        let Some((key, now)) = self.row_now(binding, &table, row) else {
            return serde_json::to_value(details).unwrap_or(Value::Null);
        };
        if let (Some(slot), Some(meta)) = (binding.slot(field), binding.record(&key)) {
            let state = binding
                .states
                .get(&key)
                .map_or(FieldState::default(), |fields| fields[slot]);
            let spilled = self.spilled_cells(binding, &table).contains(&(row, col));
            let original = Content::of_cell(&meta.originals[slot]);
            let current = &now[slot];
            details.state = if state.bits & FORMULA != 0 || spilled {
                Some("formula")
            } else if state.bits & INVALID != 0 {
                Some("invalid")
            } else if state.bits & MODIFIED != 0 {
                Some("modified")
            } else {
                None
            };
            if details.state.is_some() {
                details.original = self.erp_shown(binding, field, &original, locale);
                details.now = self.erp_shown(binding, field, current, locale);
            }
            details.rule = state
                .rule
                .map(|rule| rule_json(binding, field, rule, current));
        }
        details.key = Some(key);
        serde_json::to_value(details).unwrap_or(Value::Null)
    }

    /// The next cell a local rule refuses after `(row, col)` in reading order, round the table,
    /// or before it with `back`.
    pub(crate) fn erp_next_error_json(&self, sheet: u32, row: u32, col: u32, back: bool) -> Value {
        let Some(binding) = self.bindings.on_sheet(sheet).next() else {
            return Value::Null;
        };
        let Some(table) = self.bound_area(binding) else {
            return Value::Null;
        };
        if binding.states.counts().invalid == 0 {
            return Value::Null;
        }
        let rows = self.rows_by_key(binding, &table);
        let mut places: Vec<(u32, u32)> = Vec::new();
        for key in binding.states.keys() {
            let (Some(fields), Some(&at)) = (binding.states.get(key), rows.get(key)) else {
                continue;
            };
            for (slot, state) in fields.iter().enumerate() {
                if state.bits & INVALID != 0 {
                    places.push((at, table.left + binding.editable[slot] as u32));
                }
            }
        }
        places.sort_unstable();
        let here = (row, col);
        let next = if back {
            places
                .iter()
                .rev()
                .find(|&&place| place < here)
                .or_else(|| places.last())
        } else {
            places
                .iter()
                .find(|&&place| place > here)
                .or_else(|| places.first())
        };
        next.map_or(Value::Null, |&(row, col)| json!({ "row": row, "col": col }))
    }

    /// A lookup field's dropdown, when no validation rule gives the cell one: its active codes,
    /// with their labels, and no rule written to the file (plan B.3, item 7).
    pub(crate) fn erp_list_json(&self, sheet: u32, row: u32, col: u32) -> Option<Value> {
        let (binding, field) = self.bound_field_at(sheet, row, col)?;
        if !binding.editable_in_sheet(field) {
            return None;
        }
        let list = binding.lookup_list(field)?;
        let active: Vec<_> = list
            .iter()
            .filter(|item| item.active)
            .take(LIST_CAP)
            .collect();
        let items: Vec<&str> = active.iter().map(|item| item.code.as_str()).collect();
        let labels: Vec<&str> = active.iter().map(|item| item.label.as_str()).collect();
        Some(json!({
            "kind": "list",
            "op": "between",
            "formula1": null,
            "formula2": null,
            "allowBlank": true,
            "dropdown": true,
            "items": items,
            "labels": labels,
            "lookup": true,
            "showInput": false,
            "inputTitle": "",
            "inputText": "",
            "showError": false,
            "errorTitle": "",
            "errorText": "",
            "style": "stop",
            "extended": false,
            "areas": "",
        }))
    }

    /// Discard Changes: every editable cell that differs from what the ERP sent, in `areas` or
    /// the whole table, takes its original back, as one step undo takes back. Only cells change;
    /// the originals never do, so undoing it is safe (plan E.4).
    pub(crate) fn erp_discard(&mut self, areas: Option<&[Area]>) -> Result<Applied, String> {
        let binding = self
            .bindings
            .0
            .first()
            .ok_or_else(|| "This workbook has no ERP data.".to_string())?;
        let table = self
            .bound_area(binding)
            .ok_or_else(|| "The ERP data's table isn't there any more.".to_string())?;
        let rows = self.rows_by_key(binding, &table);
        let within = |row: u32, col: u32| {
            areas.is_none_or(|areas| {
                areas.iter().any(|area| {
                    (area.top..=area.bottom).contains(&row)
                        && (area.left..=area.right).contains(&col)
                })
            })
        };
        let spilled = self.spilled_cells(binding, &table);
        let mut edits = Vec::new();
        for key in binding.states.keys() {
            let (Some(fields), Some(&row), Some(meta)) =
                (binding.states.get(key), rows.get(key), binding.record(key))
            else {
                continue;
            };
            for (slot, state) in fields.iter().enumerate() {
                let col = table.left + binding.editable[slot] as u32;
                // A spilled value belongs to its formula elsewhere, which Discard leaves alone.
                if state.is_clean() || !within(row, col) || spilled.contains(&(row, col)) {
                    continue;
                }
                edits.push(EditCell {
                    sheet: binding.sheet,
                    row,
                    col,
                    input: Some(crate::erp::edit_of_cell(&meta.originals[slot])),
                    style: None,
                });
            }
        }
        if edits.is_empty() {
            return Ok(Applied::default());
        }
        edits.sort_by_key(|edit| (edit.row, edit.col));
        let sheet = binding.sheet;
        let first = (edits[0].row, edits[0].col);
        self.perform(
            "discard changes",
            Op::Cells(edits),
            Some((sheet, Area::cell(first.0, first.1))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_become_their_general_text() {
        assert_eq!(general_text(1001234567.0), "1001234567");
        assert_eq!(general_text(1.5), "1.5");
        assert_eq!(general_text(0.1 + 0.2), "0.3");
        assert_eq!(general_text(-12.0), "-12");
    }
}
