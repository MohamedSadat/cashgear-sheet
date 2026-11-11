//! Refresh, a 409 and Resolve Conflicts as one three-way merge (Phase 18, plan B.3 item 8 and
//! E.5; decisions, 2026-11-11), and a bound workbook's recovery draft (plan B.5). The merge itself
//! is `cgs_bind`'s, pure and property-tested; this reads the cells it compares, writes what it
//! takes, and keeps undo from bringing old ERP values back.

use std::collections::HashSet;

use cgs_bind::draft::{records_json, records_of_json, server_records_json, server_records_of_json};
use cgs_bind::guard::{Content, TableArea};
use cgs_bind::state::CONFLICT;
use cgs_bind::{ApiConflict, Binding, CellValue, MergeCounts, ServerRecord, SessionInfo};
use cgs_calc::{CalcEngine, CellRef};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::edit::EditCell;
use crate::erp::{content_of_input, edit_of_cell};
use crate::{Engine, Session};

/// What a refresh did, for the status line, with what a recovery draft needs to replay it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RefreshSummary {
    /// Records the ERP answered for.
    updated: usize,
    /// Of those, records the ERP changed since the sheet read them.
    changed: usize,
    /// Records with changes in the sheet that merged without a clash.
    merged: usize,
    /// Records left in conflict.
    conflicts: usize,
    renamed: usize,
    missing: usize,
    /// The records the merge changed anything for, and those missing: replaying them on the
    /// same cells does what the whole refresh did. The engine worker keeps it for the draft.
    journal: Value,
}

/// What `erpResolve` is given: choices by record and field, or one for every conflict.
#[derive(Deserialize)]
struct ResolveIn {
    #[serde(default)]
    choices: Vec<Choice>,
    /// `mine` or `erp`, for every clashing field of every record in conflict.
    #[serde(default)]
    all: Option<String>,
}

#[derive(Deserialize)]
struct Choice {
    key: String,
    field: String,
    /// `mine` or `erp`.
    take: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Resolved {
    /// Fields resolved.
    fields: usize,
    /// Records that took the ERP's version, with nothing left in conflict.
    records: usize,
    /// Records still in conflict.
    left: usize,
}

/// One clashing field, for the Resolve Conflicts dialog.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConflictEntry<'a> {
    key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    row: u32,
    col: u32,
    field: &'a str,
    label: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    original: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    erp: Option<String>,
    /// The ERP's answer carried the field, so Take ERP's can write it.
    known: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    by: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    at: Option<&'a str>,
    hidden: bool,
}

/// A recovery draft's bind payload (decisions, 2026-11-11): the schema as the ERP sent it, the
/// session without its token, and the records as compact arrays.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Draft {
    v: u64,
    schema: Value,
    session: SessionInfo,
    #[serde(default)]
    as_of: Option<String>,
    records: Value,
    #[serde(default)]
    missing: Vec<i64>,
}

fn cell_of_content(content: &Content) -> Option<CellValue> {
    Some(match content {
        Content::Empty => CellValue::Empty,
        Content::Text(text) => CellValue::Text(text.clone()),
        Content::Number(bits) => CellValue::Number(f64::from_bits(*bits)),
        Content::Bool(b) => CellValue::Bool(*b),
        _ => return None,
    })
}

impl Session {
    /// What each field's cell of a data row holds, in column order; editable cells a spill covers
    /// count as formulas.
    fn row_cells(
        &self,
        binding: &Binding,
        table: &TableArea,
        row: u32,
        spilled: &HashSet<(u32, u32)>,
    ) -> Vec<Content> {
        (0..binding.fields.len())
            .map(|field| {
                let col = table.left + field as u32;
                if binding.slot(field).is_some() && spilled.contains(&(row, col)) {
                    return Content::Spill;
                }
                content_of_input(&self.calc.cell_input(CellRef {
                    sheet: binding.sheet,
                    row,
                    col,
                }))
            })
            .collect()
    }

    /// Merges what the ERP holds now for `records` with the cells (see `Binding::plan_merge`),
    /// writes what the merge takes, and compares every record again. Undo is cleared, as Excel's
    /// data refresh clears it. Answers the counts and the records the merge changed anything for.
    pub(crate) fn erp_merge(
        &mut self,
        records: Vec<ServerRecord>,
        complete: bool,
    ) -> Result<(MergeCounts, Vec<ServerRecord>, Vec<i64>), String> {
        let binding = self
            .bindings
            .0
            .first()
            .ok_or_else(|| "This workbook has no ERP data.".to_string())?;
        let table = self
            .bound_area(binding)
            .ok_or_else(|| "The ERP data's table isn't there any more.".to_string())?;
        let rows = self.rows_by_key(binding, &table);
        let spilled: HashSet<(u32, u32)> =
            self.spilled_cells(binding, &table).into_iter().collect();
        let now_of = |key: &str| {
            let row = *rows.get(key)?;
            Some((row, self.row_cells(binding, &table, row, &spilled)))
        };
        let plan = binding.plan_merge(records, &now_of, complete)?;
        let sheet = binding.sheet;
        let mut edits = Vec::new();
        let mut journal = Vec::new();
        for merge in &plan.records {
            for (field, value) in &merge.writes {
                edits.push(EditCell {
                    sheet,
                    row: merge.row,
                    col: table.left + *field as u32,
                    input: Some(edit_of_cell(value)),
                    style: None,
                });
            }
            let quiet =
                merge.writes.is_empty()
                    && merge.key == merge.old_key
                    && merge.adopts()
                    && binding.record(&merge.old_key).is_some_and(|meta| {
                        Some(meta.version) == merge.server.version
                            && meta.originals.iter().zip(&binding.editable).all(
                                |(original, &field)| {
                                    merge.server.values[field].as_ref() == Some(original)
                                },
                            )
                    })
                    && !binding.marks.conflicts.contains_key(&merge.old_key);
            if !quiet {
                journal.push(merge.server.clone());
            }
        }
        let missing = plan.missing.clone().unwrap_or_default();
        let counts = plan.counts;
        if !edits.is_empty() {
            self.unguarded(|session| session.apply_cells(edits))?;
        }
        self.bindings.0[0].apply_merge(plan, complete);
        // The originals moved, so every record is compared again.
        self.restate_all();
        self.history.forget();
        Ok((counts, journal, missing))
    }

    /// Merges a refresh's records: every bound record the ERP still has, found by id.
    pub(crate) fn refresh_records(&mut self, staging: cgs_bind::Staging) -> Result<String, String> {
        let as_of = staging.as_of.clone();
        let records = staging
            .records
            .into_iter()
            .map(ServerRecord::from)
            .collect();
        let (counts, journal, missing) = self.erp_merge(records, true)?;
        if let Some(as_of) = &as_of {
            self.bindings.0[0].as_of = as_of.clone();
        }
        let summary = RefreshSummary {
            updated: counts.records,
            changed: counts.changed,
            merged: counts.merged,
            conflicts: counts.conflicts,
            renamed: counts.renamed,
            missing: counts.missing,
            journal: json!({
                "records": server_records_json(&journal),
                "missing": missing,
                "asOf": as_of,
            }),
        };
        serde_json::to_string(&summary).map_err(|error| error.to_string())
    }

    /// Replays a refresh a recovery draft kept (`RefreshSummary::journal`).
    pub(crate) fn replay_refresh(&mut self, json: &str) -> Result<(), String> {
        let journal: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
        let binding = self
            .bindings
            .0
            .first()
            .ok_or_else(|| "This workbook has no ERP data.".to_string())?;
        let records = server_records_of_json(&journal["records"], binding.fields.len())?;
        let missing: Vec<i64> = serde_json::from_value(journal["missing"].clone())
            .map_err(|error| error.to_string())?;
        // The records it left out merged to nothing then, so they would merge to nothing now.
        self.erp_merge(records, false)?;
        let binding = &mut self.bindings.0[0];
        binding.missing = missing.into_iter().collect();
        binding.marks.committed.clear();
        if let Some(as_of) = journal["asOf"].as_str() {
            binding.as_of = as_of.to_string();
        }
        let missing_keys: Vec<String> = binding
            .missing
            .iter()
            .filter_map(|id| binding.key_of(*id).map(str::to_string))
            .collect();
        for key in missing_keys {
            binding.marks.conflicts.remove(&key);
        }
        self.restate_all();
        Ok(())
    }

    /// Merges a 409's conflicts with the cells. Answers the counts.
    pub(crate) fn merge_conflicts(
        &mut self,
        conflicts: &[ApiConflict],
    ) -> Result<MergeCounts, String> {
        let Some(binding) = self.bindings.0.first() else {
            return Err("This workbook has no ERP data.".to_string());
        };
        let records = binding.conflict_records(conflicts);
        Ok(self.erp_merge(records, false)?.0)
    }

    /// Resolve Conflicts: Take ERP's writes the ERP's value into the cell, Keep Mine moves the
    /// field's original to it, and each touched record is merged again with what its conflict
    /// mark holds, so one with no clash left adopts the ERP's version.
    pub(crate) fn erp_resolve(&mut self, json: &str) -> Result<String, String> {
        let input: ResolveIn = serde_json::from_str(json)
            .map_err(|error| format!("The choices couldn't be read: {error}"))?;
        let binding = self
            .bindings
            .0
            .first()
            .ok_or_else(|| "This workbook has no ERP data.".to_string())?;
        let table = self
            .bound_area(binding)
            .ok_or_else(|| "The ERP data's table isn't there any more.".to_string())?;
        // The fields in conflict are those its state shows so.
        let flagged = |key: &str, slot: usize| {
            binding.marks.conflicts.contains_key(key)
                && binding
                    .states
                    .get(key)
                    .is_some_and(|fields| fields[slot].bits & CONFLICT != 0)
        };
        let mut choices: Vec<(String, usize, bool)> = Vec::new();
        if let Some(all) = &input.all {
            let mut keys: Vec<&String> = binding.marks.conflicts.keys().collect();
            keys.sort();
            for key in keys {
                for slot in 0..binding.editable.len() {
                    if flagged(key, slot) {
                        choices.push((key.clone(), slot, all == "erp"));
                    }
                }
            }
        }
        for choice in &input.choices {
            let Some(slot) = binding
                .field_index(&choice.field)
                .and_then(|field| binding.slot(field))
            else {
                continue;
            };
            if flagged(&choice.key, slot) {
                choices.push((choice.key.clone(), slot, choice.take == "erp"));
            }
        }
        let keys: HashSet<String> = choices.iter().map(|(key, _, _)| key.clone()).collect();
        let rows = self.rows_of(binding, &table, &|key| keys.contains(key));
        let mut edits = Vec::new();
        let mut originals = Vec::new();
        let mut fields = 0;
        for (key, slot, erp) in &choices {
            let field = binding.editable[*slot];
            let (Some(mark), Some(&row)) = (binding.marks.conflicts.get(key), rows.get(key)) else {
                continue;
            };
            // A field the ERP's answer didn't carry can only be resolved by a refresh.
            let Some(value) = mark.values.get(field).cloned().flatten() else {
                continue;
            };
            fields += 1;
            if *erp {
                edits.push(EditCell {
                    sheet: binding.sheet,
                    row,
                    col: table.left + field as u32,
                    input: Some(edit_of_cell(&value)),
                    style: None,
                });
            } else {
                originals.push((key.clone(), *slot, value));
            }
        }
        if !edits.is_empty() {
            self.unguarded(|session| session.apply_cells(edits))?;
        }
        let binding = &mut self.bindings.0[0];
        for (key, slot, value) in originals {
            binding.set_original(&key, slot, value);
        }
        let mut sorted: Vec<&String> = keys.iter().collect();
        sorted.sort();
        let records: Vec<ServerRecord> = sorted
            .into_iter()
            .filter_map(|key| binding.conflict_record(key))
            .collect();
        self.erp_merge(records, false)?;
        let conflicts = &self.bindings.0[0].marks.conflicts;
        serde_json::to_string(&Resolved {
            fields,
            records: keys
                .iter()
                .filter(|key| !conflicts.contains_key(*key))
                .count(),
            left: conflicts.len(),
        })
        .map_err(|error| error.to_string())
    }

    /// Every clashing field of every record in conflict, for the Resolve Conflicts dialog, in
    /// reading order, as JSON text.
    pub(crate) fn erp_conflicts_text(&self, locale: &cgs_numfmt::Locale) -> String {
        let Some(binding) = self.bindings.0.first() else {
            return "null".to_string();
        };
        let Some(table) = self.bound_area(binding) else {
            return "null".to_string();
        };
        let rows = self.rows_of(binding, &table, &|key| {
            binding.marks.conflicts.contains_key(key)
        });
        let name_field = self.erp_name_field(binding);
        let mut entries = Vec::new();
        let mut records = 0;
        for (key, mark) in &binding.marks.conflicts {
            let (Some(&row), Some(meta)) = (rows.get(key), binding.record(key)) else {
                continue;
            };
            let states = binding.states.get(key);
            let shown: Vec<usize> = (0..binding.editable.len())
                .filter(|&slot| states.is_some_and(|fields| fields[slot].bits & CONFLICT != 0))
                .collect();
            if shown.is_empty() {
                continue;
            }
            records += 1;
            let read = |col: u32| {
                content_of_input(&self.calc.cell_input(CellRef {
                    sheet: binding.sheet,
                    row,
                    col,
                }))
            };
            let name = name_field.and_then(|field| match read(table.left + field as u32) {
                Content::Text(text) if !text.is_empty() => Some(text),
                _ => None,
            });
            for slot in shown {
                let field = binding.editable[slot];
                let col = table.left + field as u32;
                let erp = mark.values.get(field).cloned().flatten();
                entries.push(ConflictEntry {
                    key: key.clone(),
                    name: name.clone(),
                    row,
                    col,
                    field: &binding.fields[field].name,
                    label: &binding.fields[field].label,
                    original: self.erp_shown(
                        binding,
                        field,
                        &Content::of_cell(&meta.originals[slot]),
                        locale,
                    ),
                    mine: self.erp_shown(binding, field, &read(col), locale),
                    known: erp.is_some(),
                    erp: erp.and_then(|value| {
                        self.erp_shown(binding, field, &Content::of_cell(&value), locale)
                    }),
                    by: mark.modified_by.as_deref(),
                    at: mark.modified_at.as_deref(),
                    hidden: self.row_hidden(binding.sheet, row),
                });
            }
        }
        entries.sort_unstable_by_key(|entry| (entry.row, entry.col));
        serde_json::to_string(&json!({ "entries": entries, "records": records }))
            .unwrap_or_else(|_| "null".to_string())
    }

    /// Whether anything in the bound table waits on the ERP: changed values, formulas or
    /// conflicts. A recovery draft is worth offering only then.
    pub(crate) fn erp_pending(&self) -> bool {
        self.bindings.0.first().is_some_and(|binding| {
            let counts = self.erp_counts(binding);
            counts.changed > 0 || counts.formulas > 0 || counts.conflicts > 0
        })
    }

    /// The binding as a recovery draft's bind payload: its records as the binding knows them,
    /// with read-only fields as their cells hold them.
    pub(crate) fn erp_draft_text(&self) -> Option<String> {
        let binding = self.bindings.0.first()?;
        let table = self.bound_area(binding)?;
        let rows = self.rows_by_key(binding, &table);
        let read_only = |key: &str, field: usize| {
            let row = *rows.get(key)?;
            cell_of_content(&content_of_input(&self.calc.cell_input(CellRef {
                sheet: binding.sheet,
                row,
                col: table.left + field as u32,
            })))
        };
        let records = binding.snapshot(&read_only);
        let draft = Draft {
            v: cgs_bind::draft::DRAFT_VERSION,
            schema: serde_json::from_str(&binding.schema_json).ok()?,
            session: binding.session.clone(),
            as_of: (!binding.as_of.is_empty()).then(|| binding.as_of.clone()),
            records: records_json(&records),
            missing: binding.missing.iter().copied().collect(),
        };
        serde_json::to_string(&draft).ok()
    }

    /// The first text field after the key, which names a record in lists.
    pub(crate) fn erp_name_field(&self, binding: &Binding) -> Option<usize> {
        binding
            .fields
            .iter()
            .enumerate()
            .find(|(index, field)| {
                *index > binding.key_index && field.kind == cgs_bind::FieldKind::Text
            })
            .map(|(index, _)| index)
    }
}

impl Engine {
    /// The open workbook's binding as a recovery draft's bind payload, or `None`.
    pub fn erp_draft(&self) -> Option<String> {
        self.session.as_ref()?.erp_draft_text()
    }

    /// Whether the bound table holds anything that waits on the ERP.
    pub fn erp_pending(&self) -> bool {
        self.session.as_ref().is_some_and(Session::erp_pending)
    }

    /// Binds a recovery draft's records into the open workbook, which must be new, as they were
    /// bound when the draft was written. Answers the binding's summary.
    pub fn erp_restore(&mut self, json: &str) -> Result<String, String> {
        let draft: Draft = serde_json::from_str(json)
            .map_err(|_| "The recovery draft can't be read.".to_string())?;
        if draft.v != cgs_bind::draft::DRAFT_VERSION {
            return Err("The recovery draft was written by another version.".to_string());
        }
        let schema = serde_json::to_string(&draft.schema).map_err(|error| error.to_string())?;
        self.erp_begin(&schema)?;
        let erp = self
            .erp
            .as_mut()
            .ok_or_else(|| "The recovery draft can't be read.".to_string())?;
        let fields = erp.staging.fields.len();
        let records = records_of_json(&draft.records, fields)?;
        let staging = std::mem::replace(
            &mut erp.staging,
            cgs_bind::Staging::new(Vec::new(), 0, 0, ""),
        );
        erp.staging = staging.restored(records, draft.as_of);
        let session = serde_json::to_string(&draft.session).map_err(|error| error.to_string())?;
        let summary = self.erp_bind(&session)?;
        if draft.missing.is_empty() {
            return Ok(summary);
        }
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| "No workbook is open.".to_string())?;
        session.bindings.0[0].missing = draft.missing.into_iter().collect();
        session.restate_all();
        Ok(session.binding_summary().unwrap_or_default())
    }

    /// Replays a refresh a recovery draft kept.
    pub fn erp_replay_refresh(&mut self, json: &str) -> Result<(), String> {
        self.session
            .as_mut()
            .ok_or_else(|| "No workbook is open.".to_string())?
            .replay_refresh(json)
    }

    /// Resolve Conflicts: `{choices: [{key, field, take}]}` or `{all}`, with `take` and `all`
    /// `mine` or `erp`. Answers `{fields, records, left}`.
    pub fn erp_resolve(&mut self, json: &str) -> Result<String, String> {
        self.session
            .as_mut()
            .ok_or_else(|| "No workbook is open.".to_string())?
            .erp_resolve(json)
    }
}
