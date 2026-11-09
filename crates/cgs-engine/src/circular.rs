//! Circular references, found from formulas' text: the cells Excel names in "Circular References"
//! and warns about when a formula closes a loop. Formulas are linked to the formulas they read,
//! directly or through a range, on their own sheet or another, and loops are their strongly
//! connected parts. References made by names, INDIRECT or OFFSET aren't followed.

use std::collections::HashMap;

use cgs_calc::{CalcEngine, CellInput, CellRef};
use cgs_model::{Book, SheetState};

/// Past this many cells to look through, the check isn't made.
const CELL_LIMIT: u64 = 1_000_000;

/// Excel 2016's warning when a formula closes a loop.
pub const WARNING: &str = "There are one or more circular references where a formula refers to its own cell either directly or indirectly. This might cause them to calculate incorrectly.\n\nTry removing or changing these references, or moving the formulas to different cells.";

type Cell = (u32, u32, u32);

/// A formula's references, as sheets and areas: `(sheet, top, left, bottom, right)`.
fn references(book: &Book, home: u32, formula: &str) -> Vec<(u32, u32, u32, u32, u32)> {
    let bytes = formula.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'$' || b == b'_' || b == b'.';
    let sheet_named = |name: &str| {
        book.sheets
            .iter()
            .position(|info| !info.deleted && info.name.eq_ignore_ascii_case(name))
            .map(|index| index as u32)
    };
    let area = |text: &str| -> Option<(u32, u32, u32, u32)> {
        let clean = text.replace('$', "");
        if clean.contains(':') {
            cgs_refs::parse_range(&clean).map(|((t, l), (b, r))| (t, l, b, r))
        } else {
            cgs_refs::parse_a1(&clean).map(|(r, c)| (r, c, r, c))
        }
    };
    let take = |at: &mut usize| {
        let start = *at;
        while *at < bytes.len() && (word(bytes[*at]) || bytes[*at] == b':') {
            *at += 1;
        }
        &formula[start..*at]
    };
    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                at += 1;
                while at < bytes.len() && bytes[at] != b'"' {
                    at += 1;
                }
                at += 1;
            }
            b'\'' => {
                let start = at + 1;
                at += 1;
                while at < bytes.len() && !(bytes[at] == b'\'' && bytes.get(at + 1) != Some(&b'\''))
                {
                    at += if bytes[at] == b'\'' { 2 } else { 1 };
                }
                let name = formula[start..at.min(bytes.len())].replace("''", "'");
                at += 1;
                if bytes.get(at) == Some(&b'!') {
                    at += 1;
                    let text = take(&mut at);
                    if let (Some(sheet), Some((t, l, b, r))) = (sheet_named(&name), area(text)) {
                        out.push((sheet, t, l, b, r));
                    }
                }
            }
            b if word(b) => {
                let text = take(&mut at);
                match bytes.get(at) {
                    Some(b'(') => {}
                    Some(b'!') => {
                        at += 1;
                        let reference = take(&mut at);
                        if let (Some(sheet), Some((t, l, b, r))) =
                            (sheet_named(text), area(reference))
                        {
                            out.push((sheet, t, l, b, r));
                        }
                    }
                    _ => {
                        if let Some((t, l, b, r)) = area(text) {
                            out.push((home, t, l, b, r));
                        }
                    }
                }
            }
            _ => at += 1,
        }
    }
    out
}

/// Every formula cell on a loop, sheet by sheet in tab order and then by row and column, or
/// nothing when the workbook is too big to look through.
pub fn circular_cells(book: &Book, calc: &dyn CalcEngine) -> Option<Vec<Cell>> {
    let order = book.tab_order();
    let total: u64 = order
        .iter()
        .map(|&sheet| {
            let used = book.sheets[sheet as usize].used;
            u64::from(used.0) * u64::from(used.1)
        })
        .sum();
    if total > CELL_LIMIT {
        return None;
    }
    // Each sheet's formulas, sorted by row and column.
    let mut nodes: Vec<Cell> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    for &sheet in &order {
        let info = &book.sheets[sheet as usize];
        if info.kind != cgs_model::SheetKind::Worksheet {
            continue;
        }
        for row in 1..=info.used.0 {
            for col in 1..=info.used.1 {
                if let CellInput::Formula { text, .. } =
                    calc.cell_input(CellRef { sheet, row, col })
                {
                    nodes.push((sheet, row, col));
                    texts.push(text);
                }
            }
        }
    }
    if nodes.is_empty() {
        return Some(Vec::new());
    }
    let index: HashMap<Cell, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, &cell)| (cell, i))
        .collect();
    let mut by_sheet: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, &(sheet, _, _)) in nodes.iter().enumerate() {
        by_sheet.entry(sheet).or_default().push(i);
    }
    // What each formula reads that is a formula.
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (from, text) in texts.iter().enumerate() {
        let home = nodes[from].0;
        for (sheet, top, left, bottom, right) in references(book, home, text) {
            if top == bottom && left == right {
                if let Some(&to) = index.get(&(sheet, top, left)) {
                    edges[from].push(to);
                }
                continue;
            }
            let Some(list) = by_sheet.get(&sheet) else {
                continue;
            };
            // Rows are sorted, so the range's rows are one stretch of the list.
            let start = list.partition_point(|&i| nodes[i].1 < top);
            for &to in &list[start..] {
                let (_, row, col) = nodes[to];
                if row > bottom {
                    break;
                }
                if col >= left && col <= right {
                    edges[from].push(to);
                }
            }
        }
    }
    // Tarjan's strongly connected parts, without recursion.
    let n = nodes.len();
    let mut indexes = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut circular = vec![false; n];
    let mut next = 0;
    for root in 0..n {
        if indexes[root] != usize::MAX {
            continue;
        }
        let mut work: Vec<(usize, usize)> = vec![(root, 0)];
        indexes[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&mut (v, ref mut child)) = work.last_mut() {
            if let Some(&w) = edges[v].get(*child) {
                *child += 1;
                if indexes[w] == usize::MAX {
                    indexes[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(indexes[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[v]);
            }
            if low[v] == indexes[v] {
                let mut part = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    part.push(w);
                    if w == v {
                        break;
                    }
                }
                let looped = part.len() > 1 || edges[v].contains(&v);
                if looped {
                    for w in part {
                        circular[w] = true;
                    }
                }
            }
        }
    }
    Some(
        nodes
            .into_iter()
            .zip(circular)
            .filter(|(_, on)| *on)
            .map(|(cell, _)| cell)
            .collect(),
    )
}

impl crate::Session {
    /// "Circular References": the loops' cells, the visible sheets' first.
    pub(crate) fn circular_json(&self) -> serde_json::Value {
        let Some(cells) = circular_cells(&self.book, &self.calc) else {
            return serde_json::Value::Null;
        };
        let mut cells = cells;
        cells.sort_by_key(|&(sheet, row, col)| {
            let hidden = self.book.sheets[sheet as usize].state != SheetState::Visible;
            (
                hidden,
                self.book.tab_order().iter().position(|&s| s == sheet),
                row,
                col,
            )
        });
        serde_json::Value::Array(
            cells
                .into_iter()
                .map(|(sheet, row, col)| serde_json::json!([sheet, row, col]))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_references_on_other_sheets() {
        let mut book = Book::default();
        for name in ["One", "My sheet"] {
            book.sheets.push(cgs_model::SheetInfo::new(
                name.to_string(),
                SheetState::Visible,
                cgs_model::SheetKind::Worksheet,
                None,
            ));
        }
        assert_eq!(
            references(&book, 0, "SUM(A1:A3)+'My sheet'!B2+One!C3+\"D4\""),
            [(0, 1, 1, 3, 1), (1, 2, 2, 2, 2), (0, 3, 3, 3, 3)]
        );
    }
}
