//! Questions the UI asks for the data tools, as JSON in and JSON out: `{"kind": ..., ...}`. Each
//! answer is built from the model and the engine, and nothing here changes the workbook.

use cgs_model::{Table, TotalsFunction};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::command::Area;
use crate::{Engine, Session};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Question {
    /// The table a cell is in, with how it looks.
    TableAt { sheet: u32, row: u32, col: u32 },
    /// What Ctrl+T would make a table of, and whether its first row reads as headings.
    GuessTable { sheet: u32, area: Area },
    /// Every table, for the name box and the name manager.
    Tables,
    /// What a filter's dropdown lists for one column.
    FilterList {
        sheet: u32,
        #[serde(default)]
        table: Option<String>,
        col: u32,
    },
    /// The filters on a sheet, its own and its tables', for drawing their buttons.
    Filters { sheet: u32 },
    /// A sheet's conditional formats, in priority order, for the rules manager.
    CfRules { sheet: u32 },
    /// A cell's validation rule, with a list's items.
    ValidationAt { sheet: u32, row: u32, col: u32 },
    /// Cells their rules don't allow, at most 255.
    InvalidCells { sheet: u32 },
    /// The names the Name Manager lists.
    Names,
    /// Where the selection may go on a protected sheet that keeps locked or any cells from being
    /// selected: this cell if it may be, or else the next that may along `rows` and `cols`, read
    /// on to the next rows when going right. Nothing when there is none.
    NextSelectable {
        sheet: u32,
        row: u32,
        col: u32,
        #[serde(default)]
        rows: i32,
        #[serde(default)]
        cols: i32,
    },
    /// The fills, text colours and icons a column shows from `top` to `bottom`, for the Sort
    /// dialog's colours and icons.
    ColumnLooks {
        sheet: u32,
        col: u32,
        top: u32,
        bottom: u32,
    },
    /// A cell's rich runs, for the cell editor.
    RichAt { sheet: u32, row: u32, col: u32 },
    /// Circular References: the cells on loops, as `[sheet, row, col]`, or nothing when the
    /// workbook is too big to look through.
    Circular,
    /// The errors a cell shows a green triangle for, as Error Checking names them.
    CellErrors { sheet: u32, row: u32, col: u32 },
    /// Error Checking's next cell with an error after this one, in reading order, round the
    /// sheet's used range, or before it with `back`; nothing when there is none.
    NextError {
        sheet: u32,
        row: u32,
        col: u32,
        #[serde(default)]
        back: bool,
    },
    /// A cell's note.
    NoteAt { sheet: u32, row: u32, col: u32 },
    /// A cell's hyperlink.
    LinkAt { sheet: u32, row: u32, col: u32 },
    /// A sheet's notes, in order.
    Notes { sheet: u32 },
    /// The Cell Styles gallery.
    CellStyles,
    /// A named style's format and what it includes, for Modify Cell Style.
    StyleFormat { name: String },
    /// The GETPIVOTDATA formula a click on a pivot table's value writes, from a formula on `from`.
    PivotDataFormula {
        sheet: u32,
        row: u32,
        col: u32,
        from: u32,
    },
    /// What conditional formats make of each cell of an area, for checking against Excel: its
    /// fill and font colours, whether bold, and its icon.
    CfLook { sheet: u32, area: Area },
    /// The table styles gallery: the file's own, then Excel's, each with a small preview drawn
    /// with the table's options.
    TableStyles {
        #[serde(default)]
        options: Option<PreviewOptions>,
    },
    /// A sheet's page setup, print area and titles, for the Page Layout tab and Page Setup.
    PageSetup { sheet: u32 },
    /// ERP data's changeset: each changed record's fields from and to, and the formulas and
    /// cells local rules refuse, apart. With `visible`, only rows no filter hides.
    ErpChanges {
        #[serde(default)]
        visible: bool,
    },
    /// Every bound field the Review dialog lists: changed, refused, in conflict or a formula.
    ErpReview,
    /// A bound cell's field, state, original and new values and broken rule, for hover details.
    ErpCell { sheet: u32, row: u32, col: u32 },
    /// The next bound cell a local rule refuses, round the table, or before with `back`. With
    /// `conflict`, the next one in conflict with the ERP (Next Conflict).
    ErpNextError {
        sheet: u32,
        row: u32,
        col: u32,
        #[serde(default)]
        back: bool,
        #[serde(default)]
        conflict: bool,
    },
    /// Every clashing field of every record in conflict, for the Resolve Conflicts dialog.
    ErpConflicts,
    /// Where a sheet's pages break and what each shows, for printing and Normal view.
    Pages {
        sheet: u32,
        #[serde(default)]
        request: crate::print::PrintRequest,
    },
}

/// Which parts of a table a gallery preview picks out, as the Table Design tab has them.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct PreviewOptions {
    header_row: Option<bool>,
    totals_row: bool,
    first_column: bool,
    last_column: bool,
    row_stripes: Option<bool>,
    column_stripes: bool,
}

fn area_json(top: u32, left: u32, bottom: u32, right: u32) -> Value {
    json!({ "top": top, "left": left, "bottom": bottom, "right": right })
}

fn table_json(table: &Table) -> Value {
    let columns: Vec<Value> = table
        .columns
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let info = table.column_info.get(index).cloned().unwrap_or_default();
            json!({
                "name": name,
                "totals": info.totals_function.map(TotalsFunction::xml),
                "calculated": info.calculated.is_some(),
            })
        })
        .collect();
    json!({
        "name": table.name,
        "sheet": table.sheet,
        "area": area_json(table.range.top, table.range.left, table.range.bottom, table.range.right),
        "headerRow": table.header_rows > 0,
        "totalsRow": table.totals_rows > 0,
        "style": table.style.name,
        "firstColumn": table.style.first_column,
        "lastColumn": table.style.last_column,
        "rowStripes": table.style.row_stripes,
        "columnStripes": table.style.column_stripes,
        "filterButton": table.filter.is_some(),
        "columns": columns,
    })
}

fn filter_json(filter: &cgs_model::AutoFilter, table: Option<&str>) -> Value {
    let r = filter.range;
    let filtered: Vec<u32> = filter
        .columns
        .iter()
        .filter(|column| column.criteria.is_some())
        .map(|column| r.left + column.col_id)
        .collect();
    let hidden: Vec<u32> = filter
        .columns
        .iter()
        .filter(|column| column.hidden_button)
        .map(|column| r.left + column.col_id)
        .collect();
    let sorted = filter
        .sort
        .as_ref()
        .and_then(|sort| sort.keys.first())
        .map(|key| json!({ "col": key.area.left, "descending": key.descending }));
    json!({
        "table": table,
        "area": area_json(r.top, r.left, r.bottom, r.right),
        "filtered": filtered,
        "hiddenButtons": hidden,
        "sorted": sorted,
    })
}

impl Session {
    fn answer(&self, question: Question, locale: &cgs_numfmt::Locale) -> Result<Value, String> {
        Ok(match question {
            Question::PageSetup { sheet } => self.page_setup_json(sheet)?,
            // Answered as text by `Engine::query`, without building a JSON tree.
            Question::ErpChanges { .. } | Question::ErpReview | Question::ErpConflicts => {
                Value::Null
            }
            Question::ErpCell { sheet, row, col } => self.erp_cell_json(sheet, row, col, locale),
            Question::ErpNextError {
                sheet,
                row,
                col,
                back,
                conflict,
            } => self.erp_next_error_json(sheet, row, col, back, conflict),
            Question::Pages { sheet, request } => self.pages_json(sheet, &request)?,
            Question::TableAt { sheet, row, col } => {
                let Some(info) = self.table_at(sheet, row, col) else {
                    return Ok(Value::Null);
                };
                match self
                    .book
                    .tables
                    .iter()
                    .find(|table| table.name.eq_ignore_ascii_case(&info.name))
                {
                    Some(table) => table_json(table),
                    None => Value::Null,
                }
            }
            Question::GuessTable { sheet, area } => {
                let area = if area.rows() == 1 && area.cols() == 1 {
                    self.current_region(sheet, area.top, area.left)
                } else {
                    area
                };
                json!({
                    "area": area_json(area.top, area.left, area.bottom, area.right),
                    "headers": self.guess_headers(sheet, area),
                })
            }
            Question::FilterList { sheet, table, col } => {
                let options = cgs_numfmt::Options {
                    locale,
                    date1904: self.book.date1904,
                };
                self.filter_list(sheet, &table, col, &options)?
            }
            Question::Filters { sheet } => {
                let mut list = Vec::new();
                if let Some(filter) = self
                    .book
                    .sheets
                    .get(sheet as usize)
                    .and_then(|info| info.filter.as_ref())
                {
                    list.push(filter_json(filter, None));
                }
                for table in self.book.tables.iter().filter(|table| table.sheet == sheet) {
                    if let (Some(filter), true) = (
                        &table.filter,
                        table.header_rows > 0 && self.calc_has_table(&table.name),
                    ) {
                        list.push(filter_json(filter, Some(&table.name)));
                    }
                }
                Value::Array(list)
            }
            Question::CfRules { sheet } => self.cf_rules(sheet),
            Question::ValidationAt { sheet, row, col } => self.validation_json(sheet, row, col),
            Question::Names => self.names_json(),
            Question::NextSelectable {
                sheet,
                row,
                col,
                rows,
                cols,
            } => match self.next_selectable(sheet, row, col, rows, cols) {
                Some((row, col)) => json!([row, col]),
                None => Value::Null,
            },
            Question::Circular => self.circular_json(),
            Question::RichAt { sheet, row, col } => self.rich_json(sheet, row, col),
            Question::CellErrors { sheet, row, col } => {
                self.cell_errors_json(sheet, row, col, locale)
            }
            Question::NextError {
                sheet,
                row,
                col,
                back,
            } => match self.next_error(sheet, row, col, back, locale) {
                Some((row, col)) => json!([row, col]),
                None => Value::Null,
            },
            Question::ColumnLooks {
                sheet,
                col,
                top,
                bottom,
            } => {
                if (sheet as usize) < self.book.sheets.len() {
                    self.column_looks(sheet, col, top..=bottom.min(top + 100_000))
                } else {
                    Value::Null
                }
            }
            Question::NoteAt { sheet, row, col } => self.note_json(sheet, row, col),
            Question::LinkAt { sheet, row, col } => self.link_json(sheet, row, col),
            Question::Notes { sheet } => self.notes_list(sheet),
            Question::CellStyles => self.cell_styles_json(),
            Question::StyleFormat { name } => self.style_format_json(&name),
            Question::PivotDataFormula {
                sheet,
                row,
                col,
                from,
            } => self
                .pivot_data_formula(sheet, row, col, from)
                .map_or(Value::Null, Value::String),
            Question::CfLook { sheet, area } => self.cf_look_json(sheet, area),
            Question::TableStyles { options } => {
                self.table_styles_json(&options.unwrap_or_default())
            }
            Question::InvalidCells { sheet } => Value::Array(
                self.invalid_cells(sheet)
                    .into_iter()
                    .map(|(row, col)| json!([row, col]))
                    .collect(),
            ),
            Question::Tables => Value::Array(
                self.book
                    .tables
                    .iter()
                    .filter(|table| {
                        self.calc_has_table(&table.name)
                            && !self.book.sheets[table.sheet as usize].deleted
                    })
                    .map(table_json)
                    .collect(),
            ),
        })
    }

    fn calc_has_table(&self, name: &str) -> bool {
        use cgs_calc::CalcEngine;
        self.calc
            .tables()
            .iter()
            .any(|table| table.name.eq_ignore_ascii_case(name))
    }
}

impl Engine {
    /// Answers a question about the workbook, given and answered as JSON.
    pub fn query(&self, question: &str) -> Result<String, String> {
        let question: Question =
            serde_json::from_str(question).map_err(|error| error.to_string())?;
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| "No workbook is open.".to_string())?;
        match question {
            Question::ErpChanges { visible } => return Ok(session.changeset_text(visible)),
            Question::ErpReview => return Ok(session.erp_review_text(&self.locale)),
            Question::ErpConflicts => return Ok(session.erp_conflicts_text(&self.locale)),
            _ => {}
        }
        Ok(session.answer(question, &self.locale)?.to_string())
    }
}

impl Session {
    /// Each table style with a preview of 5 columns by 7 rows: for each cell its fill, text
    /// colour, whether bold, and its top, bottom, left and right lines' colours.
    fn table_styles_json(&self, options: &PreviewOptions) -> Value {
        let header = options.header_row.unwrap_or(true);
        let rows = 7;
        let mut table = Table::new(
            String::new(),
            0,
            cgs_model::Rect {
                top: 1,
                left: 1,
                bottom: rows,
                right: 5,
            },
            (1..=5).map(|n| format!("Column{n}")).collect(),
        );
        table.header_rows = u32::from(header);
        table.totals_rows = u32::from(options.totals_row);
        table.style.first_column = options.first_column;
        table.style.last_column = options.last_column;
        table.style.row_stripes = options.row_stripes.unwrap_or(true);
        table.style.column_stripes = options.column_stripes;
        let colors = &self.book.styles.colors;
        let rgb = |color: Option<cgs_model::Color>| -> Value {
            color
                .and_then(|color| colors.resolve(color, None))
                .map_or(Value::Null, |rgb| Value::String(format!("#{rgb:06X}")))
        };
        let line = |line: Option<cgs_model::Line>| rgb(line.map(|line| line.color));
        let preview = |style: &cgs_model::TableStyleDef| -> Value {
            let cells: Vec<Value> = (1..=rows)
                .map(|row| {
                    let row_cells: Vec<Value> = (1..=5)
                        .map(|col| {
                            let look = crate::table_style::exact_look(
                                crate::table_style::cell_look(&table, style, row, col),
                                &colors.theme,
                            );
                            json!([
                                rgb(look
                                    .fill
                                    .filter(|fill| fill.pattern != cgs_model::Pattern::None)
                                    .map(|fill| fill.color)),
                                rgb(look.font.color),
                                look.font.bold.unwrap_or(false),
                                line(look.border.top),
                                line(look.border.bottom),
                                line(look.border.left),
                                line(look.border.right),
                            ])
                        })
                        .collect();
                    Value::Array(row_cells)
                })
                .collect();
            Value::Array(cells)
        };
        let own =
            self.book.styles.table_styles.iter().map(
                |style| json!({ "name": style.name, "group": "Custom", "cells": preview(style) }),
            );
        let excel = crate::table_style::built_in().iter().map(|style| {
            let group = if style.name.contains("Light") {
                "Light"
            } else if style.name.contains("Medium") {
                "Medium"
            } else {
                "Dark"
            };
            json!({ "name": style.name, "group": group, "cells": preview(style) })
        });
        Value::Array(own.chain(excel).collect())
    }
}

impl Session {
    fn cf_look_json(&self, sheet: u32, area: Area) -> Value {
        let Some(info) = self.book.sheets.get(sheet as usize) else {
            return Value::Array(Vec::new());
        };
        let reader = crate::cf::CfReader::new(
            &self.calc,
            &self.book.styles,
            sheet,
            &info.name,
            &info.cf,
            info.used,
            self.today,
        );
        let colors = &self.book.styles.colors;
        let rgb = |color: Option<cgs_model::Color>| -> Value {
            color
                .and_then(|color| colors.resolve(color, None))
                .map_or(Value::Null, |rgb| Value::String(format!("{rgb:06X}")))
        };
        let mut out = Vec::new();
        for row in area.top..=area.bottom {
            for col in area.left..=area.right {
                let Some(cell) = reader.cell(row, col) else {
                    continue;
                };
                let dxf = cell.dxf.as_ref();
                out.push(json!({
                    "row": row,
                    "col": col,
                    "fill": rgb(dxf
                        .and_then(|d| d.fill)
                        .filter(|fill| fill.pattern != cgs_model::Pattern::None)
                        .map(|fill| fill.color)),
                    "color": rgb(dxf.and_then(|d| d.font.color)),
                    "bold": dxf.and_then(|d| d.font.bold),
                    "icon": cell.icon.as_ref().map(|(set, place)| json!([set, place])),
                    "bar": cell.bar.is_some(),
                }));
            }
        }
        Value::Array(out)
    }
}
