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

impl Session {
    fn answer(&self, question: Question) -> Result<Value, String> {
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
        Ok(session.answer(question)?.to_string())
    }
}
