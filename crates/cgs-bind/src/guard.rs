//! The identity guard (plan B.3, item 2): one content rule over the real list of cell writes, so
//! no command needs predicting. Inside a bound table, an edit that touches a key, read-only or
//! heading cell must be a whole-row move: every touched row written across the table's full
//! width, its read-only cells taking exactly the read-only cells of one touched row, so the set of
//! rows stays the same. Sort and its undo pass; paste, fill, move, replace and clearing don't.
//! Edits only to editable cells pass as they are (from Phase 16; in Phase 15 every bound cell is
//! read-only).

use std::collections::HashMap;

/// A cell's contents, comparable and hashable. Numbers compare by their bits.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Content {
    Empty,
    Text(String),
    Number(u64),
    Bool(bool),
    Error(String),
    Formula(String),
    /// Part of another cell's spilled array.
    Spill,
}

impl Content {
    pub fn number(n: f64) -> Self {
        // -0 and 0 are the same number in a cell.
        Self::Number(if n == 0.0 {
            0f64.to_bits()
        } else {
            n.to_bits()
        })
    }
}

/// Where a bound table is now. Its heading row is `header`, its data `header + 1..=bottom`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableArea {
    pub header: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

impl TableArea {
    pub fn contains(&self, row: u32, col: u32) -> bool {
        (self.header..=self.bottom).contains(&row) && (self.left..=self.right).contains(&col)
    }

    pub fn first_data_row(&self) -> u32 {
        self.header + 1
    }

    /// Whether the inclusive rectangle meets the table.
    pub fn meets(&self, top: u32, left: u32, bottom: u32, right: u32) -> bool {
        top <= self.bottom && bottom >= self.header && left <= self.right && right >= self.left
    }

    /// Exactly the table's data rows, all its columns.
    pub fn is_data_area(&self, top: u32, left: u32, bottom: u32, right: u32) -> bool {
        (top, left, bottom, right) == (self.first_data_row(), self.left, self.bottom, self.right)
    }
}

/// Why an edit to ERP data is refused, with the message the user sees, in Excel's manner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A key, read-only or heading cell would change.
    ReadOnly,
    /// Rows would be added to or deleted from the table.
    Rows,
    /// Columns would be added to or deleted from the table.
    Columns,
    /// The table's shape, name or options would change, or cells would shift or merge in it.
    Table,
    /// Sorted left to right.
    SortSideways,
    /// Sorted as part of the table, or with more than it.
    SortWhole,
    /// The sheet holding it would be deleted.
    Sheet,
}

impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Self::ReadOnly => "This column is read-only in Cash Gear ERP.",
            Self::Rows => "Rows can't be added or deleted in ERP data.",
            Self::Columns => "Columns can't be added or deleted in ERP data.",
            Self::Table => "The table that holds ERP data can't be resized, renamed or reshaped.",
            Self::SortSideways => "ERP data can only be sorted top to bottom.",
            Self::SortWhole => {
                "ERP data is sorted as a whole table. Select a cell in it and sort again."
            }
            Self::Sheet => "A sheet that holds ERP data can't be deleted.",
        }
    }
}

/// Checks one batch of cell writes against a bound table. `writes` are in the order they are
/// applied, so a later write to a cell wins. `read_only(col)` says whether a column is guarded,
/// and `current(row, col)` reads a cell as it is before the batch.
pub fn check_writes(
    area: &TableArea,
    read_only: &dyn Fn(u32) -> bool,
    writes: &[(u32, u32, Content)],
    current: &dyn Fn(u32, u32) -> Content,
) -> Result<(), Refusal> {
    let mut inside: HashMap<(u32, u32), &Content> = HashMap::new();
    for (row, col, content) in writes {
        if area.contains(*row, *col) {
            inside.insert((*row, *col), content);
        }
    }
    if inside.is_empty() {
        return Ok(());
    }
    if inside.keys().any(|&(row, _)| row == area.header) {
        return Err(Refusal::ReadOnly);
    }
    if !inside.keys().any(|&(_, col)| read_only(col)) {
        return Ok(());
    }
    // A whole-row move: each touched row written all the way across.
    let width = (area.right - area.left + 1) as usize;
    let mut rows: HashMap<u32, usize> = HashMap::new();
    for &(row, _) in inside.keys() {
        *rows.entry(row).or_default() += 1;
    }
    if rows.values().any(|&n| n != width) {
        return Err(Refusal::ReadOnly);
    }
    let guarded: Vec<u32> = (area.left..=area.right).filter(|&c| read_only(c)).collect();
    // The read-only cells written into the touched rows must be those rows' own, rearranged.
    let mut before: HashMap<Vec<Content>, i64> = HashMap::new();
    for &row in rows.keys() {
        let tuple: Vec<Content> = guarded.iter().map(|&col| current(row, col)).collect();
        *before.entry(tuple).or_default() += 1;
    }
    for &row in rows.keys() {
        let tuple: Vec<Content> = guarded
            .iter()
            .map(|&col| inside[&(row, col)].clone())
            .collect();
        match before.get_mut(&tuple) {
            Some(n) if *n > 0 => *n -= 1,
            _ => return Err(Refusal::ReadOnly),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows 2 to 5 of a two-column table under a heading in row 1, keys in column 1.
    fn table() -> (TableArea, HashMap<(u32, u32), Content>) {
        let area = TableArea {
            header: 1,
            bottom: 5,
            left: 1,
            right: 2,
        };
        let mut cells = HashMap::new();
        cells.insert((1, 1), Content::Text("Account".into()));
        cells.insert((1, 2), Content::Text("Name".into()));
        for row in 2..=5 {
            cells.insert((row, 1), Content::Text(format!("V-{row}")));
            cells.insert((row, 2), Content::Text(format!("Name {row}")));
        }
        (area, cells)
    }

    fn check(
        writes: &[(u32, u32, Content)],
        read_only: &dyn Fn(u32) -> bool,
    ) -> Result<(), Refusal> {
        let (area, cells) = table();
        check_writes(&area, read_only, writes, &|row, col| {
            cells.get(&(row, col)).cloned().unwrap_or(Content::Empty)
        })
    }

    fn row_of(source: u32, target: u32) -> Vec<(u32, u32, Content)> {
        vec![
            (target, 1, Content::Text(format!("V-{source}"))),
            (target, 2, Content::Text(format!("Name {source}"))),
        ]
    }

    const ALL: &dyn Fn(u32) -> bool = &|_| true;

    #[test]
    fn rows_moved_whole_pass() {
        let swap = [row_of(3, 2), row_of(2, 3)].concat();
        assert_eq!(check(&swap, ALL), Ok(()));
        let cycle = [row_of(5, 2), row_of(2, 4), row_of(4, 5)].concat();
        assert_eq!(check(&cycle, ALL), Ok(()));
        assert_eq!(check(&row_of(2, 2), ALL), Ok(()));
    }

    #[test]
    fn writes_outside_the_table_pass() {
        assert_eq!(check(&[(6, 1, Content::Text("x".into()))], ALL), Ok(()));
        assert_eq!(check(&[(2, 3, Content::Empty)], ALL), Ok(()));
    }

    #[test]
    fn anything_else_is_refused() {
        let refused = |writes: Vec<(u32, u32, Content)>| {
            assert_eq!(check(&writes, ALL), Err(Refusal::ReadOnly))
        };
        // Typing, clearing and a half-width move.
        refused(vec![(2, 2, Content::Text("New".into()))]);
        refused(vec![(3, 1, Content::Empty)]);
        refused(vec![
            (2, 1, Content::Text("V-3".into())),
            (3, 1, Content::Text("V-2".into())),
        ]);
        // A row copied over another: a fill, or a paste of one row.
        refused(row_of(2, 3));
        refused([row_of(2, 3), row_of(2, 4)].concat());
        // A move down by one, which empties the first row.
        refused(
            [
                row_of(2, 3),
                row_of(3, 4),
                vec![(2, 1, Content::Empty), (2, 2, Content::Empty)],
            ]
            .concat(),
        );
        // Headings, and a whole-row move that brings in a row from outside.
        refused(vec![(1, 2, Content::Text("Other".into()))]);
        refused([row_of(6, 2), row_of(2, 6)].concat());
    }

    #[test]
    fn a_later_write_to_a_cell_wins() {
        let writes = [
            vec![(2, 1, Content::Text("x".into()))],
            row_of(3, 2),
            row_of(2, 3),
        ]
        .concat();
        assert_eq!(check(&writes, ALL), Ok(()));
    }

    #[test]
    fn editable_columns_change_freely_but_move_with_their_rows() {
        let keys_only = &|col: u32| col == 1;
        assert_eq!(
            check(&[(2, 2, Content::Text("New".into()))], keys_only),
            Ok(())
        );
        let swap = [row_of(3, 2), row_of(2, 3)].concat();
        assert_eq!(check(&swap, keys_only), Ok(()));
        // Keys swapped without their rows' other cells would move a record's values to another.
        let keys_swapped = vec![
            (2, 1, Content::Text("V-3".into())),
            (3, 1, Content::Text("V-2".into())),
        ];
        assert_eq!(check(&keys_swapped, keys_only), Err(Refusal::ReadOnly));
    }

    #[test]
    fn zero_is_zero() {
        assert_eq!(Content::number(-0.0), Content::number(0.0));
        assert_ne!(Content::number(1.0), Content::number(2.0));
    }
}
