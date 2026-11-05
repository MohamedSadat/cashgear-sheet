//! Tables: making them, their options and totals, resizing, renaming and turning them back into
//! cells, and growing them as you type beside them. What each does was read off Excel 2016 over COM.

use cgs_calc::{CalcEngine, CalcValue, CellInput, CellRef, TableInfo};
use cgs_model::{AutoFilter, Rect, Table, TotalsFunction};
use serde::Deserialize;

use crate::command::{Applied, Area, Op};
use crate::edit::{EditCell, EditInput};
use crate::Session;

/// What can change about a table at once, from the Table Design tab.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableOptions {
    #[serde(default)]
    pub header_row: Option<bool>,
    #[serde(default)]
    pub totals_row: Option<bool>,
    #[serde(default)]
    pub first_column: Option<bool>,
    #[serde(default)]
    pub last_column: Option<bool>,
    #[serde(default)]
    pub row_stripes: Option<bool>,
    #[serde(default)]
    pub column_stripes: Option<bool>,
    #[serde(default)]
    pub filter_button: Option<bool>,
    /// A style's name, or an empty one for no style.
    #[serde(default)]
    pub style: Option<String>,
}

pub(crate) fn model_rect(rect: cgs_calc::Rect) -> Rect {
    Rect {
        top: rect.top,
        left: rect.left,
        bottom: rect.bottom,
        right: rect.right,
    }
}

fn meets(a: &cgs_calc::Rect, b: &Area) -> bool {
    a.top <= b.bottom && a.bottom >= b.top && a.left <= b.right && a.right >= b.left
}

/// A column's name as a structured reference writes it.
fn column_ref(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if matches!(ch, '[' | ']' | '#' | '\'') {
            out.push('\'');
        }
        out.push(ch);
    }
    out
}

/// What a totals cell holds for a function.
fn totals_formula(table: &str, column: &str, function: TotalsFunction) -> Option<String> {
    let code = function.subtotal()?;
    Some(format!("SUBTOTAL({code},{table}[{}])", column_ref(column)))
}

/// Whether a table's name is one Excel takes.
fn name_problem(name: &str) -> Option<&'static str> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Some("A table needs a name.");
    };
    if name.chars().count() > 255 {
        return Some("A table's name can be at most 255 characters.");
    }
    if !(first.is_alphabetic() || first == '_' || first == '\\') {
        return Some("A table's name must start with a letter, an underscore or a backslash.");
    }
    if !chars.all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '.' | '\\')) {
        return Some("A table's name can only hold letters, numbers, periods and underscores.");
    }
    let upper = name.to_ascii_uppercase();
    let looks_like_cell = cgs_refs::parse_a1(&upper).is_some()
        || matches!(upper.as_str(), "R" | "C")
        || upper.strip_prefix('R').is_some_and(|rest| {
            rest.split('C')
                .all(|part| part.chars().all(|c| c.is_ascii_digit()))
        });
    if looks_like_cell {
        return Some("A table's name can't look like a cell's address.");
    }
    None
}

impl Session {
    /// Brings the model's tables into step with the engine's after a change, keeping how each looks.
    pub(crate) fn sync_book_tables(&mut self) {
        let live: Vec<TableInfo> = self.calc.tables().to_vec();
        for info in &live {
            let found = self.book.tables.iter_mut().find(|table| {
                table.name.eq_ignore_ascii_case(&info.name)
                    && !self
                        .book
                        .sheets
                        .get(table.sheet as usize)
                        .is_some_and(|sheet| sheet.deleted)
            });
            let table = match found {
                Some(table) => table,
                None => {
                    let mut table = Table::new(
                        info.name.clone(),
                        info.sheet,
                        model_rect(info.range),
                        info.columns.clone(),
                    );
                    table.edited = false;
                    self.book.tables.push(table);
                    self.book.tables.last_mut().expect("just pushed")
                }
            };
            table.name = info.name.clone();
            table.sheet = info.sheet;
            table.range = model_rect(info.range);
            table.header_rows = u32::from(info.header_row);
            table.totals_rows = u32::from(info.totals_row);
            table.set_columns(info.columns.clone());
            if let Some(filter) = table.filter.as_mut() {
                filter.range = Rect {
                    bottom: table.range.bottom - table.totals_rows,
                    ..table.range
                };
            }
        }
    }

    pub(crate) fn table_named(&self, name: &str) -> Result<TableInfo, String> {
        self.calc
            .tables()
            .iter()
            .find(|table| table.name.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| format!("There is no table called {name}."))
    }

    fn book_table(&self, name: &str) -> Result<Table, String> {
        self.book
            .tables
            .iter()
            .find(|table| table.name.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| format!("There is no table called {name}."))
    }

    /// The table a cell is in.
    pub(crate) fn table_at(&self, sheet: u32, row: u32, col: u32) -> Option<&TableInfo> {
        self.calc
            .tables()
            .iter()
            .find(|table| table.sheet == sheet && table.range.contains(row, col))
    }

    fn next_table_name(&self) -> String {
        let taken = |name: &str| {
            self.calc
                .tables()
                .iter()
                .any(|table| table.name.eq_ignore_ascii_case(name))
                || self
                    .book
                    .names
                    .iter()
                    .any(|defined| defined.name.eq_ignore_ascii_case(name))
        };
        let mut number = 1;
        loop {
            let name = format!("Table{number}");
            if !taken(&name) {
                return name;
            }
            number += 1;
        }
    }

    fn next_table_id(&self) -> u32 {
        self.book
            .tables
            .iter()
            .map(|table| table.id)
            .max()
            .unwrap_or(0)
            + 1
    }

    fn cell_value(&self, sheet: u32, row: u32, col: u32) -> CalcValue {
        match self.calc.cell_input(CellRef { sheet, row, col }) {
            CellInput::Value(value) => value,
            CellInput::Formula { cached, .. } => cached.unwrap_or(CalcValue::Empty),
            CellInput::Clear | CellInput::Spill { .. } => CalcValue::Empty,
        }
    }

    fn holds_anything(&self, sheet: u32, row: u32, left: u32, right: u32) -> bool {
        (left..=right).any(|col| {
            !matches!(
                self.calc.cell_input(CellRef { sheet, row, col }),
                CellInput::Clear
            )
        })
    }

    /// Whether the first row of an area reads as headings: text over cells that aren't all text.
    pub(crate) fn guess_headers(&self, sheet: u32, area: Area) -> bool {
        let first_is_text = (area.left..=area.right).all(|col| {
            matches!(self.cell_value(sheet, area.top, col), CalcValue::Text(ref text) if !text.is_empty())
        });
        if !first_is_text {
            return false;
        }
        if area.rows() == 1 {
            return true;
        }
        (area.top + 1..=area.bottom).any(|row| {
            (area.left..=area.right)
                .any(|col| !matches!(self.cell_value(sheet, row, col), CalcValue::Text(_)))
        })
    }

    /// The block of filled cells around one, as Ctrl+T picks it from a single cell.
    pub(crate) fn current_region(&self, sheet: u32, row: u32, col: u32) -> Area {
        let mut area = Area::cell(row, col);
        let filled = |r: u32, c: u32| {
            r >= 1
                && c >= 1
                && r <= cgs_model::MAX_ROWS
                && c <= cgs_model::MAX_COLS
                && !matches!(
                    self.calc.cell_input(CellRef {
                        sheet,
                        row: r,
                        col: c
                    }),
                    CellInput::Clear
                )
        };
        loop {
            let mut grew = false;
            let (top, left, bottom, right) = (area.top, area.left, area.bottom, area.right);
            let edge_row = |r: u32| (left.saturating_sub(1)..=right + 1).any(|c| filled(r, c));
            let edge_col = |c: u32| (top.saturating_sub(1)..=bottom + 1).any(|r| filled(r, c));
            if top > 1 && edge_row(top - 1) {
                area.top -= 1;
                grew = true;
            }
            if bottom < cgs_model::MAX_ROWS && edge_row(bottom + 1) {
                area.bottom += 1;
                grew = true;
            }
            if left > 1 && edge_col(left - 1) {
                area.left -= 1;
                grew = true;
            }
            if right < cgs_model::MAX_COLS && edge_col(right + 1) {
                area.right += 1;
                grew = true;
            }
            if !grew || area.rows() * area.cols() > 4_000_000 {
                return area;
            }
        }
    }

    /// Headings for a new table's first row: text, all different, as Excel makes them.
    fn headings(&self, sheet: u32, row: u32, left: u32, right: u32) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for (index, col) in (left..=right).enumerate() {
            let text = match self.cell_value(sheet, row, col) {
                CalcValue::Text(text) => text,
                CalcValue::Number(number) => cgs_xlsx::patch_sheet::number_text(number),
                CalcValue::Bool(flag) => if flag { "TRUE" } else { "FALSE" }.to_string(),
                CalcValue::Error(text) => text,
                CalcValue::Empty => String::new(),
            };
            let taken = |name: &str, names: &[String]| {
                names.iter().any(|other| other.eq_ignore_ascii_case(name))
            };
            let name = if text.is_empty() {
                let mut number = index + 1;
                while taken(&format!("Column{number}"), &names) {
                    number += 1;
                }
                format!("Column{number}")
            } else if taken(&text, &names) {
                let mut number = 2;
                while taken(&format!("{text}{number}"), &names) {
                    number += 1;
                }
                format!("{text}{number}")
            } else {
                text
            };
            names.push(name);
        }
        names
    }

    fn table_lists(&self) -> (Vec<TableInfo>, Vec<Table>) {
        (self.calc.tables().to_vec(), self.book.tables.clone())
    }

    /// Makes a table of an area. Without headers, a row of them goes on top and the data moves down.
    pub(crate) fn create_table_command(
        &mut self,
        sheet: u32,
        area: Area,
        headers: Option<bool>,
        style: Option<String>,
    ) -> Result<Applied, String> {
        let info = self.sheet_for_edit(sheet)?;
        if info.kind != cgs_model::SheetKind::Worksheet || !info.loaded {
            return Err("This sheet has no cells to make a table of.".to_string());
        }
        let area = if area.rows() == 1 && area.cols() == 1 {
            self.current_region(sheet, area.top, area.left)
        } else {
            area
        };
        if self
            .calc
            .tables()
            .iter()
            .any(|table| table.sheet == sheet && meets(&table.range, &area))
        {
            return Err("A table can't overlap another table.".to_string());
        }
        if self.calc.spill_areas().iter().any(|spill| {
            spill.sheet == sheet
                && meets(&spill.rect, &area)
                && (spill.rect.top, spill.rect.left) != (spill.rect.bottom, spill.rect.right)
        }) {
            return Err("Multi-cell array formulas aren't allowed in tables.".to_string());
        }
        let headers = headers.unwrap_or_else(|| self.guess_headers(sheet, area));
        let below_taken = !headers
            && area.bottom < cgs_model::MAX_ROWS
            && self.holds_anything(sheet, area.bottom + 1, area.left, area.right);
        if !headers && area.bottom >= cgs_model::MAX_ROWS {
            return Err("There's no room for the table's headings.".to_string());
        }
        let style = style.filter(|name| !name.is_empty());
        let name = self.next_table_name();
        let id = self.next_table_id();
        self.perform_steps(
            "making a table",
            Some((sheet, area)),
            move |session, step| {
                match (step, below_taken) {
                    (0, true) => session
                        .shift_op(
                            sheet,
                            Area {
                                top: area.bottom + 1,
                                bottom: area.bottom + 1,
                                ..area
                            },
                            true,
                            false,
                        )
                        .map(Some),
                    (0, false) | (1, true) => {
                        let mut list = Vec::new();
                        // Merged cells in a table come apart.
                        let merges = &session.book.sheets[sheet as usize].merges;
                        let kept: Vec<Rect> = merges
                            .iter()
                            .filter(|merge| {
                                !(merge.top <= area.bottom + 1
                                    && merge.bottom >= area.top
                                    && merge.left <= area.right
                                    && merge.right >= area.left)
                            })
                            .copied()
                            .collect();
                        if kept.len() != merges.len() {
                            list.push(Op::Merges {
                                sheet,
                                merges: kept,
                            });
                        }
                        let range = Area {
                            bottom: area.bottom + u32::from(!headers),
                            ..area
                        };
                        let mut cells = Vec::new();
                        let columns = if headers {
                            session.headings(sheet, area.top, area.left, area.right)
                        } else {
                            cells =
                                session.move_edits(sheet, area, sheet, area.top + 1, area.left)?;
                            cells.retain(|cell| cell.row != area.top);
                            (1..=area.cols()).map(|n| format!("Column{n}")).collect()
                        };
                        // Headings are text, whatever they held.
                        for (index, column) in columns.iter().enumerate() {
                            cells.push(EditCell {
                                sheet,
                                row: area.top,
                                col: area.left + index as u32,
                                input: Some(EditInput::Value(CalcValue::Text(column.clone()))),
                                style: None,
                            });
                        }
                        list.push(Op::Cells(cells));
                        let (mut calc, mut book) = session.table_lists();
                        calc.push(TableInfo {
                            name: name.clone(),
                            sheet,
                            range: cgs_calc::Rect {
                                top: range.top,
                                left: range.left,
                                bottom: range.bottom,
                                right: range.right,
                            },
                            header_row: true,
                            totals_row: false,
                            columns: columns.clone(),
                        });
                        let rect = Rect {
                            top: range.top,
                            left: range.left,
                            bottom: range.bottom,
                            right: range.right,
                        };
                        let mut table = Table::new(name.clone(), sheet, rect, columns);
                        table.id = id;
                        if let Some(style) = &style {
                            table.style.name = Some(style.clone());
                        }
                        table.filter = Some(AutoFilter::new(rect));
                        book.push(table);
                        list.push(Op::BookTables(book));
                        list.push(Op::Tables(calc));
                        Ok(Some(Op::Many(list)))
                    }
                    _ => Ok(None),
                }
            },
        )
    }

    /// Changes a table's options: its heading and totals rows, which parts its style picks out, its
    /// filter buttons and its style.
    pub(crate) fn table_options_command(
        &mut self,
        name: &str,
        options: TableOptions,
    ) -> Result<Applied, String> {
        let info = self.table_named(name)?;
        let mut look = self.book_table(name)?;
        let sheet = info.sheet;
        let mut changed = false;
        let mut set = |flag: Option<bool>, field: &mut bool| {
            if let Some(flag) = flag {
                if *field != flag {
                    *field = flag;
                    changed = true;
                }
            }
        };
        set(options.first_column, &mut look.style.first_column);
        set(options.last_column, &mut look.style.last_column);
        set(options.row_stripes, &mut look.style.row_stripes);
        set(options.column_stripes, &mut look.style.column_stripes);
        if let Some(style) = &options.style {
            let style = (!style.is_empty()).then(|| style.clone());
            if look.style.name != style {
                look.style.name = style;
                changed = true;
            }
        }
        if let Some(buttons) = options.filter_button {
            if buttons != look.filter.is_some() {
                look.filter = if buttons {
                    Some(AutoFilter::new(Rect {
                        bottom: look.range.bottom - look.totals_rows,
                        ..look.range
                    }))
                } else {
                    None
                };
                changed = true;
            }
        }
        let area = Area {
            top: info.range.top,
            left: info.range.left,
            bottom: info.range.bottom,
            right: info.range.right,
        };
        if let Some(totals) = options
            .totals_row
            .filter(|&wanted| wanted != info.totals_row)
        {
            return self.totals_row_command(info, look, totals);
        }
        if let Some(headers) = options
            .header_row
            .filter(|&wanted| wanted != info.header_row)
        {
            return self.header_row_command(info, look, headers);
        }
        if !changed {
            return Ok(Applied {
                selection: Some((sheet, area)),
                replaced: 0,
            });
        }
        look.edited = true;
        let book = self.with_book_table(look);
        self.perform("table options", Op::BookTables(book), Some((sheet, area)))
    }

    /// The model's tables with one replaced.
    fn with_book_table(&self, look: Table) -> Vec<Table> {
        let mut book = self.book.tables.clone();
        match book
            .iter_mut()
            .find(|table| table.name.eq_ignore_ascii_case(&look.name) && table.sheet == look.sheet)
        {
            Some(table) => *table = look,
            None => book.push(look),
        }
        book
    }

    fn with_calc_table(&self, info: TableInfo, old_name: &str) -> Vec<TableInfo> {
        let mut calc = self.calc.tables().to_vec();
        match calc
            .iter_mut()
            .find(|table| table.name.eq_ignore_ascii_case(old_name))
        {
            Some(table) => *table = info,
            None => calc.push(info),
        }
        calc
    }

    fn totals_row_command(
        &mut self,
        info: TableInfo,
        mut look: Table,
        on: bool,
    ) -> Result<Applied, String> {
        let sheet = info.sheet;
        let t = info.range;
        let band = Area {
            top: t.bottom + 1,
            left: t.left,
            bottom: t.bottom + 1,
            right: t.right,
        };
        look.edited = true;
        if on {
            if t.bottom >= cgs_model::MAX_ROWS {
                return Err("There's no room for a totals row.".to_string());
            }
            let below_taken = self.holds_anything(sheet, t.bottom + 1, t.left, t.right);
            // A column that remembers a function keeps it. Otherwise the first gets a label and the
            // last a sum, or a count for text.
            let remembered = look
                .column_info
                .iter()
                .any(|column| column.totals_function.is_some());
            if !remembered {
                let last = info.columns.len() - 1;
                let numbers = (t.top + u32::from(info.header_row)..=t.bottom).any(|row| {
                    matches!(self.cell_value(sheet, row, t.right), CalcValue::Number(_))
                });
                if let Some(column) = look.column_info.get_mut(last) {
                    column.totals_function = Some(if numbers {
                        TotalsFunction::Sum
                    } else {
                        TotalsFunction::Count
                    });
                }
                if last > 0 {
                    if let Some(column) = look.column_info.get_mut(0) {
                        if column.totals_label.is_none() {
                            column.totals_label = Some("Total".to_string());
                        }
                    }
                }
            }
            return self.perform_steps(
                "totals row",
                Some((sheet, band)),
                move |session, step| match (step, below_taken) {
                    (0, true) => session.shift_op(sheet, band, true, false).map(Some),
                    (0, false) | (1, true) => {
                        let row = t.bottom + 1;
                        let mut cells = Vec::new();
                        for (index, column) in info.columns.iter().enumerate() {
                            let extra = look.column_info.get(index).cloned().unwrap_or_default();
                            let input = match (extra.totals_function, &extra.totals_label) {
                                (Some(TotalsFunction::Custom), _) => extra
                                    .totals_formula
                                    .clone()
                                    .map(EditInput::Formula)
                                    .unwrap_or(EditInput::Clear),
                                (Some(function), _) => {
                                    match totals_formula(&info.name, column, function) {
                                        Some(text) => EditInput::Formula(text),
                                        None => EditInput::Clear,
                                    }
                                }
                                (None, Some(label)) => {
                                    EditInput::Value(CalcValue::Text(label.clone()))
                                }
                                (None, None) => EditInput::Clear,
                            };
                            cells.push(EditCell {
                                sheet,
                                row,
                                col: t.left + index as u32,
                                input: Some(input),
                                style: None,
                            });
                        }
                        let mut next = info.clone();
                        next.totals_row = true;
                        next.range.bottom = row;
                        let calc = session.with_calc_table(next, &info.name);
                        let book = session.with_book_table(look.clone());
                        Ok(Some(Op::Many(vec![
                            Op::BookTables(book),
                            Op::Tables(calc),
                            Op::Cells(cells),
                        ])))
                    }
                    _ => Ok(None),
                },
            );
        }
        // Off: the row goes, and what's below moves up.
        let row = t.bottom;
        self.perform_steps(
            "totals row",
            Some((sheet, band)),
            move |session, step| match step {
                0 => {
                    let mut next = info.clone();
                    next.totals_row = false;
                    next.range.bottom = row - 1;
                    let calc = session.with_calc_table(next, &info.name);
                    let book = session.with_book_table(look.clone());
                    Ok(Some(Op::Many(vec![Op::BookTables(book), Op::Tables(calc)])))
                }
                1 => session
                    .shift_op(
                        sheet,
                        Area {
                            top: row,
                            left: t.left,
                            bottom: row,
                            right: t.right,
                        },
                        true,
                        true,
                    )
                    .map(Some),
                _ => Ok(None),
            },
        )
    }

    fn header_row_command(
        &mut self,
        info: TableInfo,
        mut look: Table,
        on: bool,
    ) -> Result<Applied, String> {
        let sheet = info.sheet;
        let t = info.range;
        look.edited = true;
        let area = Area {
            top: t.top,
            left: t.left,
            bottom: t.bottom,
            right: t.right,
        };
        if !on {
            if t.top == t.bottom - u32::from(info.totals_row) {
                return Err("A table needs a row of data.".to_string());
            }
            return self.perform_steps("header row", Some((sheet, area)), move |session, step| {
                match step {
                    0 => {
                        let mut next = info.clone();
                        next.header_row = false;
                        next.range.top = t.top + 1;
                        let calc = session.with_calc_table(next, &info.name);
                        let book = session.with_book_table(look.clone());
                        Ok(Some(Op::Many(vec![Op::BookTables(book), Op::Tables(calc)])))
                    }
                    1 => Ok(Some(Op::Cells(
                        (t.left..=t.right)
                            .map(|col| EditCell {
                                sheet,
                                row: t.top,
                                col,
                                input: Some(EditInput::Clear),
                                style: None,
                            })
                            .collect(),
                    ))),
                    _ => Ok(None),
                }
            });
        }
        // On: the row above if it's free, or else a new one, which moves the table down.
        let above_free = t.top > 1 && !self.holds_anything(sheet, t.top - 1, t.left, t.right);
        let band = Area {
            top: t.top,
            left: t.left,
            bottom: t.top,
            right: t.right,
        };
        self.perform_steps(
            "header row",
            Some((sheet, area)),
            move |session, step| match (step, above_free) {
                (0, false) => session.shift_op(sheet, band, true, false).map(Some),
                (0, true) | (1, false) => {
                    let now = session.table_named(&info.name)?;
                    let row = now.range.top - 1;
                    let cells = now
                        .columns
                        .iter()
                        .enumerate()
                        .map(|(index, column)| EditCell {
                            sheet,
                            row,
                            col: now.range.left + index as u32,
                            input: Some(EditInput::Value(CalcValue::Text(column.clone()))),
                            style: None,
                        })
                        .collect();
                    let mut next = now.clone();
                    next.header_row = true;
                    next.range.top = row;
                    let calc = session.with_calc_table(next, &info.name);
                    let mut look = look.clone();
                    look.range.top = row;
                    let book = session.with_book_table(look);
                    Ok(Some(Op::Many(vec![
                        Op::Cells(cells),
                        Op::BookTables(book),
                        Op::Tables(calc),
                    ])))
                }
                _ => Ok(None),
            },
        )
    }

    /// Picks what a totals cell works out, or nothing.
    pub(crate) fn totals_function_command(
        &mut self,
        name: &str,
        column: &str,
        function: Option<String>,
    ) -> Result<Applied, String> {
        let info = self.table_named(name)?;
        if !info.totals_row {
            return Err("This table has no totals row.".to_string());
        }
        let index = info
            .columns
            .iter()
            .position(|other| other.eq_ignore_ascii_case(column))
            .ok_or_else(|| format!("The table has no column called {column}."))?;
        let function = match function.as_deref() {
            None | Some("") | Some("none") => None,
            Some(text) => Some(
                TotalsFunction::from_xml(text)
                    .filter(|function| *function != TotalsFunction::Custom)
                    .ok_or_else(|| format!("{text} isn't a totals function."))?,
            ),
        };
        let mut look = self.book_table(name)?;
        look.edited = true;
        if let Some(extra) = look.column_info.get_mut(index) {
            extra.totals_function = function;
            extra.totals_label = None;
            extra.totals_formula = None;
        }
        let input = match function.and_then(|function| totals_formula(&info.name, column, function))
        {
            Some(text) => EditInput::Formula(text),
            None => EditInput::Clear,
        };
        let cell = EditCell {
            sheet: info.sheet,
            row: info.range.bottom,
            col: info.range.left + index as u32,
            input: Some(input),
            style: None,
        };
        let at = Area::cell(cell.row, cell.col);
        let book = self.with_book_table(look);
        self.perform(
            "totals",
            Op::Many(vec![Op::BookTables(book), Op::Cells(vec![cell])]),
            Some((info.sheet, at)),
        )
    }

    pub(crate) fn rename_table_command(
        &mut self,
        name: &str,
        new: &str,
    ) -> Result<Applied, String> {
        let info = self.table_named(name)?;
        let new = new.trim();
        if let Some(problem) = name_problem(new) {
            return Err(problem.to_string());
        }
        let clash = self.calc.tables().iter().any(|table| {
            !table.name.eq_ignore_ascii_case(name) && table.name.eq_ignore_ascii_case(new)
        }) || self
            .book
            .names
            .iter()
            .any(|defined| defined.name.eq_ignore_ascii_case(new));
        if clash {
            return Err("That name is already taken.".to_string());
        }
        if new == info.name {
            return Ok(Applied::default());
        }
        let mut rewrites = Vec::new();
        for (at, text) in self.calc.all_formulas().map_err(|error| error.0)? {
            if let std::borrow::Cow::Owned(next) = cgs_refs::rename_table(&text, &info.name, new) {
                let input = self.rewritten(at, next);
                rewrites.push(EditCell {
                    sheet: at.sheet,
                    row: at.row,
                    col: at.col,
                    input: Some(input),
                    style: None,
                });
            }
        }
        let mut names = self.calc.names();
        let before = names.clone();
        let mut names_changed = false;
        for entry in &mut names {
            if let std::borrow::Cow::Owned(next) =
                cgs_refs::rename_table(&entry.formula, &info.name, new)
            {
                entry.formula = next;
                names_changed = true;
            }
        }
        let mut next = info.clone();
        next.name = new.to_string();
        let calc = self.with_calc_table(next, &info.name);
        let mut look = self.book_table(name)?;
        let old = look.name.clone();
        look.name = new.to_string();
        look.edited = true;
        let mut book = self.book.tables.clone();
        if let Some(table) = book.iter_mut().find(|table| table.name == old) {
            *table = look;
        }
        // The new name first, so that formulas find it.
        let mut list = vec![Op::BookTables(book), Op::Tables(calc), Op::Cells(rewrites)];
        if names_changed {
            list.push(Op::NamesTo { names, before });
        }
        self.perform("renaming a table", Op::Many(list), None)
    }

    /// Gives a table a new area. Its headings stay in their row, and it keeps a row of data.
    pub(crate) fn resize_table_command(
        &mut self,
        name: &str,
        area: Area,
    ) -> Result<Applied, String> {
        let info = self.table_named(name)?;
        let t = info.range;
        let sheet = info.sheet;
        let header_moves = info.header_row && area.top != t.top;
        let data = area.rows() > u32::from(info.header_row) + u32::from(info.totals_row);
        if header_moves || !data || !meets(&t, &area) {
            return Err("The headers must remain in the same row, and the new range must overlap the table and keep a row of data.".to_string());
        }
        if self.calc.tables().iter().any(|table| {
            table.sheet == sheet
                && !table.name.eq_ignore_ascii_case(name)
                && meets(&table.range, &area)
        }) {
            return Err("A table can't overlap another table.".to_string());
        }
        // Columns stay by position; new ones are named from their heading cells.
        let mut columns = Vec::new();
        let mut headers = Vec::new();
        let mut lost = Vec::new();
        for col in area.left..=area.right {
            if col >= t.left && col <= t.right {
                columns.push(info.columns[(col - t.left) as usize].clone());
                continue;
            }
            let text = if info.header_row {
                match self.cell_value(sheet, area.top, col) {
                    CalcValue::Text(text) => text,
                    CalcValue::Number(number) => cgs_xlsx::patch_sheet::number_text(number),
                    _ => String::new(),
                }
            } else {
                String::new()
            };
            let name = if text.is_empty()
                || columns
                    .iter()
                    .any(|other: &String| other.eq_ignore_ascii_case(&text))
            {
                cgs_calc::new_column_name(&columns)
            } else {
                text
            };
            if info.header_row {
                headers.push(EditCell {
                    sheet,
                    row: area.top,
                    col,
                    input: Some(EditInput::Value(CalcValue::Text(name.clone()))),
                    style: None,
                });
            }
            columns.push(name);
        }
        for (index, column) in info.columns.iter().enumerate() {
            let col = t.left + index as u32;
            if col < area.left || col > area.right {
                lost.push(column.clone());
            }
        }
        let mut rewrites = Vec::new();
        if !lost.is_empty() {
            for (at, text) in self.calc.all_formulas().map_err(|error| error.0)? {
                if let std::borrow::Cow::Owned(next) =
                    cgs_refs::delete_table_columns(&text, &info.name, &lost)
                {
                    let input = self.rewritten(at, next);
                    rewrites.push(EditCell {
                        sheet: at.sheet,
                        row: at.row,
                        col: at.col,
                        input: Some(input),
                        style: None,
                    });
                }
            }
        }
        let mut next = info.clone();
        next.range = cgs_calc::Rect {
            top: area.top,
            left: area.left,
            bottom: area.bottom,
            right: area.right,
        };
        next.columns = columns;
        let calc = self.with_calc_table(next, &info.name);
        let mut look = self.book_table(name)?;
        look.edited = true;
        let book = self.with_book_table(look);
        rewrites.extend(headers);
        self.perform(
            "resizing a table",
            Op::Many(vec![
                Op::Cells(rewrites),
                Op::BookTables(book),
                Op::Tables(calc),
            ]),
            Some((sheet, area)),
        )
    }

    /// Turns a table back into cells. References to it become the cells they meant.
    pub(crate) fn convert_to_range_command(&mut self, name: &str) -> Result<Applied, String> {
        let info = self.table_named(name)?;
        let sheet_name = cgs_refs::quote_sheet(&self.book.sheets[info.sheet as usize].name);
        let place = cgs_refs::TablePlace {
            name: &info.name,
            columns: &info.columns,
            top: info.range.top,
            left: info.range.left,
            bottom: info.range.bottom,
            right: info.range.right,
            header_row: info.header_row,
            totals_row: info.totals_row,
            sheet: &sheet_name,
        };
        let mut rewrites = Vec::new();
        for (at, text) in self.calc.all_formulas().map_err(|error| error.0)? {
            let home = (at.sheet == info.sheet && info.range.contains(at.row, at.col))
                .then_some(info.name.as_str());
            if let std::borrow::Cow::Owned(next) =
                cgs_refs::table_to_cells(&text, &place, at.row, home)
            {
                let input = self.rewritten(at, next);
                rewrites.push(EditCell {
                    sheet: at.sheet,
                    row: at.row,
                    col: at.col,
                    input: Some(input),
                    style: None,
                });
            }
        }
        let mut names = self.calc.names();
        let before = names.clone();
        let mut names_changed = false;
        for entry in &mut names {
            if let std::borrow::Cow::Owned(next) =
                cgs_refs::table_to_cells(&entry.formula, &place, 1, None)
            {
                entry.formula = next;
                names_changed = true;
            }
        }
        let calc: Vec<TableInfo> = self
            .calc
            .tables()
            .iter()
            .filter(|table| !table.name.eq_ignore_ascii_case(name))
            .cloned()
            .collect();
        let book: Vec<Table> = self
            .book
            .tables
            .iter()
            .filter(|table| !(table.name.eq_ignore_ascii_case(name) && table.sheet == info.sheet))
            .cloned()
            .collect();
        let area = Area {
            top: info.range.top,
            left: info.range.left,
            bottom: info.range.bottom,
            right: info.range.right,
        };
        let mut list = vec![Op::Cells(rewrites)];
        if names_changed {
            list.push(Op::NamesTo { names, before });
        }
        if let Some(looks) = self.table_look_formats(name)? {
            list.push(looks);
        }
        list.push(Op::Tables(calc));
        list.push(Op::BookTables(book));
        self.perform(
            "converting to a range",
            Op::Many(list),
            Some((info.sheet, area)),
        )
    }

    /// What typing these cells does to tables beside them: a table grows by the row below or the
    /// column to its right when the rest of that row or column is empty, and a formula typed in an
    /// empty column of a table fills the column.
    pub(crate) fn table_growth(&self, sheet: u32, edits: &[EditCell]) -> Option<Vec<Op>> {
        if self.calc.tables().is_empty() {
            return None;
        }
        let typed: Vec<&EditCell> = edits
            .iter()
            .filter(|edit| {
                matches!(&edit.input, Some(EditInput::Value(value)) if *value != CalcValue::Empty)
                    || matches!(edit.input, Some(EditInput::Formula(_)))
            })
            .collect();
        if typed.is_empty() {
            return None;
        }
        let is_typed =
            |row: u32, col: u32| typed.iter().any(|edit| edit.row == row && edit.col == col);
        for info in self
            .calc
            .tables()
            .iter()
            .filter(|table| table.sheet == sheet)
        {
            let t = info.range;
            // A calculated column: a formula in an empty column of the table fills it.
            if typed.len() == 1 && t.contains(typed[0].row, typed[0].col) {
                let edit = typed[0];
                let Some(EditInput::Formula(text)) = &edit.input else {
                    continue;
                };
                let (first, last) = (
                    t.top + u32::from(info.header_row),
                    t.bottom - u32::from(info.totals_row),
                );
                if edit.row < first || edit.row > last {
                    continue;
                }
                let empty = (first..=last).filter(|&row| row != edit.row).all(|row| {
                    matches!(
                        self.calc.cell_input(CellRef {
                            sheet,
                            row,
                            col: edit.col
                        }),
                        CellInput::Clear
                    )
                });
                if !empty || first == last {
                    continue;
                }
                let mut cells = Vec::new();
                for row in first..=last {
                    if row == edit.row {
                        continue;
                    }
                    cells.push(EditCell {
                        sheet,
                        row,
                        col: edit.col,
                        input: Some(EditInput::Formula(cgs_refs::moved_formula(
                            text,
                            i64::from(row) - i64::from(edit.row),
                            0,
                        ))),
                        style: None,
                    });
                }
                let mut look = self.book_table(&info.name).ok()?;
                let index = (edit.col - t.left) as usize;
                if let Some(extra) = look.column_info.get_mut(index) {
                    let at_first =
                        cgs_refs::moved_formula(text, i64::from(first) - i64::from(edit.row), 0);
                    let file =
                        cgs_refs::table_refs_for_file(&at_first, Some(&info.name)).into_owned();
                    extra.calculated = Some(cgs_refs::formula_for_file(&file, false));
                }
                look.edited = true;
                return Some(vec![
                    Op::Cells(cells),
                    Op::BookTables(self.with_book_table(look)),
                ]);
            }
            if typed.iter().any(|edit| t.contains(edit.row, edit.col)) {
                continue;
            }
            // The row below, when the table has no totals.
            let below = t.bottom + 1;
            if !info.totals_row
                && below <= cgs_model::MAX_ROWS
                && typed
                    .iter()
                    .all(|edit| edit.row == below && edit.col >= t.left && edit.col <= t.right)
            {
                let rest_empty = (t.left..=t.right).all(|col| {
                    is_typed(below, col)
                        || matches!(
                            self.calc.cell_input(CellRef {
                                sheet,
                                row: below,
                                col
                            }),
                            CellInput::Clear
                        )
                });
                if !rest_empty {
                    continue;
                }
                let mut next = info.clone();
                next.range.bottom = below;
                let mut cells = Vec::new();
                if let Ok(look) = self.book_table(&info.name) {
                    let first = t.top + u32::from(info.header_row);
                    for (index, extra) in look.column_info.iter().enumerate() {
                        let col = t.left + index as u32;
                        let Some(formula) = &extra.calculated else {
                            continue;
                        };
                        if is_typed(below, col) {
                            continue;
                        }
                        cells.push(EditCell {
                            sheet,
                            row: below,
                            col,
                            input: Some(EditInput::Formula(cgs_refs::moved_formula(
                                formula,
                                i64::from(below) - i64::from(first),
                                0,
                            ))),
                            style: None,
                        });
                    }
                }
                let calc = self.with_calc_table(next, &info.name);
                return Some(vec![Op::Tables(calc), Op::Cells(cells)]);
            }
            // The column to the right.
            let right = t.right + 1;
            if right <= cgs_model::MAX_COLS
                && typed
                    .iter()
                    .all(|edit| edit.col == right && edit.row >= t.top && edit.row <= t.bottom)
            {
                let rest_empty = (t.top..=t.bottom).all(|row| {
                    is_typed(row, right)
                        || matches!(
                            self.calc.cell_input(CellRef {
                                sheet,
                                row,
                                col: right
                            }),
                            CellInput::Clear
                        )
                });
                if !rest_empty {
                    continue;
                }
                let header = info.header_row.then_some(t.top);
                let typed_header = header.and_then(|row| {
                    typed
                        .iter()
                        .find(|edit| edit.row == row)
                        .and_then(|edit| match &edit.input {
                            Some(EditInput::Value(CalcValue::Text(text))) => Some(text.clone()),
                            Some(EditInput::Value(CalcValue::Number(number))) => {
                                Some(cgs_xlsx::patch_sheet::number_text(*number))
                            }
                            _ => None,
                        })
                });
                let name = match typed_header {
                    Some(text)
                        if !info
                            .columns
                            .iter()
                            .any(|other| other.eq_ignore_ascii_case(&text)) =>
                    {
                        text
                    }
                    _ => cgs_calc::new_column_name(&info.columns),
                };
                let mut next = info.clone();
                next.range.right = right;
                next.columns.push(name.clone());
                let mut list = vec![Op::Tables(self.with_calc_table(next, &info.name))];
                if let Some(row) = header {
                    list.push(Op::Cells(vec![EditCell {
                        sheet,
                        row,
                        col: right,
                        input: Some(EditInput::Value(CalcValue::Text(name))),
                        style: None,
                    }]));
                }
                // A lone formula in the new column fills it.
                let data: Vec<&&EditCell> = typed
                    .iter()
                    .filter(|edit| Some(edit.row) != header)
                    .collect();
                if let [edit] = data.as_slice() {
                    if let Some(EditInput::Formula(text)) = &edit.input {
                        let (first, last) = (
                            t.top + u32::from(info.header_row),
                            t.bottom - u32::from(info.totals_row),
                        );
                        let cells = (first..=last)
                            .filter(|&row| row != edit.row)
                            .map(|row| EditCell {
                                sheet,
                                row,
                                col: right,
                                input: Some(EditInput::Formula(cgs_refs::moved_formula(
                                    text,
                                    i64::from(row) - i64::from(edit.row),
                                    0,
                                ))),
                                style: None,
                            })
                            .collect();
                        list.push(Op::Cells(cells));
                    }
                }
                return Some(list);
            }
        }
        None
    }

    /// The direct formats that keep a table's look once it is cells again. None until table styles
    /// are drawn.
    fn table_look_formats(&self, _name: &str) -> Result<Option<Op>, String> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgs_model::TableStyle;

    #[test]
    fn table_names_follow_excels_rules() {
        assert!(name_problem("Sales").is_none());
        assert!(name_problem("_x.y").is_none());
        assert!(name_problem("A1").is_some());
        assert!(name_problem("R1C1").is_some());
        assert!(name_problem("C").is_some());
        assert!(name_problem("1st").is_some());
        assert!(name_problem("a b").is_some());
        assert!(name_problem("").is_some());
    }

    #[test]
    fn totals_use_subtotal() {
        assert_eq!(
            totals_formula("T", "Cost", TotalsFunction::Sum).as_deref(),
            Some("SUBTOTAL(109,T[Cost])")
        );
        assert_eq!(
            totals_formula("T", "a[b", TotalsFunction::Average).as_deref(),
            Some("SUBTOTAL(101,T[a'[b])")
        );
    }

    #[test]
    fn a_new_table_looks_as_excels_do() {
        let style = TableStyle::default();
        assert_eq!(style.name.as_deref(), Some("TableStyleMedium2"));
        assert!(style.row_stripes);
    }
}
