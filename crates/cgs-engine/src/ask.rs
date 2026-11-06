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
        Ok(session.answer(question, &self.locale)?.to_string())
    }
}
