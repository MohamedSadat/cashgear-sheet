//! The Name Manager: defining, changing and deleting names, and making them from a selection's
//! labels. A renamed name follows into formulas, as in Excel.

use cgs_calc::{CalcEngine, CalcValue, CellInput, CellRef, NameInfo};
use cgs_model::DefinedName;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::command::{Applied, Area, Op};
use crate::edit::EditCell;
use crate::Session;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameRef {
    pub name: String,
    /// A sheet's own name, or none for the workbook's.
    #[serde(default)]
    pub sheet: Option<u32>,
}

/// Whether a name is one Excel takes.
fn name_problem(name: &str) -> Option<&'static str> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Some("Enter a name.");
    };
    if name.chars().count() > 255 {
        return Some("A name can be at most 255 characters.");
    }
    if !(first.is_alphabetic() || first == '_' || first == '\\') {
        return Some("A name must start with a letter, an underscore or a backslash.");
    }
    if !chars.all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '.' | '\\' | '?')) {
        return Some("A name can't hold spaces or most punctuation.");
    }
    let upper = name.to_ascii_uppercase();
    if cgs_refs::parse_a1(&upper).is_some()
        || matches!(upper.as_str(), "R" | "C")
        || upper.strip_prefix('R').is_some_and(|rest| {
            rest.split('C')
                .all(|part| part.chars().all(|c| c.is_ascii_digit()))
        })
    {
        return Some("A name can't look like a cell's address.");
    }
    None
}

/// `text` with a defined name's new name wherever the name is used.
pub fn rename_name(text: &str, old: &str, new: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut changed = false;
    let word =
        |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'\\' | b'?') || b >= 0x80;
    while at < bytes.len() {
        let b = bytes[at];
        if b == b'"' || b == b'\'' {
            let start = at;
            at += 1;
            while at < bytes.len() {
                if bytes[at] == b {
                    if bytes.get(at + 1) == Some(&b) {
                        at += 2;
                        continue;
                    }
                    at += 1;
                    break;
                }
                at += 1;
            }
            out.push_str(&text[start..at.min(text.len())]);
            continue;
        }
        if word(b) && (at == 0 || !word(bytes[at - 1])) {
            let start = at;
            while at < bytes.len() && word(bytes[at]) {
                at += 1;
            }
            let found = &text[start..at];
            let next = bytes.get(at).copied();
            let is_name =
                found.eq_ignore_ascii_case(old) && !matches!(next, Some(b'(' | b'!' | b'['));
            if is_name {
                out.push_str(new);
                changed = true;
            } else {
                out.push_str(found);
            }
            continue;
        }
        let width = text[at..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&text[at..at + width]);
        at += width;
    }
    changed.then_some(out)
}

impl Session {
    fn names_op(&self, names: Vec<DefinedName>) -> Op {
        Op::BookNames(names)
    }

    /// Adds a name, or changes one. A changed name's formulas follow it.
    pub(crate) fn set_name_command(
        &mut self,
        old: Option<NameRef>,
        name: &str,
        formula: &str,
        sheet: Option<u32>,
    ) -> Result<Applied, String> {
        let name = name.trim();
        if let Some(problem) = name_problem(name) {
            return Err(problem.to_string());
        }
        let formula = formula.trim().trim_start_matches('=');
        if formula.is_empty() {
            return Err("Enter what the name refers to.".to_string());
        }
        self.calc.check_formula(formula).map_err(|error| error.0)?;
        let mut names = self.book.names.clone();
        let existing = old.as_ref().and_then(|old| {
            names
                .iter()
                .position(|n| n.name.eq_ignore_ascii_case(&old.name) && n.local_sheet == old.sheet)
        });
        let clash = names.iter().enumerate().any(|(at, n)| {
            Some(at) != existing && n.name.eq_ignore_ascii_case(name) && n.local_sheet == sheet
        }) || self
            .calc
            .tables()
            .iter()
            .any(|table| table.name.eq_ignore_ascii_case(name));
        if clash {
            return Err(
                "The name that you entered already exists. Enter a unique name.".to_string(),
            );
        }
        let mut rewrites = Vec::new();
        let entry = DefinedName {
            name: name.to_string(),
            formula: formula.to_string(),
            local_sheet: sheet,
            hidden: false,
        };
        match existing {
            Some(at) => {
                let hidden = names[at].hidden;
                let before = std::mem::replace(&mut names[at], DefinedName { hidden, ..entry });
                if !before.name.eq_ignore_ascii_case(name) || before.name != name {
                    for (cell, text) in self.calc.all_formulas().map_err(|error| error.0)? {
                        if let Some(next) = rename_name(&text, &before.name, name) {
                            let input = self.rewritten(cell, next);
                            rewrites.push(EditCell {
                                sheet: cell.sheet,
                                row: cell.row,
                                col: cell.col,
                                input: Some(input),
                                style: None,
                            });
                        }
                    }
                    for other in names.iter_mut() {
                        if let Some(next) = rename_name(&other.formula, &before.name, name) {
                            other.formula = next;
                        }
                    }
                }
            }
            None => names.push(entry),
        }
        let mut list = vec![self.names_op(names)];
        if !rewrites.is_empty() {
            list.push(Op::Cells(rewrites));
        }
        self.perform("names", Op::Many(list), None)
    }

    pub(crate) fn delete_name_command(&mut self, which: &NameRef) -> Result<Applied, String> {
        let mut names = self.book.names.clone();
        let before = names.len();
        names.retain(|n| {
            !(n.name.eq_ignore_ascii_case(&which.name) && n.local_sheet == which.sheet)
        });
        if names.len() == before {
            return Err(format!("There is no name {}.", which.name));
        }
        let op = self.names_op(names);
        self.perform("names", op, None)
    }

    /// Names from a selection's labels: its top row, left column, bottom row or right column.
    pub(crate) fn names_from_selection(
        &mut self,
        sheet: u32,
        area: Area,
        sides: (bool, bool, bool, bool),
    ) -> Result<Applied, String> {
        let (top, left, bottom, right) = sides;
        let sheet_name = cgs_refs::quote_sheet(&self.book.sheets[sheet as usize].name);
        let label = |session: &Self, row: u32, col: u32| -> Option<String> {
            let value = match session.calc.cell_input(CellRef { sheet, row, col }) {
                CellInput::Value(value) => value,
                CellInput::Formula { cached, .. } => cached.unwrap_or(CalcValue::Empty),
                _ => return None,
            };
            let text = match value {
                CalcValue::Text(text) => text,
                CalcValue::Number(number) => {
                    format!("_{}", cgs_xlsx::patch_sheet::number_text(number))
                }
                _ => return None,
            };
            // Spaces and other marks become underscores, as Excel makes them.
            let mut name: String = text
                .trim()
                .chars()
                .map(|ch| {
                    if ch.is_alphanumeric() || matches!(ch, '_' | '.' | '\\') {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect();
            if name.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
                name.insert(0, '_');
            }
            (name_problem(&name).is_none()).then_some(name)
        };
        let cell = |row: u32, col: u32| {
            format!(
                "{sheet_name}!${}${row}",
                cgs_refs::col_to_letters(i64::from(col))
            )
        };
        let span = |a: (u32, u32), b: (u32, u32)| {
            if a == b {
                cell(a.0, a.1)
            } else {
                format!(
                    "{}:${}${}",
                    cell(a.0, a.1),
                    cgs_refs::col_to_letters(i64::from(b.1)),
                    b.0
                )
            }
        };
        let (rows, cols) = (
            area.top + u32::from(top)..=area.bottom - u32::from(bottom),
            area.left + u32::from(left)..=area.right - u32::from(right),
        );
        let mut made: Vec<(String, String)> = Vec::new();
        if top || bottom {
            let at = if top { area.top } else { area.bottom };
            for col in cols.clone() {
                if let Some(name) = label(self, at, col) {
                    made.push((name, span((*rows.start(), col), (*rows.end(), col))));
                }
            }
        }
        if left || right {
            let at = if left { area.left } else { area.right };
            for row in rows.clone() {
                if let Some(name) = label(self, row, at) {
                    made.push((name, span((row, *cols.start()), (row, *cols.end()))));
                }
            }
        }
        if made.is_empty() {
            return Err("No names could be made from the selection.".to_string());
        }
        let mut names = self.book.names.clone();
        for (name, formula) in made {
            match names
                .iter_mut()
                .find(|n| n.name.eq_ignore_ascii_case(&name) && n.local_sheet.is_none())
            {
                Some(existing) => existing.formula = formula,
                None => names.push(DefinedName {
                    name,
                    formula,
                    local_sheet: None,
                    hidden: false,
                }),
            }
        }
        let op = self.names_op(names);
        self.perform("names", op, Some((sheet, area)))
    }

    /// Every name the manager lists, with what each refers to and its value now.
    pub(crate) fn names_json(&self) -> Value {
        let list: Vec<Value> = self
            .book
            .names
            .iter()
            .filter(|n| !n.hidden && !n.name.starts_with("_xlnm."))
            .map(|n| {
                let scope = n
                    .local_sheet
                    .and_then(|index| self.book.sheets.get(index as usize))
                    .map_or("Workbook".to_string(), |sheet| sheet.name.clone());
                json!({
                    "name": n.name,
                    "formula": format!("={}", n.formula),
                    "sheet": n.local_sheet,
                    "scope": scope,
                })
            })
            .collect();
        Value::Array(list)
    }

    /// Applies the model's names to the engine as well.
    pub(crate) fn apply_names(&mut self, names: Vec<DefinedName>) -> Result<Op, String> {
        let before = std::mem::replace(&mut self.book.names, names);
        let infos: Vec<NameInfo> = self
            .book
            .names
            .iter()
            .map(|n| NameInfo {
                name: n.name.clone(),
                formula: n.formula.clone(),
                local_sheet: n.local_sheet,
            })
            .collect();
        self.calc.replace_names(&infos).map_err(|error| error.0)?;
        self.edits.names_changed = true;
        self.dirty.sheets.extend(0..self.book.sheets.len() as u32);
        Ok(Op::BookNames(before))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renamed_name_follows_into_formulas_and_nothing_else() {
        assert_eq!(
            rename_name("SUM(Rate)*rate+Rates+\"Rate\"", "Rate", "Tax").as_deref(),
            Some("SUM(Tax)*Tax+Rates+\"Rate\"")
        );
        assert_eq!(rename_name("Rate(1)+'Rate'!A1", "Rate", "Tax"), None);
    }

    #[test]
    fn names_follow_excels_rules() {
        assert!(name_problem("Sales_2024").is_none());
        assert!(name_problem("A1").is_some());
        assert!(name_problem("my name").is_some());
    }
}
