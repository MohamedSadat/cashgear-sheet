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
pub(crate) fn split_sheet(text: &str) -> (Option<String>, &str) {
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

pub(crate) fn read_area(text: &str) -> Option<Area> {
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

    /// Rows waiting to be measured, as JSON: see `RowFit` in `packages/engine-client`. Each goes
    /// back as `Command::FitRows`.
    pub fn take_row_fits(&mut self) -> String {
        let options_locale = self.locale.clone();
        let Some(session) = self.session.as_mut() else {
            return "[]".to_string();
        };
        let rows: Vec<(u32, u32)> = std::mem::take(&mut session.pending_fits)
            .into_iter()
            .collect();
        if rows.is_empty() {
            return "[]".to_string();
        }
        let options = cgs_numfmt::Options {
            locale: &options_locale,
            date1904: session.book.date1904,
        };
        session.fit_rows_json(&rows, &options)
    }

    /// Rows of a sheet that wrap and hold no stored height, from another program's file, as
    /// `take_row_fits` gives them. Their heights only change how the file looks here.
    pub fn opening_row_fits(&mut self, sheet: u32) -> String {
        let options_locale = self.locale.clone();
        let Some(session) = self.session.as_mut() else {
            return "[]".to_string();
        };
        let Some(info) = session.book.sheets.get_mut(sheet as usize) else {
            return "[]".to_string();
        };
        let rows: Vec<(u32, u32)> = std::mem::take(&mut info.unfitted_rows)
            .into_iter()
            .map(|row| (sheet, row))
            .collect();
        if rows.is_empty() {
            return "[]".to_string();
        }
        let options = cgs_numfmt::Options {
            locale: &options_locale,
            date1904: session.book.date1904,
        };
        session.fit_rows_json(&rows, &options)
    }

    /// What to measure to fit columns, as JSON: see `ColFit` in `packages/engine-client`.
    pub fn col_fits(&mut self, sheet: u32, first: u32, last: u32) -> String {
        let options_locale = self.locale.clone();
        let Some(session) = self.session.as_mut() else {
            return "[]".to_string();
        };
        let options = cgs_numfmt::Options {
            locale: &options_locale,
            date1904: session.book.date1904,
        };
        session.fit_cols_json(sheet, first, last, &options)
    }

    /// Whether merging these areas, given as JSON, would throw away anything but their first values.
    pub fn merge_discards_json(
        &self,
        sheet: u32,
        areas: &str,
        across: bool,
    ) -> Result<bool, String> {
        let areas: Vec<Area> = serde_json::from_str(areas).map_err(|error| error.to_string())?;
        Ok(self.merge_discards(sheet, &areas, across))
    }

    pub fn merge_discards(&self, sheet: u32, areas: &[Area], across: bool) -> bool {
        let Some(session) = self.session.as_ref() else {
            return false;
        };
        areas.iter().any(|area| {
            let filled = session.merge_discards(sheet, *area);
            if across {
                let mut rows: Vec<u32> = filled.iter().map(|&(row, _)| row).collect();
                let before = rows.len();
                rows.dedup();
                before > rows.len()
            } else {
                filled.len() > 1
            }
        })
    }

    /// How a cell is formatted, for the ribbon, as JSON.
    pub fn cell_format_json(&self, sheet: u32, row: u32, col: u32) -> String {
        let Some(book) = self.book() else {
            return "null".to_string();
        };
        let Some(info) = book.sheets.get(sheet as usize) else {
            return "null".to_string();
        };
        let table = &book.styles;
        let index = match info.merge_at(row, col) {
            Some(merge) => info.style_at(merge.top, merge.left),
            None => info.style_at(row, col),
        };
        format_json(book, table.xf(index), info.merge_at(row, col).is_some())
    }

    /// The workbook's theme colours and the codes of its own number formats, as JSON.
    pub fn format_lists_json(&self) -> String {
        let Some(book) = self.book() else {
            return "null".to_string();
        };
        let mut out = String::from("{\"theme\":[");
        for (index, rgb) in book.styles.colors.theme.0.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&format!("\"#{rgb:06x}\""));
        }
        out.push_str("],\"codes\":[");
        let mut codes: Vec<(&u32, &String)> = book.styles.num_fmts.iter().collect();
        codes.sort();
        for (index, (_, code)) in codes.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            json::push_string(&mut out, code);
        }
        out.push_str("]}");
        out
    }

    /// What a value looks like in a format code, for the Format Cells dialog's sample. Nothing if the
    /// code can't be used.
    pub fn format_sample(&mut self, sheet: u32, row: u32, col: u32, code: &str) -> Option<String> {
        let format = cgs_numfmt::NumFmt::parse(code);
        if !format.is_valid() {
            return None;
        }
        let session = self.session.as_ref()?;
        let options = cgs_numfmt::Options {
            locale: &self.locale,
            date1904: session.book.date1904,
        };
        let value = session.value_of(cgs_calc::CellRef { sheet, row, col });
        let mut out = String::new();
        match &value {
            CalcValue::Number(number) => {
                format.format(cgs_numfmt::Value::Number(*number), &options, &mut out);
            }
            CalcValue::Text(text) => {
                format.format(cgs_numfmt::Value::Text(text), &options, &mut out);
            }
            CalcValue::Bool(flag) => out.push_str(if *flag { "TRUE" } else { "FALSE" }),
            CalcValue::Error(text) => out.push_str(text),
            CalcValue::Empty => {}
        }
        Some(out)
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
pub fn applied_json(selection: Option<(u32, Area)>, replaced: u32, state: &UndoState) -> String {
    let mut out = format!("{{\"replaced\":{replaced},\"selection\":");
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

/// A cell format as the ribbon and Format Cells read it.
pub(crate) fn format_json(book: &cgs_model::Book, xf: &cgs_model::Xf, merged: bool) -> String {
    let table = &book.styles;
    let font = table.font(xf);
    let fill = table.fill(xf);
    let border = table.border(xf);
    let code = table
        .num_fmts
        .get(&xf.num_fmt)
        .cloned()
        .or_else(|| cgs_numfmt::builtin_code(xf.num_fmt).map(str::to_string))
        .unwrap_or_else(|| "General".to_string());
    let color = |color: cgs_model::Color| -> String {
        let hex = table
            .colors
            .resolve(color, None)
            .map(|rgb| format!("#{rgb:06x}"));
        match (color.kind, hex) {
            (cgs_model::ColorKind::Theme(theme), Some(hex)) => format!(
                "{{\"kind\":\"theme\",\"theme\":{theme},\"tint\":{},\"color\":\"{hex}\"}}",
                color.tint
            ),
            (_, Some(hex)) => format!("{{\"kind\":\"rgb\",\"color\":\"{hex}\"}}"),
            _ => "{\"kind\":\"auto\"}".to_string(),
        }
    };
    let line = |line: cgs_model::Line| -> String {
        let style = crate::format::line_style_name(line.style);
        format!("{{\"style\":\"{style}\",\"color\":{}}}", color(line.color))
    };
    let mut out = String::from("{\"fontName\":");
    json::push_string(&mut out, &font.name);
    out.push_str(&format!(
            ",\"fontSize\":{},\"bold\":{},\"italic\":{},\"strike\":{},\"underline\":\"{}\",\"fontColor\":{},\"vertAlign\":\"{}\"",
            font.size,
            font.bold,
            font.italic,
            font.strike,
            crate::format::underline_name(font.underline),
            color(font.color),
            font.vert_align.name().unwrap_or("baseline")
        ));
    out.push_str(&format!(
        ",\"fill\":{{\"pattern\":\"{}\",\"color\":{},\"patternColor\":{}}}",
        crate::format::pattern_name(fill.pattern),
        match fill.pattern {
            cgs_model::Pattern::Solid => color(fill.fg),
            _ => color(fill.bg),
        },
        color(fill.fg)
    ));
    let alignment = xf.alignment;
    out.push_str(&format!(
            ",\"horizontal\":\"{}\",\"vertical\":\"{}\",\"wrap\":{},\"shrink\":{},\"indent\":{},\"rotation\":{},\"readingOrder\":{}",
            crate::format::horizontal_name(alignment.horizontal),
            crate::format::vertical_name(alignment.vertical),
            alignment.wrap,
            alignment.shrink,
            alignment.indent,
            alignment.rotation,
            match alignment.reading_order {
                cgs_model::ReadingOrder::Context => 0,
                cgs_model::ReadingOrder::LeftToRight => 1,
                cgs_model::ReadingOrder::RightToLeft => 2,
            }
        ));
    out.push_str(",\"style\":");
    let style = table
        .named
        .iter()
        .find(|named| named.xf == xf.style)
        .map_or("Normal", |named| named.name.as_str());
    json::push_string(&mut out, style);
    out.push_str(",\"numberFormat\":");
    json::push_string(&mut out, &code);
    out.push_str(&format!(
            ",\"numberFormatId\":{},\"merged\":{},\"locked\":{},\"formulaHidden\":{},\"border\":{{\"top\":{},\"bottom\":{},\"left\":{},\"right\":{},\"diagonal\":{},\"diagonalUp\":{},\"diagonalDown\":{}}}}}",
            xf.num_fmt,
            merged,
            xf.locked,
            xf.formula_hidden,
            line(border.top),
            line(border.bottom),
            line(border.left),
            line(border.right),
            line(border.diagonal),
            border.diagonal_up,
            border.diagonal_down,
        ));
    out
}
