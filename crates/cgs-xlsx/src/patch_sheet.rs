//! Patches one worksheet part at byte spans. Every row without a change goes out as the bytes it
//! came in, and a changed cell keeps its unknown attributes and children, so saving can't lose
//! what this app doesn't understand.

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Read, Write};

use cgs_calc::CalcValue;
use cgs_refs::SharedFormula;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::scan::{
    attr_value, closed, edit_tag, escape_text, escape_x, local_name, opened, prefix, Kind, Scanner,
};
use crate::xml::{parse_cell, parse_rect, parse_u32, unescape_attr, unescape_x};
use crate::zipdir::{Packed, Packer};

/// What happens to a cell's contents.
#[derive(Debug, Clone, PartialEq)]
pub enum CellChange {
    /// Keeps the contents, so only the style changes.
    Keep,
    /// Empties it, keeping its style.
    Clear,
    /// A constant.
    Value(CalcValue),
    /// A formula, as a file stores it, with its result.
    Formula { text: String, value: CalcValue },
    /// The formula or spill stays as the file has it, and this is its new result.
    Result(CalcValue),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CellPatch {
    pub change: CellChange,
    /// A cell format's index, if it changes.
    pub style: Option<u32>,
}

/// New text cells, which go on the end of `sharedStrings.xml`.
#[derive(Debug, Default)]
pub struct StringTable {
    pub base: u32,
    pub added: Vec<String>,
    by_text: HashMap<String, u32>,
    /// Cells that now point here, for the part's `count`.
    pub references: u64,
}

impl StringTable {
    pub fn new(base: u32) -> Self {
        Self {
            base,
            ..Self::default()
        }
    }

    pub fn intern(&mut self, text: &str) -> u32 {
        self.references += 1;
        if let Some(&index) = self.by_text.get(text) {
            return index;
        }
        let index = self.base + self.added.len() as u32;
        self.added.push(text.to_string());
        self.by_text.insert(text.to_string(), index);
        index
    }
}

/// `CT_Worksheet`'s children in the order the schema wants them.
pub const WORKSHEET_ORDER: &[&str] = &[
    "sheetPr",
    "dimension",
    "sheetViews",
    "sheetFormatPr",
    "cols",
    "sheetData",
    "sheetCalcPr",
    "sheetProtection",
    "protectedRanges",
    "scenarios",
    "autoFilter",
    "sortState",
    "dataConsolidate",
    "customSheetViews",
    "mergeCells",
    "phoneticPr",
    "conditionalFormatting",
    "dataValidations",
    "hyperlinks",
    "printOptions",
    "pageMargins",
    "pageSetup",
    "headerFooter",
    "rowBreaks",
    "colBreaks",
    "customProperties",
    "cellWatches",
    "ignoredErrors",
    "smartTags",
    "drawing",
    "legacyDrawing",
    "legacyDrawingHF",
    "drawingHF",
    "picture",
    "oleObjects",
    "controls",
    "webPublishItems",
    "tableParts",
    "extLst",
];

/// Excel wraps `oleObjects` and `controls` this way, which sets where the wrapper sits.
const ALTERNATE_CONTENT_ORDER: &str = "oleObjects";

/// Where a child element of the worksheet belongs, or `None` if nothing says.
pub fn worksheet_order(local: &[u8]) -> Option<usize> {
    let local = if local == b"AlternateContent" {
        ALTERNATE_CONTENT_ORDER.as_bytes()
    } else {
        local
    };
    WORKSHEET_ORDER
        .iter()
        .position(|name| name.as_bytes() == local)
}

/// A new child of the worksheet, written with no namespace prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewElement {
    pub name: String,
    pub xml: String,
}

/// What a pass finds that an earlier part of the sheet needed to know.
#[derive(Debug, Default, Clone)]
struct Facts {
    /// Rows that exist, known once a file turned out to have them out of order.
    rows: Option<FxHashSet<u32>>,
    /// Shared formulas whose master is being changed: index to text and place.
    masters: FxHashMap<u32, (String, u32, u32)>,
}

enum Pass {
    Finished,
    /// The file isn't in the order a single pass needs. Run again knowing this.
    Again(Facts),
}

/// How a pass went.
#[derive(Debug, Default)]
pub struct Outcome {
    pub changed: bool,
    pub cells_written: u64,
    /// New results outside the area a dynamic array's file entry covers, left to Excel.
    pub spills_skipped: u64,
}

/// A cell of the row being looked at, as ranges of `Pass`'s row buffer.
#[derive(Debug, Clone, Copy)]
struct CellSpan {
    start: usize,
    tag_end: usize,
    end: usize,
    row: u32,
    /// 0 for a cell with no usable address, which stays as it is.
    col: u32,
    has_address: bool,
}

/// Where one cell's children are.
#[derive(Debug, Default, Clone, Copy)]
struct Children {
    formula: Option<(usize, usize)>,
    value: Option<(usize, usize)>,
    inline: Option<(usize, usize)>,
    /// Everything else, as the span from its start to its end.
    other_start: Option<usize>,
    other_end: usize,
}

fn column_name(mut col: u32) -> String {
    let mut out = Vec::new();
    while col > 0 {
        col -= 1;
        out.push(b'A' + (col % 26) as u8);
        col /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

pub fn cell_name(row: u32, col: u32) -> String {
    format!("{}{row}", column_name(col))
}

/// Writes a number the way the file format reads it back exactly.
pub fn number_text(number: f64) -> String {
    let magnitude = number.abs();
    if magnitude != 0.0 && !(1e-5..1e21).contains(&magnitude) {
        format!("{number:e}")
    } else {
        format!("{number}")
    }
}

fn has_value(value: &CalcValue) -> bool {
    !matches!(value, CalcValue::Empty)
}

struct Patcher<'a> {
    patches: &'a BTreeMap<(u32, u32), CellPatch>,
    strings: &'a mut StringTable,
    elements: Vec<&'a NewElement>,
    /// Characters outside ASCII become references, because the part isn't in UTF-8.
    ascii_only: bool,
    facts: Facts,
    outcome: Outcome,
    /// Some patch replaces a cell's contents, so it could be a shared formula's master.
    has_direct: bool,

    prefix: String,
    /// What the worksheet's default namespace would need to be for a new element.
    namespace: Vec<u8>,
    col_styles: Vec<(u32, u32, u32)>,
    next_row_patch: Option<u32>,
    row: u32,
    last_row: u32,
    /// Row numbers this pass has met, in case it has to go round again.
    seen_rows: FxHashSet<u32>,
    disordered: bool,
    /// Followers of shared formulas that came before any change to their master.
    seen_followers: FxHashSet<u32>,
    rerun_for_masters: bool,

    row_buffer: Vec<u8>,
    cells: Vec<CellSpan>,
    scratch: Vec<u8>,
    /// Patches already applied to a cell whose address names a row other than the one it sits in.
    applied_elsewhere: FxHashSet<(u32, u32)>,
}

fn io_err(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

impl Patcher<'_> {
    fn patches_in_row(&self, row: u32) -> impl Iterator<Item = (&(u32, u32), &CellPatch)> {
        self.patches.range((row, 0)..=(row, u32::MAX))
    }

    fn name(&self, local: &str) -> String {
        format!("{}{local}", self.prefix)
    }

    fn escape_into(&self, text: &str, out: &mut Vec<u8>) {
        if self.ascii_only {
            let mut plain = Vec::with_capacity(text.len());
            escape_text(text, &mut plain);
            for ch in String::from_utf8_lossy(&plain).chars() {
                if ch.is_ascii() {
                    out.push(ch as u8);
                } else {
                    out.extend_from_slice(format!("&#{};", ch as u32).as_bytes());
                }
            }
        } else {
            escape_text(text, out);
        }
    }

    /// The `t` attribute and `<v>` for a value, which may add to the shared strings.
    fn value_parts(&mut self, value: &CalcValue, formula: bool) -> (Option<&'static str>, Vec<u8>) {
        let mut body = Vec::new();
        let open_value = |body: &mut Vec<u8>, name: &str| {
            body.push(b'<');
            body.extend_from_slice(name.as_bytes());
            body.push(b'>');
        };
        let name = self.name("v");
        let kind = match value {
            CalcValue::Empty => return (None, body),
            CalcValue::Number(number) => {
                open_value(&mut body, &name);
                body.extend_from_slice(number_text(*number).as_bytes());
                None
            }
            CalcValue::Bool(flag) => {
                open_value(&mut body, &name);
                body.push(if *flag { b'1' } else { b'0' });
                Some("b")
            }
            CalcValue::Error(text) => {
                open_value(&mut body, &name);
                self.escape_into(text, &mut body);
                Some("e")
            }
            CalcValue::Text(text) if formula => {
                open_value(&mut body, &name);
                self.escape_into(&escape_x(text), &mut body);
                Some("str")
            }
            CalcValue::Text(text) => {
                open_value(&mut body, &name);
                let index = self.strings.intern(text);
                body.extend_from_slice(index.to_string().as_bytes());
                Some("s")
            }
        };
        body.extend_from_slice(format!("</{name}>").as_bytes());
        (kind, body)
    }

    /// The style a new cell here takes: the row's, or else the column's, as Excel does.
    fn inherited_style(&self, row_tag: Option<&[u8]>, col: u32) -> Option<u32> {
        if let Some(tag) = row_tag {
            let custom = attr_value(tag, b"customFormat").is_some_and(crate::xml::parse_bool);
            if custom {
                if let Some(style) = attr_value(tag, b"s").and_then(parse_u32) {
                    return Some(style);
                }
            }
        }
        self.col_styles
            .iter()
            .find(|&&(first, last, _)| col >= first && col <= last)
            .map(|&(_, _, style)| style)
    }

    /// A cell the file doesn't have yet. Returns its bytes, or nothing if it would be empty.
    fn new_cell(
        &mut self,
        row: u32,
        col: u32,
        patch: &CellPatch,
        row_tag: Option<&[u8]>,
    ) -> Vec<u8> {
        let style = patch.style.or_else(|| self.inherited_style(row_tag, col));
        let mut out = Vec::new();
        let (kind, body) = match &patch.change {
            CellChange::Keep | CellChange::Clear => (None, Vec::new()),
            CellChange::Value(value) => self.value_parts(value, false),
            // A result with no cell is a spill that grew. Excel rebuilds it on opening.
            CellChange::Result(value) => {
                if has_value(value) {
                    self.outcome.spills_skipped += 1;
                }
                return out;
            }
            CellChange::Formula { text, value } => {
                let (kind, mut body) = self.value_parts(value, true);
                let mut formula = format!("<{}>", self.name("f")).into_bytes();
                self.escape_into(text, &mut formula);
                formula.extend_from_slice(format!("</{}>", self.name("f")).as_bytes());
                formula.append(&mut body);
                (kind, formula)
            }
        };
        if body.is_empty() && style.is_none_or(|style| style == 0) {
            return out;
        }
        out.extend_from_slice(
            format!("<{} r=\"{}\"", self.name("c"), cell_name(row, col)).as_bytes(),
        );
        if let Some(style) = style.filter(|&style| style != 0) {
            out.extend_from_slice(format!(" s=\"{style}\"").as_bytes());
        }
        if let Some(kind) = kind {
            out.extend_from_slice(format!(" t=\"{kind}\"").as_bytes());
        }
        if body.is_empty() {
            out.extend_from_slice(b"/>");
        } else {
            out.push(b'>');
            out.extend_from_slice(&body);
            out.extend_from_slice(format!("</{}>", self.name("c")).as_bytes());
        }
        self.outcome.changed = true;
        self.outcome.cells_written += 1;
        out
    }

    /// Writes rows the file doesn't have, numbered below `limit`.
    fn new_rows_before(&mut self, limit: u32, out: &mut dyn Write) -> io::Result<()> {
        while let Some(row) = self.next_row_patch.filter(|&row| row < limit) {
            self.next_row_patch = self
                .patches
                .range((row + 1, 0)..)
                .next()
                .map(|(&(next, _), _)| next);
            if self
                .facts
                .rows
                .as_ref()
                .is_some_and(|rows| rows.contains(&row))
            {
                continue;
            }
            let mut body = Vec::new();
            let list: Vec<((u32, u32), CellPatch)> = self
                .patches_in_row(row)
                .filter(|(at, _)| !self.applied_elsewhere.contains(at))
                .map(|(&at, patch)| (at, patch.clone()))
                .collect();
            for ((_, col), patch) in &list {
                let cell = self.new_cell(row, *col, patch, None);
                body.extend_from_slice(&cell);
            }
            if body.is_empty() {
                continue;
            }
            out.write_all(format!("<{} r=\"{row}\">", self.name("row")).as_bytes())?;
            out.write_all(&body)?;
            out.write_all(format!("</{}>", self.name("row")).as_bytes())?;
        }
        Ok(())
    }

    /// Reads a row's tokens into the buffer, noting where its cells are.
    fn read_row<R: Read>(&mut self, scanner: &mut Scanner<R>) -> io::Result<()> {
        self.row_buffer.clear();
        self.cells.clear();
        let mut col = 0;
        let mut open_cell: Option<CellSpan> = None;
        while let Some(kind) = scanner.next()? {
            let token = scanner.token();
            let at = self.row_buffer.len();
            self.row_buffer.extend_from_slice(token);
            match kind {
                Kind::Start | Kind::Empty if local_name(token) == b"c" => {
                    if let Some(mut cell) = open_cell.take() {
                        // A cell left open by a damaged file ends where the next begins.
                        cell.end = at;
                        self.cells.push(cell);
                    }
                    let (mut row, address) = (self.row, attr_value(token, b"r"));
                    let has_address = address.is_some();
                    let target = match address {
                        Some(text) => parse_cell(text),
                        None => {
                            col += 1;
                            Some((row, col))
                        }
                    };
                    let valid_col = match target {
                        Some((cell_row, cell_col))
                            if cell_col <= cgs_model::MAX_COLS
                                && cell_row <= cgs_model::MAX_ROWS =>
                        {
                            row = cell_row;
                            self.row = cell_row;
                            col = cell_col;
                            cell_col
                        }
                        _ => 0,
                    };
                    let span = CellSpan {
                        start: at,
                        tag_end: at + token.len(),
                        end: at + token.len(),
                        row,
                        col: valid_col,
                        has_address,
                    };
                    if kind == Kind::Empty {
                        self.cells.push(span);
                    } else {
                        open_cell = Some(span);
                    }
                }
                Kind::End => match local_name(token) {
                    b"c" => {
                        if let Some(mut cell) = open_cell.take() {
                            cell.end = at + token.len();
                            self.cells.push(cell);
                        }
                    }
                    b"row" => {
                        if let Some(mut cell) = open_cell.take() {
                            cell.end = at;
                            self.cells.push(cell);
                        }
                        // The closing tag isn't part of the row's body.
                        self.row_buffer.truncate(at);
                        return Ok(());
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        if let Some(mut cell) = open_cell.take() {
            cell.end = self.row_buffer.len();
            self.cells.push(cell);
        }
        Err(io_err("a row of this sheet never ends"))
    }

    /// Finds a cell's children, by scanning its body in the row buffer.
    fn children(&self, cell: CellSpan) -> io::Result<Children> {
        let mut found = Children::default();
        let closing = if cell.end > cell.tag_end {
            self.row_buffer[cell.tag_end..cell.end]
                .iter()
                .rposition(|&byte| byte == b'<')
                .map_or(cell.end, |at| cell.tag_end + at)
        } else {
            cell.end
        };
        let body = &self.row_buffer[cell.tag_end..closing];
        let mut scanner = Scanner::new(body);
        let mut depth = 0;
        let mut current: Option<(u8, usize)> = None;
        while let Some(kind) = scanner.next()? {
            let at = cell.tag_end + scanner.position() as usize;
            let token = scanner.token();
            let end = at + token.len();
            let close = |found: &mut Children, which: u8, start: usize| match which {
                b'f' => found.formula = Some((start, end)),
                b'v' => found.value = Some((start, end)),
                b'i' => found.inline = Some((start, end)),
                _ => {
                    if found.other_start.is_none() {
                        found.other_start = Some(start);
                    }
                    found.other_end = end;
                }
            };
            let which = |token: &[u8]| match local_name(token) {
                b"f" => b'f',
                b"v" => b'v',
                b"is" => b'i',
                _ => b'x',
            };
            match kind {
                Kind::Start => {
                    if depth == 0 {
                        current = Some((which(token), at));
                    }
                    depth += 1;
                }
                Kind::Empty => {
                    if depth == 0 {
                        close(&mut found, which(token), at);
                    }
                }
                Kind::End => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some((kind, start)) = current.take() {
                            close(&mut found, kind, start);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(found)
    }

    /// A formula element's `t="shared"` index, and whether it holds the group's text.
    fn shared_formula(&self, span: (usize, usize)) -> Option<(u32, bool)> {
        let formula = &self.row_buffer[span.0..span.1];
        let tag_end = formula.iter().position(|&byte| byte == b'>')? + 1;
        let tag = &formula[..tag_end];
        if attr_value(tag, b"t") != Some(b"shared") {
            return None;
        }
        let index = attr_value(tag, b"si").and_then(parse_u32)?;
        let text_end = formula.iter().rposition(|&byte| byte == b'<')?;
        Some((index, tag_end <= text_end && text_end > tag_end))
    }

    fn formula_text(&self, span: (usize, usize)) -> String {
        let formula = &self.row_buffer[span.0..span.1];
        let tag_end = formula
            .iter()
            .position(|&byte| byte == b'>')
            .map_or(0, |at| at + 1);
        let text_end = formula
            .iter()
            .rposition(|&byte| byte == b'<')
            .unwrap_or(tag_end);
        unescape_attr(&formula[tag_end..text_end.max(tag_end)])
    }

    /// A follower of a master that is changing, as a formula of its own.
    fn expanded_follower(
        &self,
        cell: CellSpan,
        span: (usize, usize),
        index: u32,
    ) -> Option<Vec<u8>> {
        let (text, master_row, master_col) = self.facts.masters.get(&index)?;
        let formula = &self.row_buffer[span.0..span.1];
        let tag_end = formula.iter().position(|&byte| byte == b'>')? + 1;
        let tag = opened(&edit_tag(
            &formula[..tag_end],
            &[("t", None), ("si", None), ("ref", None)],
        ));
        let mut rendered = String::new();
        SharedFormula::new(text).render(
            i64::from(cell.row) - i64::from(*master_row),
            i64::from(cell.col) - i64::from(*master_col),
            &mut rendered,
        );
        let mut out = tag;
        self.escape_into(&rendered, &mut out);
        out.extend_from_slice(format!("</{}>", self.name("f")).as_bytes());
        Some(out)
    }

    /// The value a cell stores, for telling whether a new result is any different.
    fn stored_result(&self, cell: CellSpan, children: &Children) -> Option<CalcValue> {
        let tag = &self.row_buffer[cell.start..cell.tag_end];
        let kind = attr_value(tag, b"t").unwrap_or(b"n");
        let Some(span) = children.value else {
            return match kind {
                b"str" => Some(CalcValue::Text(String::new())),
                _ => Some(CalcValue::Empty),
            };
        };
        let element = &self.row_buffer[span.0..span.1];
        let start = element.iter().position(|&byte| byte == b'>')? + 1;
        let end = element.iter().rposition(|&byte| byte == b'<')?;
        if end <= start {
            return match kind {
                b"str" => Some(CalcValue::Text(String::new())),
                _ => Some(CalcValue::Empty),
            };
        }
        let text = std::str::from_utf8(&element[start..end]).ok()?;
        match kind {
            b"n" => text.trim().parse().ok().map(CalcValue::Number),
            b"b" => Some(CalcValue::Bool(matches!(
                text.trim(),
                "1" | "true" | "TRUE"
            ))),
            b"e" => Some(CalcValue::Error(text.trim().to_string())),
            b"str" => {
                let plain = unescape_attr(text.as_bytes());
                Some(CalcValue::Text(unescape_x(&plain).into_owned()))
            }
            // A shared string or a date isn't worth resolving: the result goes in as written.
            _ => None,
        }
    }

    /// Writes a changed cell, leaving alone what the change doesn't cover.
    fn patched_cell(
        &mut self,
        cell: CellSpan,
        patch: &CellPatch,
        out: &mut Vec<u8>,
    ) -> io::Result<()> {
        let children = self.children(cell)?;
        let tag = self.row_buffer[cell.start..cell.tag_end].to_vec();
        let address = (!cell.has_address).then(|| cell_name(cell.row, cell.col));
        let style = patch.style.map(|style| style.to_string());
        let mut changes: Vec<(&str, Option<&str>)> = Vec::new();
        if let Some(address) = &address {
            changes.push(("r", Some(address)));
        }
        if let Some(style) = &style {
            changes.push(("s", Some(style)));
        }
        let (kind, new_body, keep_formula) = match &patch.change {
            CellChange::Keep => {
                if changes.is_empty() {
                    out.extend_from_slice(&self.row_buffer[cell.start..cell.end]);
                    return Ok(());
                }
                out.extend_from_slice(&edit_tag(&tag, &changes));
                out.extend_from_slice(&self.row_buffer[cell.tag_end..cell.end]);
                self.outcome.changed = true;
                return Ok(());
            }
            CellChange::Clear => (None, Vec::new(), false),
            CellChange::Value(value) => {
                let (kind, body) = self.value_parts(value, false);
                (kind, body, false)
            }
            CellChange::Formula { text, value } => {
                let mut formula = format!("<{}>", self.name("f")).into_bytes();
                self.escape_into(text, &mut formula);
                formula.extend_from_slice(format!("</{}>", self.name("f")).as_bytes());
                let (kind, mut body) = self.value_parts(value, true);
                formula.append(&mut body);
                (kind, formula, false)
            }
            CellChange::Result(value) => {
                if style.is_none() && address.is_none() {
                    if let Some(stored) = self.stored_result(cell, &children) {
                        let same = match (&stored, value) {
                            // A value the file holds has the same bits when nothing changed it.
                            (CalcValue::Number(a), CalcValue::Number(b)) => {
                                a.to_bits() == b.to_bits()
                            }
                            (a, b) => a == b,
                        };
                        if same {
                            self.write_follower_checked(cell, &children, out)?;
                            return Ok(());
                        }
                    }
                }
                let (kind, body) = self.value_parts(value, true);
                (kind, body, true)
            }
        };
        changes.push(("t", kind));
        // A new value isn't the array or rich value those attributes described.
        if !keep_formula {
            changes.push(("cm", None));
        }
        changes.push(("vm", None));
        self.outcome.changed = true;
        self.outcome.cells_written += 1;

        let mut body = Vec::new();
        if keep_formula {
            if let Some(span) = children.formula {
                match self
                    .shared_formula(span)
                    .and_then(|(index, master)| (!master).then_some(index))
                    .and_then(|index| self.expanded_follower(cell, span, index))
                {
                    Some(formula) => body.extend_from_slice(&formula),
                    None => body.extend_from_slice(&self.row_buffer[span.0..span.1]),
                }
            }
        }
        body.extend_from_slice(&new_body);
        if let Some(start) = children.other_start {
            body.extend_from_slice(&self.row_buffer[start..children.other_end]);
        }
        let tag = edit_tag(&tag, &changes);
        if body.is_empty() {
            out.extend_from_slice(&closed(&tag));
        } else {
            out.extend_from_slice(&opened(&tag));
            out.extend_from_slice(&body);
            out.extend_from_slice(format!("</{}>", self.name("c")).as_bytes());
        }
        Ok(())
    }

    /// Writes a cell as it is, unless its shared formula's master is changing.
    fn write_follower_checked(
        &mut self,
        cell: CellSpan,
        children: &Children,
        out: &mut Vec<u8>,
    ) -> io::Result<()> {
        if !self.facts.masters.is_empty() {
            if let Some(span) = children.formula {
                let follower = self
                    .shared_formula(span)
                    .and_then(|(index, master)| (!master).then_some(index));
                if let Some(formula) =
                    follower.and_then(|index| self.expanded_follower(cell, span, index))
                {
                    out.extend_from_slice(&self.row_buffer[cell.start..span.0]);
                    out.extend_from_slice(&formula);
                    out.extend_from_slice(&self.row_buffer[span.1..cell.end]);
                    self.outcome.changed = true;
                    return Ok(());
                }
            }
        }
        out.extend_from_slice(&self.row_buffer[cell.start..cell.end]);
        Ok(())
    }

    /// Notes shared formulas in a row, so that changes to their masters reach their followers.
    fn note_shared_formulas(&mut self) -> io::Result<()> {
        if !self.has_direct {
            return Ok(());
        }
        let cells = std::mem::take(&mut self.cells);
        for &cell in &cells {
            // Scanning every cell costs little, and only a body holding `<f` can matter.
            if !self.row_buffer[cell.tag_end..cell.end]
                .windows(2)
                .any(|window| window == b"<f" || window == b":f")
            {
                continue;
            }
            let children = self.children(cell)?;
            let Some(span) = children.formula else {
                continue;
            };
            let Some((index, master)) = self.shared_formula(span) else {
                continue;
            };
            if !master {
                if !self.facts.masters.contains_key(&index) {
                    self.seen_followers.insert(index);
                }
                continue;
            }
            let patched = self
                .patches
                .get(&(cell.row, cell.col))
                .is_some_and(|patch| {
                    matches!(
                        patch.change,
                        CellChange::Clear | CellChange::Value(_) | CellChange::Formula { .. }
                    )
                });
            if patched && !self.facts.masters.contains_key(&index) {
                if self.seen_followers.contains(&index) {
                    self.rerun_for_masters = true;
                }
                let text = self.formula_text(span);
                self.facts.masters.insert(index, (text, cell.row, cell.col));
            }
        }
        self.cells = cells;
        Ok(())
    }

    /// Writes one row, after `read_row` has filled the buffer.
    fn write_row(
        &mut self,
        row_tag: &[u8],
        row: u32,
        closing: Option<&[u8]>,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        let has_patches = self.patches_in_row(row).next().is_some();
        let spreads = self.cells.iter().any(|cell| cell.row != row);
        if !has_patches && !spreads && self.facts.masters.is_empty() {
            out.write_all(row_tag)?;
            out.write_all(&self.row_buffer)?;
            if let Some(closing) = closing {
                out.write_all(closing)?;
            }
            return Ok(());
        }
        let mut body = std::mem::take(&mut self.scratch);
        body.clear();
        let cells = std::mem::take(&mut self.cells);
        let listed: Vec<((u32, u32), CellPatch)> = self
            .patches_in_row(row)
            .map(|(&at, patch)| (at, patch.clone()))
            .collect();
        let matched: FxHashSet<u32> = cells
            .iter()
            .filter(|c| c.row == row)
            .map(|c| c.col)
            .collect();
        let mut waiting = listed
            .iter()
            .filter(|((_, col), _)| !matched.contains(col))
            .peekable();
        let mut copied = 0;
        let mut added_outside = (u32::MAX, 0u32);
        let mut changed_here = false;
        for &cell in &cells {
            while let Some(((_, col), patch)) = waiting.peek().copied() {
                if cell.col == 0 || cell.row != row || *col > cell.col {
                    break;
                }
                let bytes = self.new_cell(row, *col, patch, Some(row_tag));
                if !bytes.is_empty() {
                    body.extend_from_slice(&self.row_buffer[copied..cell.start]);
                    copied = cell.start;
                    body.extend_from_slice(&bytes);
                    added_outside = (added_outside.0.min(*col), added_outside.1.max(*col));
                    changed_here = true;
                }
                waiting.next();
            }
            let patch = self
                .patches
                .get(&(cell.row, cell.col))
                .filter(|_| cell.col != 0)
                .cloned();
            if patch.is_some() && cell.row != row {
                self.applied_elsewhere.insert((cell.row, cell.col));
            }
            let needs_address = has_patches && !cell.has_address && cell.col != 0;
            let was = self.outcome.changed;
            self.outcome.changed = false;
            let mut written = Vec::new();
            match &patch {
                Some(patch) => self.patched_cell(cell, patch, &mut written)?,
                None if needs_address => {
                    let keep = CellPatch {
                        change: CellChange::Keep,
                        style: None,
                    };
                    self.patched_cell(cell, &keep, &mut written)?;
                }
                None => {
                    let children = if self.facts.masters.is_empty() {
                        Children::default()
                    } else {
                        self.children(cell)?
                    };
                    self.write_follower_checked(cell, &children, &mut written)?;
                }
            }
            let cell_changed = self.outcome.changed;
            self.outcome.changed = was || cell_changed;
            if cell_changed {
                body.extend_from_slice(&self.row_buffer[copied..cell.start]);
                body.extend_from_slice(&written);
                copied = cell.end;
                changed_here = true;
            }
        }
        for ((_, col), patch) in waiting {
            let bytes = self.new_cell(row, *col, patch, Some(row_tag));
            if !bytes.is_empty() {
                // After the last cell, and before anything that isn't a cell.
                let after = cells.last().map_or(0, |cell| cell.end).max(copied);
                body.extend_from_slice(&self.row_buffer[copied..after]);
                copied = after;
                body.extend_from_slice(&bytes);
                added_outside = (added_outside.0.min(*col), added_outside.1.max(*col));
                changed_here = true;
            }
        }
        body.extend_from_slice(&self.row_buffer[copied..]);
        self.cells = cells;

        if !changed_here {
            out.write_all(row_tag)?;
            out.write_all(&self.row_buffer)?;
            if let Some(closing) = closing {
                out.write_all(closing)?;
            }
            self.scratch = body;
            return Ok(());
        }
        let number = row.to_string();
        let mut changes: Vec<(&str, Option<&str>)> = Vec::new();
        if attr_value(row_tag, b"r").is_none() {
            changes.push(("r", Some(&number)));
        }
        // `spans` is only a hint, and one that a new cell makes wrong has to go.
        if let Some(spans) = attr_value(row_tag, b"spans") {
            if added_outside.1 != 0 && !spans_cover(spans, added_outside) {
                changes.push(("spans", None));
            }
        }
        let tag = edit_tag(row_tag, &changes);
        if closing.is_some() {
            out.write_all(&tag)?;
        } else {
            out.write_all(&opened(&tag))?;
        }
        out.write_all(&body)?;
        match closing {
            Some(closing) => out.write_all(closing)?,
            None => out.write_all(format!("</{}>", self.name("row")).as_bytes())?,
        }
        self.scratch = body;
        Ok(())
    }

    fn read_sheet_data<R: Read>(
        &mut self,
        scanner: &mut Scanner<R>,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        loop {
            let Some(kind) = scanner.next()? else {
                return Err(io_err("this sheet's cells never end"));
            };
            let token = scanner.token();
            match kind {
                Kind::End if local_name(token) == b"sheetData" => {
                    let closing = token.to_vec();
                    self.new_rows_before(u32::MAX, out)?;
                    out.write_all(&closing)?;
                    return Ok(());
                }
                Kind::Start | Kind::Empty if local_name(token) == b"row" => {
                    let row_tag = token.to_vec();
                    let number = attr_value(&row_tag, b"r").and_then(parse_u32);
                    let row = number.unwrap_or(self.row + 1);
                    if row < self.last_row {
                        self.disordered = true;
                    }
                    self.seen_rows.insert(row);
                    self.row = row;
                    self.last_row = self.last_row.max(row);
                    self.new_rows_before(row, out)?;
                    if kind == Kind::Empty {
                        self.row_buffer.clear();
                        self.cells.clear();
                        self.write_row(&row_tag, row, None, out)?;
                        self.skip_row_patches(row);
                        continue;
                    }
                    self.read_row(scanner)?;
                    let closing = format!("</{}>", self.name("row"));
                    self.note_shared_formulas()?;
                    self.write_row(&row_tag, row, Some(closing.as_bytes()), out)?;
                    self.skip_row_patches(row);
                }
                _ => out.write_all(token)?,
            }
        }
    }

    /// A row that exists takes its own patches, so the row-by-row walk moves past them.
    fn skip_row_patches(&mut self, row: u32) {
        if self.next_row_patch == Some(row) {
            self.next_row_patch = self
                .patches
                .range((row + 1, 0)..)
                .next()
                .map(|(&(next, _), _)| next);
        }
    }

    /// Writes new elements that belong before `limit` in the schema's order.
    fn elements_before(&mut self, limit: Option<usize>, out: &mut dyn Write) -> io::Result<()> {
        let mut keep = Vec::new();
        for element in std::mem::take(&mut self.elements) {
            let order = worksheet_order(element.name.as_bytes());
            // Past its place, or at the very end for a name the table doesn't have.
            let due = limit.is_none_or(|limit| order.is_some_and(|order| order < limit));
            if !due {
                keep.push(element);
                continue;
            }
            self.outcome.changed = true;
            out.write_all(&self.element_bytes(&element.xml))?;
        }
        self.elements = keep;
        Ok(())
    }

    /// A new element in a worksheet with a prefix declares the default namespace for itself.
    fn element_bytes(&self, xml: &str) -> Vec<u8> {
        if self.prefix.is_empty() {
            return xml.as_bytes().to_vec();
        }
        let end = xml
            .bytes()
            .position(|byte| byte.is_ascii_whitespace() || byte == b'>' || byte == b'/')
            .unwrap_or(xml.len());
        let mut out = xml.as_bytes()[..end].to_vec();
        out.extend_from_slice(b" xmlns=\"");
        out.extend_from_slice(&self.namespace);
        out.push(b'"');
        out.extend_from_slice(&xml.as_bytes()[end..]);
        out
    }

    fn dimension_tag(&self, tag: &[u8]) -> Vec<u8> {
        let mut bounds = None;
        for (&(row, col), patch) in self.patches {
            let grows = match &patch.change {
                CellChange::Value(value) => has_value(value),
                CellChange::Formula { .. } => true,
                CellChange::Keep | CellChange::Clear => patch.style.is_some(),
                // Nothing new is written for a result.
                CellChange::Result(_) => false,
            };
            if grows {
                let (top, left, bottom, right): (u32, u32, u32, u32) =
                    bounds.unwrap_or((row, col, row, col));
                bounds = Some((top.min(row), left.min(col), bottom.max(row), right.max(col)));
            }
        }
        let Some((top, left, bottom, right)) = bounds else {
            return tag.to_vec();
        };
        let text = attr_value(tag, b"ref").map(|raw| String::from_utf8_lossy(raw).into_owned());
        let Some(area) = text.as_deref().and_then(parse_rect) else {
            return tag.to_vec();
        };
        let (new_top, new_left) = (area.top.min(top), area.left.min(left));
        let (new_bottom, new_right) = (area.bottom.max(bottom), area.right.max(right));
        // `A1` with nothing in it means an empty sheet, not one holding A1.
        let empty = text.as_deref() == Some("A1");
        let (new_top, new_left) = if empty {
            (top, left)
        } else {
            (new_top, new_left)
        };
        let reference = if (new_top, new_left) == (new_bottom, new_right) {
            cell_name(new_top, new_left)
        } else {
            format!(
                "{}:{}",
                cell_name(new_top, new_left),
                cell_name(new_bottom, new_right)
            )
        };
        edit_tag(tag, &[("ref", Some(&reference))])
    }

    fn run<R: Read>(&mut self, reader: R, out: &mut dyn Write) -> io::Result<Pass> {
        let mut scanner = Scanner::new(reader);
        let mut depth = 0u32;
        let mut wrote_data = false;
        let needs_data = !self.patches.is_empty();
        while let Some(kind) = scanner.next()? {
            let token = scanner.token();
            match kind {
                Kind::Start | Kind::Empty if depth == 1 => {
                    let tag = token.to_vec();
                    let local = local_name(&tag).to_vec();
                    let order = worksheet_order(&local);
                    let data_order = worksheet_order(b"sheetData");
                    if needs_data && !wrote_data && order > data_order && local != b"sheetData" {
                        // A worksheet with no `<sheetData>` at all, which the schema requires.
                        self.elements_before(data_order, out)?;
                        self.write_new_sheet_data(out)?;
                        wrote_data = true;
                    }
                    if order.is_some() {
                        self.elements_before(order, out)?;
                    }
                    match local.as_slice() {
                        b"sheetData" if kind == Kind::Start => {
                            out.write_all(&tag)?;
                            wrote_data = true;
                            self.read_sheet_data(&mut scanner, out)?;
                        }
                        b"sheetData" => {
                            wrote_data = true;
                            if needs_data {
                                self.write_new_sheet_data(out)?;
                            } else {
                                out.write_all(&tag)?;
                            }
                        }
                        b"dimension" => {
                            out.write_all(&self.dimension_tag(&tag))?;
                            if kind == Kind::Start {
                                depth += 1;
                            }
                        }
                        b"cols" => {
                            out.write_all(&tag)?;
                            if kind == Kind::Start {
                                self.read_cols(&mut scanner, out)?;
                            }
                        }
                        _ => {
                            out.write_all(&tag)?;
                            if kind == Kind::Start {
                                depth += 1;
                            }
                        }
                    }
                }
                Kind::Start => {
                    if depth == 0 {
                        self.prefix = prefix(token);
                        let key = format!(
                            "xmlns{}{}",
                            if self.prefix.is_empty() { "" } else { ":" },
                            self.prefix.trim_end_matches(':')
                        );
                        self.namespace = attr_value(token, key.as_bytes())
                            .unwrap_or_default()
                            .to_vec();
                    }
                    depth += 1;
                    out.write_all(token)?;
                }
                Kind::End => {
                    if depth == 1 {
                        let closing = token.to_vec();
                        if needs_data && !wrote_data {
                            self.write_new_sheet_data(out)?;
                            wrote_data = true;
                        }
                        self.elements_before(None, out)?;
                        depth = 0;
                        out.write_all(&closing)?;
                    } else {
                        depth = depth.saturating_sub(1);
                        out.write_all(token)?;
                    }
                }
                _ => out.write_all(token)?,
            }
        }
        if !self.elements.is_empty() || (needs_data && !wrote_data) {
            return Err(io_err("this sheet ends before its worksheet does"));
        }
        if self.rerun_for_masters || (self.disordered && self.facts.rows.is_none()) {
            let mut facts = std::mem::take(&mut self.facts);
            if self.disordered {
                facts.rows = Some(std::mem::take(&mut self.seen_rows));
            }
            return Ok(Pass::Again(facts));
        }
        Ok(Pass::Finished)
    }

    fn write_new_sheet_data(&mut self, out: &mut dyn Write) -> io::Result<()> {
        let name = self.name("sheetData");
        out.write_all(format!("<{name}>").as_bytes())?;
        self.new_rows_before(u32::MAX, out)?;
        out.write_all(format!("</{name}>").as_bytes())
    }

    /// Copies `<cols>`, noting column styles, which a new cell takes.
    fn read_cols<R: Read>(
        &mut self,
        scanner: &mut Scanner<R>,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        while let Some(kind) = scanner.next()? {
            let token = scanner.token();
            out.write_all(token)?;
            match kind {
                Kind::Start | Kind::Empty if local_name(token) == b"col" => {
                    let first = attr_value(token, b"min").and_then(parse_u32);
                    let last = attr_value(token, b"max").and_then(parse_u32).or(first);
                    let style = attr_value(token, b"style").and_then(parse_u32);
                    if let (Some(first), Some(last), Some(style)) = (first, last, style) {
                        self.col_styles.push((first, last, style));
                    }
                }
                Kind::End if local_name(token) == b"cols" => return Ok(()),
                _ => {}
            }
        }
        Ok(())
    }
}

/// Whether a row's `spans` hint covers this range of columns.
fn spans_cover(spans: &[u8], (first, last): (u32, u32)) -> bool {
    String::from_utf8_lossy(spans)
        .split_whitespace()
        .any(|span| {
            let mut ends = span.split(':').map(|end| end.parse::<u32>().unwrap_or(0));
            let low = ends.next().unwrap_or(0);
            let high = ends.next().unwrap_or(low);
            first >= low && last <= high
        })
}

/// Runs passes until one finishes, with new output each time, so a pass that has to be repeated
/// leaves nothing behind.
pub fn patch_into<R: Read, W: Write>(
    mut open: impl FnMut() -> io::Result<R>,
    mut output: impl FnMut() -> W,
    patches: &BTreeMap<(u32, u32), CellPatch>,
    elements: &[NewElement],
    strings: &mut StringTable,
    ascii_only: bool,
) -> io::Result<(W, Outcome)> {
    let has_direct = patches.values().any(|patch| {
        matches!(
            patch.change,
            CellChange::Clear | CellChange::Value(_) | CellChange::Formula { .. }
        )
    });
    let references = strings.references;
    let mut facts = Facts::default();
    // Two rounds learn everything there is to learn, so a third can only be a bug.
    for _ in 0..3 {
        strings.references = references;
        let mut patcher = Patcher {
            patches,
            strings: &mut *strings,
            elements: elements.iter().collect(),
            ascii_only,
            facts: facts.clone(),
            outcome: Outcome::default(),
            has_direct,
            prefix: String::new(),
            namespace: Vec::new(),
            col_styles: Vec::new(),
            next_row_patch: patches.keys().next().map(|&(row, _)| row),
            row: 0,
            last_row: 0,
            seen_rows: FxHashSet::default(),
            disordered: false,
            seen_followers: FxHashSet::default(),
            rerun_for_masters: false,
            row_buffer: Vec::new(),
            cells: Vec::new(),
            scratch: Vec::new(),
            applied_elsewhere: FxHashSet::default(),
        };
        let mut out = output();
        match patcher.run(open()?, &mut out)? {
            Pass::Finished => return Ok((out, std::mem::take(&mut patcher.outcome))),
            Pass::Again(next) => facts = next,
        }
    }
    Err(io_err("this sheet can't be put in an order that saves"))
}

/// Patches a worksheet part. Returns nothing if the part comes out the same, which only a sheet
/// whose new results all match what it stores can do, and then it was read without being packed.
pub fn patch_sheet<R: Read>(
    mut open: impl FnMut() -> io::Result<R>,
    patches: &BTreeMap<(u32, u32), CellPatch>,
    elements: &[NewElement],
    strings: &mut StringTable,
    ascii_only: bool,
) -> io::Result<(Option<Packed>, Outcome)> {
    let only_results = elements.is_empty()
        && patches
            .values()
            .all(|patch| matches!(patch.change, CellChange::Result(_)) && patch.style.is_none());
    if only_results {
        let (_, probe) = patch_into(&mut open, io::sink, patches, elements, strings, ascii_only)?;
        if !probe.changed {
            return Ok((None, probe));
        }
    }
    let (packer, outcome) = patch_into(
        &mut open,
        Packer::default,
        patches,
        elements,
        strings,
        ascii_only,
    )?;
    Ok((Some(packer.finish()?), outcome))
}

#[cfg(test)]
#[path = "patch_sheet_tests.rs"]
mod tests;
