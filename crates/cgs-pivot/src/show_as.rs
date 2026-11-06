//! Each value cell: the summary of its records, then Show Values As.

use cgs_model::pivot::{BaseItem, ItemType, PivotTable, ShowAs, Summary};

use crate::agg::Out;
use crate::layout::{AccGrid, LineInfo, LineRole, Tree};

pub struct Values<'a> {
    pub table: &'a PivotTable,
    pub rows: &'a Tree,
    pub cols: &'a Tree,
    pub grid: &'a AccGrid,
    pub row_lines: &'a [LineInfo],
    pub col_lines: &'a [LineInfo],
    pub rows_have_values: bool,
    pub cols_have_values: bool,
}

pub struct Results {
    cols: usize,
    cells: Vec<Option<(usize, Out)>>,
}

impl Results {
    pub fn get(&self, row: usize, col: usize) -> Option<(usize, &Out)> {
        self.cells
            .get(row * self.cols + col)?
            .as_ref()
            .map(|(d, out)| (*d, out))
    }
}

fn divide(a: &Out, b: &Out) -> Out {
    match (a, b) {
        (Out::Error(e), _) | (_, Out::Error(e)) => Out::Error(e.clone()),
        (Out::Empty, _) => Out::Empty,
        (Out::Number(x), Out::Number(y)) if *y != 0.0 => Out::Number(x / y),
        (Out::Number(_), _) => Out::Error("#DIV/0!".to_string()),
    }
}

/// Which axis a field is on, and at which level of its tree.
#[derive(Clone, Copy)]
enum Side {
    Rows(usize),
    Cols(usize),
}

impl Values<'_> {
    fn summary(&self, row: &LineInfo, col: &LineInfo, d: usize) -> Summary {
        for line in [row, col] {
            if let LineRole::Subtotal(kind) = line.role {
                if kind != ItemType::Default {
                    if let Some(summary) = kind.function() {
                        return summary;
                    }
                }
            }
        }
        self.table.data_fields[d].summary
    }

    fn raw(&self, row: u32, col: u32, d: usize, summary: Summary) -> Out {
        let Some(acc) = self.grid.get(&(row, col)).and_then(|accs| accs.get(d)) else {
            return Out::Empty;
        };
        // A count looks past an error only where no total gathers it.
        let leaf = self.rows.nodes[row as usize].depth as usize == self.rows.levels.len()
            && self.cols.nodes[col as usize].depth as usize == self.cols.levels.len();
        match (&acc.error, summary, leaf) {
            (Some(e), Summary::Count | Summary::CountNums, false) => Out::Error(e.clone()),
            _ => acc.result(summary),
        }
    }

    fn side(&self, field: u32) -> Option<Side> {
        if let Some(level) = self.rows.levels.iter().position(|&f| f == field as usize) {
            return Some(Side::Rows(level));
        }
        self.cols
            .levels
            .iter()
            .position(|&f| f == field as usize)
            .map(Side::Cols)
    }

    /// The node at `level` above `node`, or `node` itself when it is at that level.
    fn ancestor_at(tree: &Tree, mut node: u32, depth: u32) -> u32 {
        while tree.nodes[node as usize].depth > depth {
            node = tree.nodes[node as usize].parent;
        }
        node
    }

    fn path(tree: &Tree, mut node: u32) -> Vec<u32> {
        let mut out = Vec::new();
        while node != 0 {
            out.push(tree.nodes[node as usize].pos);
            node = tree.nodes[node as usize].parent;
        }
        out.reverse();
        out
    }

    fn find(tree: &Tree, path: &[u32]) -> Option<u32> {
        let mut node = 0u32;
        for &pos in path {
            node = *tree.child.get(&(node, pos))?;
        }
        Some(node)
    }

    /// The same node with the item at `level` replaced by `pos`.
    fn swap(tree: &Tree, node: u32, level: usize, pos: u32) -> Option<u32> {
        let mut path = Self::path(tree, node);
        *path.get_mut(level)? = pos;
        Self::find(tree, &path)
    }

    /// The siblings at `level` of the node's ancestor there, in display order.
    fn siblings(tree: &Tree, node: u32, level: usize) -> Option<(Vec<u32>, u32)> {
        let at = Self::ancestor_at(tree, node, level as u32 + 1);
        if tree.nodes[at as usize].depth as usize != level + 1 {
            return None;
        }
        let parent = tree.nodes[at as usize].parent;
        Some((
            tree.nodes[parent as usize]
                .children
                .iter()
                .map(|&c| tree.nodes[c as usize].pos)
                .collect(),
            tree.nodes[at as usize].pos,
        ))
    }

    fn base_pos(
        &self,
        field: u32,
        base: BaseItem,
        tree: &Tree,
        node: u32,
        level: usize,
    ) -> Option<u32> {
        match base {
            BaseItem::Item(item) => {
                let _ = field;
                Some(item)
            }
            BaseItem::Previous | BaseItem::Next => {
                let (siblings, me) = Self::siblings(tree, node, level)?;
                let at = siblings.iter().position(|&p| p == me)?;
                let other = if base == BaseItem::Previous {
                    at.checked_sub(1)
                } else {
                    Some(at + 1).filter(|&i| i < siblings.len())
                };
                // The first item has no previous one; Excel compares it with itself.
                Some(other.map_or(me, |i| siblings[i]))
            }
        }
    }

    fn cell(&self, row: &LineInfo, col: &LineInfo, d: usize) -> Out {
        let df = &self.table.data_fields[d];
        let summary = self.summary(row, col, d);
        let raw = self.raw(row.node, col.node, d, summary);
        let value = |r: u32, c: u32| self.raw(r, c, d, summary);
        // Where nothing falls under a total that has something, a share shows 0.
        let share = |whole: Out| match (&raw, &whole) {
            (_, Out::Empty) => Out::Empty,
            (Out::Empty, Out::Number(_)) => Out::Number(0.0),
            _ => divide(&raw, &whole),
        };
        match df.show_as {
            ShowAs::Normal => raw,
            ShowAs::PercentOfTotal => share(value(0, 0)),
            ShowAs::PercentOfRow => share(value(row.node, 0)),
            ShowAs::PercentOfCol => share(value(0, col.node)),
            ShowAs::Index => {
                if let Out::Error(e) = &raw {
                    return Out::Error(e.clone());
                }
                let (Some(v), Some(rt), Some(ct), Some(gt)) = (
                    raw.number().or(Some(0.0)),
                    value(row.node, 0).number(),
                    value(0, col.node).number(),
                    value(0, 0).number(),
                ) else {
                    return raw;
                };
                if rt == 0.0 || ct == 0.0 {
                    return Out::Error("#DIV/0!".to_string());
                }
                Out::Number(v * gt / (rt * ct))
            }
            ShowAs::PercentOfParentRow => {
                let parent = self.rows.nodes[row.node as usize].parent;
                share(value(parent, col.node))
            }
            ShowAs::PercentOfParentCol => {
                let parent = self.cols.nodes[col.node as usize].parent;
                share(value(row.node, parent))
            }
            _ => self.with_base(row, col, d, raw, summary),
        }
    }

    fn with_base(
        &self,
        row: &LineInfo,
        col: &LineInfo,
        d: usize,
        raw: Out,
        summary: Summary,
    ) -> Out {
        let df = &self.table.data_fields[d];
        let na = || Out::Error("#N/A".to_string());
        let Some(side) = self.side(df.base_field) else {
            return na();
        };
        let (tree, node, level) = match side {
            Side::Rows(level) => (self.rows, row.node, level),
            Side::Cols(level) => (self.cols, col.node, level),
        };
        let at_value = |other: u32| match side {
            Side::Rows(_) => self.raw(other, col.node, d, summary),
            Side::Cols(_) => self.raw(row.node, other, d, summary),
        };
        let depth = tree.nodes[node as usize].depth as usize;
        // A total above the base field's level.
        let above = depth <= level;
        match df.show_as {
            ShowAs::Percent | ShowAs::Difference | ShowAs::PercentDiff => {
                if above {
                    return Out::Empty;
                }
                match &raw {
                    Out::Error(e) => return Out::Error(e.clone()),
                    Out::Empty if df.show_as == ShowAs::Percent => return Out::Empty,
                    _ => {}
                }
                let Some(base) = self.base_pos(df.base_field, df.base_item, tree, node, level)
                else {
                    return na();
                };
                let me = Self::path(tree, node)[level];
                let Some(base_node) = Self::swap(tree, node, level, base) else {
                    return na();
                };
                let base_value = at_value(base_node);
                match df.show_as {
                    ShowAs::Percent => match (&raw, &base_value) {
                        (_, Out::Empty) => na(),
                        _ => divide(&raw, &base_value),
                    },
                    ShowAs::Difference => {
                        if me == base {
                            return Out::Empty;
                        }
                        match (&raw, &base_value) {
                            (Out::Error(e), _) | (_, Out::Error(e)) => Out::Error(e.clone()),
                            (Out::Empty, Out::Empty) => Out::Empty,
                            _ => Out::Number(
                                raw.number().unwrap_or(0.0) - base_value.number().unwrap_or(0.0),
                            ),
                        }
                    }
                    _ => {
                        if me == base {
                            return Out::Empty;
                        }
                        match (&raw, &base_value) {
                            (_, Out::Empty) => na(),
                            (Out::Error(e), _) | (_, Out::Error(e)) => Out::Error(e.clone()),
                            (_, Out::Number(b)) if *b == 0.0 => Out::Error("#DIV/0!".to_string()),
                            (_, Out::Number(b)) => {
                                Out::Number((raw.number().unwrap_or(0.0) - b) / b)
                            }
                        }
                    }
                }
            }
            ShowAs::RunTotal | ShowAs::PercentOfRunningTotal => {
                if above {
                    return Out::Empty;
                }
                let Some((siblings, me)) = Self::siblings(tree, node, level) else {
                    return raw;
                };
                let mut total = 0.0;
                let mut seen = false;
                for &pos in &siblings {
                    if let Some(other) = Self::swap(tree, node, level, pos) {
                        match at_value(other) {
                            Out::Error(e) => return Out::Error(e),
                            Out::Number(n) => {
                                total += n;
                                seen = true;
                            }
                            Out::Empty => {}
                        }
                    }
                    if pos == me {
                        break;
                    }
                }
                if df.show_as == ShowAs::RunTotal {
                    return if seen { Out::Number(total) } else { Out::Empty };
                }
                let whole = at_value(Self::ancestor_at(tree, node, level as u32));
                match whole {
                    Out::Empty => Out::Empty,
                    whole => divide(&Out::Number(total), &whole),
                }
            }
            ShowAs::PercentOfParent => {
                if depth < level + 1 {
                    return Out::Empty;
                }
                let parent = Self::ancestor_at(tree, node, level as u32 + 1);
                match (&raw, at_value(parent)) {
                    (_, Out::Empty) => Out::Empty,
                    (Out::Empty, Out::Number(_)) => Out::Number(0.0),
                    (_, whole) => divide(&raw, &whole),
                }
            }
            ShowAs::RankAscending | ShowAs::RankDescending => {
                if above {
                    return Out::Empty;
                }
                let Some((siblings, _)) = Self::siblings(tree, node, level) else {
                    return Out::Empty;
                };
                let Some(mine) = raw.number() else { return raw };
                let mut rank = 1;
                for &pos in &siblings {
                    if let Some(other) = Self::swap(tree, node, level, pos) {
                        if let Some(v) = at_value(other).number() {
                            let better = if df.show_as == ShowAs::RankDescending {
                                v > mine
                            } else {
                                v < mine
                            };
                            if better {
                                rank += 1;
                            }
                        }
                    }
                }
                Out::Number(f64::from(rank))
            }
            _ => raw,
        }
    }

    pub fn compute(&self) -> Results {
        let cols = self.col_lines.len();
        let mut cells = Vec::with_capacity(self.row_lines.len() * cols);
        for row in self.row_lines {
            let shows = !matches!(
                row.role,
                LineRole::Blank | LineRole::Header { values: false }
            );
            for col in self.col_lines {
                if !shows {
                    cells.push(None);
                    continue;
                }
                let d = if self.rows_have_values {
                    row.data as usize
                } else if self.cols_have_values {
                    col.data as usize
                } else {
                    0
                };
                if d >= self.table.data_fields.len() {
                    cells.push(None);
                    continue;
                }
                cells.push(Some((d, self.cell(row, col, d))));
            }
        }
        Results { cols, cells }
    }
}
