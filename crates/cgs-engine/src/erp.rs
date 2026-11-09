//! ERP-bound tables (Phases 15 to 19, `docs/ERP_SHEET_INTEGRATION_PLAN.md`). Record pages are
//! staged as they arrive, then bound into a new workbook as one table with typed values; a refresh
//! replaces the bound records by id. The identity guard is called on every path that changes cells
//! (`apply_cells`), tables (`Op::Tables`, `Op::BookTables`) or the sheet's shape (`apply_axis`),
//! and before commands so that each refusal has its own message. Undo, redo and rollback replay
//! only what was allowed, so they run unguarded.

use cgs_bind::guard::{check_writes, Content, Refusal, TableArea};
use cgs_bind::wire::Staging;
use cgs_bind::{Binding, CellValue, EntitySchema, SessionInfo};
use cgs_calc::{CalcEngine, CalcValue, CellInput, CellRef, TableInfo};
use cgs_model::Table;
use cgs_refs::{Axis, AxisEdit};
use serde::Serialize;

use crate::command::{Area, AxisName, Command, PasteMode};
use crate::edit::{EditCell, EditInput, NewStyle};
use crate::{Engine, Session};

/// Pages on their way in: for a new binding with its schema, or for a refresh of the one there.
pub(crate) struct ErpStaging {
    schema: Option<EntitySchema>,
    staging: Staging,
}

fn content_of_value(value: &CalcValue) -> Content {
    match value {
        CalcValue::Empty => Content::Empty,
        CalcValue::Number(n) => Content::number(*n),
        CalcValue::Bool(b) => Content::Bool(*b),
        CalcValue::Text(text) => Content::Text(text.clone()),
        CalcValue::Error(error) => Content::Error(error.clone()),
    }
}

fn content_of_input(input: &CellInput) -> Content {
    match input {
        CellInput::Clear => Content::Empty,
        CellInput::Value(value) => content_of_value(value),
        CellInput::Formula { text, .. } => Content::Formula(text.clone()),
        CellInput::Spill { .. } => Content::Spill,
    }
}

fn content_of_edit(input: &EditInput) -> Content {
    match input {
        EditInput::Clear => Content::Empty,
        EditInput::Value(value) => content_of_value(value),
        EditInput::Formula(text) | EditInput::Array(text, _) => Content::Formula(text.clone()),
    }
}

fn content_of_cell(value: &CellValue) -> Content {
    match value {
        CellValue::Empty => Content::Empty,
        CellValue::Text(text) => Content::Text(text.clone()),
        CellValue::Number(n) => Content::number(*n),
        CellValue::Bool(b) => Content::Bool(*b),
    }
}

fn edit_of_cell(value: &CellValue) -> EditInput {
    match value {
        CellValue::Empty => EditInput::Clear,
        CellValue::Text(text) => EditInput::Value(CalcValue::Text(text.clone())),
        CellValue::Number(n) => EditInput::Value(CalcValue::Number(*n)),
        CellValue::Bool(b) => EditInput::Value(CalcValue::Bool(*b)),
    }
}

fn area_meets(table: &TableArea, area: &Area) -> bool {
    table.meets(area.top, area.left, area.bottom, area.right)
}

/// A table name made from the entity: `vendors` names its table `Vendors`.
fn table_name_for(entity: &str) -> String {
    let mut name: String = entity
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        name.insert_str(0, "Erp");
    }
    let mut chars = name.chars();
    let first = chars.next().map(|c| c.to_ascii_uppercase());
    first.into_iter().chain(chars).collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FieldSummary<'a> {
    name: &'a str,
    label: &'a str,
    editable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    edit_reason: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BindingSummary<'a> {
    entity: &'a str,
    label: &'a str,
    label_en: &'a str,
    company: &'a str,
    user_id: &'a str,
    user_name: &'a str,
    language: &'a str,
    entity_route: &'a str,
    schema_version: &'a str,
    table: &'a str,
    sheet: u32,
    /// The table's place now: its heading row, last row and columns.
    top: u32,
    left: u32,
    bottom: u32,
    right: u32,
    records: usize,
    missing: usize,
    as_of: &'a str,
    max_records: u32,
    max_page_size: u32,
    fields: Vec<FieldSummary<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RefreshSummary {
    updated: usize,
    changed: usize,
    renamed: usize,
    missing: usize,
}

impl Session {
    /// A bound table's place now, from the model's table, which moving rows keeps up to date.
    pub(crate) fn bound_area(&self, binding: &Binding) -> Option<TableArea> {
        let table = self
            .book
            .tables
            .iter()
            .find(|table| table.id == binding.table_id)?;
        Some(TableArea {
            header: table.range.top,
            bottom: table.range.bottom,
            left: table.range.left,
            right: table.range.right,
        })
    }

    fn bound_areas(&self, sheet: u32) -> Vec<(&Binding, TableArea)> {
        self.bindings
            .on_sheet(sheet)
            .filter_map(|binding| Some((binding, self.bound_area(binding)?)))
            .collect()
    }

    fn bound_meets(&self, sheet: u32, areas: &[Area]) -> bool {
        self.bound_areas(sheet)
            .iter()
            .any(|(_, table)| areas.iter().any(|area| area_meets(table, area)))
    }

    fn bound_table_named(&self, name: &str) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|binding| binding.table_name.eq_ignore_ascii_case(name))
    }

    /// Runs `f` with the guard off, for undo, redo, rollback and the binding's own writes.
    pub(crate) fn unguarded<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let was = std::mem::replace(&mut self.unguarded, true);
        let result = f(self);
        self.unguarded = was;
        result
    }

    fn guarding(&self) -> bool {
        !self.unguarded && !self.bindings.is_empty()
    }

    /// A command refused before it runs, with its own message. The content rule in `guard_cells`
    /// still checks whatever gets through, so this only has to be right, not complete.
    pub(crate) fn binding_refusal(&self, command: &Command) -> Option<&'static str> {
        use Command as C;
        if !self.guarding() || !self.erp_precheck {
            return None;
        }
        let read_only = |sheet: u32, areas: &[Area]| {
            self.bound_meets(sheet, areas)
                .then_some(Refusal::ReadOnly.message())
        };
        let refusal = match command {
            C::Input {
                sheet,
                row,
                col,
                areas,
                ..
            } => {
                let mut all = areas.clone();
                all.push(Area::cell(*row, *col));
                return read_only(*sheet, &all);
            }
            C::ArrayInput { sheet, area, .. } => return read_only(*sheet, &[*area]),
            C::RichInput {
                sheet, row, col, ..
            }
            | C::ReplaceOne {
                sheet, row, col, ..
            }
            | C::IncludeCells { sheet, row, col }
            | C::PasteExternal {
                sheet, row, col, ..
            } => return read_only(*sheet, &[Area::cell(*row, *col)]),
            C::Clear { sheet, areas } | C::ConvertToNumber { sheet, areas } => {
                return read_only(*sheet, areas)
            }
            C::Fill { sheet, to, .. } => return read_only(*sheet, &[*to]),
            C::Paste {
                sheet, to, mode, ..
            } => {
                if *mode == PasteMode::Formats {
                    return None;
                }
                return read_only(*sheet, &[*to]);
            }
            C::Move {
                from_sheet,
                from,
                sheet,
                row,
                col,
            } => {
                let to = Area {
                    top: *row,
                    left: *col,
                    bottom: (row + from.rows() - 1).min(cgs_model::MAX_ROWS),
                    right: (col + from.cols() - 1).min(cgs_model::MAX_COLS),
                };
                return read_only(*from_sheet, &[*from]).or_else(|| read_only(*sheet, &[to]));
            }
            C::Axis {
                sheet,
                axis,
                at,
                count,
                delete,
            } => {
                let axis = match axis {
                    AxisName::Row => Axis::Row,
                    AxisName::Col => Axis::Col,
                };
                let edit = AxisEdit {
                    axis,
                    at: *at,
                    count: (*count).max(1),
                    delete: *delete,
                    band: None,
                };
                self.axis_refusal(*sheet, &edit)
            }
            C::ShiftCells {
                sheet, area, down, ..
            } => {
                let reach = if *down {
                    Area {
                        bottom: cgs_model::MAX_ROWS,
                        ..*area
                    }
                } else {
                    Area {
                        right: cgs_model::MAX_COLS,
                        ..*area
                    }
                };
                self.bound_meets(*sheet, &[reach]).then_some(Refusal::Table)
            }
            C::Merge { sheet, areas, .. } => {
                self.bound_meets(*sheet, areas).then_some(Refusal::Table)
            }
            C::CreateTable { sheet, area, .. } => {
                self.bound_meets(*sheet, &[*area]).then_some(Refusal::Table)
            }
            C::DeleteSheet { sheet } => self
                .bindings
                .on_sheet(*sheet)
                .next()
                .map(|_| Refusal::Sheet),
            C::TableOptions { table, options } => {
                self.bound_table_named(table)?;
                let header_off = options.header_row == Some(false);
                let totals_on = options.totals_row == Some(true);
                (header_off || totals_on).then_some(Refusal::Table)
            }
            C::TotalsFunction { table, .. }
            | C::RenameTable { table, .. }
            | C::ResizeTable { table, .. }
            | C::ConvertToRange { table } => self.bound_table_named(table).map(|_| Refusal::Table),
            C::RemoveDuplicates { sheet, area, .. } => {
                self.bound_meets(*sheet, &[*area]).then_some(Refusal::Rows)
            }
            C::Sort {
                sheet,
                area,
                header,
                by_columns,
                ..
            } => {
                let info = self.book.sheets.get(*sheet as usize)?;
                // As `sort_command` reads it: whole columns reach as far as what's in them.
                let bottom = area.bottom.min(info.used.0.max(area.top));
                let right = area.right.min(info.used.1.max(area.left));
                let top = area.top + u32::from(*header);
                self.bound_areas(*sheet).iter().find_map(|(_, table)| {
                    if !table.meets(area.top, area.left, bottom, right) {
                        return None;
                    }
                    if *by_columns {
                        return Some(Refusal::SortSideways);
                    }
                    (!table.is_data_area(top, area.left, bottom, right))
                        .then_some(Refusal::SortWhole)
                })
            }
            _ => None,
        };
        refusal.map(Refusal::message)
    }

    fn axis_refusal(&self, sheet: u32, edit: &AxisEdit) -> Option<Refusal> {
        let last = edit.at + edit.count.max(1) - 1;
        self.bound_areas(sheet).iter().find_map(|(_, table)| {
            let (first_line, last_line, across) = match edit.axis {
                Axis::Row => (table.header, table.bottom, (table.left, table.right)),
                Axis::Col => (table.left, table.right, (table.header, table.bottom)),
            };
            if let Some((from, to)) = edit.band {
                // Cells shifting in a band: anything of the table at or past `at` would move.
                let in_band = from <= across.1 && to >= across.0;
                return (in_band && edit.at <= last_line).then_some(Refusal::Table);
            }
            let refused = if edit.delete {
                edit.at <= last_line && last >= first_line
            } else {
                edit.at > first_line && edit.at <= last_line
            };
            refused.then_some(match edit.axis {
                Axis::Row => Refusal::Rows,
                Axis::Col => Refusal::Columns,
            })
        })
    }

    /// The content rule, on the edits `apply_cells` is about to make.
    pub(crate) fn guard_cells(&self, edits: &[EditCell]) -> Result<(), String> {
        if !self.guarding() {
            return Ok(());
        }
        let mut sheets: Vec<u32> = edits
            .iter()
            .filter(|edit| edit.input.is_some())
            .map(|edit| edit.sheet)
            .collect();
        sheets.sort_unstable();
        sheets.dedup();
        for sheet in sheets {
            for (binding, table) in self.bound_areas(sheet) {
                let writes: Vec<(u32, u32, Content)> = edits
                    .iter()
                    .filter(|edit| edit.sheet == sheet && table.contains(edit.row, edit.col))
                    .filter_map(|edit| {
                        let input = edit.input.as_ref()?;
                        Some((edit.row, edit.col, content_of_edit(input)))
                    })
                    .collect();
                if writes.is_empty() {
                    continue;
                }
                let left = table.left;
                let read_only = |col: u32| !binding.editable_in_sheet((col - left) as usize);
                let current = |row: u32, col: u32| {
                    content_of_input(&self.calc.cell_input(CellRef { sheet, row, col }))
                };
                check_writes(&table, &read_only, &writes, &current)
                    .map_err(|refusal| refusal.message().to_string())?;
            }
        }
        Ok(())
    }

    /// The engine's tables as an op would put them: a bound one keeps its shape, name, columns
    /// and heading, with no totals.
    pub(crate) fn guard_calc_tables(&self, tables: &[TableInfo]) -> Result<(), String> {
        if !self.guarding() {
            return Ok(());
        }
        for binding in self.bindings.iter() {
            let named = |list: &[TableInfo]| {
                list.iter()
                    .find(|t| t.name.eq_ignore_ascii_case(&binding.table_name))
                    .cloned()
            };
            let (Some(now), Some(next)) = (named(self.calc.tables()), named(tables)) else {
                if named(self.calc.tables()).is_some() {
                    return Err(Refusal::Table.message().to_string());
                }
                continue;
            };
            let size = |t: &TableInfo| {
                (
                    t.range.bottom - t.range.top,
                    t.range.right - t.range.left,
                    t.sheet,
                )
            };
            if size(&now) != size(&next)
                || now.header_row != next.header_row
                || now.totals_row != next.totals_row
                || now.columns != next.columns
            {
                return Err(Refusal::Table.message().to_string());
            }
        }
        Ok(())
    }

    /// The model's tables as an op would put them: as `guard_calc_tables`, and no calculated
    /// columns.
    pub(crate) fn guard_book_tables(&self, tables: &[Table]) -> Result<(), String> {
        if !self.guarding() {
            return Ok(());
        }
        for binding in self.bindings.iter() {
            let by_id = |list: &[Table]| list.iter().find(|t| t.id == binding.table_id).cloned();
            let Some(now) = by_id(&self.book.tables) else {
                continue;
            };
            let Some(next) = by_id(tables) else {
                return Err(Refusal::Table.message().to_string());
            };
            let size = |t: &Table| {
                (
                    t.range.bottom - t.range.top,
                    t.range.right - t.range.left,
                    t.sheet,
                )
            };
            if size(&now) != size(&next)
                || now.name != next.name
                || now.header_rows != next.header_rows
                || now.totals_rows != next.totals_rows
                || now.columns != next.columns
                || next.column_info.iter().any(|c| c.calculated.is_some())
            {
                return Err(Refusal::Table.message().to_string());
            }
        }
        Ok(())
    }

    /// Rows or columns going in or out, or cells shifting, in a bound table.
    pub(crate) fn guard_axis(&self, sheet: u32, edit: &AxisEdit) -> Result<(), String> {
        if !self.guarding() {
            return Ok(());
        }
        match self.axis_refusal(sheet, edit) {
            Some(refusal) => Err(refusal.message().to_string()),
            None => Ok(()),
        }
    }

    /// A sheet holding a bound table going away.
    pub(crate) fn guard_sheet(&self, sheet: u32) -> Result<(), String> {
        if self.guarding() && self.bindings.on_sheet(sheet).next().is_some() {
            return Err(Refusal::Sheet.message().to_string());
        }
        Ok(())
    }

    /// Whether a table is bound, so it neither grows nor fills calculated columns.
    pub(crate) fn is_bound_table(&self, name: &str) -> bool {
        self.bindings.holds_table_named(name)
    }

    /// Writes staged records into this new workbook's first sheet as one table, and binds it.
    fn bind_records(
        &mut self,
        schema: EntitySchema,
        session: SessionInfo,
        staging: Staging,
        locale: &cgs_numfmt::Locale,
    ) -> Result<(), String> {
        if !self.bindings.is_empty() {
            return Err("This workbook is already bound to ERP data.".to_string());
        }
        let fresh = self.book.sheets.len() == 1
            && self.book.tables.is_empty()
            && self.book.sheets[0].kind == cgs_model::SheetKind::Worksheet
            && self.book.sheets[0].loaded
            && self.book.sheets[0].used == (0, 0);
        if !fresh {
            return Err("ERP data opens in a new workbook.".to_string());
        }
        if staging.records.is_empty() {
            return Err(format!(
                "No {} match this list in Cash Gear ERP.",
                schema.label.en.to_lowercase()
            ));
        }
        let fields = staging.fields.clone();
        if fields.len() as u32 > cgs_model::MAX_COLS {
            return Err("The ERP's schema has more fields than a sheet has columns.".to_string());
        }
        let sheet = 0;
        let label = schema.label.get(session.language()).to_string();
        if cgs_model_sheet_name_ok(&self.book, &label) {
            self.command(
                Command::RenameSheet {
                    sheet,
                    name: label.clone(),
                },
                locale,
            )?;
        }
        // Each column's number format, made once.
        let styles: Vec<Option<u32>> = fields
            .iter()
            .map(|field| {
                field.kind.number_format().map(|code| {
                    self.add_style(
                        0,
                        &NewStyle {
                            number_format: Some(code),
                            ..Default::default()
                        },
                    )
                })
            })
            .collect();
        // Headings must differ, as a table's do.
        let mut headings: Vec<String> = Vec::with_capacity(fields.len());
        for field in &fields {
            let base = if field.label.trim().is_empty() {
                field.name.clone()
            } else {
                field.label.clone()
            };
            let mut heading = base.clone();
            let mut n = 2;
            while headings
                .iter()
                .any(|h| h.to_lowercase() == heading.to_lowercase())
            {
                heading = format!("{base}{n}");
                n += 1;
            }
            headings.push(heading);
        }
        let rows = staging.records.len() as u32;
        let mut edits = Vec::with_capacity((rows as usize + 1) * fields.len());
        for (index, heading) in headings.iter().enumerate() {
            edits.push(EditCell {
                sheet,
                row: 1,
                col: index as u32 + 1,
                input: Some(EditInput::Value(CalcValue::Text(heading.clone()))),
                style: None,
            });
        }
        for (r, record) in staging.records.iter().enumerate() {
            for (index, value) in record.values.iter().enumerate() {
                if *value == CellValue::Empty && styles[index].is_none() {
                    continue;
                }
                edits.push(EditCell {
                    sheet,
                    row: r as u32 + 2,
                    col: index as u32 + 1,
                    input: (*value != CellValue::Empty).then(|| edit_of_cell(value)),
                    style: styles[index],
                });
            }
        }
        self.apply_loaded(edits)?;
        let name = {
            let base = table_name_for(&schema.entity);
            let mut name = base.clone();
            let mut n = 2;
            while self.taken_name(&name) {
                name = format!("{base}{n}");
                n += 1;
            }
            name
        };
        let area = Area {
            top: 1,
            left: 1,
            bottom: rows + 1,
            right: fields.len() as u32,
        };
        self.create_table_command(sheet, area, Some(true), None, Some(name.clone()))?;
        self.command(
            Command::Freeze {
                sheet,
                rows: 1,
                cols: 0,
                top: 1,
                left: 1,
            },
            locale,
        )?;
        let table_id = self
            .book
            .tables
            .iter()
            .find(|table| table.name == name)
            .map(|table| table.id)
            .ok_or_else(|| "The ERP data's table wasn't made.".to_string())?;
        self.bindings.push(Binding::new(
            &schema, session, staging, table_id, name, sheet,
        ));
        // Opening ERP data is no step to undo, as opening a file isn't.
        self.history.forget();
        Ok(())
    }

    /// Replaces the bound records' cells with what the ERP has now, found by id.
    fn refresh_records(&mut self, staging: Staging) -> Result<RefreshSummary, String> {
        let binding = self
            .bindings
            .0
            .first()
            .ok_or_else(|| "This workbook has no ERP data.".to_string())?;
        let table = self
            .bound_area(binding)
            .ok_or_else(|| "The ERP data's table isn't there any more.".to_string())?;
        let sheet = binding.sheet;
        let key_col = table.left + binding.key_index as u32;
        let mut rows = std::collections::HashMap::new();
        for row in table.first_data_row()..=table.bottom {
            if let Content::Text(key) = content_of_input(&self.calc.cell_input(CellRef {
                sheet,
                row,
                col: key_col,
            })) {
                rows.insert(key, row);
            }
        }
        let as_of = staging.as_of.clone();
        let plan = binding.plan_refresh(staging, &|key| rows.get(key).copied())?;
        let mut edits = Vec::new();
        let mut changed = 0;
        for (row, values) in &plan.rows {
            let mut differs = false;
            for (index, value) in values.iter().enumerate() {
                let col = table.left + index as u32;
                let now = content_of_input(&self.calc.cell_input(CellRef {
                    sheet,
                    row: *row,
                    col,
                }));
                if now != content_of_cell(value) {
                    differs = true;
                    edits.push(EditCell {
                        sheet,
                        row: *row,
                        col,
                        input: Some(edit_of_cell(value)),
                        style: None,
                    });
                }
            }
            changed += usize::from(differs);
        }
        let summary = RefreshSummary {
            updated: plan.updated,
            changed,
            renamed: plan.renamed,
            missing: plan.missing.len(),
        };
        if !edits.is_empty() {
            self.unguarded(|session| session.apply_cells(edits))?;
        }
        let binding = &mut self.bindings.0[0];
        binding.apply_refresh(plan);
        if let Some(as_of) = as_of {
            binding.as_of = as_of;
        }
        // As Excel's data refresh does, so undo never brings old ERP values back.
        self.history.forget();
        Ok(summary)
    }

    fn binding_summary(&self) -> Option<String> {
        let binding = self.bindings.0.first()?;
        let table = self.bound_area(binding)?;
        let summary = BindingSummary {
            entity: &binding.session.entity,
            label: &binding.entity_label,
            label_en: &binding.entity_label_en,
            company: &binding.session.company,
            user_id: &binding.session.user_id,
            user_name: &binding.session.user_name,
            language: &binding.session.language,
            entity_route: &binding.session.entity_route,
            schema_version: &binding.schema_version,
            table: &binding.table_name,
            sheet: binding.sheet,
            top: table.header,
            left: table.left,
            bottom: table.bottom,
            right: table.right,
            records: binding.len(),
            missing: binding.missing.len(),
            as_of: &binding.as_of,
            max_records: binding.max_records,
            max_page_size: binding.max_page_size,
            fields: binding
                .fields
                .iter()
                .map(|field| FieldSummary {
                    name: &field.name,
                    label: &field.label,
                    editable: false,
                    edit_reason: field.edit_reason.as_deref(),
                })
                .collect(),
        };
        serde_json::to_string(&summary).ok()
    }
}

/// Whether a label can name a sheet as it is.
fn cgs_model_sheet_name_ok(book: &cgs_model::Book, name: &str) -> bool {
    crate::command::sheet_name_problem(book, name, Some(0)).is_none()
}

impl Engine {
    /// Starts staging a new binding's record pages, against the schema the ERP sent.
    pub fn erp_begin(&mut self, schema_json: &str) -> Result<(), String> {
        let schema = EntitySchema::parse(schema_json)?;
        // The language only changes headings, which `erp_bind` takes from the session.
        let fields = schema.bound_fields(cgs_bind::Language::En);
        let staging = Staging::new(
            fields,
            schema.key_index(),
            schema.max_records,
            &schema.label.en.to_lowercase(),
        );
        self.erp = Some(ErpStaging {
            schema: Some(schema),
            staging,
        });
        Ok(())
    }

    /// Starts staging a refresh of the open workbook's binding.
    pub fn erp_begin_refresh(&mut self) -> Result<(), String> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| "No workbook is open.".to_string())?;
        let binding = session
            .bindings
            .0
            .first()
            .ok_or_else(|| "This workbook has no ERP data.".to_string())?;
        self.erp = Some(ErpStaging {
            schema: None,
            staging: binding.refresh_staging(),
        });
        Ok(())
    }

    /// Reads one record page's bytes into the staging. Answers `PageInfo` as JSON.
    pub fn erp_page(&mut self, bytes: &[u8]) -> Result<String, String> {
        let erp = self
            .erp
            .as_mut()
            .ok_or_else(|| "No ERP records are being loaded.".to_string())?;
        match erp.staging.page(bytes) {
            Ok(info) => serde_json::to_string(&info).map_err(|error| error.to_string()),
            Err(problem) => {
                self.erp = None;
                Err(problem)
            }
        }
    }

    /// Drops whatever was staged.
    pub fn erp_cancel(&mut self) {
        self.erp = None;
    }

    /// Binds the staged records into the open workbook, which must be new. `session_json` is
    /// Redeem's answer without its token.
    pub fn erp_bind(&mut self, session_json: &str) -> Result<String, String> {
        let session_info: SessionInfo =
            serde_json::from_str(session_json).map_err(|error| error.to_string())?;
        let erp = self
            .erp
            .take()
            .ok_or_else(|| "No ERP records were loaded.".to_string())?;
        let schema = erp
            .schema
            .ok_or_else(|| "These records are a refresh, not a new binding.".to_string())?;
        if !erp.staging.complete() {
            return Err("The ERP's records didn't all arrive.".to_string());
        }
        let mut staging = erp.staging;
        // Headings in the session's language.
        staging.fields = schema.bound_fields(session_info.language());
        let locale = self.locale.clone();
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| "No workbook is open.".to_string())?;
        session.bind_records(schema, session_info, staging, &locale)?;
        Ok(session.binding_summary().unwrap_or_default())
    }

    /// Replaces the bound records with the staged refresh.
    pub fn erp_refresh(&mut self) -> Result<String, String> {
        let erp = self
            .erp
            .take()
            .ok_or_else(|| "No ERP records were loaded.".to_string())?;
        if erp.schema.is_some() {
            return Err("These records are a new binding, not a refresh.".to_string());
        }
        if !erp.staging.complete() {
            return Err("The ERP's records didn't all arrive.".to_string());
        }
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| "No workbook is open.".to_string())?;
        let summary = session.refresh_records(erp.staging)?;
        serde_json::to_string(&summary).map_err(|error| error.to_string())
    }

    /// After opening ERP data, nothing done to set it up is a step to undo: the widths fitted to
    /// its columns among them.
    pub fn erp_settle(&mut self) {
        if let Some(session) = self.session.as_mut() {
            if !session.bindings.is_empty() {
                session.history.forget();
            }
        }
    }

    /// The open workbook's binding, as JSON, or `null`.
    pub fn binding_json(&self) -> String {
        self.session
            .as_ref()
            .and_then(Session::binding_summary)
            .unwrap_or_else(|| "null".to_string())
    }

    /// The bound records' ids, ascending, as a refresh asks for them.
    pub fn binding_ids(&self) -> Vec<f64> {
        self.session
            .as_ref()
            .and_then(|session| session.bindings.0.first())
            .map(|binding| binding.ids().into_iter().map(|id| id as f64).collect())
            .unwrap_or_default()
    }

    /// For tests: turns the commands' own checks against bound tables off or on, leaving the
    /// content rule and the table and shape guards to refuse on their own.
    #[doc(hidden)]
    pub fn set_erp_precheck(&mut self, on: bool) {
        if let Some(session) = self.session.as_mut() {
            session.erp_precheck = on;
        }
    }

    /// For tests: a bound record's id and version by its key.
    pub fn bound_record(&self, key: &str) -> Option<(i64, String)> {
        let binding = self.session.as_ref()?.bindings.0.first()?;
        Some((binding.record(key)?.id, binding.version_text(key)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_named_after_their_entity() {
        assert_eq!(table_name_for("vendors"), "Vendors");
        assert_eq!(table_name_for("price-lists"), "Pricelists");
        assert_eq!(table_name_for("1x"), "Erp1x");
    }
}
