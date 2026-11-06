//! A pivot table's layout worked out as Excel does it: the tree of items on each axis, the lines it
//! makes in compact, outline and tabular form, what each cell sums up, and where it all goes.

use std::cmp::Ordering;
use std::collections::HashMap;

use cgs_model::pivot::{
    AxisLine, CacheValue, FieldSort, ItemType, PivotAxis, PivotField, PivotItem, PivotLocation,
    PivotTable, Summary, VALUES_FIELD,
};
use cgs_model::Rect;

use crate::agg::{Acc, Out};
use crate::show_as;
use crate::view::CacheView;
use crate::{Captions, CellValue, OutCell, Role};

/// What a line of an axis is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRole {
    /// A leaf, or an item whose details are hidden.
    Data,
    /// An item above its children in outline form; with values when subtotals go at the top.
    Header {
        values: bool,
    },
    Subtotal(ItemType),
    Grand,
    Blank,
}

/// One row or column of the output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineInfo {
    pub role: LineRole,
    /// The item index at each level it names, the values level holding the value field.
    pub members: Vec<u32>,
    /// Leading members that are the same as the line before's.
    pub repeat: u32,
    /// The value field, when the values are on this axis.
    pub data: u32,
    /// The tree node whose records it sums.
    pub node: u32,
}

impl LineInfo {
    pub fn level(&self) -> usize {
        self.members.len().saturating_sub(1)
    }

    fn axis_line(&self) -> AxisLine {
        let (kind, repeat, members) = match self.role {
            LineRole::Grand => (ItemType::Grand, 0, vec![0]),
            LineRole::Data | LineRole::Header { .. } => (
                ItemType::Data,
                self.repeat,
                self.members[self.repeat as usize..].to_vec(),
            ),
            LineRole::Subtotal(kind) => (
                kind,
                self.repeat,
                self.members[self.repeat as usize..].to_vec(),
            ),
            LineRole::Blank => (
                ItemType::Blank,
                self.repeat,
                self.members[self.repeat as usize..].to_vec(),
            ),
        };
        AxisLine {
            kind,
            repeat,
            data: self.data,
            members,
        }
    }
}

/// An axis's fields and lines, for drawing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AxisInfo {
    pub fields: Vec<i32>,
    pub lines: Vec<LineInfo>,
    /// The column of the row area each row field's labels go in.
    pub columns: Vec<u32>,
}

/// A refreshed pivot table.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub location: PivotLocation,
    /// Everything the pivot table covers, report filters included.
    pub area: Rect,
    pub row_items: Vec<AxisLine>,
    pub col_items: Vec<AxisLine>,
    pub rows: AxisInfo,
    pub cols: AxisInfo,
    pub cells: Vec<OutCell>,
    /// Items for each field, where they had to be made or changed.
    pub items: Vec<Option<Vec<PivotItem>>>,
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub parent: u32,
    pub depth: u32,
    pub pos: u32,
    pub children: Vec<u32>,
}

pub(crate) struct Tree {
    /// Cache field per real level.
    pub levels: Vec<usize>,
    pub nodes: Vec<Node>,
    pub child: HashMap<(u32, u32), u32>,
}

impl Tree {
    fn new(levels: Vec<usize>) -> Self {
        Self {
            levels,
            nodes: vec![Node {
                parent: 0,
                depth: 0,
                pos: 0,
                children: Vec::new(),
            }],
            child: HashMap::new(),
        }
    }

    fn get_or_add(&mut self, parent: u32, pos: u32) -> u32 {
        if let Some(&id) = self.child.get(&(parent, pos)) {
            return id;
        }
        let id = self.nodes.len() as u32;
        let depth = self.nodes[parent as usize].depth + 1;
        self.nodes.push(Node {
            parent,
            depth,
            pos,
            children: Vec::new(),
        });
        self.nodes[parent as usize].children.push(id);
        self.child.insert((parent, pos), id);
        id
    }

    /// The node and all its ancestors, root included.
    fn chain(&self, mut node: u32, out: &mut Vec<u32>) {
        out.clear();
        loop {
            out.push(node);
            if node == 0 {
                break;
            }
            node = self.nodes[node as usize].parent;
        }
    }
}

/// Excel's order for items: numbers and dates, then text without case, then logicals, then errors,
/// with blanks last.
pub fn compare_values(a: &CacheValue, b: &CacheValue) -> Ordering {
    fn rank(v: &CacheValue) -> u8 {
        match v {
            CacheValue::Number(_) | CacheValue::Date(_) => 0,
            CacheValue::Text(_) => 1,
            CacheValue::Bool(_) => 2,
            CacheValue::Error(_) => 3,
            CacheValue::Missing => 4,
        }
    }
    match (a, b) {
        (
            CacheValue::Number(x) | CacheValue::Date(x),
            CacheValue::Number(y) | CacheValue::Date(y),
        ) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (CacheValue::Text(x), CacheValue::Text(y)) => {
            let fold = |s: &str| s.to_lowercase();
            fold(x).cmp(&fold(y))
        }
        (CacheValue::Bool(x), CacheValue::Bool(y)) => x.cmp(y),
        (CacheValue::Error(x), CacheValue::Error(y)) => x.cmp(y),
        _ => rank(a).cmp(&rank(b)),
    }
}

/// A field's items in Excel's starting order, with its automatic subtotal.
pub fn default_items(values: &[CacheValue]) -> Vec<PivotItem> {
    let mut order: Vec<u32> = (0..values.len() as u32).collect();
    order.sort_by(|&a, &b| compare_values(&values[a as usize], &values[b as usize]));
    let mut items: Vec<PivotItem> = order.into_iter().map(PivotItem::data).collect();
    items.push(PivotItem::total(ItemType::Default));
    items
}

struct Ctx<'a> {
    format: &'a dyn Fn(f64, Option<u32>) -> String,
    table: &'a PivotTable,
    view: &'a CacheView<'a>,
    captions: &'a Captions,
    /// Per field, its items as used.
    items: Vec<std::borrow::Cow<'a, [PivotItem]>>,
    /// Per field, the item index showing each cache item.
    pos_of_x: Vec<Vec<Option<u32>>>,
}

impl Ctx<'_> {
    fn field(&self, f: usize) -> &PivotField {
        &self.table.fields[f]
    }

    fn item_value(&self, f: usize, pos: u32) -> Option<&CacheValue> {
        let x = self.items[f].get(pos as usize)?.x?;
        self.view.items(f).get(x as usize)
    }

    /// An item's label as a cell shows it.
    fn label(&self, f: usize, pos: u32) -> CellValue {
        let item = &self.items[f][pos as usize];
        if let Some(name) = &item.name {
            return CellValue::Text(name.clone());
        }
        match self.item_value(f, pos) {
            Some(CacheValue::Text(t)) => CellValue::Text(t.clone()),
            Some(CacheValue::Number(n) | CacheValue::Date(n)) => CellValue::Number(*n),
            Some(CacheValue::Bool(b)) => CellValue::Text(if *b { "TRUE" } else { "FALSE" }.into()),
            Some(CacheValue::Error(e)) => CellValue::Error(e.clone()),
            Some(CacheValue::Missing) | None => CellValue::Text(self.captions.blank.clone()),
        }
    }

    /// An item's label as text, for "North Total".
    fn label_text(&self, f: usize, pos: u32) -> String {
        match self.label(f, pos) {
            CellValue::Text(t) => t,
            CellValue::Number(n) => (self.format)(n, self.label_format(f)),
            CellValue::Bool(b) => if b { "TRUE" } else { "FALSE" }.to_string(),
            CellValue::Error(e) => e,
        }
    }

    fn label_format(&self, f: usize) -> Option<u32> {
        let field = self.field(f);
        field
            .num_fmt_id
            .or(Some(self.view.cache.fields[f].num_fmt_id))
            .filter(|&id| id != 0)
    }

    fn field_caption(&self, f: i32) -> String {
        if f == VALUES_FIELD {
            return self.table.options.data_caption.clone();
        }
        let f = f as usize;
        self.field(f)
            .name
            .clone()
            .unwrap_or_else(|| self.view.cache.fields[f].name.clone())
    }
}

/// Which records the filters let through.
fn passing(ctx: &Ctx<'_>) -> Vec<bool> {
    let table = ctx.table;
    let count = ctx.view.count();
    let mut pass = vec![true; count];
    for (f, field) in table.fields.iter().enumerate() {
        let on_axis = matches!(
            field.axis,
            Some(PivotAxis::Row | PivotAxis::Col | PivotAxis::Page)
        );
        if !on_axis {
            continue;
        }
        let page = table.page_fields.iter().find(|p| p.field as usize == f);
        let chosen = page.and_then(|p| p.item);
        let any_hidden = ctx.items[f].iter().any(|i| i.hidden);
        if chosen.is_none() && !any_hidden {
            continue;
        }
        for (rec, ok) in pass.iter_mut().enumerate() {
            if !*ok {
                continue;
            }
            let pos = ctx
                .view
                .item(f, rec)
                .and_then(|x| ctx.pos_of_x[f].get(x as usize).copied().flatten());
            *ok = match (pos, chosen) {
                (Some(p), Some(c)) => p == c,
                (Some(p), None) => !ctx.items[f][p as usize].hidden,
                (None, _) => false,
            };
        }
    }
    pass
}

struct AxisWork {
    fields: Vec<i32>,
    tree: Tree,
    values_at: Option<usize>,
}

fn axis_work(fields: &[i32]) -> AxisWork {
    let levels: Vec<usize> = fields
        .iter()
        .filter(|&&f| f != VALUES_FIELD)
        .map(|&f| f as usize)
        .collect();
    AxisWork {
        fields: fields.to_vec(),
        tree: Tree::new(levels),
        values_at: fields.iter().position(|&f| f == VALUES_FIELD),
    }
}

/// Adds every visible item under each parent of a field that shows items with no data.
fn add_empty_items(ctx: &Ctx<'_>, tree: &mut Tree) {
    for level in 0..tree.levels.len() {
        let f = tree.levels[level];
        if !ctx.field(f).show_all {
            continue;
        }
        let parents: Vec<u32> = (0..tree.nodes.len() as u32)
            .filter(|&n| tree.nodes[n as usize].depth as usize == level)
            .collect();
        for parent in parents {
            for (pos, item) in ctx.items[f].iter().enumerate() {
                if item.kind == ItemType::Data && !item.hidden && !item.missing {
                    tree.get_or_add(parent, pos as u32);
                }
            }
        }
    }
}

pub(crate) type Grid = HashMap<(u32, u32), Vec<Acc>>;

fn result(grid: &Grid, row: u32, col: u32, d: usize, summary: Summary) -> Out {
    grid.get(&(row, col))
        .and_then(|accs| accs.get(d))
        .map_or(Out::Empty, |acc| acc.result(summary))
}

/// The order items last showed in, per level of an axis, from the lines the file holds.
fn shown_order(lines: &[AxisLine], fields: &[i32]) -> Vec<HashMap<u32, u32>> {
    let mut out: Vec<HashMap<u32, u32>> = vec![HashMap::new(); fields.len()];
    for line in lines {
        if line.kind == ItemType::Grand {
            continue;
        }
        for (k, &member) in line.members.iter().enumerate() {
            let level = line.repeat as usize + k;
            if let Some(seen) = out.get_mut(level) {
                let next = seen.len() as u32;
                seen.entry(member).or_insert(next);
            }
        }
    }
    out
}

fn sort_children(
    ctx: &Ctx<'_>,
    tree: &mut Tree,
    grid: &Grid,
    rows: bool,
    shown: &[HashMap<u32, u32>],
    fields: &[i32],
) {
    // Levels of the tree are the axis's fields without the values; find each one's place.
    let real_at: Vec<usize> = fields
        .iter()
        .enumerate()
        .filter(|(_, &f)| f != VALUES_FIELD)
        .map(|(i, _)| i)
        .collect();
    for n in 0..tree.nodes.len() {
        let depth = tree.nodes[n].depth as usize;
        if depth >= tree.levels.len() || tree.nodes[n].children.is_empty() {
            continue;
        }
        let f = tree.levels[depth];
        let field = ctx.field(f);
        let mut children = std::mem::take(&mut tree.nodes[n].children);
        let pos = |id: &u32| tree.nodes[*id as usize].pos;
        match (field.sort, field.sort_by_data) {
            (FieldSort::Manual, _) => children.sort_by_key(pos),
            (order, Some(d)) => {
                let d = d as usize;
                let summary = ctx
                    .table
                    .data_fields
                    .get(d)
                    .map_or(Summary::Sum, |df| df.summary);
                // A share of its own line's total is the same for every item.
                let flat = ctx.table.data_fields.get(d).is_some_and(|df| {
                    use cgs_model::pivot::ShowAs;
                    matches!(
                        (df.show_as, rows),
                        (ShowAs::PercentOfRow, true) | (ShowAs::PercentOfCol, false)
                    )
                });
                let value = |id: u32| {
                    if flat {
                        return Some(1.0);
                    }
                    let r = if rows {
                        result(grid, id, 0, d, summary)
                    } else {
                        result(grid, 0, id, d, summary)
                    };
                    r.number()
                };
                children.sort_by(|a, b| {
                    let (va, vb) = (value(*a), value(*b));
                    // Items with nothing to sort by go last either way.
                    match (va, vb) {
                        (Some(_), None) => return Ordering::Less,
                        (None, Some(_)) => return Ordering::Greater,
                        _ => {}
                    }
                    let ord = match (va, vb) {
                        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
                        _ => Ordering::Equal,
                    };
                    let ord = if order == FieldSort::Descending {
                        ord.reverse()
                    } else {
                        ord
                    };
                    // Ties keep the order they last showed in, and new ones go by label.
                    let before = real_at.get(depth).and_then(|&l| shown.get(l));
                    let rank = |id: &u32| {
                        before
                            .and_then(|m| m.get(&pos(id)).copied())
                            .unwrap_or(u32::MAX)
                    };
                    ord.then(rank(a).cmp(&rank(b)))
                        .then_with(|| {
                            match (ctx.item_value(f, pos(a)), ctx.item_value(f, pos(b))) {
                                (Some(x), Some(y)) => compare_values(x, y),
                                _ => Ordering::Equal,
                            }
                        })
                        .then(pos(a).cmp(&pos(b)))
                });
            }
            (order, None) => {
                children.sort_by(|a, b| {
                    let va = ctx.item_value(f, pos(a));
                    let vb = ctx.item_value(f, pos(b));
                    let ord = match (va, vb) {
                        (Some(x), Some(y)) => compare_values(x, y),
                        _ => Ordering::Equal,
                    };
                    let ord = if order == FieldSort::Descending {
                        ord.reverse()
                    } else {
                        ord
                    };
                    ord.then(pos(a).cmp(&pos(b)))
                });
            }
        }
        tree.nodes[n].children = children;
    }
}

struct Emitter<'a> {
    ctx: &'a Ctx<'a>,
    work: &'a AxisWork,
    /// Rows use outline form where fields ask for it; columns are always tabular.
    rows: bool,
    data_count: u32,
    out: Vec<LineInfo>,
}

impl Emitter<'_> {
    fn push(&mut self, role: LineRole, members: Vec<u32>, data: u32, node: u32) {
        self.out.push(LineInfo {
            role,
            members,
            repeat: 0,
            data,
            node,
        });
    }

    fn emit(&mut self, node: u32, level: usize, prefix: &[u32], data: u32) {
        let fields = &self.work.fields;
        let last = fields.len() - 1;
        if fields[level] == VALUES_FIELD {
            for d in 0..self.data_count {
                let mut members = prefix.to_vec();
                members.push(d);
                if level == last {
                    self.push(LineRole::Data, members, d, node);
                } else {
                    self.emit(node, level + 1, &members, d);
                }
            }
            return;
        }
        let f = fields[level] as usize;
        let field = self.ctx.field(f);
        let deeper_real = fields[level + 1..].iter().any(|&g| g != VALUES_FIELD);
        let values_below = fields[level + 1..].contains(&VALUES_FIELD);
        let children = self.work.tree.nodes[node as usize].children.clone();
        for child in children {
            let pos = self.work.tree.nodes[child as usize].pos;
            let mut members = prefix.to_vec();
            members.push(pos);
            let item = &self.ctx.items[f][pos as usize];
            if level == last {
                self.push(LineRole::Data, members, data, child);
                continue;
            }
            if !item.show_details {
                if values_below {
                    // The value lines still follow, with nothing below them.
                    self.emit_values_only(child, level + 1, &members);
                } else {
                    self.push(LineRole::Data, members, data, child);
                }
                continue;
            }
            let outline = self.rows && field.outline;
            let custom = !field.subtotals.is_empty();
            let top = outline && field.subtotal_top && field.default_subtotal && !custom;
            if outline {
                let values = top && deeper_real && !values_below;
                self.push(LineRole::Header { values }, members.clone(), data, child);
            }
            self.emit(child, level + 1, &members, data);
            if deeper_real {
                let kinds: Vec<ItemType> = if custom {
                    field.subtotals.clone()
                } else if field.default_subtotal && !top {
                    vec![ItemType::Default]
                } else {
                    Vec::new()
                };
                for kind in kinds {
                    if values_below {
                        for d in 0..self.data_count {
                            self.push(LineRole::Subtotal(kind), members.clone(), d, child);
                        }
                    } else {
                        self.push(LineRole::Subtotal(kind), members.clone(), data, child);
                    }
                }
            }
            if field.insert_blank_row && self.rows {
                self.push(LineRole::Blank, members, data, child);
            }
        }
    }

    /// Below a collapsed item: one line per value field when the values come next.
    fn emit_values_only(&mut self, node: u32, level: usize, prefix: &[u32]) {
        if self.work.fields.get(level) == Some(&VALUES_FIELD) {
            for d in 0..self.data_count {
                let mut members = prefix.to_vec();
                members.push(d);
                self.push(LineRole::Data, members, d, node);
            }
        } else {
            self.push(LineRole::Data, prefix.to_vec(), 0, node);
        }
    }
}

fn lines(ctx: &Ctx<'_>, work: &AxisWork, rows: bool, grand: bool) -> Vec<LineInfo> {
    let data_count = ctx.table.data_fields.len() as u32;
    let mut emitter = Emitter {
        ctx,
        work,
        rows,
        data_count,
        out: Vec::new(),
    };
    let real = work.fields.iter().any(|&f| f != VALUES_FIELD);
    if work.fields.is_empty() {
        emitter.push(LineRole::Data, Vec::new(), 0, 0);
    } else {
        emitter.emit(0, 0, &[], 0);
        if real && grand {
            if work.values_at.is_some() {
                for d in 0..data_count {
                    emitter.push(LineRole::Grand, vec![0], d, 0);
                }
            } else {
                emitter.push(LineRole::Grand, vec![0], 0, 0);
            }
        }
    }
    let mut out = emitter.out;
    // Leading members the same as the line before's, leaving at least one.
    let mut previous: Vec<u32> = Vec::new();
    for line in &mut out {
        if line.role == LineRole::Grand {
            previous.clear();
            continue;
        }
        let common = line
            .members
            .iter()
            .zip(&previous)
            .take_while(|(a, b)| a == b)
            .count();
        line.repeat = common.min(line.members.len().saturating_sub(1)) as u32;
        previous = line.members.clone();
    }
    out
}

/// Which column of the row area each row field's labels go in.
fn row_columns(ctx: &Ctx<'_>, fields: &[i32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(fields.len());
    let mut col = 0u32;
    for (i, &f) in fields.iter().enumerate() {
        if i > 0 {
            let prev = fields[i - 1];
            let shares = if prev == VALUES_FIELD {
                ctx.table.options.compact_data && ctx.table.options.outline_data
            } else {
                let field = ctx.field(prev as usize);
                field.compact && field.outline
            };
            if !shares {
                col += 1;
            }
        }
        let _ = f;
        out.push(col);
    }
    out
}

/// Works out a pivot table from its cache. `anchor` is the top-left corner the table keeps.
/// `format` shows a number in a number format, by id, for labels such as "1/5/2024 Total".
pub fn layout(
    table: &PivotTable,
    view: &CacheView<'_>,
    captions: &Captions,
    format: &dyn Fn(f64, Option<u32>) -> String,
) -> Result<Layout, String> {
    let cache = view.cache;
    if table.fields.len() != cache.fields.len() {
        return Err("its fields don't match its cache".to_string());
    }
    // Items: as the file has them, or Excel's starting order for a field that has none.
    let mut made: Vec<Option<Vec<PivotItem>>> = vec![None; table.fields.len()];
    let mut items: Vec<std::borrow::Cow<'_, [PivotItem]>> = Vec::new();
    for (f, field) in table.fields.iter().enumerate() {
        let on_axis = matches!(
            field.axis,
            Some(PivotAxis::Row | PivotAxis::Col | PivotAxis::Page)
        );
        if on_axis && !view.has_items(f) {
            return Err(format!(
                "field {} has no items",
                cache.fields.get(f).map_or("", |c| c.name.as_str())
            ));
        }
        if on_axis && !field.items.iter().any(|i| i.kind == ItemType::Data) {
            let fresh = default_items(view.items(f));
            made[f] = Some(fresh.clone());
            items.push(std::borrow::Cow::Owned(fresh));
        } else {
            items.push(std::borrow::Cow::Borrowed(&field.items));
        }
    }
    let pos_of_x: Vec<Vec<Option<u32>>> = items
        .iter()
        .enumerate()
        .map(|(f, list)| {
            let mut map = vec![None; view.items(f).len()];
            for (pos, item) in list.iter().enumerate() {
                if let (ItemType::Data, Some(x)) = (item.kind, item.x) {
                    if let Some(slot) = map.get_mut(x as usize) {
                        *slot = Some(pos as u32);
                    }
                }
            }
            map
        })
        .collect();
    let ctx = Ctx {
        format,
        table,
        view,
        captions,
        items,
        pos_of_x,
    };
    for df in &table.data_fields {
        if df.field as usize >= table.fields.len() {
            return Err("a value field has no field".to_string());
        }
    }
    let pass = passing(&ctx);

    let mut rows = axis_work(&table.row_fields);
    let mut cols = axis_work(&table.col_fields);
    let nd = table.data_fields.len();
    let mut grid: Grid = HashMap::new();
    let mut row_chain = Vec::new();
    let mut col_chain = Vec::new();
    let pos_at = |f: usize, rec: usize| -> Option<u32> {
        let x = view.item(f, rec)?;
        ctx.pos_of_x[f].get(x as usize).copied().flatten()
    };
    for rec in 0..view.count() {
        if !pass[rec] {
            continue;
        }
        let mut leaf = 0u32;
        let mut ok = true;
        for level in 0..rows.tree.levels.len() {
            let Some(pos) = pos_at(rows.tree.levels[level], rec) else {
                ok = false;
                break;
            };
            leaf = rows.tree.get_or_add(leaf, pos);
        }
        let mut col_leaf = 0u32;
        for level in 0..cols.tree.levels.len() {
            let Some(pos) = pos_at(cols.tree.levels[level], rec) else {
                ok = false;
                break;
            };
            col_leaf = cols.tree.get_or_add(col_leaf, pos);
        }
        if !ok {
            continue;
        }
        rows.tree.chain(leaf, &mut row_chain);
        cols.tree.chain(col_leaf, &mut col_chain);
        for &r in &row_chain {
            for &c in &col_chain {
                let accs = grid
                    .entry((r, c))
                    .or_insert_with(|| vec![Acc::default(); nd.max(1)]);
                for (d, df) in table.data_fields.iter().enumerate() {
                    accs[d].add(view.value(df.field as usize, rec));
                }
                if nd == 0 {
                    accs[0].add(&CacheValue::Missing);
                }
            }
        }
    }
    add_empty_items(&ctx, &mut rows.tree);
    add_empty_items(&ctx, &mut cols.tree);
    let row_shown = shown_order(&table.row_items, &table.row_fields);
    let col_shown = shown_order(&table.col_items, &table.col_fields);
    sort_children(
        &ctx,
        &mut rows.tree,
        &grid,
        true,
        &row_shown,
        &table.row_fields,
    );
    sort_children(
        &ctx,
        &mut cols.tree,
        &grid,
        false,
        &col_shown,
        &table.col_fields,
    );

    let row_lines = lines(&ctx, &rows, true, table.options.row_grand_totals);
    let col_lines = lines(&ctx, &cols, false, table.options.col_grand_totals);

    // Geometry.
    let r_fields = &table.row_fields;
    let c_fields = &table.col_fields;
    let real_cols = c_fields.iter().any(|&f| f != VALUES_FIELD);
    let values_on_cols = c_fields.contains(&VALUES_FIELD);
    let columns = row_columns(&ctx, r_fields);
    let width = if r_fields.is_empty() {
        u32::from(real_cols)
    } else {
        columns.last().map_or(0, |c| c + 1)
    };
    let classic = table.options.grid_drop_zones;
    let hide_values = table.options.hide_values_row && !classic;
    let header_rows: u32 = if real_cols {
        1 + c_fields.len() as u32
    } else if values_on_cols {
        if hide_values {
            1
        } else {
            2
        }
    } else if classic {
        2
    } else {
        1
    };
    let first_header_row = if !real_cols && values_on_cols && hide_values {
        0
    } else if classic && c_fields.is_empty() {
        2
    } else {
        1
    };
    let pages = table.page_fields.len() as u32;
    let mut top = table.location.range.top;
    let left = table.location.range.left;
    if pages > 0 && top < pages + 2 {
        top = pages + 2;
    }
    let data_rows = row_lines.len() as u32;
    let data_cols = if nd == 0 && c_fields.is_empty() {
        0
    } else {
        col_lines.len() as u32
    };
    let range = Rect {
        top,
        left,
        bottom: top + header_rows + data_rows - 1,
        right: (left + width + data_cols).max(left + 1) - 1,
    };
    let location = PivotLocation {
        range,
        first_header_row,
        first_data_row: header_rows,
        first_data_col: width,
        row_page_count: pages,
        col_page_count: u32::from(pages > 0),
    };

    let mut cells: Vec<OutCell> = Vec::new();
    let show_headers = table.options.show_headers;
    let text = |s: &str| CellValue::Text(s.to_string());
    let mut put = |row: u32, col: u32, value: CellValue, role: Role, num_fmt: Option<u32>| {
        cells.push(OutCell {
            row,
            col,
            value,
            role,
            num_fmt,
        });
    };

    // Report filters, above with a row between.
    for (i, page) in table.page_fields.iter().enumerate() {
        let row = top - 1 - pages + i as u32;
        let f = page.field as usize;
        put(
            row,
            left,
            text(&ctx.field_caption(page.field as i32)),
            Role::PageLabel,
            None,
        );
        let shown: Vec<u32> = (0..ctx.items[f].len() as u32)
            .filter(|&p| {
                let item = &ctx.items[f][p as usize];
                item.kind == ItemType::Data && !item.hidden
            })
            .collect();
        let several = ctx.field(f).multiple_item_selection
            && ctx.items[f]
                .iter()
                .any(|i| i.kind == ItemType::Data && i.hidden);
        let value = match page.item {
            Some(pos) if (pos as usize) < ctx.items[f].len() => ctx.label(f, pos),
            _ if several && shown.len() == 1 => ctx.label(f, shown[0]),
            _ if several => text(&captions.multiple),
            _ => text(&captions.all),
        };
        let fmt = ctx.label_format(f);
        put(row, left + 1, value, Role::PageValue, fmt);
    }

    let data_name = |d: usize| table.data_fields.get(d).map(|df| df.name.clone());
    // The top-left caption: the one value field's name, when columns have fields.
    if (real_cols || (classic && c_fields.is_empty())) && nd == 1 && !r_fields.is_empty() {
        put(
            top,
            left,
            text(&data_name(0).unwrap_or_default()),
            Role::DataCaption,
            None,
        );
    }
    // "Column Labels", or the column fields' names.
    if show_headers && (real_cols || (values_on_cols && !hide_values)) {
        let row = top + header_rows - c_fields.len() as u32 - 1;
        let compact = c_fields
            .iter()
            .filter(|&&f| f != VALUES_FIELD)
            .all(|&f| ctx.field(f as usize).compact && ctx.field(f as usize).outline);
        if !real_cols {
            put(
                row,
                left + width,
                text(&table.options.data_caption),
                Role::Button,
                None,
            );
        } else if compact {
            let caption = table
                .options
                .col_header_caption
                .clone()
                .unwrap_or_else(|| captions.col_labels.clone());
            put(row, left + width, text(&caption), Role::Button, None);
        } else {
            for (j, &f) in c_fields.iter().enumerate() {
                put(
                    row,
                    left + width + j as u32,
                    text(&ctx.field_caption(f)),
                    Role::Button,
                    None,
                );
            }
        }
    }
    // Row fields' captions, on the last header row.
    let caption_row = top + header_rows - 1;
    if show_headers && !r_fields.is_empty() {
        let mut col = 0;
        while col < width {
            let here: Vec<i32> = r_fields
                .iter()
                .zip(&columns)
                .filter(|(_, &c)| c == col)
                .map(|(&f, _)| f)
                .collect();
            if let Some(&first) = here.first() {
                // In a compact table the first column says "Row Labels", and another names its
                // first field only when that field is compact.
                let compact = col == 0 && table.options.compact;
                let named = here
                    .iter()
                    .all(|&f| f != VALUES_FIELD && ctx.field(f as usize).compact);
                if col > 0 && table.options.compact && !named {
                    col += 1;
                    continue;
                }
                let caption = if compact {
                    table
                        .options
                        .row_header_caption
                        .clone()
                        .unwrap_or_else(|| captions.row_labels.clone())
                } else {
                    ctx.field_caption(first)
                };
                put(caption_row, left + col, text(&caption), Role::Button, None);
            }
            col += 1;
        }
    }
    // The single value column's caption, when nothing else heads it: in classic layout, its total.
    if !real_cols && !values_on_cols && nd == 1 {
        let caption = if classic {
            captions.total.clone()
        } else {
            data_name(0).unwrap_or_default()
        };
        put(
            caption_row,
            left + width,
            text(&caption),
            Role::DataCaption,
            None,
        );
    }

    let total_label = |label: String, kind: ItemType, data: Option<usize>| -> String {
        match kind {
            ItemType::Default => match data {
                Some(d) => format!("{label} {}", data_name(d).unwrap_or_default()),
                None => format!("{label} {}", captions.total),
            },
            other => {
                let summary = other.function().unwrap_or(Summary::Sum);
                format!("{label} {}", captions.function(summary))
            }
        }
    };
    let grand_label = |data: Option<usize>| -> String {
        match (data, table.options.grand_total_caption.as_ref()) {
            (Some(d), _) => format!("{} {}", captions.total, data_name(d).unwrap_or_default()),
            (None, Some(caption)) => caption.clone(),
            (None, None) => captions.grand_total.clone(),
        }
    };

    // Column labels.
    let first_item_row = top + header_rows - c_fields.len() as u32;
    for (k, line) in col_lines.iter().enumerate() {
        let col = left + width + k as u32;
        let data_here = values_on_cols.then_some(line.data as usize);
        match line.role {
            LineRole::Grand => {
                let data = data_here.filter(|_| nd > 1 || values_on_cols);
                put(
                    first_item_row,
                    col,
                    text(&grand_label(data)),
                    Role::ColLabel,
                    None,
                );
            }
            LineRole::Subtotal(kind) => {
                let level = line.level();
                let f = c_fields[level];
                let label = ctx.label_text(f as usize, line.members[level]);
                let data = data_here.filter(|_| cols.values_at.is_some_and(|v| v > level));
                put(
                    first_item_row + level as u32,
                    col,
                    text(&total_label(label, kind, data)),
                    Role::ColLabel,
                    None,
                );
            }
            _ => {
                for level in line.repeat as usize..line.members.len() {
                    let f = c_fields[level];
                    let value = if f == VALUES_FIELD {
                        text(&data_name(line.members[level] as usize).unwrap_or_default())
                    } else {
                        ctx.label(f as usize, line.members[level])
                    };
                    let fmt = (f != VALUES_FIELD)
                        .then(|| ctx.label_format(f as usize))
                        .flatten();
                    put(
                        first_item_row + level as u32,
                        col,
                        value,
                        Role::ColLabel,
                        fmt,
                    );
                }
            }
        }
    }
    // With no row fields, the one data row is named after the value field.
    if r_fields.is_empty() && real_cols && nd == 1 {
        put(
            top + header_rows,
            left,
            text(&data_name(0).unwrap_or_default()),
            Role::RowLabel { indent: 0 },
            None,
        );
    }

    // Row labels.
    let indent_of = |level: usize| -> u8 {
        let col = columns[level];
        columns[..level].iter().filter(|&&c| c == col).count() as u8
    };
    for (k, line) in row_lines.iter().enumerate() {
        let row = top + header_rows + k as u32;
        let data_here = rows.values_at.is_some().then_some(line.data as usize);
        match line.role {
            LineRole::Blank => {}
            LineRole::Grand => {
                let data = data_here;
                put(
                    row,
                    left,
                    text(&grand_label(data)),
                    Role::RowLabel { indent: 0 },
                    None,
                );
            }
            LineRole::Subtotal(kind) => {
                let level = line.level();
                let f = r_fields[level];
                let label = ctx.label_text(f as usize, line.members[level]);
                let data = data_here.filter(|_| rows.values_at.is_some_and(|v| v > level));
                put(
                    row,
                    left + columns[level],
                    text(&total_label(label, kind, data)),
                    Role::RowLabel {
                        indent: indent_of(level),
                    },
                    None,
                );
            }
            _ => {
                if r_fields.is_empty() {
                    continue;
                }
                for level in 0..line.members.len() {
                    let f = r_fields[level];
                    let repeated = level < line.repeat as usize;
                    if repeated
                        && (f == VALUES_FIELD
                            || !ctx.field(f as usize).repeat_labels
                            || matches!(line.role, LineRole::Header { .. })
                            || columns[level] == columns[line.level()])
                    {
                        continue;
                    }
                    let value = if f == VALUES_FIELD {
                        text(&data_name(line.members[level] as usize).unwrap_or_default())
                    } else {
                        ctx.label(f as usize, line.members[level])
                    };
                    let fmt = (f != VALUES_FIELD)
                        .then(|| ctx.label_format(f as usize))
                        .flatten();
                    put(
                        row,
                        left + columns[level],
                        value,
                        Role::RowLabel {
                            indent: indent_of(level),
                        },
                        fmt,
                    );
                }
            }
        }
    }

    // Values.
    if nd > 0 {
        let values = show_as::Values {
            table,
            rows: &rows.tree,
            cols: &cols.tree,
            grid: &grid,
            row_lines: &row_lines,
            col_lines: &col_lines,
            rows_have_values: rows.values_at.is_some(),
            cols_have_values: cols.values_at.is_some(),
        };
        let results = values.compute();
        for (i, line) in row_lines.iter().enumerate() {
            for (j, _) in col_lines.iter().enumerate() {
                let Some((d, out)) = results.get(i, j) else {
                    continue;
                };
                let row = top + header_rows + i as u32;
                let col = left + width + j as u32;
                let fmt = table.data_fields[d].num_fmt_id.filter(|&id| id != 0);
                let role = Role::Data { field: d as u16 };
                match out {
                    Out::Empty => {
                        if let (true, Some(caption)) = (
                            table.options.show_missing,
                            table.options.missing_caption.as_ref(),
                        ) {
                            if !caption.is_empty() {
                                put(row, col, text(caption), role, fmt);
                            }
                        }
                    }
                    Out::Number(n) => put(row, col, CellValue::Number(*n), role, fmt),
                    Out::Error(e) => {
                        let shown = match (table.options.show_error, &table.options.error_caption) {
                            (true, Some(caption)) => text(caption),
                            _ => CellValue::Error(e.clone()),
                        };
                        put(row, col, shown, role, fmt);
                    }
                }
                let _ = line;
            }
        }
    }

    let area = Rect {
        top: if pages > 0 { top - 1 - pages } else { top },
        left,
        bottom: range.bottom,
        right: if pages > 0 {
            range.right.max(left + 1)
        } else {
            range.right
        },
    };
    Ok(Layout {
        location,
        area,
        row_items: row_lines.iter().map(LineInfo::axis_line).collect(),
        col_items: col_lines.iter().map(LineInfo::axis_line).collect(),
        rows: AxisInfo {
            fields: r_fields.clone(),
            lines: row_lines,
            columns,
        },
        cols: AxisInfo {
            fields: c_fields.clone(),
            lines: col_lines,
            columns: Vec::new(),
        },
        cells,
        items: made,
    })
}

pub(crate) type AccGrid = Grid;
