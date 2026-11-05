//! What the UI asks about a workbook that isn't a cell's text: where Ctrl+arrow lands, what the name
//! box's text means, and which functions there are.

use cgs_calc::{CalcEngine, CalcValue, CellSink, Rect, SheetRect};
use cgs_refs::{col_from_letters, parse_range, sheet_key};

use crate::command::{Area, Dirty, UndoState};
use crate::{json, Engine};

/// Which cells of a stretch hold anything.
#[derive(Default)]
struct Filled(Vec<(u32, u32)>);

impl CellSink for Filled {
    fn cell(&mut self, row: u32, col: u32, value: &CalcValue) {
        if *value != CalcValue::Empty {
            self.0.push((row, col));
        }
    }
}

/// The cell of a sheet name and a reference, as `Sheet1!A1` or `'My Sheet'!A1:B2`.
fn split_sheet(text: &str) -> (Option<String>, &str) {
    let Some(at) = text.rfind('!') else {
        return (None, text);
    };
    let name = &text[..at];
    let name = name
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .map_or_else(|| name.to_string(), |inner| inner.replace("''", "'"));
    (Some(name), &text[at + 1..])
}

/// `A:C` or `2:5`, which `parse_range` doesn't read.
fn whole_lines(text: &str) -> Option<Area> {
    let (first, last) = text.split_once(':')?;
    let (first, last) = (first.trim_matches('$'), last.trim_matches('$'));
    if let (Ok(top), Ok(bottom)) = (first.parse::<u32>(), last.parse::<u32>()) {
        let valid = (1..=cgs_model::MAX_ROWS).contains(&top.min(bottom))
            && (1..=cgs_model::MAX_ROWS).contains(&top.max(bottom));
        return valid.then_some(Area {
            top: top.min(bottom),
            left: 1,
            bottom: top.max(bottom),
            right: cgs_model::MAX_COLS,
        });
    }
    let (left, right) = (
        col_from_letters(first)? as u32,
        col_from_letters(last)? as u32,
    );
    Some(Area {
        top: 1,
        left: left.min(right),
        bottom: cgs_model::MAX_ROWS,
        right: left.max(right),
    })
}

fn read_area(text: &str) -> Option<Area> {
    if let Some(((top, left), (bottom, right))) = parse_range(text) {
        return Some(Area {
            top: top.min(bottom),
            left: left.min(right),
            bottom: top.max(bottom),
            right: left.max(right),
        });
    }
    whole_lines(text)
}

impl Engine {
    /// Where Ctrl+arrow goes from a cell: to the edge of the block it is in, or the next block, or
    /// the sheet's edge. `rows` and `cols` say which way, one of them 1 or -1.
    pub fn jump_target(&self, sheet: u32, row: u32, col: u32, rows: i32, cols: i32) -> (u32, u32) {
        let Some(session) = self.session.as_ref() else {
            return (row, col);
        };
        let Some(info) = session.book.sheets.get(sheet as usize) else {
            return (row, col);
        };
        let down = rows != 0;
        let forward = rows > 0 || cols > 0;
        let at = if down { row } else { col };
        let edge = if !forward {
            1
        } else if down {
            cgs_model::MAX_ROWS
        } else {
            cgs_model::MAX_COLS
        };
        let finish = |place: u32| if down { (place, col) } else { (row, place) };
        if at == edge {
            return finish(at);
        }
        // Everything in this line within what the sheet uses.
        let used = if down { info.used.0 } else { info.used.1 };
        let mut filled = Filled::default();
        let rect = if down {
            Rect {
                top: 1,
                left: col,
                bottom: used.max(1),
                right: col,
            }
        } else {
            Rect {
                top: row,
                left: 1,
                bottom: row,
                right: used.max(1),
            }
        };
        session
            .calc
            .read_block(SheetRect { sheet, rect }, &mut filled);
        let mut places: Vec<u32> = filled
            .0
            .iter()
            .map(|&(r, c)| if down { r } else { c })
            .collect();
        places.sort_unstable();
        let holds = |place: u32| places.binary_search(&place).is_ok();
        let step = |place: u32| if forward { place + 1 } else { place - 1 };
        let next = step(at);
        if holds(at) && holds(next) {
            // To the end of this block.
            let mut place = next;
            while place != edge && holds(step(place)) {
                place = step(place);
            }
            return finish(place);
        }
        // To the next cell that holds anything.
        let found = if forward {
            places.iter().copied().find(|&place| place > at)
        } else {
            places.iter().rev().copied().find(|&place| place < at)
        };
        finish(found.unwrap_or(edge))
    }

    /// What the name box's text refers to, as a sheet and an area.
    pub fn resolve_name(&self, text: &str, current: u32) -> Option<(u32, Area)> {
        let session = self.session.as_ref()?;
        let book = &session.book;
        let text = text.trim();
        let (sheet_name, reference) = split_sheet(text);
        let sheet = match &sheet_name {
            Some(name) => {
                let key = sheet_key(name);
                book.sheets
                    .iter()
                    .position(|sheet| !sheet.deleted && sheet_key(&sheet.name) == key)?
                    as u32
            }
            None => current,
        };
        if let Some(area) = read_area(reference) {
            return Some((sheet, area));
        }
        // A defined name: this sheet's own, or the workbook's. Only one that is a plain reference.
        if sheet_name.is_some() {
            return None;
        }
        let key = text.to_lowercase();
        let defined = book
            .names
            .iter()
            .filter(|name| name.name.to_lowercase() == key)
            .max_by_key(|name| name.local_sheet == Some(current))?;
        let (sheet_name, reference) = split_sheet(&defined.formula);
        let sheet_name = sheet_name?;
        let name_key = sheet_key(&sheet_name);
        let sheet = book
            .sheets
            .iter()
            .position(|sheet| !sheet.deleted && sheet_key(&sheet.name) == name_key)?;
        Some((sheet as u32, read_area(reference)?))
    }

    /// Every function the engine knows, for autocomplete, sorted.
    pub fn function_names(&self) -> Vec<String> {
        cgs_calc::formualizer::function_names()
    }

    /// Defined names people can use, for autocomplete.
    pub fn defined_names(&self) -> Vec<String> {
        let Some(book) = self.book() else {
            return Vec::new();
        };
        let mut names: Vec<String> = book
            .names
            .iter()
            .filter(|name| !name.hidden && !name.name.starts_with("_xlnm."))
            .map(|name| name.name.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

/// `take_dirty`'s result as numbers that cross into JS without being parsed: sheets to repaint,
/// sheets whose layout changed, whether the list of sheets did, and tiles as threes.
pub fn dirty_numbers(dirty: &Dirty) -> Vec<u32> {
    let mut out = Vec::with_capacity(4 + dirty.sheets.len() + dirty.tiles.len() * 3);
    out.push(dirty.sheets.len() as u32);
    out.extend(dirty.sheets.iter().copied());
    out.push(dirty.layouts.len() as u32);
    out.extend(dirty.layouts.iter().copied());
    out.push(u32::from(dirty.book));
    out.push(dirty.tiles.len() as u32);
    for &(sheet, row, col) in &dirty.tiles {
        out.extend_from_slice(&[sheet, row, col]);
    }
    out
}

/// What a command leaves for the shell, as JSON.
pub fn applied_json(selection: Option<(u32, Area)>, state: &UndoState) -> String {
    let mut out = String::from("{\"selection\":");
    match selection {
        Some((sheet, area)) => out.push_str(&format!(
            "{{\"sheet\":{sheet},\"top\":{},\"left\":{},\"bottom\":{},\"right\":{}}}",
            area.top, area.left, area.bottom, area.right
        )),
        None => out.push_str("null"),
    }
    out.push_str(",\"undo\":");
    let label = |out: &mut String, text: Option<&str>| match text {
        Some(text) => json::push_string(out, text),
        None => out.push_str("null"),
    };
    label(&mut out, state.undo);
    out.push_str(",\"redo\":");
    label(&mut out, state.redo);
    out.push_str(",\"lost\":");
    out.push_str(if state.lost { "true" } else { "false" });
    out.push('}');
    out
}
