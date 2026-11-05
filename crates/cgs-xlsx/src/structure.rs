//! Carries changes to a workbook's shape into its parts: rows and columns inserted or deleted, and
//! sheets renamed, added or deleted. This runs before the cell patcher, so that the patcher sees a
//! worksheet whose addresses and formulas are already where they end up, and its patches can all
//! be in final coordinates. What isn't about position stays exactly as the file has it.

use std::borrow::Cow;
use std::io::{self, Read, Write};

use cgs_refs::{apply_edits, edit_rect, edit_sqref, Axis, AxisEdit, BookEdit, SharedFormula};
use rustc_hash::FxHashMap;

use crate::patch_sheet::cell_name;
use crate::scan::{attr_value, edit_tag, escape_text, local_name, Kind, Scanner};
use crate::xml::{parse_cell, parse_rect, parse_u32, unescape_attr};

/// One change to the workbook's shape, in the order they were made. Sheets are the book's stable
/// indexes, and names are as they stood when the change was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookOp {
    Axis {
        sheet: u32,
        name: String,
        edit: AxisEdit,
    },
    Rename {
        sheet: u32,
        old: String,
        new: String,
    },
    Delete {
        sheet: u32,
        name: String,
    },
    Insert {
        sheet: u32,
    },
    /// Tables nothing is left of.
    DeleteTables {
        names: Vec<String>,
    },
    /// Columns gone from a table.
    DeleteTableColumns {
        table: String,
        columns: Vec<String>,
    },
}

/// The list as formula text sees it.
pub fn book_edits(ops: &[BookOp]) -> Vec<BookEdit> {
    ops.iter()
        .filter_map(|op| match op {
            BookOp::Axis { name, edit, .. } => Some(BookEdit::Axis {
                sheet: name.clone(),
                edit: *edit,
            }),
            BookOp::Rename { old, new, .. } => Some(BookEdit::Rename {
                old: old.clone(),
                new: new.clone(),
            }),
            BookOp::Delete { name, .. } => Some(BookEdit::DeleteSheet { name: name.clone() }),
            BookOp::DeleteTables { names } => Some(BookEdit::DeleteTables {
                names: names.clone(),
            }),
            BookOp::DeleteTableColumns { table, columns } => Some(BookEdit::DeleteTableColumns {
                table: table.clone(),
                columns: columns.clone(),
            }),
            BookOp::Insert { .. } => None,
        })
        .collect()
}

/// A sheet's own edits, in order. Cells that shift within a band move as ordinary changes to cells,
/// so only rows and columns that move whole come back here. `all_own_edits` has them all, for what
/// holds addresses of cells.
pub fn own_edits(ops: &[BookOp], sheet: u32) -> Vec<AxisEdit> {
    all_own_edits(ops, sheet)
        .into_iter()
        .filter(|edit| edit.band.is_none())
        .collect()
}

pub fn all_own_edits(ops: &[BookOp], sheet: u32) -> Vec<AxisEdit> {
    ops.iter()
        .filter_map(|op| match op {
            BookOp::Axis {
                sheet: on, edit, ..
            } if *on == sheet => Some(*edit),
            _ => None,
        })
        .collect()
}

/// What a sheet's part goes through.
#[derive(Debug, Clone, Copy)]
pub struct SheetEdits<'a> {
    pub own: &'a [AxisEdit],
    /// `own` with cells that shift within a band, in order, for what holds addresses of cells.
    pub all_own: &'a [AxisEdit],
    pub book: &'a [BookEdit],
    /// The sheet's name in the file.
    pub home: &'a str,
    /// Relationships of table parts that have gone from this sheet.
    pub gone_tables: &'a [String],
}

impl SheetEdits<'_> {
    fn point(&self, axis: Axis, at: u32) -> Option<u32> {
        let mut at = at;
        for edit in self.own.iter().filter(|edit| edit.axis == axis) {
            at = edit.point(at)?;
        }
        Some(at)
    }

    fn clamp(&self, axis: Axis, at: u32) -> u32 {
        let mut at = at;
        for edit in self.own.iter().filter(|edit| edit.axis == axis) {
            at = edit.clamp(at);
        }
        at
    }

    fn row(&self, row: u32) -> Option<u32> {
        self.point(Axis::Row, row)
    }

    fn col(&self, col: u32) -> Option<u32> {
        self.point(Axis::Col, col)
    }

    fn formula<'t>(&self, text: &'t str) -> Cow<'t, str> {
        apply_edits(text, self.home, self.book)
    }

    fn sqref(&self, text: &str) -> String {
        let mut current = text.to_string();
        for edit in self.all_own {
            current = edit_sqref(&current, edit);
        }
        current
    }

    fn cell(&self, text: &[u8]) -> Option<String> {
        let (mut row, mut col) = parse_cell(text)?;
        for edit in self.all_own {
            match edit.axis {
                Axis::Row if edit.covers((col, col)) => row = edit.clamp(row),
                Axis::Col if edit.covers((row, row)) => col = edit.clamp(col),
                _ => {}
            }
        }
        Some(cell_name(row, col))
    }

    fn moves_nothing(&self) -> bool {
        self.own.is_empty()
    }

    fn changes_nothing(&self) -> bool {
        self.all_own.is_empty()
    }
}

/// How one pass over a sheet went.
#[derive(Debug, Default, Clone)]
pub struct Rewritten {
    /// Anything differs. A pass that changes nothing leaves the part as raw bytes.
    pub changed: bool,
    /// A follower of a shared formula came before its master, which a second pass now knows.
    pub again: bool,
    /// Extensions this pass doesn't understand, kept as they were, by URI.
    pub left_extensions: Vec<String>,
}

/// What a sheet's extensions may hold that follows moved cells: `sqref` and `f` are visited wherever
/// they stand, and these are the extensions known to keep their addresses there.
const FOLLOWED_EXTENSIONS: [&[u8]; 3] = [
    b"conditionalFormattings",
    b"dataValidations",
    b"sparklineGroups",
];

/// Extensions under a sheet's `extLst` that nothing here knows the addresses of.
fn unknown_extensions(element: &Element, found: &mut Vec<String>) {
    if element.local() != b"extLst" {
        return;
    }
    for child in &element.children {
        let Node::Element(ext) = child else { continue };
        if ext.local() != b"ext" {
            continue;
        }
        let known = ext.children.iter().any(|inner| {
            matches!(inner, Node::Element(inner) if FOLLOWED_EXTENSIONS.contains(&inner.local()))
        });
        if !known {
            let uri = ext.attr_text(b"uri").unwrap_or_default();
            if !found.contains(&uri) {
                found.push(uri);
            }
        }
    }
}

/// Shared formulas' texts, by index: the text as the file has it, and where it is stored.
pub type Masters = FxHashMap<u32, (String, u32, u32)>;

#[derive(Debug)]
pub enum Node {
    Text(Vec<u8>),
    Element(Element),
}

#[derive(Debug)]
pub struct Element {
    pub tag: Vec<u8>,
    pub empty: bool,
    pub children: Vec<Node>,
    pub end: Vec<u8>,
}

impl Element {
    pub fn local(&self) -> &[u8] {
        local_name(&self.tag)
    }

    pub fn attr(&self, name: &[u8]) -> Option<&[u8]> {
        attr_value(&self.tag, name)
    }

    pub fn attr_text(&self, name: &[u8]) -> Option<String> {
        self.attr(name).map(unescape_attr)
    }

    pub fn set_attr(&mut self, name: &str, value: Option<&str>) {
        self.tag = edit_tag(&self.tag, &[(name, value)]);
    }

    /// The text directly inside, with escapes removed.
    pub fn text(&self) -> String {
        let mut raw = Vec::new();
        for child in &self.children {
            if let Node::Text(text) = child {
                raw.extend_from_slice(text);
            }
        }
        unescape_attr(&raw)
    }

    pub fn set_text(&mut self, text: &str) {
        let mut raw = Vec::new();
        escape_text(text, &mut raw);
        self.children
            .retain(|child| !matches!(child, Node::Text(_)));
        self.children.insert(0, Node::Text(raw));
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|child| match child {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        })
    }

    pub fn elements_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children.iter_mut().filter_map(|child| match child {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        })
    }

    fn element_count(&self) -> usize {
        self.elements().count()
    }
}

/// Reads XML into a tree that writes back the same bytes.
pub fn parse_tree(xml: &[u8]) -> io::Result<Vec<Node>> {
    let mut scanner = Scanner::new(xml);
    let mut stack: Vec<Element> = Vec::new();
    let mut top = Vec::new();
    let add = |stack: &mut Vec<Element>, top: &mut Vec<Node>, node: Node| match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => top.push(node),
    };
    while let Some(kind) = scanner.next()? {
        let token = scanner.token().to_vec();
        match kind {
            Kind::Start => stack.push(Element {
                tag: token,
                empty: false,
                children: Vec::new(),
                end: Vec::new(),
            }),
            Kind::Empty => {
                let element = Element {
                    tag: token,
                    empty: true,
                    children: Vec::new(),
                    end: Vec::new(),
                };
                add(&mut stack, &mut top, Node::Element(element));
            }
            Kind::End => {
                let Some(mut element) = stack.pop() else {
                    return Err(damaged("a tag closes that never opened"));
                };
                element.end = token;
                add(&mut stack, &mut top, Node::Element(element));
            }
            Kind::Text | Kind::Other => add(&mut stack, &mut top, Node::Text(token)),
        }
    }
    if !stack.is_empty() {
        return Err(damaged("a part ends before its elements do"));
    }
    Ok(top)
}

pub fn write_tree(nodes: &[Node], out: &mut Vec<u8>) {
    for node in nodes {
        match node {
            Node::Text(text) => out.extend_from_slice(text),
            Node::Element(element) => {
                out.extend_from_slice(&element.tag);
                if !element.empty {
                    write_tree(&element.children, out);
                    out.extend_from_slice(&element.end);
                }
            }
        }
    }
}

fn damaged(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Keep {
    Yes,
    No,
    /// Without this child the parent means nothing, such as a rule with no cells left to rule.
    NotParent,
}

fn set_count(element: &mut Element, before: usize) {
    let now = element.element_count();
    if now == before {
        return;
    }
    let matched = element
        .attr(b"count")
        .and_then(parse_u32)
        .is_some_and(|count| count as usize == before);
    if matched {
        let text = now.to_string();
        element.set_attr("count", Some(&text));
    }
}

/// Rewrites what one element of a worksheet says about position, and its children.
fn visit(element: &mut Element, edits: &SheetEdits<'_>) -> Keep {
    match element.local() {
        b"dimension" => {
            if let Some(reference) = element.attr_text(b"ref") {
                let next = edits.sqref(&reference);
                element.set_attr("ref", Some(if next.is_empty() { "A1" } else { &next }));
            }
            return Keep::Yes;
        }
        b"sheetView" => {
            if let Some(cell) = element.attr(b"topLeftCell").and_then(|at| edits.cell(at)) {
                element.set_attr("topLeftCell", Some(&cell));
            }
            let keep = visit_children(element, edits);
            settle_frozen_panes(element);
            return keep;
        }
        b"pane" => {
            if let Some(cell) = element.attr(b"topLeftCell").and_then(|at| edits.cell(at)) {
                element.set_attr("topLeftCell", Some(&cell));
            }
            let frozen = matches!(element.attr(b"state"), Some(b"frozen" | b"frozenSplit"));
            if frozen {
                for (name, axis) in [("ySplit", Axis::Row), ("xSplit", Axis::Col)] {
                    let Some(mut count) = element.attr(name.as_bytes()).and_then(parse_u32) else {
                        continue;
                    };
                    for edit in edits.own.iter().filter(|edit| edit.axis == axis) {
                        // What is frozen grows or shrinks as rows inside it come and go.
                        count = edit.span((1, count)).map_or(0, |span| span.1);
                    }
                    let text = count.to_string();
                    element.set_attr(name, Some(&text));
                }
            }
            return Keep::Yes;
        }
        b"selection" => {
            if let Some(cell) = element.attr(b"activeCell").and_then(|at| edits.cell(at)) {
                element.set_attr("activeCell", Some(&cell));
                if let Some(area) = element.attr_text(b"sqref") {
                    let next = edits.sqref(&area);
                    element.set_attr("sqref", Some(if next.is_empty() { &cell } else { &next }));
                }
            }
            return Keep::Yes;
        }
        b"col" => {
            let first = element.attr(b"min").and_then(parse_u32);
            let last = element.attr(b"max").and_then(parse_u32).or(first);
            if let (Some(mut first), Some(mut last)) = (first, last) {
                for edit in edits.own.iter().filter(|edit| edit.axis == Axis::Col) {
                    // New columns take the width of the column to their left.
                    let reaches = !edit.delete && last + 1 == edit.at;
                    let Some(span) = edit.span((first, last)) else {
                        return Keep::No;
                    };
                    first = span.0;
                    last = if reaches {
                        (span.1 + edit.count).min(Axis::Col.max())
                    } else {
                        span.1
                    };
                }
                let (first, last) = (first.to_string(), last.to_string());
                element.set_attr("min", Some(&first));
                element.set_attr("max", Some(&last));
            }
            return Keep::Yes;
        }
        b"mergeCell" => {
            let Some(reference) = element.attr_text(b"ref") else {
                return Keep::Yes;
            };
            let next = edits.sqref(&reference);
            if next.is_empty() || !next.contains(':') {
                return Keep::No;
            }
            element.set_attr("ref", Some(&next));
            return Keep::Yes;
        }
        b"autoFilter" | b"sortState" | b"sortCondition" | b"hyperlink" | b"table" => {
            let local = element.local().to_vec();
            if local == b"autoFilter" && edits.own.iter().any(|edit| edit.axis == Axis::Col) {
                move_filter_columns(element, edits);
            }
            if let Some(reference) = element.attr_text(b"ref") {
                let next = edits.sqref(&reference);
                if next.is_empty() {
                    return Keep::No;
                }
                element.set_attr("ref", Some(&next));
            }
            if local == b"hyperlink" {
                if let Some(place) = element.attr_text(b"location") {
                    if let Cow::Owned(next) = edits.formula(&place) {
                        let escaped = crate::scan::escape_attr(&next);
                        element.set_attr("location", Some(&escaped));
                    }
                }
                return Keep::Yes;
            }
        }
        b"conditionalFormatting" | b"dataValidation" | b"protectedRange" | b"ignoredError" => {
            if let Some(area) = element.attr_text(b"sqref") {
                let next = edits.sqref(&area);
                if next.is_empty() {
                    return Keep::No;
                }
                element.set_attr("sqref", Some(&next));
            }
        }
        b"tablePart" => {
            let gone = edits
                .gone_tables
                .iter()
                .any(|id| sheet_rel_id(&element.tag).as_deref() == Some(id.as_str()));
            return if gone { Keep::No } else { Keep::Yes };
        }
        b"formula"
        | b"formula1"
        | b"formula2"
        | b"f"
        | b"totalsRowFormula"
        | b"calculatedColumnFormula" => {
            let text = element.text();
            if let Cow::Owned(next) = edits.formula(&text) {
                element.set_text(&next);
            }
            return Keep::Yes;
        }
        b"sparkline" => {
            let keep = visit_children(element, edits);
            // Excel refuses `Data!#REF!` here, as a sparkline has to read a range or nothing.
            for child in &mut element.children {
                if let Node::Element(inner) = child {
                    if inner.local() == b"f" && inner.text().contains("#REF!") {
                        inner.set_text("#REF!");
                    }
                }
            }
            return keep;
        }
        b"sparklineGroup" => {
            // Excel refuses a group left with no sparklines, and it holds other things too.
            let keep = visit_children(element, edits);
            let any = element.children.iter().any(
                |child| matches!(child, Node::Element(inner) if inner.local() == b"sparklines"),
            );
            return if any { keep } else { Keep::No };
        }
        b"sqref" => {
            // `xm:sqref`: the cells a rule in an extension applies to.
            let next = edits.sqref(&element.text());
            if next.is_empty() {
                return Keep::NotParent;
            }
            element.set_text(&next);
            return Keep::Yes;
        }
        b"cfvo" => {
            if element.attr(b"type") == Some(b"formula") {
                if let Some(value) = element.attr_text(b"val") {
                    if let Cow::Owned(next) = edits.formula(&value) {
                        let escaped = crate::scan::escape_attr(&next);
                        element.set_attr("val", Some(&escaped));
                    }
                }
            }
            return Keep::Yes;
        }
        b"rowBreaks" | b"colBreaks" => {
            let axis = if element.local() == b"rowBreaks" {
                Axis::Row
            } else {
                Axis::Col
            };
            let before = element.element_count();
            element.children.retain_mut(|child| match child {
                Node::Element(brk) => {
                    let Some(moved) = brk.attr(b"id").and_then(parse_u32) else {
                        return true;
                    };
                    match edits.point(axis, moved) {
                        Some(next) => {
                            let text = next.to_string();
                            brk.set_attr("id", Some(&text));
                            true
                        }
                        None => false,
                    }
                }
                Node::Text(_) => true,
            });
            let now = element.element_count();
            if before > 0 && now == 0 {
                return Keep::No;
            }
            if now != before {
                let text = now.to_string();
                element.set_attr("count", Some(&text));
                let manual = element.attr(b"manualBreakCount").map(|_| now.to_string());
                if let Some(manual) = manual {
                    element.set_attr("manualBreakCount", Some(&manual));
                }
            }
            return Keep::Yes;
        }
        _ => {}
    }
    visit_children(element, edits)
}

/// Everything frozen may have gone with the rows or columns. Excel refuses a frozen pane with a split
/// of 0, or a selection in a pane the sheet no longer has.
fn settle_frozen_panes(view: &mut Element) {
    let mut gone = false;
    let mut active = None;
    let mut old_active = None;
    for child in &mut view.children {
        let Node::Element(pane) = child else { continue };
        if pane.local() != b"pane" {
            continue;
        }
        if !matches!(pane.attr(b"state"), Some(b"frozen" | b"frozenSplit")) {
            return;
        }
        old_active = pane.attr_text(b"activePane");
        let split = |pane: &Element, name: &[u8]| pane.attr(name).and_then(parse_u32).unwrap_or(0);
        let (x, y) = (split(pane, b"xSplit"), split(pane, b"ySplit"));
        if x > 0 && y > 0 {
            return;
        }
        if x == 0 {
            pane.set_attr("xSplit", None);
        }
        if y == 0 {
            pane.set_attr("ySplit", None);
        }
        active = match (x > 0, y > 0) {
            (true, _) => Some("topRight"),
            (_, true) => Some("bottomLeft"),
            _ => None,
        };
        gone = active.is_none();
        pane.set_attr("activePane", active);
    }
    if old_active.is_none() && active.is_none() && !gone {
        return;
    }
    if gone {
        view.children
            .retain(|child| !matches!(child, Node::Element(inner) if inner.local() == b"pane"));
    }
    // One selection, the one the user was in, now in the pane that's left.
    let mut kept = false;
    let has_active = view.children.iter().any(|child| {
        matches!(child, Node::Element(inner)
            if inner.local() == b"selection" && inner.attr_text(b"pane") == old_active)
    });
    view.children.retain_mut(|child| {
        let Node::Element(inner) = child else {
            return true;
        };
        if inner.local() != b"selection" {
            return true;
        }
        if kept || (has_active && inner.attr_text(b"pane") != old_active) {
            return false;
        }
        kept = true;
        inner.set_attr("pane", active);
        true
    });
}

fn visit_children(element: &mut Element, edits: &SheetEdits<'_>) -> Keep {
    let before = element.element_count();
    let mut lost_meaning = false;
    let children = std::mem::take(&mut element.children);
    for mut child in children {
        let keep = match &mut child {
            Node::Element(inner) => visit(inner, edits),
            Node::Text(_) => Keep::Yes,
        };
        match keep {
            Keep::Yes => element.children.push(child),
            Keep::No => {}
            Keep::NotParent => lost_meaning = true,
        }
    }
    if lost_meaning || (before > 0 && element.element_count() == 0) {
        return Keep::No;
    }
    set_count(element, before);
    Keep::Yes
}

/// A tag's relationship id: `r:id`, whatever its prefix.
fn sheet_rel_id(tag: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(tag);
    let found = text.find(":id=\"")?;
    let rest = &text[found + 5..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// An autofilter's columns are counted from its left edge.
fn move_filter_columns(element: &mut Element, edits: &SheetEdits<'_>) {
    let Some(area) = element.attr_text(b"ref").and_then(|text| parse_rect(&text)) else {
        return;
    };
    let new_left = match edits.col(area.left) {
        Some(left) => left,
        None => edits.clamp(Axis::Col, area.left),
    };
    let before = element.element_count();
    element.children.retain_mut(|child| match child {
        Node::Element(column) if column.local() == b"filterColumn" => {
            let Some(offset) = column.attr(b"colId").and_then(parse_u32) else {
                return true;
            };
            match edits.col(area.left + offset) {
                Some(moved) if moved >= new_left => {
                    let text = (moved - new_left).to_string();
                    column.set_attr("colId", Some(&text));
                    true
                }
                _ => false,
            }
        }
        _ => true,
    });
    set_count(element, before);
}

/// Rewrites one worksheet from `reader` to `out`, into final coordinates.
pub fn rewrite_sheet<R: Read, W: Write>(
    reader: R,
    out: &mut W,
    edits: &SheetEdits<'_>,
    masters: &mut Masters,
) -> io::Result<Rewritten> {
    let mut scanner = Scanner::new(reader);
    let mut result = Rewritten {
        changed: !edits.changes_nothing(),
        again: false,
        left_extensions: Vec::new(),
    };
    let mut depth = 0u32;
    while let Some(kind) = scanner.next()? {
        let token = scanner.token();
        match kind {
            Kind::Start if depth == 1 && local_name(token) == b"sheetData" => {
                out.write_all(token)?;
                rewrite_cells(&mut scanner, out, edits, masters, &mut result)?;
            }
            Kind::Start | Kind::Empty if depth == 1 => {
                let mut raw = token.to_vec();
                if kind == Kind::Start {
                    read_element(&mut scanner, &mut raw)?;
                }
                let mut nodes = parse_tree(&raw)?;
                let mut keep = Keep::Yes;
                if let Some(Node::Element(element)) = nodes.first_mut() {
                    keep = visit(element, edits);
                    if !edits.moves_nothing() {
                        unknown_extensions(element, &mut result.left_extensions);
                    }
                }
                if keep == Keep::Yes {
                    let mut written = Vec::with_capacity(raw.len());
                    write_tree(&nodes, &mut written);
                    if written != raw {
                        result.changed = true;
                    }
                    out.write_all(&written)?;
                } else {
                    result.changed = true;
                }
            }
            Kind::Start => {
                depth += 1;
                out.write_all(token)?;
            }
            Kind::End => {
                depth = depth.saturating_sub(1);
                out.write_all(token)?;
            }
            _ => out.write_all(token)?,
        }
    }
    Ok(result)
}

/// A worksheet's `<sheetViews>` with its first view changed, keeping whatever else it says.
pub fn edit_sheet_views(xml: &[u8], patch: &crate::patch_sheet::ViewPatch) -> io::Result<Vec<u8>> {
    let mut tree = parse_tree(xml)?;
    for node in &mut tree {
        let Node::Element(views) = node else { continue };
        if let Some(view) = views
            .elements_mut()
            .find(|view| view.local() == b"sheetView")
        {
            edit_view(view, patch);
        }
    }
    let mut out = Vec::with_capacity(xml.len() + 128);
    write_tree(&tree, &mut out);
    Ok(out)
}

fn edit_view(view: &mut Element, patch: &crate::patch_sheet::ViewPatch) {
    let flag = |on: bool| if on { None } else { Some("0") };
    if let Some(show) = patch.show_grid {
        view.set_attr("showGridLines", flag(show));
    }
    if let Some(show) = patch.show_headers {
        view.set_attr("showRowColHeaders", flag(show));
    }
    let Some(freeze) = patch.freeze else {
        return;
    };
    // The name a new child needs, in a part that has a prefix.
    let prefix = crate::scan::prefix(&view.tag);
    let active = (patch.active_cell.0.max(1), patch.active_cell.1.max(1));
    let cell = crate::patch_sheet::cell_name(active.0, active.1);
    let selection = |pane: Option<&str>| {
        let pane = pane.map_or(String::new(), |pane| format!(" pane=\"{pane}\""));
        Node::Element(Element {
            tag: format!("<{prefix}selection{pane} activeCell=\"{cell}\" sqref=\"{cell}\"/>")
                .into_bytes(),
            empty: true,
            children: Vec::new(),
            end: Vec::new(),
        })
    };
    view.children.retain(|child| {
        !matches!(child, Node::Element(inner) if matches!(inner.local(), b"pane" | b"selection"))
    });
    let mut front = Vec::new();
    match freeze {
        Some(freeze) if freeze.rows > 0 || freeze.cols > 0 => {
            let first_row = freeze.top.max(1);
            let first_col = freeze.left.max(1);
            let top_left =
                crate::patch_sheet::cell_name(first_row + freeze.rows, first_col + freeze.cols);
            let pane_name = match (freeze.cols > 0, freeze.rows > 0) {
                (true, true) => "bottomRight",
                (true, false) => "topRight",
                _ => "bottomLeft",
            };
            let mut tag = format!("<{prefix}pane");
            if freeze.cols > 0 {
                tag.push_str(&format!(" xSplit=\"{}\"", freeze.cols));
            }
            if freeze.rows > 0 {
                tag.push_str(&format!(" ySplit=\"{}\"", freeze.rows));
            }
            tag.push_str(&format!(
                " topLeftCell=\"{top_left}\" activePane=\"{pane_name}\" state=\"frozen\"/>"
            ));
            front.push(Node::Element(Element {
                tag: tag.into_bytes(),
                empty: true,
                children: Vec::new(),
                end: Vec::new(),
            }));
            if freeze.cols > 0 && freeze.rows > 0 {
                front.push(selection(Some("topRight")));
                front.push(selection(Some("bottomLeft")));
            }
            front.push(selection(Some(pane_name)));
            let corner = (first_row, first_col) != (1, 1);
            let name = crate::patch_sheet::cell_name(first_row, first_col);
            view.set_attr("topLeftCell", corner.then_some(name.as_str()));
        }
        _ => {
            view.set_attr("topLeftCell", None);
            front.push(selection(None));
        }
    }
    let rest = std::mem::take(&mut view.children);
    view.children = front;
    view.children.extend(rest);
    if view.empty && !view.children.is_empty() {
        let tag = crate::scan::opened(&view.tag);
        view.tag = tag;
        view.empty = false;
        view.end = format!("</{prefix}sheetView>").into_bytes();
    }
}

/// Copies tokens up to the end of the element that has just opened.
pub(crate) fn read_element<R: Read>(scanner: &mut Scanner<R>, out: &mut Vec<u8>) -> io::Result<()> {
    let mut depth = 1u32;
    while let Some(kind) = scanner.next()? {
        out.extend_from_slice(scanner.token());
        match kind {
            Kind::Start => depth += 1,
            Kind::End => {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    Err(damaged("an element never ends"))
}

/// Moves past a row or cell that is going, noting any shared formula's master in it, as the cells
/// that stay still read their formulas from it.
fn skip_noting_masters<R: Read>(
    scanner: &mut Scanner<R>,
    masters: &mut Masters,
    mut row: u32,
    mut col: u32,
) -> io::Result<()> {
    let mut depth = 1u32;
    while let Some(kind) = scanner.next()? {
        let token = scanner.token();
        match kind {
            Kind::Start if local_name(token) == b"f" => {
                let tag = token.to_vec();
                let mut body = Vec::new();
                read_element(scanner, &mut body)?;
                if attr_value(&tag, b"t") == Some(b"shared") {
                    let close = body
                        .iter()
                        .rposition(|&byte| byte == b'<')
                        .unwrap_or(body.len());
                    let text = unescape_attr(&body[..close]);
                    if let (false, Some(index)) =
                        (text.is_empty(), attr_value(&tag, b"si").and_then(parse_u32))
                    {
                        masters.entry(index).or_insert((text, row, col));
                    }
                }
            }
            Kind::Start => {
                if local_name(token) == b"c" {
                    if let Some(at) = attr_value(token, b"r").and_then(parse_cell) {
                        (row, col) = at;
                    }
                }
                depth += 1;
            }
            Kind::End => {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    Err(damaged("this sheet's cells never end"))
}

fn rewrite_cells<R: Read, W: Write>(
    scanner: &mut Scanner<R>,
    out: &mut W,
    edits: &SheetEdits<'_>,
    masters: &mut Masters,
    result: &mut Rewritten,
) -> io::Result<()> {
    let columns_move = edits.own.iter().any(|edit| edit.axis == Axis::Col);
    let mut row = 0u32;
    let mut col = 0u32;
    let mut new_row = 0u32;
    while let Some(kind) = scanner.next()? {
        let token = scanner.token();
        match kind {
            Kind::End if local_name(token) == b"sheetData" => {
                out.write_all(token)?;
                return Ok(());
            }
            Kind::Start | Kind::Empty if local_name(token) == b"row" => {
                row = attr_value(token, b"r")
                    .and_then(parse_u32)
                    .unwrap_or(row + 1);
                col = 0;
                match edits.row(row) {
                    None => {
                        if kind == Kind::Start {
                            skip_noting_masters(scanner, masters, row, 0)?;
                        }
                    }
                    Some(moved) => {
                        new_row = moved;
                        let number = moved.to_string();
                        let mut changes: Vec<(&str, Option<&str>)> = Vec::new();
                        if moved != row {
                            changes.push(("r", Some(&number)));
                        }
                        // Only a hint, and one a changed column makes wrong.
                        if columns_move && attr_value(token, b"spans").is_some() {
                            changes.push(("spans", None));
                        }
                        if changes.is_empty() {
                            out.write_all(token)?;
                        } else {
                            out.write_all(&edit_tag(token, &changes))?;
                        }
                    }
                }
            }
            Kind::Start | Kind::Empty if local_name(token) == b"c" => {
                let address = attr_value(token, b"r").and_then(parse_cell);
                if let Some((at_row, at_col)) = address {
                    // A cell that names a row other than the one it sits in goes by its own name.
                    if at_row != row {
                        row = at_row;
                        new_row = edits.row(row).unwrap_or(0);
                    }
                    col = at_col;
                } else {
                    col += 1;
                }
                let moved = if new_row == 0 { None } else { edits.col(col) };
                let Some(moved) = moved else {
                    if kind == Kind::Start {
                        skip_noting_masters(scanner, masters, row, col)?;
                    }
                    continue;
                };
                let name = cell_name(new_row, moved);
                if (new_row, moved) != (row, col) || (address.is_none() && !edits.moves_nothing()) {
                    out.write_all(&edit_tag(token, &[("r", Some(&name))]))?;
                } else {
                    out.write_all(token)?;
                }
            }
            Kind::Start if local_name(token) == b"f" => {
                let tag = token.to_vec();
                let mut body = Vec::new();
                read_element(scanner, &mut body)?;
                let close = body
                    .iter()
                    .rposition(|&byte| byte == b'<')
                    .unwrap_or(body.len());
                let text = unescape_attr(&body[..close]);
                let formula = Formula {
                    tag: &tag,
                    text: &text,
                    raw: &body[..close],
                    end: &body[close..],
                    at: (row, col),
                };
                write_formula(&formula, out, edits, masters, result)?;
            }
            Kind::Empty if local_name(token) == b"f" => {
                let tag = token.to_vec();
                let formula = Formula {
                    tag: &tag,
                    text: "",
                    raw: b"",
                    end: b"",
                    at: (row, col),
                };
                write_formula(&formula, out, edits, masters, result)?;
            }
            _ => out.write_all(token)?,
        }
    }
    Err(damaged("this sheet's cells never end"))
}

/// A formula element as the file has it.
struct Formula<'a> {
    tag: &'a [u8],
    text: &'a str,
    raw: &'a [u8],
    end: &'a [u8],
    at: (u32, u32),
}

fn write_formula<W: Write>(
    formula: &Formula<'_>,
    out: &mut W,
    edits: &SheetEdits<'_>,
    masters: &mut Masters,
    result: &mut Rewritten,
) -> io::Result<()> {
    let tag = formula.tag;
    let kind = attr_value(tag, b"t");
    let plain_close = || {
        let name = crate::scan::prefix(tag);
        format!("</{name}f>").into_bytes()
    };
    match kind {
        Some(b"shared") => {
            let Some(index) = attr_value(tag, b"si").and_then(parse_u32) else {
                out.write_all(tag)?;
                out.write_all(formula.raw)?;
                out.write_all(formula.end)?;
                return Ok(());
            };
            let is_master = !formula.text.is_empty();
            if is_master {
                masters
                    .entry(index)
                    .or_insert_with(|| (formula.text.to_string(), formula.at.0, formula.at.1));
            }
            let Some((master, row, col)) = masters.get(&index) else {
                // Before its master. The next pass knows it.
                result.again = true;
                out.write_all(tag)?;
                out.write_all(formula.raw)?;
                out.write_all(formula.end)?;
                return Ok(());
            };
            let mut rendered = String::new();
            SharedFormula::new(master).render(
                i64::from(formula.at.0) - i64::from(*row),
                i64::from(formula.at.1) - i64::from(*col),
                &mut rendered,
            );
            let next = edits.formula(&rendered);
            if matches!(next, Cow::Owned(_)) {
                result.changed = true;
            }
            // Each cell gets its own formula: what was one pattern needn't stay one. This only
            // reaches the file if something in the sheet changes.
            let bare = edit_tag(tag, &[("t", None), ("si", None), ("ref", None)]);
            out.write_all(&crate::scan::opened(&bare))?;
            let mut escaped = Vec::new();
            escape_text(&next, &mut escaped);
            out.write_all(&escaped)?;
            out.write_all(&plain_close())?;
        }
        _ => {
            let reference = attr_value(tag, b"ref").map(unescape_attr);
            let next_ref = reference.as_deref().map(|area| edits.sqref(area));
            let mut changes: Vec<(&str, Option<String>)> = Vec::new();
            if let (Some(old), Some(next)) = (&reference, &next_ref) {
                if old != next && !next.is_empty() {
                    changes.push(("ref", Some(next.clone())));
                }
            }
            if kind == Some(b"dataTable") {
                // A data table's input cells, which become nothing if they are deleted.
                for (name, flag) in [("r1", "del1"), ("r2", "del2")] {
                    let Some(input) = attr_value(tag, name.as_bytes()).and_then(parse_cell) else {
                        continue;
                    };
                    match (edits.row(input.0), edits.col(input.1)) {
                        (Some(row), Some(col)) if (row, col) != input => {
                            changes.push((name, Some(cell_name(row, col))));
                        }
                        (Some(_), Some(_)) => {}
                        _ => changes.push((flag, Some("1".to_string()))),
                    }
                }
            }
            let next = edits.formula(formula.text);
            let text_changed = matches!(next, Cow::Owned(_));
            if text_changed || !changes.is_empty() {
                result.changed = true;
            }
            let as_str: Vec<(&str, Option<&str>)> = changes
                .iter()
                .map(|(name, value)| (*name, value.as_deref()))
                .collect();
            if as_str.is_empty() {
                out.write_all(tag)?;
            } else {
                out.write_all(&edit_tag(tag, &as_str))?;
            }
            if text_changed {
                let mut escaped = Vec::new();
                escape_text(&next, &mut escaped);
                out.write_all(&escaped)?;
            } else {
                out.write_all(formula.raw)?;
            }
            out.write_all(formula.end)?;
        }
    }
    Ok(())
}

/// A part with position in it, rewritten. `None` if nothing changed.
fn tree_edit(xml: &[u8], change: impl FnOnce(&mut Vec<Node>)) -> io::Result<Option<Vec<u8>>> {
    let mut nodes = parse_tree(xml)?;
    change(&mut nodes);
    let mut out = Vec::with_capacity(xml.len());
    write_tree(&nodes, &mut out);
    Ok((out != xml).then_some(out))
}

/// A table part: its area, its filter, and its columns as they come and go. `new_name` picks the
/// name of a column an insert adds inside the table.
pub fn rewrite_table(
    xml: &[u8],
    edits: &SheetEdits<'_>,
    new_name: &mut dyn FnMut(u32) -> String,
) -> io::Result<Option<Vec<u8>>> {
    tree_edit(xml, |nodes| {
        for node in nodes {
            let Node::Element(table) = node else {
                continue;
            };
            if table.local() != b"table" {
                continue;
            }
            let area = table.attr_text(b"ref").and_then(|text| parse_rect(&text));
            if let Some(area) = area {
                let mut left = area.left;
                let mut right = area.right;
                for edit in edits.own.iter().filter(|edit| edit.axis == Axis::Col) {
                    edit_table_columns(table, edit, left, right, new_name);
                    if let Some(span) = edit.span((left, right)) {
                        left = span.0;
                        right = span.1;
                    }
                }
            }
            let _ = visit(table, edits);
        }
    })
}

fn edit_table_columns(
    table: &mut Element,
    edit: &AxisEdit,
    left: u32,
    right: u32,
    new_name: &mut dyn FnMut(u32) -> String,
) {
    let Some(columns) = table
        .elements_mut()
        .find(|element| element.local() == b"tableColumns")
    else {
        return;
    };
    let before = columns.element_count();
    if edit.delete {
        let mut index = 0;
        columns.children.retain(|child| {
            let Node::Element(_) = child else {
                return true;
            };
            let col = left + index;
            index += 1;
            edit.point(col).is_some()
        });
    } else if edit.at > left && edit.at <= right {
        let mut next_id = columns
            .elements()
            .filter_map(|column| column.attr(b"id").and_then(parse_u32))
            .max()
            .unwrap_or(0);
        let offset = (edit.at - left) as usize;
        let mut seen = 0;
        let mut at = columns.children.len();
        for (position, child) in columns.children.iter().enumerate() {
            if matches!(child, Node::Element(_)) {
                if seen == offset {
                    at = position;
                    break;
                }
                seen += 1;
            }
        }
        let prefix = crate::scan::prefix(&columns.tag);
        for step in 0..edit.count {
            next_id += 1;
            let name = new_name(offset as u32 + step);
            let tag = format!(
                "<{prefix}tableColumn id=\"{next_id}\" name=\"{}\"/>",
                crate::patch_parts::table_column_attr(&name)
            );
            columns.children.insert(
                at + step as usize,
                Node::Element(Element {
                    tag: tag.into_bytes(),
                    empty: true,
                    children: Vec::new(),
                    end: Vec::new(),
                }),
            );
        }
    }
    let now = columns.element_count();
    if now != before {
        let text = now.to_string();
        columns.set_attr("count", Some(&text));
    }
}

/// A comments part: comments follow their cells and go with them.
pub fn rewrite_comments(xml: &[u8], edits: &SheetEdits<'_>) -> io::Result<Option<Vec<u8>>> {
    tree_edit(xml, |nodes| {
        fn walk(nodes: &mut Vec<Node>, edits: &SheetEdits<'_>) {
            nodes.retain_mut(|node| {
                let Node::Element(element) = node else {
                    return true;
                };
                if matches!(element.local(), b"comment" | b"threadedComment") {
                    let Some(at) = element.attr(b"ref").and_then(parse_cell) else {
                        return true;
                    };
                    match (edits.row(at.0), edits.col(at.1)) {
                        (Some(row), Some(col)) => {
                            let name = cell_name(row, col);
                            element.set_attr("ref", Some(&name));
                            true
                        }
                        _ => false,
                    }
                } else {
                    walk(&mut element.children, edits);
                    true
                }
            });
        }
        walk(nodes, edits);
    })
}

/// A number inside `<x:Name>…</x:Name>` in a VML shape, if the shape has one.
fn vml_number(shape: &str, name: &str) -> Option<(usize, usize, u32)> {
    let open = format!("<x:{name}>");
    let start = shape.find(&open)? + open.len();
    let end = start + shape[start..].find('<')?;
    let number = shape[start..end].trim().parse().ok()?;
    Some((start, end, number))
}

/// A VML drawing, which isn't always well-formed XML, so this goes by text: a comment's shape says
/// which cell it belongs to, and where it sits on the sheet.
pub fn rewrite_vml(xml: &[u8], edits: &SheetEdits<'_>) -> Option<Vec<u8>> {
    let text = String::from_utf8_lossy(xml);
    let mut out = String::with_capacity(text.len());
    let mut rest: &str = &text;
    let mut changed = false;
    while let Some(start) = rest.find("<v:shape ").or_else(|| rest.find("<v:shape>")) {
        let Some(end_at) = rest[start..].find("</v:shape>") else {
            break;
        };
        let end = start + end_at + "</v:shape>".len();
        out.push_str(&rest[..start]);
        let shape = &rest[start..end];
        let mut edited = shape.to_string();
        let row = vml_number(&edited, "Row");
        let col = vml_number(&edited, "Column");
        let mut gone = false;
        if let (Some((_, _, row)), Some((_, _, col))) = (row, col) {
            match (edits.row(row + 1), edits.col(col + 1)) {
                (Some(new_row), Some(new_col)) => {
                    // Row first: it comes first in the shape, so positions stay good from the end.
                    let (start, end, _) = vml_number(&edited, "Column").unwrap_or((0, 0, 0));
                    edited.replace_range(start..end, &(new_col - 1).to_string());
                    let (start, end, _) = vml_number(&edited, "Row").unwrap_or((0, 0, 0));
                    edited.replace_range(start..end, &(new_row - 1).to_string());
                }
                _ => gone = true,
            }
        }
        if !gone {
            if let Some(next) = vml_anchor(&edited, edits) {
                edited = next;
            }
            changed |= edited != shape;
            out.push_str(&edited);
        } else {
            changed = true;
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    changed.then(|| out.into_bytes())
}

/// `<x:Anchor>` holds a shape's corners: column, offset, row, offset, twice over, from 0.
fn vml_anchor(shape: &str, edits: &SheetEdits<'_>) -> Option<String> {
    let open = "<x:Anchor>";
    let start = shape.find(open)? + open.len();
    let end = start + shape[start..].find('<')?;
    let mut parts: Vec<i64> = shape[start..end]
        .split(',')
        .map(|part| part.trim().parse().ok())
        .collect::<Option<_>>()?;
    if parts.len() != 8 {
        return None;
    }
    for (at, axis) in [
        (0, Axis::Col),
        (2, Axis::Row),
        (4, Axis::Col),
        (6, Axis::Row),
    ] {
        parts[at] = i64::from(edits.clamp(axis, parts[at] as u32 + 1)) - 1;
    }
    let text = parts
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = shape.to_string();
    out.replace_range(start..end, &text);
    Some(out)
}

/// Reads and writes a `row` or `col` child of a drawing's corner.
fn corner_value(corner: &Element, name: &[u8]) -> Option<u32> {
    corner
        .elements()
        .find(|element| element.local() == name)
        .and_then(|element| element.text().trim().parse().ok())
}

fn set_corner_value(corner: &mut Element, name: &[u8], value: u32) {
    if let Some(element) = corner
        .elements_mut()
        .find(|element| element.local() == name)
    {
        element.set_text(&value.to_string());
    }
}

/// Puts a corner where it goes after the edits. Counted from 0 in the file.
fn move_corner(corner: &mut Element, edits: &SheetEdits<'_>) {
    for (name, offset, axis) in [
        (&b"row"[..], &b"rowOff"[..], Axis::Row),
        (&b"col"[..], &b"colOff"[..], Axis::Col),
    ] {
        let Some(place) = corner_value(corner, name) else {
            continue;
        };
        match edits.point(axis, place + 1) {
            Some(next) => set_corner_value(corner, name, next - 1),
            None => {
                set_corner_value(corner, name, edits.clamp(axis, place + 1) - 1);
                set_corner_value(corner, offset, 0);
            }
        }
    }
}

/// A drawing part: anchors follow the cells they are tied to, as `editAs` says.
pub fn rewrite_drawing(xml: &[u8], edits: &SheetEdits<'_>) -> io::Result<Option<Vec<u8>>> {
    tree_edit(xml, |nodes| {
        for node in nodes {
            let Node::Element(root) = node else {
                continue;
            };
            for anchor in root.elements_mut() {
                match anchor.local() {
                    b"twoCellAnchor" | b"oneCellAnchor" => {}
                    _ => continue,
                }
                let two_cell = anchor.local() == b"twoCellAnchor";
                let edit_as = anchor.attr_text(b"editAs");
                // A picture that keeps its size moves its far corner with its near one.
                let keeps_size = two_cell && edit_as.as_deref() == Some("oneCell");
                let fixed = two_cell && edit_as.as_deref() == Some("absolute");
                if fixed {
                    continue;
                }
                let mut from_shift = (0i64, 0i64);
                for corner in anchor.elements_mut() {
                    if corner.local() == b"from" {
                        let before = (corner_value(corner, b"row"), corner_value(corner, b"col"));
                        move_corner(corner, edits);
                        let after = (corner_value(corner, b"row"), corner_value(corner, b"col"));
                        if let ((Some(row), Some(col)), (Some(next_row), Some(next_col))) =
                            (before, after)
                        {
                            from_shift = (
                                i64::from(next_row) - i64::from(row),
                                i64::from(next_col) - i64::from(col),
                            );
                        }
                    }
                }
                for corner in anchor.elements_mut() {
                    if corner.local() != b"to" {
                        continue;
                    }
                    if keeps_size {
                        if let Some(row) = corner_value(corner, b"row") {
                            let moved = (i64::from(row) + from_shift.0).max(0) as u32;
                            set_corner_value(corner, b"row", moved);
                        }
                        if let Some(col) = corner_value(corner, b"col") {
                            let moved = (i64::from(col) + from_shift.1).max(0) as u32;
                            set_corner_value(corner, b"col", moved);
                        }
                    } else {
                        move_corner(corner, edits);
                    }
                }
            }
        }
    })
}

/// A chart part, where every series says which cells it reads in a `c:f`.
pub fn rewrite_chart(xml: &[u8], book: &[BookEdit]) -> io::Result<Option<Vec<u8>>> {
    tree_edit(xml, |nodes| {
        fn walk(nodes: &mut [Node], book: &[BookEdit]) {
            for node in nodes {
                let Node::Element(element) = node else {
                    continue;
                };
                if element.local() == b"f" && !element.empty {
                    let text = element.text();
                    // Never unqualified, so which sheet it is read from doesn't matter.
                    if let Cow::Owned(next) = apply_edits(&text, "", book) {
                        element.set_text(&next);
                    }
                } else {
                    walk(&mut element.children, book);
                }
            }
        }
        walk(nodes, book);
    })
}

/// A pivot cache's source: which sheet, and which cells.
pub fn rewrite_pivot_cache(xml: &[u8], book: &[BookEdit]) -> io::Result<Option<Vec<u8>>> {
    tree_edit(xml, |nodes| {
        fn walk(nodes: &mut [Node], book: &[BookEdit]) {
            for node in nodes {
                let Node::Element(element) = node else {
                    continue;
                };
                if element.local() == b"worksheetSource" {
                    let Some(sheet) = element.attr_text(b"sheet") else {
                        continue;
                    };
                    let mut name = sheet.clone();
                    let mut area = element.attr_text(b"ref");
                    for edit in book {
                        match edit {
                            BookEdit::Axis { sheet: on, edit } => {
                                if cgs_refs::sheet_key(on) == cgs_refs::sheet_key(&name) {
                                    if let Some(text) = area.as_deref() {
                                        let next = edit_sqref(text, edit);
                                        area = (!next.is_empty()).then_some(next);
                                    }
                                }
                            }
                            BookEdit::Rename { old, new } => {
                                if cgs_refs::sheet_key(old) == cgs_refs::sheet_key(&name) {
                                    name = new.clone();
                                }
                            }
                            BookEdit::DeleteSheet { .. }
                            | BookEdit::DeleteTables { .. }
                            | BookEdit::DeleteTableColumns { .. } => {}
                        }
                    }
                    if name != sheet {
                        let escaped = crate::scan::escape_attr(&name);
                        element.set_attr("sheet", Some(&escaped));
                    }
                    if let Some(area) = area {
                        element.set_attr("ref", Some(&area));
                    }
                } else {
                    walk(&mut element.children, book);
                }
            }
        }
        walk(nodes, book);
    })
}

/// A pivot table's place on its sheet.
pub fn rewrite_pivot_table(xml: &[u8], edits: &SheetEdits<'_>) -> io::Result<Option<Vec<u8>>> {
    tree_edit(xml, |nodes| {
        for node in nodes {
            let Node::Element(root) = node else {
                continue;
            };
            for child in root.elements_mut() {
                if child.local() == b"location" {
                    if let Some(area) = child.attr_text(b"ref") {
                        let next = edits.sqref(&area);
                        if !next.is_empty() {
                            child.set_attr("ref", Some(&next));
                        }
                    }
                }
            }
        }
    })
}

/// Where a rectangle ends up, for tables in the model.
pub fn moved_rect(rect: (u32, u32, u32, u32), edits: &[AxisEdit]) -> Option<(u32, u32, u32, u32)> {
    let mut rect = rect;
    for edit in edits {
        rect = edit_rect(rect, edit)?;
    }
    Some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(at: u32, count: u32) -> AxisEdit {
        AxisEdit::insert(Axis::Row, at, count)
    }

    fn cols(at: u32, count: u32) -> AxisEdit {
        AxisEdit::insert(Axis::Col, at, count)
    }

    fn book(sheet: &str, edit: AxisEdit) -> Vec<BookEdit> {
        vec![BookEdit::Axis {
            sheet: sheet.to_string(),
            edit,
        }]
    }

    fn sheet_xml(data: &str, after: &str) -> String {
        format!(
            "<worksheet xmlns=\"m\"><dimension ref=\"A1:C5\"/><sheetData>{data}</sheetData>{after}</worksheet>"
        )
    }

    fn run(xml: &str, edit: AxisEdit) -> String {
        let own = [edit];
        let list = book("S", edit);
        let edits = SheetEdits {
            own: &own,
            all_own: &own,
            book: &list,
            home: "S",
            gone_tables: &[],
        };
        let mut masters = Masters::default();
        let mut out = Vec::new();
        let done = rewrite_sheet(xml.as_bytes(), &mut out, &edits, &mut masters).unwrap();
        assert!(!done.again);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn rows_and_cells_move_and_formulas_follow() {
        let xml = sheet_xml(
            "<row r=\"1\" ht=\"20\"><c r=\"A1\"><v>1</v></c></row><row r=\"2\"><c r=\"A2\" s=\"3\" x=\"y\"><f>A1+B5</f><v>2</v></c></row>",
            "",
        );
        let out = run(&xml, rows(2, 3));
        assert!(
            out.contains("<row r=\"1\" ht=\"20\"><c r=\"A1\"><v>1</v></c></row>"),
            "{out}"
        );
        assert!(
            out.contains("<row r=\"5\"><c r=\"A5\" s=\"3\" x=\"y\"><f>A1+B8</f><v>2</v></c></row>"),
            "{out}"
        );
        assert!(out.contains("<dimension ref=\"A1:C8\"/>"), "{out}");
    }

    #[test]
    fn a_deleted_row_and_its_cells_go() {
        let xml = sheet_xml(
            "<row r=\"1\"><c r=\"A1\"><v>1</v></c></row><row r=\"2\"><c r=\"A2\"><f>A1</f><v>1</v></c></row><row r=\"3\"><c r=\"A3\"><f>A2</f><v>1</v></c></row>",
            "",
        );
        let out = run(&xml, AxisEdit::delete(Axis::Row, 2, 1));
        assert!(
            !out.contains("A1</f><v>1</v></c></row><row r=\"3\""),
            "{out}"
        );
        assert!(
            out.contains("<row r=\"2\"><c r=\"A2\"><f>#REF!</f>"),
            "{out}"
        );
    }

    #[test]
    fn columns_move_and_spans_go() {
        let xml = sheet_xml(
            "<row r=\"1\" spans=\"1:3\"><c r=\"A1\"><v>1</v></c><c r=\"B1\"><v>2</v></c></row>",
            "",
        );
        let out = run(&xml, cols(2, 1));
        assert!(
            out.contains("<row r=\"1\"><c r=\"A1\"><v>1</v></c><c r=\"C1\"><v>2</v></c></row>"),
            "{out}"
        );
    }

    #[test]
    fn shared_formulas_become_their_own_when_anything_moves() {
        let xml = sheet_xml(
            "<row r=\"1\"><c r=\"B1\"><f t=\"shared\" ref=\"B1:B2\" si=\"0\">A1*2</f><v>2</v></c></row><row r=\"2\"><c r=\"B2\"><f t=\"shared\" si=\"0\"/><v>4</v></c></row>",
            "",
        );
        let out = run(&xml, rows(2, 1));
        assert!(out.contains("<c r=\"B1\"><f>A1*2</f>"), "{out}");
        assert!(out.contains("<c r=\"B3\"><f>A3*2</f>"), "{out}");
    }

    #[test]
    fn array_formulas_move_their_area() {
        let xml = sheet_xml(
            "<row r=\"2\"><c r=\"B2\" cm=\"1\"><f t=\"array\" ref=\"B2:B4\">SEQUENCE(3)</f><v>1</v></c></row>",
            "",
        );
        let out = run(&xml, rows(1, 2));
        assert!(
            out.contains("<f t=\"array\" ref=\"B4:B6\">SEQUENCE(3)</f>"),
            "{out}"
        );
        assert!(out.contains("cm=\"1\""), "{out}");
    }

    #[test]
    fn what_refers_to_cells_outside_sheet_data_moves_too() {
        let after = "<mergeCells count=\"2\"><mergeCell ref=\"A1:B1\"/><mergeCell ref=\"A3:A4\"/></mergeCells>\
<conditionalFormatting sqref=\"A3:A5\"><cfRule type=\"expression\" priority=\"1\"><formula>A3&gt;1</formula></cfRule></conditionalFormatting>\
<dataValidations count=\"1\"><dataValidation sqref=\"C3\"><formula1>$A$3:$A$5</formula1></dataValidation></dataValidations>\
<hyperlinks><hyperlink ref=\"A3\" location=\"S!A3\"/></hyperlinks>\
<extLst><ext uri=\"{x}\"><x14:conditionalFormattings><x14:conditionalFormatting><xm:f>S!A3</xm:f><xm:sqref>A3</xm:sqref></x14:conditionalFormatting></x14:conditionalFormattings></ext><ext uri=\"{new}\"><x99:thing/></ext></extLst>";
        let out = run(&sheet_xml("", after), rows(2, 2));
        assert!(
            out.contains("<mergeCell ref=\"A1:B1\"/><mergeCell ref=\"A5:A6\"/>"),
            "{out}"
        );
        assert!(out.contains("sqref=\"A5:A7\""), "{out}");
        assert!(out.contains("<formula>A5&gt;1</formula>"), "{out}");
        assert!(out.contains("<formula1>$A$5:$A$7</formula1>"), "{out}");
        assert!(
            out.contains("<hyperlink ref=\"A5\" location=\"S!A5\"/>"),
            "{out}"
        );
        assert!(
            out.contains("<xm:f>S!A5</xm:f><xm:sqref>A5</xm:sqref>"),
            "{out}"
        );
    }

    /// Each of these made a file Excel refuses, found by `bench/run-excel-matrix.mjs`.
    #[test]
    fn a_frozen_pane_with_nothing_left_frozen_goes() {
        let sheet = |view: &str| {
            format!("<worksheet xmlns=\"m\">{view}<sheetData></sheetData></worksheet>")
        };
        let view = |pane: &str| {
            format!(
                "<sheetViews><sheetView workbookViewId=\"0\">{pane}<selection pane=\"bottomLeft\" activeCell=\"A5\" sqref=\"A5\"/></sheetView></sheetViews>"
            )
        };
        let before = view(
            "<pane ySplit=\"2\" topLeftCell=\"A3\" activePane=\"bottomLeft\" state=\"frozen\"/>",
        );
        let out = run(&sheet(&before), AxisEdit::delete(Axis::Row, 1, 2));
        assert!(!out.contains("<pane"), "{out}");
        assert!(!out.contains("pane=\"bottomLeft\""), "{out}");
        assert!(out.contains("activeCell=\"A3\""), "{out}");

        // With both frozen, losing the rows leaves the columns.
        let before = "<sheetViews><sheetView workbookViewId=\"0\"><pane xSplit=\"1\" ySplit=\"2\" topLeftCell=\"B3\" activePane=\"bottomRight\" state=\"frozen\"/><selection pane=\"topRight\"/><selection pane=\"bottomLeft\"/><selection pane=\"bottomRight\" activeCell=\"B5\" sqref=\"B5\"/></sheetView></sheetViews>";
        let out = run(&sheet(before), AxisEdit::delete(Axis::Row, 1, 2));
        assert!(out.contains("xSplit=\"1\""), "{out}");
        assert!(!out.contains("ySplit"), "{out}");
        assert!(out.contains("activePane=\"topRight\""), "{out}");
        assert_eq!(out.matches("<selection").count(), 1, "{out}");
        assert!(
            out.contains("<selection pane=\"topRight\" activeCell=\"B3\""),
            "{out}"
        );

        // A pane that's only been made smaller stays.
        let before = view(
            "<pane ySplit=\"2\" topLeftCell=\"A3\" activePane=\"bottomLeft\" state=\"frozen\"/>",
        );
        let out = run(&sheet(&before), AxisEdit::delete(Axis::Row, 1, 1));
        assert!(out.contains("ySplit=\"1\""), "{out}");
        assert!(out.contains("pane=\"bottomLeft\""), "{out}");
    }

    #[test]
    fn a_sparkline_that_lost_what_it_reads_says_only_ref() {
        let group = |cell: &str| {
            format!(
                "<extLst><ext uri=\"{{y}}\"><x14:sparklineGroups><x14:sparklineGroup><x14:colorSeries rgb=\"FF000000\"/><x14:sparklines><x14:sparkline><xm:f>S!A3:A4</xm:f><xm:sqref>{cell}</xm:sqref></x14:sparkline></x14:sparklines></x14:sparklineGroup></x14:sparklineGroups></ext></extLst>"
            )
        };
        let out = run(
            &sheet_xml("", &group("C9")),
            AxisEdit::delete(Axis::Row, 3, 2),
        );
        assert!(
            out.contains("<xm:f>#REF!</xm:f><xm:sqref>C7</xm:sqref>"),
            "{out}"
        );

        // With its own cell gone, nothing is left that Excel would accept.
        let out = run(
            &sheet_xml("", &group("C3")),
            AxisEdit::delete(Axis::Row, 3, 2),
        );
        assert!(!out.contains("extLst"), "{out}");
    }

    #[test]
    fn a_shared_formula_outlives_the_row_of_its_master() {
        let cells = "<row r=\"2\"><c r=\"B2\"><f t=\"shared\" ref=\"B2:B4\" si=\"0\">A2*2</f><v>2</v></c></row>\
<row r=\"3\"><c r=\"B3\"><f t=\"shared\" si=\"0\"/><v>4</v></c></row>\
<row r=\"4\"><c r=\"B4\"><f t=\"shared\" si=\"0\"/><v>6</v></c></row>";
        let xml = format!(
            "<worksheet xmlns=\"m\" xmlns:x14=\"x\" xmlns:xm=\"y\"><sheetData>{cells}</sheetData></worksheet>"
        );
        let out = run(&xml, AxisEdit::delete(Axis::Row, 2, 1));
        assert!(out.contains("<c r=\"B2\"><f>A2*2</f>"), "{out}");
        assert!(out.contains("<c r=\"B3\"><f>A3*2</f>"), "{out}");
    }

    #[test]
    fn a_rule_with_no_cells_left_goes_with_its_empty_parents() {
        let after = "<mergeCells count=\"1\"><mergeCell ref=\"A3:A4\"/></mergeCells>\
<conditionalFormatting sqref=\"A3\"><cfRule type=\"expression\" priority=\"1\"><formula>1</formula></cfRule></conditionalFormatting>\
<dataValidations count=\"1\"><dataValidation sqref=\"A3\"/></dataValidations>\
<hyperlinks><hyperlink ref=\"A3\"/></hyperlinks>\
<extLst><ext uri=\"{x}\"><x14:conditionalFormattings><x14:conditionalFormatting><xm:sqref>A3</xm:sqref></x14:conditionalFormatting></x14:conditionalFormattings></ext></extLst>";
        let out = run(&sheet_xml("", after), AxisEdit::delete(Axis::Row, 3, 2));
        for gone in [
            "mergeCells",
            "conditionalFormatting",
            "dataValidation",
            "hyperlink",
            "extLst",
        ] {
            assert!(!out.contains(gone), "{gone} in {out}");
        }
    }

    #[test]
    fn counts_follow_what_is_left() {
        let after = "<dataValidations count=\"2\"><dataValidation sqref=\"A3\"/><dataValidation sqref=\"A9\"/></dataValidations>";
        let out = run(&sheet_xml("", after), AxisEdit::delete(Axis::Row, 3, 1));
        assert!(
            out.contains("<dataValidations count=\"1\"><dataValidation sqref=\"A8\"/>"),
            "{out}"
        );
    }

    #[test]
    fn column_widths_stretch_and_shrink() {
        let after = "<cols><col min=\"2\" max=\"3\" width=\"20\"/><col min=\"5\" max=\"5\" width=\"30\"/></cols>";
        let xml = format!("<worksheet>{after}<sheetData/></worksheet>");
        let out = run(&xml, cols(4, 2));
        assert!(
            out.contains(
                "<col min=\"2\" max=\"5\" width=\"20\"/><col min=\"7\" max=\"7\" width=\"30\"/>"
            ),
            "{out}"
        );
        let out = run(&xml, AxisEdit::delete(Axis::Col, 5, 1));
        assert!(
            out.contains("<cols><col min=\"2\" max=\"3\" width=\"20\"/></cols>"),
            "{out}"
        );
    }

    #[test]
    fn an_edit_on_another_sheet_only_touches_formulas_that_read_it() {
        let xml = sheet_xml(
            "<row r=\"1\"><c r=\"A1\"><f>Other!A5+A5</f></c><c r=\"B1\"><f>A5</f></c></row>",
            "",
        );
        let list = book("Other", rows(1, 1));
        let edits = SheetEdits {
            own: &[],
            all_own: &[],
            book: &list,
            home: "S",
            gone_tables: &[],
        };
        let mut out = Vec::new();
        let mut masters = Masters::default();
        let done = rewrite_sheet(xml.as_bytes(), &mut out, &edits, &mut masters).unwrap();
        assert!(done.changed);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("<f>Other!A6+A5</f>"), "{text}");
        assert!(text.contains("<f>A5</f>"), "{text}");
        let quiet = sheet_xml("<row r=\"1\"><c r=\"A1\"><f>A5</f></c></row>", "");
        let mut out = Vec::new();
        let done = rewrite_sheet(quiet.as_bytes(), &mut out, &edits, &mut masters).unwrap();
        assert!(!done.changed);
    }

    #[test]
    fn a_follower_before_its_master_asks_for_another_pass() {
        let xml = sheet_xml(
            "<row r=\"1\"><c r=\"A1\"><f t=\"shared\" si=\"0\"/></c></row><row r=\"2\"><c r=\"A2\"><f t=\"shared\" ref=\"A1:A2\" si=\"0\">B2</f></c></row>",
            "",
        );
        let own = [rows(1, 1)];
        let list = book("S", own[0]);
        let edits = SheetEdits {
            own: &own,
            all_own: &own,
            book: &list,
            home: "S",
            gone_tables: &[],
        };
        let mut masters = Masters::default();
        let mut out = Vec::new();
        let done = rewrite_sheet(xml.as_bytes(), &mut out, &edits, &mut masters).unwrap();
        assert!(done.again);
        let mut out = Vec::new();
        let done = rewrite_sheet(xml.as_bytes(), &mut out, &edits, &mut masters).unwrap();
        assert!(!done.again);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("<c r=\"A2\"><f>B2</f>"), "{text}");
        assert!(text.contains("<c r=\"A3\"><f>B3</f>"), "{text}");
    }

    #[test]
    fn a_table_grows_a_column_and_its_area_moves() {
        let xml = b"<table id=\"1\" ref=\"B2:C4\"><autoFilter ref=\"B2:C4\"/><tableColumns count=\"2\"><tableColumn id=\"1\" name=\"a\"/><tableColumn id=\"2\" name=\"b\"/></tableColumns></table>";
        let own = [cols(3, 1)];
        let edits = SheetEdits {
            own: &own,
            all_own: &own,
            book: &[],
            home: "S",
            gone_tables: &[],
        };
        let out = rewrite_table(xml, &edits, &mut |_| "Column1".to_string())
            .unwrap()
            .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("ref=\"B2:D4\""), "{text}");
        assert!(text.contains("<autoFilter ref=\"B2:D4\"/>"), "{text}");
        assert!(text.contains("count=\"3\""), "{text}");
        assert!(text.contains("<tableColumn id=\"1\" name=\"a\"/><tableColumn id=\"3\" name=\"Column1\"/><tableColumn id=\"2\" name=\"b\"/>"), "{text}");
    }

    #[test]
    fn comments_follow_their_cells() {
        let xml = b"<comments><commentList><comment ref=\"A2\"/><comment ref=\"A3\"/></commentList></comments>";
        let own = [AxisEdit::delete(Axis::Row, 2, 1)];
        let edits = SheetEdits {
            own: &own,
            all_own: &own,
            book: &[],
            home: "S",
            gone_tables: &[],
        };
        let out = rewrite_comments(xml, &edits).unwrap().unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "<comments><commentList><comment ref=\"A2\"/></commentList></comments>"
        );
    }

    #[test]
    fn vml_shapes_follow_and_go() {
        let xml = b"<xml><v:shape id=\"a\"><x:ClientData><x:Anchor>1, 15, 1, 2, 3, 15, 5, 4</x:Anchor><x:Row>1</x:Row><x:Column>0</x:Column></x:ClientData></v:shape><v:shape id=\"b\"><x:Row>4</x:Row><x:Column>0</x:Column></v:shape></xml>";
        let own = [AxisEdit::delete(Axis::Row, 2, 1)];
        let edits = SheetEdits {
            own: &own,
            all_own: &own,
            book: &[],
            home: "S",
            gone_tables: &[],
        };
        let out = rewrite_vml(xml, &edits).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("id=\"a\""), "{text}");
        assert!(text.contains("<x:Row>3</x:Row>"), "{text}");
        let inserted = [rows(1, 1)];
        let edits = SheetEdits {
            own: &inserted,
            all_own: &inserted,
            book: &[],
            home: "S",
            gone_tables: &[],
        };
        let out = rewrite_vml(xml, &edits).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("<x:Anchor>1, 15, 2, 2, 3, 15, 6, 4</x:Anchor>"),
            "{text}"
        );
        assert!(text.contains("<x:Row>2</x:Row>"), "{text}");
    }

    #[test]
    fn drawings_follow_as_edit_as_says() {
        let xml = br#"<xdr:wsDr><xdr:twoCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>5</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>6</xdr:rowOff></xdr:from><xdr:to><xdr:col>3</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>8</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to></xdr:twoCellAnchor><xdr:twoCellAnchor editAs="oneCell"><xdr:from><xdr:col>1</xdr:col><xdr:row>2</xdr:row></xdr:from><xdr:to><xdr:col>3</xdr:col><xdr:row>8</xdr:row></xdr:to></xdr:twoCellAnchor><xdr:twoCellAnchor editAs="absolute"><xdr:from><xdr:col>1</xdr:col><xdr:row>2</xdr:row></xdr:from><xdr:to><xdr:col>3</xdr:col><xdr:row>8</xdr:row></xdr:to></xdr:twoCellAnchor></xdr:wsDr>"#;
        let own = [rows(5, 2)];
        let edits = SheetEdits {
            own: &own,
            all_own: &own,
            book: &[],
            home: "S",
            gone_tables: &[],
        };
        let out = rewrite_drawing(xml, &edits).unwrap().unwrap();
        let text = String::from_utf8(out).unwrap();
        // Inserted inside: the plain one stretches, and the one that keeps its size doesn't.
        assert_eq!(text.matches("<xdr:row>10</xdr:row>").count(), 1, "{text}");
        assert_eq!(text.matches("<xdr:row>8</xdr:row>").count(), 2, "{text}");
        let own = [rows(1, 2)];
        let edits = SheetEdits {
            own: &own,
            all_own: &own,
            book: &[],
            home: "S",
            gone_tables: &[],
        };
        let out = rewrite_drawing(xml, &edits).unwrap().unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("<xdr:row>4</xdr:row>").count(), 2, "{text}");
        assert_eq!(text.matches("<xdr:row>10</xdr:row>").count(), 2, "{text}");
        assert_eq!(text.matches("<xdr:row>2</xdr:row>").count(), 1, "{text}");
    }

    #[test]
    fn charts_read_their_cells_after_the_edit() {
        let xml = b"<c:chartSpace><c:ser><c:f>S!$A$1:$A$5</c:f></c:ser></c:chartSpace>";
        let list = book("S", rows(2, 1));
        let out = rewrite_chart(xml, &list).unwrap().unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "<c:chartSpace><c:ser><c:f>S!$A$1:$A$6</c:f></c:ser></c:chartSpace>"
        );
        let renamed = [BookEdit::Rename {
            old: "S".to_string(),
            new: "My S".to_string(),
        }];
        let out = rewrite_chart(xml, &renamed).unwrap().unwrap();
        assert!(String::from_utf8(out).unwrap().contains("'My S'!$A$1:$A$5"));
    }

    #[test]
    fn a_pivot_source_follows_its_sheet() {
        let xml = br#"<pivotCacheDefinition><cacheSource type="worksheet"><worksheetSource ref="A1:B5" sheet="S"/></cacheSource></pivotCacheDefinition>"#;
        let list = vec![
            BookEdit::Axis {
                sheet: "S".to_string(),
                edit: rows(2, 1),
            },
            BookEdit::Rename {
                old: "S".to_string(),
                new: "T".to_string(),
            },
        ];
        let out = rewrite_pivot_cache(xml, &list).unwrap().unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("ref=\"A1:B6\""), "{text}");
        assert!(text.contains("sheet=\"T\""), "{text}");
    }
}
