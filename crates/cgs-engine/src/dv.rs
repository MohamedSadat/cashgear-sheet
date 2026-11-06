//! Data validation: whether what's typed is allowed, what a list offers, and the rules themselves.
//! Pasting doesn't validate, as in Excel.

use cgs_calc::{CalcEngine, CalcValue, CellRef, CellSink, Rect as CalcRect, SheetRect};
use cgs_model::{DvKind, DvOp, DvStyle, Rect, Validation};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::command::{Applied, Area, Op};
use crate::Session;

/// A rule as the Data Validation dialog gives it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DvSpec {
    /// `any`, `whole`, `decimal`, `list`, `date`, `time`, `textLength` or `custom`.
    pub kind: String,
    #[serde(default)]
    pub op: Option<String>,
    #[serde(default)]
    pub formula1: Option<String>,
    #[serde(default)]
    pub formula2: Option<String>,
    #[serde(default = "yes")]
    pub allow_blank: bool,
    #[serde(default = "yes")]
    pub dropdown: bool,
    #[serde(default = "yes")]
    pub show_input: bool,
    #[serde(default)]
    pub input_title: String,
    #[serde(default)]
    pub input_text: String,
    #[serde(default = "yes")]
    pub show_error: bool,
    #[serde(default)]
    pub error_title: String,
    #[serde(default)]
    pub error_text: String,
    /// `stop`, `warning` or `information`.
    #[serde(default)]
    pub style: Option<String>,
}

fn yes() -> bool {
    true
}

/// Why typing was refused, for the UI to show Excel's message for: `VALIDATION\u{1}` then the
/// style, title and text, each after a `\u{1}`.
pub const REFUSED: &str = "VALIDATION";

#[derive(Default)]
struct Values(Vec<CalcValue>);

impl CellSink for Values {
    fn cell(&mut self, _row: u32, _col: u32, value: &CalcValue) {
        self.0.push(value.clone());
    }
}

fn as_number(value: &CalcValue) -> Option<f64> {
    match value {
        CalcValue::Number(number) => Some(*number),
        CalcValue::Text(text) => text.trim().parse().ok(),
        CalcValue::Bool(flag) => Some(f64::from(u8::from(*flag))),
        _ => None,
    }
}

fn as_text(value: &CalcValue) -> String {
    match value {
        CalcValue::Text(text) => text.clone(),
        CalcValue::Number(number) => cgs_xlsx::patch_sheet::number_text(*number),
        CalcValue::Bool(flag) => if *flag { "TRUE" } else { "FALSE" }.to_string(),
        CalcValue::Error(text) => text.clone(),
        CalcValue::Empty => String::new(),
    }
}

fn subtract(rect: Rect, cut: Rect) -> Vec<Rect> {
    if cut.top > rect.bottom
        || cut.bottom < rect.top
        || cut.left > rect.right
        || cut.right < rect.left
    {
        return vec![rect];
    }
    let mut out = Vec::new();
    if cut.top > rect.top {
        out.push(Rect {
            bottom: cut.top - 1,
            ..rect
        });
    }
    if cut.bottom < rect.bottom {
        out.push(Rect {
            top: cut.bottom + 1,
            ..rect
        });
    }
    let (top, bottom) = (rect.top.max(cut.top), rect.bottom.min(cut.bottom));
    if cut.left > rect.left {
        out.push(Rect {
            top,
            bottom,
            right: cut.left - 1,
            ..rect
        });
    }
    if cut.right < rect.right {
        out.push(Rect {
            top,
            bottom,
            left: cut.right + 1,
            ..rect
        });
    }
    out
}

impl Session {
    pub(crate) fn validation_at(&self, sheet: u32, row: u32, col: u32) -> Option<&Validation> {
        self.book
            .sheets
            .get(sheet as usize)?
            .validations
            .iter()
            .rev()
            .find(|validation| validation.covers(row, col))
    }

    fn dv_formula(
        &self,
        sheet: u32,
        validation: &Validation,
        text: &str,
        row: u32,
        col: u32,
    ) -> CalcValue {
        let name = &self.book.sheets[sheet as usize].name;
        self.calc.evaluate_at(
            name,
            validation.anchor(),
            row,
            col,
            text.trim_start_matches('='),
        )
    }

    /// What a list offers: its own items, or the values of the cells it names.
    pub(crate) fn list_items(
        &self,
        sheet: u32,
        validation: &Validation,
        row: u32,
        col: u32,
    ) -> Vec<String> {
        let Some(formula) = validation.formula1.as_deref() else {
            return Vec::new();
        };
        let formula = formula.trim().trim_start_matches('=');
        if let Some(inner) = formula
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
        {
            return inner
                .replace("\"\"", "\"")
                .split(',')
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect();
        }
        // A range, on this sheet or another, or a name for one.
        let target = self
            .book
            .names
            .iter()
            .find(|name| name.name.eq_ignore_ascii_case(formula))
            .map_or(formula.to_string(), |name| {
                name.formula.trim_start_matches('=').to_string()
            });
        let (sheet_index, reference) = match target.rsplit_once('!') {
            Some((sheet_name, reference)) => {
                let sheet_name = sheet_name.trim_matches('\'').replace("''", "'");
                let found =
                    self.book.sheets.iter().position(|info| {
                        !info.deleted && info.name.eq_ignore_ascii_case(&sheet_name)
                    });
                (found.map(|at| at as u32), reference.to_string())
            }
            None => (Some(sheet), target.clone()),
        };
        let Some(sheet_index) = sheet_index else {
            return Vec::new();
        };
        let reference = reference.replace('$', "");
        let Some(((top, left), (bottom, right))) = cgs_refs::parse_range(&reference) else {
            // Anything else is worked out as a formula and read as one value.
            let value = self.dv_formula(sheet, validation, formula, row, col);
            return vec![as_text(&value)];
        };
        let mut values = Values::default();
        self.calc.read_block(
            SheetRect {
                sheet: sheet_index,
                rect: CalcRect {
                    top: top.min(bottom),
                    left: left.min(right),
                    bottom: top.max(bottom).min(top.min(bottom) + 32_767),
                    right: left.max(right),
                },
            },
            &mut values,
        );
        values
            .0
            .iter()
            .map(as_text)
            .filter(|text| !text.is_empty())
            .collect()
    }

    /// Whether a value is allowed in a cell. Custom rules read the cell as it now stands. Excel
    /// takes a list's item typed in any case, but counts a cell that differs in case from an item
    /// of a list typed into the rule as invalid when it circles them, so `exact` says which.
    pub(crate) fn allowed(
        &self,
        sheet: u32,
        validation: &Validation,
        row: u32,
        col: u32,
        value: &CalcValue,
        exact: bool,
    ) -> bool {
        let blank = matches!(value, CalcValue::Empty)
            || matches!(value, CalcValue::Text(text) if text.is_empty());
        if blank {
            return validation.allow_blank || validation.kind == DvKind::Any;
        }
        let bound = |which: &Option<String>| {
            which
                .as_deref()
                .and_then(|text| as_number(&self.dv_formula(sheet, validation, text, row, col)))
        };
        let numbers = |value: f64| match bound(&validation.formula1) {
            Some(first) => validation.op.holds(
                value,
                first,
                matches!(validation.op, DvOp::Between | DvOp::NotBetween)
                    .then(|| bound(&validation.formula2))
                    .flatten(),
            ),
            None => true,
        };
        match validation.kind {
            DvKind::Any => true,
            DvKind::Whole => match value {
                CalcValue::Number(number) => number.fract() == 0.0 && numbers(*number),
                _ => false,
            },
            DvKind::Decimal | DvKind::Date | DvKind::Time => match value {
                CalcValue::Number(number) => numbers(*number),
                _ => false,
            },
            DvKind::TextLength => numbers(as_text(value).chars().count() as f64),
            DvKind::List => {
                let text = as_text(value);
                // Only a list typed into the rule; one read from cells takes any case.
                let literal = validation
                    .formula1
                    .as_deref()
                    .is_some_and(|formula| formula.trim_start().starts_with('"'));
                self.list_items(sheet, validation, row, col).iter().any(|item| {
                    if exact && literal {
                        *item == text
                    } else {
                        item.to_lowercase() == text.to_lowercase()
                    }
                })
            }
            DvKind::Custom => validation.formula1.as_deref().is_none_or(|formula| {
                matches!(
                    self.dv_formula(sheet, validation, formula, row, col),
                    CalcValue::Bool(true)
                ) || matches!(self.dv_formula(sheet, validation, formula, row, col), CalcValue::Number(n) if n != 0.0)
            }),
        }
    }

    /// The refusal for a value typed into a cell, if its rule refuses it.
    pub(crate) fn refusal(
        &self,
        sheet: u32,
        row: u32,
        col: u32,
        value: &CalcValue,
    ) -> Option<String> {
        let validation = self.validation_at(sheet, row, col)?;
        if !validation.show_error || validation.kind == DvKind::Custom {
            return None;
        }
        if self.allowed(sheet, validation, row, col, value, false) {
            return None;
        }
        Some(self.refusal_text(validation))
    }

    pub(crate) fn refusal_text(&self, validation: &Validation) -> String {
        let text = if validation.error_text.is_empty() {
            "This value doesn't match the data validation restrictions defined for this cell."
        } else {
            &validation.error_text
        };
        format!(
            "{REFUSED}\u{1}{}\u{1}{}\u{1}{text}",
            validation.style.xml(),
            validation.error_title
        )
    }

    /// Whether a custom rule, read after the cell has its new value, refuses it.
    pub(crate) fn custom_refusal(&self, sheet: u32, row: u32, col: u32) -> Option<String> {
        let validation = self.validation_at(sheet, row, col)?;
        if !validation.show_error || validation.kind != DvKind::Custom {
            return None;
        }
        let value = match self.calc.cell_input(CellRef { sheet, row, col }) {
            cgs_calc::CellInput::Value(value) => value,
            _ => return None,
        };
        (!self.allowed(sheet, validation, row, col, &value, false))
            .then(|| self.refusal_text(validation))
    }

    /// Puts a rule on cells, or takes rules off them with none.
    pub(crate) fn set_validation_command(
        &mut self,
        sheet: u32,
        areas: &[Area],
        spec: Option<&DvSpec>,
    ) -> Result<Applied, String> {
        self.sheet_for_edit(sheet)?;
        if areas.is_empty() {
            return Err("Select the cells first.".to_string());
        }
        let cuts: Vec<Rect> = areas
            .iter()
            .map(|a| Rect {
                top: a.top,
                left: a.left,
                bottom: a.bottom,
                right: a.right,
            })
            .collect();
        let mut list: Vec<Validation> = Vec::new();
        for mut validation in self.book.sheets[sheet as usize].validations.clone() {
            let mut kept = validation.areas.clone();
            for cut in &cuts {
                kept = kept
                    .into_iter()
                    .flat_map(|rect| subtract(rect, *cut))
                    .collect();
            }
            if kept.is_empty() {
                continue;
            }
            if kept != validation.areas {
                validation.areas = kept;
                validation.raw = None;
            }
            list.push(validation);
        }
        if let Some(spec) = spec {
            let kind = DvKind::from_xml(&spec.kind);
            let check = |text: &Option<String>| -> Result<Option<String>, String> {
                match text.as_deref().map(str::trim) {
                    None | Some("") => Ok(None),
                    Some(text) => {
                        let text = text.trim_start_matches('=');
                        // A list of items is kept as Excel keeps it, in quotes.
                        if kind == DvKind::List
                            && !text.starts_with('"')
                            && text.contains(',')
                            && cgs_refs::parse_range(&text.replace('$', "")).is_none()
                        {
                            return Ok(Some(format!("\"{text}\"")));
                        }
                        self.calc.check_formula(text).map_err(|error| error.0)?;
                        Ok(Some(text.to_string()))
                    }
                }
            };
            let formula1 = check(&spec.formula1)?;
            let formula2 = check(&spec.formula2)?;
            if kind != DvKind::Any && formula1.is_none() {
                return Err("You must enter a value in the Source box.".to_string());
            }
            list.push(Validation {
                areas: cuts.clone(),
                kind,
                op: spec.op.as_deref().map(DvOp::from_xml).unwrap_or_default(),
                formula1,
                formula2,
                allow_blank: spec.allow_blank,
                dropdown: spec.dropdown,
                show_input: spec.show_input,
                input_title: spec.input_title.clone(),
                input_text: spec.input_text.clone(),
                show_error: spec.show_error,
                error_title: spec.error_title.clone(),
                error_text: spec.error_text.clone(),
                style: spec
                    .style
                    .as_deref()
                    .map(DvStyle::from_xml)
                    .unwrap_or_default(),
                extended: false,
                raw: None,
            });
        }
        self.perform(
            "data validation",
            Op::SheetDv { sheet, list },
            Some((sheet, areas[0])),
        )
    }

    /// What the dialog and the cell show of a cell's rule.
    pub(crate) fn validation_json(&self, sheet: u32, row: u32, col: u32) -> Value {
        let Some(validation) = self.validation_at(sheet, row, col) else {
            return Value::Null;
        };
        let items = if validation.kind == DvKind::List {
            self.list_items(sheet, validation, row, col)
        } else {
            Vec::new()
        };
        json!({
            "kind": validation.kind.xml(),
            "op": validation.op.xml(),
            "formula1": validation.formula1,
            "formula2": validation.formula2,
            "allowBlank": validation.allow_blank,
            "dropdown": validation.dropdown && validation.kind == DvKind::List,
            "items": items,
            "showInput": validation.show_input,
            "inputTitle": validation.input_title,
            "inputText": validation.input_text,
            "showError": validation.show_error,
            "errorTitle": validation.error_title,
            "errorText": validation.error_text,
            "style": validation.style.xml(),
            "extended": validation.extended,
            "areas": cgs_xlsx::cf::sqref_text(&validation.areas),
        })
    }

    /// Cells whose values their rules don't allow, for Circle Invalid Data, as Excel's limit of 255.
    pub(crate) fn invalid_cells(&self, sheet: u32) -> Vec<(u32, u32)> {
        let Some(info) = self.book.sheets.get(sheet as usize) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for validation in &info.validations {
            for area in &validation.areas {
                let bottom = area.bottom.min(info.used.0);
                let right = area.right.min(info.used.1);
                for row in area.top..=bottom {
                    for col in area.left..=right {
                        if out.len() >= 255 {
                            return out;
                        }
                        let value = match self.calc.cell_input(CellRef { sheet, row, col }) {
                            cgs_calc::CellInput::Value(value) => value,
                            cgs_calc::CellInput::Formula { cached, .. } => {
                                cached.unwrap_or(CalcValue::Empty)
                            }
                            _ => continue,
                        };
                        if !self.allowed(sheet, validation, row, col, &value, true) {
                            out.push((row, col));
                        }
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_hold_either_way_round() {
        assert!(DvOp::Between.holds(5.0, 10.0, Some(1.0)));
        assert!(!DvOp::NotBetween.holds(5.0, 1.0, Some(10.0)));
        assert!(DvOp::GreaterThan.holds(5.0, 1.0, None));
    }
}
