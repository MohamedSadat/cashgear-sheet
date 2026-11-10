//! Editing ERP data (Phase 16, `docs/ERP_SHEET_INTEGRATION_PLAN.md` B.3 items 2, 3, 5, 6 and 7):
//! text and lookup fields keep what is typed as text on every input path; each record's editable
//! fields are compared with what the ERP sent as rows are written, so the status badge, the tiles
//! and the changeset read a dirty-record set rather than diffing every cell; Discard Changes,
//! Next Error, hover details and lookup dropdowns read the same states. Phase 17 sends the
//! changeset, lays what Cash Gear ERP answered over the states (errors, conflicts, committed
//! fields), lists everything for the Review dialog, and applies a commit's result.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cgs_bind::guard::{Content, TableArea};
use cgs_bind::marks::wire_of_json;
use cgs_bind::state::{
    same, wire_of, FieldState, Wire, COMMITTED, CONFLICT, FORMULA, INVALID, MODIFIED, SERVER,
};
use cgs_bind::{ApiConflict, ApiError, Binding, Bindings, FieldKind, Rule, SentRecord, WireRecord};
use cgs_calc::{CalcEngine, CalcValue, CellRef, Rect as CalcRect, SheetRect};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::command::{Applied, Area, Op};
use crate::edit::{EditCell, EditInput};
use crate::erp::content_of_input;
use crate::input::Typed;
use crate::view_block::{
    CELL_ERP_COMMITTED, CELL_ERP_CONFLICT, CELL_ERP_FORMULA, CELL_ERP_INVALID, CELL_ERP_LOCK,
    CELL_ERP_MODIFIED, CELL_ERP_READ_ONLY,
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
    if bits & CONFLICT != 0 {
        flags |= CELL_ERP_CONFLICT;
    }
    if bits & COMMITTED != 0 {
        flags |= CELL_ERP_COMMITTED;
    }
    flags
}

/// A field's state as hover details and the Review dialog name it, the first that applies.
fn state_name(bits: u8, spilled: bool) -> Option<&'static str> {
    Some(if bits & FORMULA != 0 || spilled {
        "formula"
    } else if bits & CONFLICT != 0 {
        "conflict"
    } else if bits & INVALID != 0 {
        "invalid"
    } else if bits & MODIFIED != 0 {
        "modified"
    } else if bits & COMMITTED != 0 {
        "committed"
    } else {
        return None;
    })
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

/// One changed field, from and to, as the contract sends it.
#[derive(Serialize)]
struct ChangeOut {
    from: Wire,
    to: Wire,
}

/// A record's changed fields, as a JSON object by field name.
struct FieldChanges<'a>(Vec<(&'a str, ChangeOut)>);

impl Serialize for FieldChanges<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (field, change) in &self.0 {
            map.serialize_entry(field, change)?;
        }
        map.end()
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Serialize)]
struct RecordOut<'a> {
    id: i64,
    key: &'a str,
    version: String,
    row: u32,
    /// In a row a filter or the user hid.
    #[serde(skip_serializing_if = "is_false")]
    hidden: bool,
    changes: FieldChanges<'a>,
}

#[derive(Serialize)]
struct PlaceOut<'a> {
    key: String,
    field: &'a str,
    row: u32,
    col: u32,
    #[serde(skip_serializing_if = "is_false")]
    hidden: bool,
}

#[derive(Serialize)]
struct ErrorOut<'a> {
    key: &'a str,
    field: &'a str,
    code: &'static str,
    row: u32,
    col: u32,
}

/// The changeset as `erpChanges` answers it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangesetOut<'a> {
    company: &'a str,
    schema_version: &'a str,
    records: Vec<RecordOut<'a>>,
    formulas: Vec<PlaceOut<'a>>,
    errors: Vec<ErrorOut<'a>>,
    counts: cgs_bind::Counts,
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
    /// `formula`, `conflict`, `invalid`, `modified` or `committed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    original: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    now: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rule: Option<Value>,
    /// The error Cash Gear ERP refused the value with: `{code, message}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<Value>,
    /// Who changed the record in Cash Gear ERP, when, and the field's value there now.
    #[serde(skip_serializing_if = "Option::is_none")]
    conflict: Option<Value>,
}

/// One field for the Review dialog.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewEntry<'a> {
    key: String,
    /// The record's name, its first text field after the key, as the cell holds it now.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    row: u32,
    col: u32,
    field: &'a str,
    label: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    original: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    now: Option<String>,
    /// `formula`, `conflict`, `invalid` or `modified`, the first that applies.
    state: &'static str,
    invalid: bool,
    conflict: bool,
    hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    rule: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "conflictWith")]
    conflict_with: Option<Value>,
}

/// What `erpSetStates` is given.
#[derive(Deserialize)]
struct StatesIn {
    sent: Vec<SentRecord>,
    #[serde(default)]
    errors: Vec<ApiError>,
    #[serde(default)]
    conflicts: Vec<ApiConflict>,
}

/// What `erpApplyCommit` is given: the records as sent, and as the ERP persisted them.
#[derive(Deserialize)]
struct CommitIn {
    sent: Vec<SentRecord>,
    records: Vec<WireRecord>,
}

/// What applying a commit did.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommitApplied {
    records: usize,
    /// Fields that took the persisted value and show Committed.
    saved: usize,
    /// Sent fields whose cells changed since, which keep what they hold.
    kept: usize,
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

    /// The data rows of a binding's table whose keys `wanted` takes, by key: one read of the key
    /// column, keeping only those keys.
    fn rows_of(
        &self,
        binding: &Binding,
        table: &TableArea,
        wanted: &dyn Fn(&str) -> bool,
    ) -> HashMap<String, u32> {
        struct Wanted<'a> {
            wanted: &'a dyn Fn(&str) -> bool,
            rows: HashMap<String, u32>,
        }
        impl cgs_calc::CellSink for Wanted<'_> {
            fn cell(&mut self, row: u32, _col: u32, value: &CalcValue) {
                if let CalcValue::Text(key) = value {
                    if (self.wanted)(key) {
                        self.rows.insert(key.clone(), row);
                    }
                }
            }
        }
        let key_col = table.left + binding.key_index as u32;
        let mut sink = Wanted {
            wanted,
            rows: HashMap::new(),
        };
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
            &mut sink,
        );
        sink.rows
    }

    /// The key a data row holds now.
    fn key_at(&self, binding: &Binding, table: &TableArea, row: u32) -> Option<String> {
        match content_of_input(&self.calc.cell_input(CellRef {
            sheet: binding.sheet,
            row,
            col: table.left + binding.key_index as u32,
        })) {
            Content::Text(key) => Some(key),
            _ => None,
        }
    }

    /// Builds the changeset (plan B.3, item 5; contract C.4) and hands it to `f`: each changed
    /// record's id, key, version and changed fields from and to, in id order, with the row it is on
    /// now. Formulas and spilled values are listed apart and left out, and the changes local rules
    /// refuse are listed too. It reads only the changed records' cells, so it takes time in
    /// proportion to what changed, past one read of the key column.
    fn with_changeset<R>(
        &self,
        visible: bool,
        f: impl FnOnce(&ChangesetOut<'_>) -> R,
    ) -> Option<R> {
        let binding = self.bindings.0.first()?;
        let table = self.bound_area(binding)?;
        let hidden = |row: u32| self.row_hidden(binding.sheet, row);
        let rows = self.rows_of(binding, &table, &|key| binding.states.get(key).is_some());
        // Spills into empty editable cells come from formulas elsewhere, so no write noted them.
        let spilled: HashSet<(u32, u32)> =
            self.spilled_cells(binding, &table).into_iter().collect();
        let mut records = Vec::with_capacity(rows.len());
        let mut formulas = Vec::new();
        let mut errors = Vec::new();
        let mut changed = 0;
        let mut conflicts = 0;
        for (key, &row) in &rows {
            let (Some(meta), Some(fields)) = (binding.record(key), binding.states.get(key)) else {
                continue;
            };
            let hidden = hidden(row);
            if visible && hidden {
                continue;
            }
            let mut changes = Vec::new();
            for (slot, state) in fields.iter().enumerate() {
                let field = binding.editable[slot];
                let col = table.left + field as u32;
                let bound = &binding.fields[field];
                if state.bits & FORMULA != 0 || spilled.contains(&(row, col)) {
                    formulas.push(PlaceOut {
                        key: key.clone(),
                        field: &bound.name,
                        row,
                        col,
                        hidden,
                    });
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
                    errors.push(ErrorOut {
                        key,
                        field: &bound.name,
                        code: rule.code(),
                        row,
                        col,
                    });
                }
                changes.push((
                    bound.name.as_str(),
                    ChangeOut {
                        from: wire_of(&bound.kind, &Content::of_cell(&meta.originals[slot])),
                        to: wire_of(&bound.kind, &now),
                    },
                ));
            }
            if !changes.is_empty() {
                changed += changes.len();
                conflicts += usize::from(fields.iter().any(|field| field.bits & CONFLICT != 0));
                records.push(RecordOut {
                    id: meta.id,
                    key,
                    version: binding.version_text(key).unwrap_or_default(),
                    row,
                    hidden,
                    changes: FieldChanges(changes),
                });
            }
        }
        records.sort_unstable_by_key(|record| record.id);
        for &(row, col) in &spilled {
            if formulas.iter().any(|f| (f.row, f.col) == (row, col)) {
                continue;
            }
            let hidden = hidden(row);
            if visible && hidden {
                continue;
            }
            let Some(key) = self.key_at(binding, &table, row) else {
                continue;
            };
            formulas.push(PlaceOut {
                key,
                field: &binding.fields[(col - table.left) as usize].name,
                row,
                col,
                hidden,
            });
        }
        formulas.sort_unstable_by_key(|f| (f.row, f.col));
        errors.sort_unstable_by_key(|e| (e.row, e.col));
        let counts = cgs_bind::Counts {
            changed,
            changed_records: records.len(),
            invalid: errors.len(),
            formulas: formulas.len(),
            conflicts,
        };
        Some(f(&ChangesetOut {
            company: &binding.session.company,
            schema_version: &binding.schema_version,
            records,
            formulas,
            errors,
            counts,
        }))
    }

    /// The changeset as JSON text, or `null`. With `visible`, only rows no filter hides, for
    /// Commit Changes in Visible Rows.
    pub(crate) fn changeset_text(&self, visible: bool) -> String {
        self.with_changeset(visible, |set| {
            serde_json::to_string(set).unwrap_or_default()
        })
        .unwrap_or_else(|| "null".to_string())
    }

    /// Whether a row is hidden, by a filter or by hand.
    fn row_hidden(&self, sheet: u32, row: u32) -> bool {
        self.book
            .sheets
            .get(sheet as usize)
            .and_then(|info| info.rows.info(row))
            .is_some_and(|line| line.hidden)
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
        self.with_changeset(false, |set| set.counts)
            .unwrap_or_else(|| binding.states.counts())
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
            server: None,
            conflict: None,
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
            details.state = state_name(state.bits, spilled);
            if details.state.is_some_and(|name| name != "committed") {
                details.original = self.erp_shown(binding, field, &original, locale);
                details.now = self.erp_shown(binding, field, current, locale);
            }
            details.rule = state
                .rule
                .map(|rule| rule_json(binding, field, rule, current));
            details.server = self.server_json(binding, &key, slot, state.bits);
            details.conflict = self.conflict_json(binding, &key, field, state.bits, locale);
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

    /// The error Cash Gear ERP set on a field, while it shows.
    fn server_json(&self, binding: &Binding, key: &str, slot: usize, bits: u8) -> Option<Value> {
        if bits & SERVER == 0 {
            return None;
        }
        let (_, error) = binding
            .marks
            .errors
            .get(key)?
            .iter()
            .find(|(at, _)| *at == slot)?;
        Some(json!({ "code": error.code, "message": error.message }))
    }

    /// Who changed a conflicting record in Cash Gear ERP, when, and the field's value there now.
    fn conflict_json(
        &self,
        binding: &Binding,
        key: &str,
        field: usize,
        bits: u8,
        locale: &cgs_numfmt::Locale,
    ) -> Option<Value> {
        if bits & CONFLICT == 0 {
            return None;
        }
        let mark = binding.marks.conflicts.get(key)?;
        let now =
            mark.values.get(field).cloned().flatten().and_then(|value| {
                self.erp_shown(binding, field, &Content::of_cell(&value), locale)
            });
        Some(json!({
            "by": mark.modified_by,
            "at": mark.modified_at,
            "now": now,
            "known": mark.values.get(field).is_some_and(Option::is_some),
        }))
    }

    /// Every field the Review dialog lists: changed, refused, in conflict or a formula, in
    /// reading order, as JSON text.
    pub(crate) fn erp_review_text(&self, locale: &cgs_numfmt::Locale) -> String {
        let Some(binding) = self.bindings.0.first() else {
            return "null".to_string();
        };
        let Some(table) = self.bound_area(binding) else {
            return "null".to_string();
        };
        let rows = self.rows_of(binding, &table, &|key| binding.states.get(key).is_some());
        let spilled: HashSet<(u32, u32)> =
            self.spilled_cells(binding, &table).into_iter().collect();
        let name_field = binding
            .fields
            .iter()
            .enumerate()
            .find(|(index, field)| *index > binding.key_index && field.kind == FieldKind::Text)
            .map(|(index, _)| index);
        let read = |row: u32, col: u32| {
            content_of_input(&self.calc.cell_input(CellRef {
                sheet: binding.sheet,
                row,
                col,
            }))
        };
        let name_at = |row: u32| {
            let field = name_field?;
            match read(row, table.left + field as u32) {
                Content::Text(text) if !text.is_empty() => Some(text),
                _ => None,
            }
        };
        let mut entries = Vec::new();
        let mut entry = |key: &str, row: u32, field: usize, state: FieldState, spill: bool| {
            let col = table.left + field as u32;
            let Some(name) = state_name(state.bits, spill) else {
                return;
            };
            if name == "committed" {
                return;
            }
            let bound = &binding.fields[field];
            let now = read(row, col);
            let original = binding
                .slot(field)
                .zip(binding.record(key))
                .map(|(slot, meta)| Content::of_cell(&meta.originals[slot]));
            let slot = binding.slot(field).unwrap_or_default();
            entries.push(ReviewEntry {
                key: key.to_string(),
                name: name_at(row),
                row,
                col,
                field: &bound.name,
                label: &bound.label,
                original: original.and_then(|o| self.erp_shown(binding, field, &o, locale)),
                now: self.erp_shown(binding, field, &now, locale),
                state: name,
                invalid: state.bits & INVALID != 0,
                conflict: state.bits & CONFLICT != 0,
                hidden: self.row_hidden(binding.sheet, row),
                rule: state.rule.map(|rule| rule_json(binding, field, rule, &now)),
                server: self.server_json(binding, key, slot, state.bits),
                conflict_with: self.conflict_json(binding, key, field, state.bits, locale),
            });
        };
        for (key, &row) in &rows {
            let Some(fields) = binding.states.get(key) else {
                continue;
            };
            for (slot, &state) in fields.iter().enumerate() {
                let field = binding.editable[slot];
                let spill = spilled.contains(&(row, table.left + field as u32));
                entry(key, row, field, state, spill);
            }
        }
        for &(row, col) in &spilled {
            let Some(key) = self.key_at(binding, &table, row) else {
                continue;
            };
            if binding.states.get(&key).is_some() {
                continue;
            }
            let field = (col - table.left) as usize;
            entry(&key, row, field, FieldState::default(), true);
        }
        entries.sort_unstable_by_key(|entry| (entry.row, entry.col));
        serde_json::to_string(&json!({ "entries": entries, "counts": self.erp_counts(binding) }))
            .unwrap_or_else(|_| "null".to_string())
    }

    /// Takes in what Cash Gear ERP answered to a dry run or a commit (see `Engine::erp_set_states`).
    pub(crate) fn erp_set_states(&mut self, json: &str) -> Result<String, String> {
        let input: StatesIn = serde_json::from_str(json)
            .map_err(|error| format!("The ERP's answer couldn't be read: {error}"))?;
        if self.bindings.is_empty() {
            return Err("This workbook has no ERP data.".to_string());
        }
        let summary = self.bindings.0[0].set_marks(&input.sent, &input.errors, &input.conflicts);
        let binding = &self.bindings.0[0];
        let keys: HashSet<String> = input
            .sent
            .iter()
            .filter_map(|record| binding.key_of(record.id).map(str::to_string))
            .collect();
        self.restate_keys(0, &keys);
        serde_json::to_string(&summary).map_err(|error| error.to_string())
    }

    /// Compares the rows of some records again.
    fn restate_keys(&mut self, index: usize, keys: &HashSet<String>) {
        let binding = &self.bindings.0[index];
        let Some(table) = self.bound_area(binding) else {
            return;
        };
        let sheet = binding.sheet;
        let rows: Vec<u32> = self
            .rows_of(binding, &table, &|key| keys.contains(key))
            .into_values()
            .collect();
        self.restate_rows(index, rows);
        self.dirty.sheets.insert(sheet);
    }

    /// Applies a commit's answer (see `Engine::erp_apply_commit`). Every record is read first,
    /// so an answer the sheet can't take writes nothing.
    pub(crate) fn erp_apply_commit(&mut self, json: &str) -> Result<String, String> {
        let input: CommitIn = serde_json::from_str(json)
            .map_err(|error| format!("The ERP's answer couldn't be read: {error}"))?;
        let binding = self
            .bindings
            .0
            .first()
            .ok_or_else(|| "This workbook has no ERP data.".to_string())?;
        let table = self
            .bound_area(binding)
            .ok_or_else(|| "The ERP data's table isn't there any more.".to_string())?;
        let sheet = binding.sheet;
        let sent: HashMap<i64, &SentRecord> = input
            .sent
            .iter()
            .map(|record| (record.id, record))
            .collect();
        let mut staged = Vec::with_capacity(input.records.len());
        for record in input.records {
            let id = record.id;
            let key = binding
                .key_of(id)
                .ok_or_else(|| {
                    format!("Cash Gear ERP answered for a record this sheet doesn't hold ({id}).")
                })?
                .to_string();
            let read = cgs_bind::read_record(&binding.fields, binding.key_index, record)?;
            if read.key != key {
                return Err(format!(
                    "Cash Gear ERP answered {key} with another key, {}.",
                    read.key
                ));
            }
            staged.push(read);
        }
        let keys: HashSet<&str> = staged.iter().map(|record| record.key.as_str()).collect();
        let rows = self.rows_of(binding, &table, &|key| keys.contains(key));
        let mut edits = Vec::new();
        let mut taken = Vec::with_capacity(staged.len());
        let (mut saved, mut kept) = (0, 0);
        for record in &staged {
            let row = *rows
                .get(&record.key)
                .ok_or_else(|| format!("{} isn't in the table any more.", record.key))?;
            let Some(meta) = binding.record(&record.key) else {
                continue;
            };
            let changes = sent.get(&record.id).map(|record| &record.changes);
            let mut committed = Vec::new();
            for (field, value) in record.values.iter().enumerate() {
                let col = table.left + field as u32;
                let now = content_of_input(&self.calc.cell_input(CellRef { sheet, row, col }));
                let bound = &binding.fields[field];
                let write = match binding.slot(field) {
                    // Read-only cells take what the ERP has, such as who modified the record.
                    None => true,
                    Some(slot) => match changes.and_then(|changes| changes.get(&bound.name)) {
                        Some(change) => {
                            let holds = wire_of(&bound.kind, &now) == wire_of_json(&change.to);
                            if holds {
                                committed.push(slot);
                                saved += 1;
                            } else {
                                kept += 1;
                            }
                            holds
                        }
                        None => same(&bound.kind, &meta.originals[slot], &now),
                    },
                };
                if write && now != Content::of_cell(value) {
                    edits.push(EditCell {
                        sheet,
                        row,
                        col,
                        input: Some(crate::erp::edit_of_cell(value)),
                        style: None,
                    });
                }
            }
            taken.push((
                record.key.clone(),
                record.version,
                record.values.clone(),
                committed,
            ));
        }
        if !edits.is_empty() {
            self.unguarded(|session| session.apply_cells(edits))?;
        }
        let binding = &mut self.bindings.0[0];
        binding.clear_committed();
        for (key, version, values, committed) in taken {
            binding.take_commit(&key, version, &values, committed);
        }
        // The originals moved, so every record is compared again.
        self.restate_all();
        // As Excel's data refresh does, so undo never brings the old values back as changes.
        self.history.forget();
        self.erp_frozen = false;
        serde_json::to_string(&CommitApplied {
            records: staged.len(),
            saved,
            kept,
        })
        .map_err(|error| error.to_string())
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
                if !state.has_change() || !within(row, col) || spilled.contains(&(row, col)) {
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
