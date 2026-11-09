//! What comes from and goes to the outside: the clipboard's tab-separated text and Excel's HTML,
//! and CSV files. Excel's HTML says each cell's exact number and its look, in a small style sheet.

use std::collections::HashMap;

use cgs_calc::{CalcEngine, CalcValue, CellRef};
use cgs_model::Rect;
use cgs_numfmt::Locale;

use crate::command::Area;
use crate::edit::{EditCell, EditInput, NewStyle};
use crate::input::{parse_input, Typed};
use crate::Session;

/// Cells to write, the sheet's merges if they change, and where it all lands.
type Pasted = (Vec<EditCell>, Option<Vec<Rect>>, Area);

/// A cell read from the clipboard's HTML.
#[derive(Debug, Clone, Default, PartialEq)]
struct HtmlCell {
    text: String,
    /// `x:num`: the number behind what the cell shows.
    number: Option<f64>,
    /// `x:str`: text, whatever it looks like.
    is_text: bool,
    /// Which rule in the style sheet.
    class: Option<String>,
    /// `style="…"` on the cell itself.
    own_style: Option<String>,
    cols: u32,
    rows: u32,
}

fn lower_tag(tag: &str) -> String {
    tag.to_ascii_lowercase()
}

/// An attribute of a tag, with or without quotes.
fn html_attr(tag: &str, name: &str) -> Option<String> {
    let lower = lower_tag(tag);
    let mut search = 0;
    while let Some(found) = lower[search..].find(name) {
        let at = search + found;
        let before = lower[..at].chars().last();
        let after = at + name.len();
        search = after;
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let rest = tag[after..].trim_start();
        let Some(value) = rest.strip_prefix('=') else {
            if lower[after..].trim_start().is_empty() || !rest.starts_with('=') {
                // An attribute with no value, such as `x:str`.
                return Some(String::new());
            }
            continue;
        };
        let value = value.trim_start();
        return Some(match value.chars().next() {
            Some(quote @ ('"' | '\'')) => {
                let inner = &value[1..];
                let end = inner.find(quote).unwrap_or(inner.len());
                inner[..end].to_string()
            }
            _ => {
                let end = value
                    .find(|c: char| c.is_whitespace() || c == '>')
                    .unwrap_or(value.len());
                value[..end].to_string()
            }
        });
    }
    None
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|&end| end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(ch) => {
                out.push(ch);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text inside a cell, with its tags gone. `<br>` is a line break, and other line breaks and runs
/// of spaces are only how the HTML was laid out.
fn cell_text(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut rest = inner;
    while let Some(at) = rest.find('<') {
        push_collapsed(&rest[..at], &mut out);
        let Some(end) = rest[at..].find('>') else {
            rest = "";
            break;
        };
        let tag = lower_tag(&rest[at..at + end]);
        if tag.starts_with("<br") {
            out.push('\n');
        }
        rest = &rest[at + end + 1..];
    }
    push_collapsed(rest, &mut out);
    decode_entities(out.trim_matches(|c| c == ' ' || c == '\r' || c == '\n'))
}

fn push_collapsed(text: &str, out: &mut String) {
    let mut space = false;
    for ch in text.chars() {
        if ch == '\r' || ch == '\n' || ch == '\t' || ch == ' ' {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(ch);
        }
    }
    if space && !out.is_empty() {
        out.push(' ');
    }
}

/// The rows of the first table in the HTML, between Excel's fragment comments if it has them.
fn html_rows(html: &str) -> Vec<Vec<HtmlCell>> {
    let lower = html.to_ascii_lowercase();
    let mut rows = Vec::new();
    let Some(mut at) = lower.find("<table") else {
        return rows;
    };
    let end = lower[at..]
        .find("</table")
        .map_or(lower.len(), |end| at + end);
    let mut row: Option<Vec<HtmlCell>> = None;
    while at < end {
        let Some(open) = lower[at..end].find('<') else {
            break;
        };
        at += open;
        let Some(close) = lower[at..].find('>') else {
            break;
        };
        let tag_end = at + close;
        let tag_lower = &lower[at..tag_end];
        if tag_lower.starts_with("<tr") {
            if let Some(done) = row.take() {
                rows.push(done);
            }
            row = Some(Vec::new());
            at = tag_end + 1;
        } else if tag_lower.starts_with("</tr") {
            if let Some(done) = row.take() {
                rows.push(done);
            }
            at = tag_end + 1;
        } else if tag_lower.starts_with("<td") || tag_lower.starts_with("<th") {
            let tag = &html[at..tag_end];
            let inner_start = tag_end + 1;
            let next = ["</td", "</th"]
                .iter()
                .filter_map(|close| lower[inner_start..end].find(close))
                .min()
                .map_or(end, |found| inner_start + found);
            let cell = HtmlCell {
                text: cell_text(&html[inner_start..next]),
                number: html_attr(tag, "x:num").and_then(|value| {
                    let value = value.trim().to_string();
                    if value.is_empty() {
                        cell_text(&html[inner_start..next]).parse().ok()
                    } else {
                        value.parse().ok()
                    }
                }),
                is_text: html_attr(tag, "x:str").is_some(),
                class: html_attr(tag, "class"),
                own_style: html_attr(tag, "style"),
                cols: html_attr(tag, "colspan")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1),
                rows: html_attr(tag, "rowspan")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1),
            };
            row.get_or_insert_with(Vec::new).push(cell);
            at = next;
            if let Some(after) = lower[next..end].find('>') {
                at = next + after + 1;
            }
        } else {
            at = tag_end + 1;
        }
    }
    if let Some(done) = row.take() {
        rows.push(done);
    }
    rows
}

/// The rules in Excel's style sheet: selector to declarations.
fn style_rules(html: &str) -> HashMap<String, String> {
    let mut rules = HashMap::new();
    let lower = html.to_ascii_lowercase();
    let Some(open) = lower.find("<style") else {
        return rules;
    };
    let close = lower[open..]
        .find("</style")
        .map_or(lower.len(), |end| open + end);
    let mut body = html[open..close].to_string();
    // Comment marks hide the rules from old browsers, and don't mean anything else here.
    body = body.replace("<!--", " ").replace("-->", " ");
    let mut rest = body.as_str();
    while let Some(brace) = rest.find('{') {
        let selector = rest[..brace]
            .rsplit(['}', '>'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        let Some(end) = rest[brace..].find('}') else {
            break;
        };
        let declarations = rest[brace + 1..brace + end].to_string();
        rules.insert(selector.to_ascii_lowercase(), declarations);
        rest = &rest[brace + end + 1..];
    }
    rules
}

/// `name: value` pairs. Excel writes number formats with quotes that can hold `;`.
fn declarations(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut key = String::new();
    let mut value = String::new();
    let mut in_key = true;
    let mut quote = None;
    let mut flush = |key: &mut String, value: &mut String| {
        if !key.trim().is_empty() {
            out.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
        key.clear();
        value.clear();
    };
    for ch in text.chars() {
        if let Some(open) = quote {
            value.push(ch);
            if ch == open {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' if !in_key => {
                quote = Some(ch);
                value.push(ch);
            }
            ':' if in_key => in_key = false,
            ';' => {
                flush(&mut key, &mut value);
                in_key = true;
            }
            _ if in_key => key.push(ch),
            _ => value.push(ch),
        }
    }
    flush(&mut key, &mut value);
    out
}

/// `#RRGGBB`, `#RGB` or a few names, as `RRGGBB`.
fn css_color(text: &str) -> Option<String> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix('#') {
        return match hex.len() {
            6 if hex.bytes().all(|b| b.is_ascii_hexdigit()) => Some(hex.to_ascii_uppercase()),
            3 if hex.bytes().all(|b| b.is_ascii_hexdigit()) => Some(
                hex.chars()
                    .flat_map(|c| [c, c])
                    .collect::<String>()
                    .to_ascii_uppercase(),
            ),
            _ => None,
        };
    }
    match lower.as_str() {
        "black" | "windowtext" => Some("000000".to_string()),
        "white" | "window" => Some("FFFFFF".to_string()),
        "red" => Some("FF0000".to_string()),
        "lime" => Some("00FF00".to_string()),
        "blue" => Some("0000FF".to_string()),
        "yellow" => Some("FFFF00".to_string()),
        "aqua" => Some("00FFFF".to_string()),
        "fuchsia" => Some("FF00FF".to_string()),
        "silver" => Some("C0C0C0".to_string()),
        "gray" => Some("808080".to_string()),
        "maroon" => Some("800000".to_string()),
        "green" => Some("008000".to_string()),
        "navy" => Some("000080".to_string()),
        "olive" => Some("808000".to_string()),
        "teal" => Some("008080".to_string()),
        "purple" => Some("800080".to_string()),
        _ => None,
    }
}

/// `mso-number-format`, with its CSS escapes and quoting taken off.
fn number_format_code(text: &str) -> Option<String> {
    let inner = text.trim();
    let inner = inner
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            inner
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(inner);
    // `\0022` is a double quote, and `\.` is a dot that CSS would otherwise read.
    let mut out = String::with_capacity(inner.len());
    let chars: Vec<char> = inner.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        if chars[at] == '\\' {
            let hex: String = chars[at + 1..]
                .iter()
                .take(6)
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            if hex.len() >= 4 {
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
                at += 1 + hex.len();
                continue;
            }
            // CSS quotes a character this way, and it means only that character.
            at += 1;
            if let Some(next) = chars.get(at) {
                out.push(*next);
                at += 1;
            }
            continue;
        }
        out.push(chars[at]);
        at += 1;
    }
    let lower = out.to_ascii_lowercase();
    if out.is_empty() || lower == "general" {
        return None;
    }
    Some(out)
}

fn border_side(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("none") {
        return None;
    }
    let width = lower
        .split_whitespace()
        .next()
        .and_then(|width| width.trim_end_matches("pt").parse::<f64>().ok())
        .unwrap_or(0.5);
    let style = if lower.contains("double") {
        "double"
    } else if lower.contains("dashed") || lower.contains("dot") {
        "dashed"
    } else if width >= 1.5 {
        "thick"
    } else if width >= 1.0 {
        "medium"
    } else if width < 0.5 {
        "hair"
    } else {
        "thin"
    };
    let color = lower
        .split_whitespace()
        .last()
        .and_then(css_color)
        .unwrap_or_else(|| "000000".to_string());
    Some(format!("{style}|{color}"))
}

/// What a cell's declarations ask for, as a `NewStyle` on top of the default look.
fn style_from_css(values: &HashMap<String, String>) -> NewStyle {
    let mut new = NewStyle::default();
    let mut font = String::new();
    let weight = values.get("font-weight").map(String::as_str);
    let bold = matches!(weight, Some("700" | "bold" | "800" | "900"));
    let italic = values
        .get("font-style")
        .is_some_and(|style| style.contains("italic"));
    let underline = values
        .get("text-decoration")
        .is_some_and(|text| text.contains("underline"));
    let strike = values
        .get("text-decoration")
        .is_some_and(|text| text.contains("line-through"));
    if bold {
        font.push_str("<b/>");
    }
    if italic {
        font.push_str("<i/>");
    }
    if strike {
        font.push_str("<strike/>");
    }
    if underline {
        font.push_str("<u/>");
    }
    let size = values
        .get("font-size")
        .and_then(|size| size.trim_end_matches("pt").trim().parse::<f64>().ok());
    let color = values.get("color").and_then(|color| css_color(color));
    let family = values.get("font-family").map(|family| {
        family
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_string()
    });
    if !font.is_empty() || size.is_some() || color.is_some() || family.is_some() {
        font.push_str(&format!("<sz val=\"{}\"/>", size.unwrap_or(11.0)));
        if let Some(color) = color {
            font.push_str(&format!("<color rgb=\"FF{color}\"/>"));
        } else {
            font.push_str("<color theme=\"1\"/>");
        }
        let family = family
            .filter(|family| !family.is_empty())
            .unwrap_or_else(|| "Calibri".to_string());
        font.push_str(&format!(
            "<name val=\"{}\"/>",
            cgs_xlsx::scan::escape_attr(&family)
        ));
        new.font = Some(font);
    }
    new.fill = values
        .get("background")
        .or_else(|| values.get("background-color"))
        .and_then(|fill| css_color(fill.split_whitespace().next().unwrap_or("")));
    new.number_format = values
        .get("mso-number-format")
        .and_then(|code| number_format_code(code));
    let horizontal = values.get("text-align").map(|align| match align.as_str() {
        "center" => "center",
        "right" => "right",
        "left" => "left",
        "justify" => "justify",
        "fill" => "fill",
        _ => "general",
    });
    let vertical = values
        .get("vertical-align")
        .map(|align| match align.as_str() {
            "top" => "top",
            "middle" | "center" => "center",
            _ => "bottom",
        });
    let wrap = values
        .get("white-space")
        .is_some_and(|white| white.contains("normal"));
    let mut alignment = String::new();
    if let Some(horizontal) = horizontal.filter(|&align| align != "general") {
        alignment.push_str(&format!(" horizontal=\"{horizontal}\""));
    }
    if let Some(vertical) = vertical.filter(|&align| align != "bottom") {
        alignment.push_str(&format!(" vertical=\"{vertical}\""));
    }
    if wrap {
        alignment.push_str(" wrapText=\"1\"");
    }
    if !alignment.is_empty() {
        new.alignment = Some(alignment);
    }
    let shared = values.get("border").and_then(|side| border_side(side));
    let mut border = String::new();
    let mut any = false;
    for (name, key) in [
        ("left", "border-left"),
        ("right", "border-right"),
        ("top", "border-top"),
        ("bottom", "border-bottom"),
    ] {
        let side = values
            .get(key)
            .map(|side| border_side(side))
            .unwrap_or_else(|| shared.clone());
        match side.as_deref().and_then(|side| side.split_once('|')) {
            Some((style, color)) => {
                any = true;
                border.push_str(&format!(
                    "<{name} style=\"{style}\"><color rgb=\"FF{color}\"/></{name}>"
                ));
            }
            None => border.push_str(&format!("<{name}/>")),
        }
    }
    if any {
        border.push_str("<diagonal/>");
        new.border = Some(border);
    }
    new
}

/// Splits tab-separated text. A cell can hold line breaks if it is in quotes, as Excel writes them.
pub fn parse_delimited(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    let mut at_start = true;
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                cell.push(ch);
            }
            continue;
        }
        match ch {
            '"' if at_start => {
                in_quotes = true;
                at_start = false;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                at_start = true;
            }
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                at_start = true;
            }
            _ if ch == delimiter => {
                row.push(std::mem::take(&mut cell));
                at_start = true;
            }
            _ => {
                cell.push(ch);
                at_start = false;
            }
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

/// Which separator a CSV file uses: whichever gives the most regular columns in its first lines.
pub fn csv_delimiter(text: &str) -> char {
    let sample: String = text.lines().take(20).collect::<Vec<_>>().join("\n");
    let mut best = (',', 0usize);
    for candidate in [',', ';', '\t', '|'] {
        let rows = parse_delimited(&sample, candidate);
        let Some(first) = rows.first() else {
            continue;
        };
        let width = first.len();
        if width > 1 && rows.iter().all(|row| row.len() == width) && width > best.1 {
            best = (candidate, width);
        }
    }
    best.0
}

/// One field of a CSV line.
fn csv_field(text: &str, delimiter: char, out: &mut String) {
    let needs_quotes = text.contains(delimiter)
        || text.contains('"')
        || text.contains('\n')
        || text.contains('\r');
    if needs_quotes {
        out.push('"');
        out.push_str(&text.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(text);
    }
}

/// Joins what cells show into lines, the way both CSV and the clipboard's text want it.
pub fn join_delimited(rows: &[Vec<String>], delimiter: char, line_end: &str) -> String {
    let mut out = String::new();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            if index > 0 {
                out.push(delimiter);
            }
            csv_field(cell, delimiter, &mut out);
        }
        out.push_str(line_end);
    }
    out
}

impl Session {
    /// The edits for what came from another program, and new merges if it had any.
    pub(crate) fn external_edits(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        text: &str,
        html: Option<&str>,
        locale: &Locale,
    ) -> Result<Pasted, String> {
        let rows = html.map(html_rows).filter(|rows| !rows.is_empty());
        match rows {
            Some(rows) => self.html_edits(sheet, row, col, &rows, html.unwrap_or(""), locale),
            None => {
                let rows = parse_delimited(text, '\t');
                let mut edits = Vec::new();
                let mut bottom = row;
                let mut right = col;
                for (r, cells) in rows.iter().enumerate() {
                    for (c, cell) in cells.iter().enumerate() {
                        let at = (row + r as u32, col + c as u32);
                        if at.0 > cgs_model::MAX_ROWS || at.1 > cgs_model::MAX_COLS {
                            return Err("That doesn't fit on the sheet.".to_string());
                        }
                        bottom = bottom.max(at.0);
                        right = right.max(at.1);
                        edits.push(self.pasted_cell(sheet, at, cell, locale));
                    }
                }
                Ok((
                    edits,
                    None,
                    Area {
                        top: row,
                        left: col,
                        bottom,
                        right,
                    },
                ))
            }
        }
    }

    fn pasted_cell(&mut self, sheet: u32, at: (u32, u32), text: &str, locale: &Locale) -> EditCell {
        let typed = parse_input(text, locale, self.book.date1904, self.today);
        // A pasted formula is only text unless it reads as one.
        let typed = match typed {
            Typed::Formula(formula) => match self.calc.check_formula(&formula) {
                Ok(fixed) => Typed::Formula(fixed),
                Err(_) => Typed::Value(CalcValue::Text(text.to_string())),
            },
            other => other,
        };
        self.typed_edit_at(sheet, at, &typed)
    }

    fn html_edits(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        rows: &[Vec<HtmlCell>],
        html: &str,
        locale: &Locale,
    ) -> Result<Pasted, String> {
        let rules = style_rules(html);
        let base_rule = rules
            .get("td")
            .map(|rule| declarations(rule))
            .unwrap_or_default();
        let mut styles: HashMap<(Option<String>, Option<String>), u32> = HashMap::new();
        let mut edits = Vec::new();
        let mut merges = Vec::new();
        let mut taken: HashMap<(u32, u32), ()> = HashMap::new();
        let (mut bottom, mut right) = (row, col);
        for (offset, cells) in rows.iter().enumerate() {
            let at_row = row + offset as u32;
            let mut at_col = col;
            for cell in cells {
                while taken.contains_key(&(at_row, at_col)) {
                    at_col += 1;
                }
                if at_row > cgs_model::MAX_ROWS || at_col + cell.cols - 1 > cgs_model::MAX_COLS {
                    return Err("That doesn't fit on the sheet.".to_string());
                }
                for r in 0..cell.rows {
                    for c in 0..cell.cols {
                        taken.insert((at_row + r, at_col + c), ());
                    }
                }
                if cell.rows > 1 || cell.cols > 1 {
                    merges.push(Rect {
                        top: at_row,
                        left: at_col,
                        bottom: at_row + cell.rows - 1,
                        right: at_col + cell.cols - 1,
                    });
                }
                bottom = bottom.max(at_row + cell.rows - 1);
                right = right.max(at_col + cell.cols - 1);
                let key = (cell.class.clone(), cell.own_style.clone());
                let style = match styles.get(&key) {
                    Some(&style) => style,
                    None => {
                        let mut values = base_rule.clone();
                        if let Some(class) = &cell.class {
                            let selector = format!(".{}", class.to_ascii_lowercase());
                            if let Some(rule) = rules.get(&selector) {
                                values.extend(declarations(rule));
                            }
                        }
                        if let Some(own) = &cell.own_style {
                            values.extend(declarations(own));
                        }
                        let new = style_from_css(&values);
                        let style = if new == NewStyle::default() {
                            0
                        } else {
                            self.add_style(0, &new)
                        };
                        styles.insert(key, style);
                        style
                    }
                };
                let input = if let Some(number) = cell.number {
                    EditInput::Value(CalcValue::Number(number))
                } else if cell.text.is_empty() {
                    EditInput::Clear
                } else if cell.is_text {
                    EditInput::Value(CalcValue::Text(cell.text.clone()))
                } else {
                    let plain = self.pasted_cell(sheet, (at_row, at_col), &cell.text, locale);
                    plain.input.unwrap_or(EditInput::Clear)
                };
                edits.push(EditCell {
                    sheet,
                    row: at_row,
                    col: at_col,
                    input: Some(input),
                    style: Some(style),
                });
                at_col += cell.cols;
            }
        }
        let area = Area {
            top: row,
            left: col,
            bottom,
            right,
        };
        let all_merges = if merges.is_empty() {
            None
        } else {
            let info = &self.book.sheets[sheet as usize];
            let mut list: Vec<Rect> = info
                .merges
                .iter()
                .filter(|merge| {
                    !(merge.top <= area.bottom
                        && merge.bottom >= area.top
                        && merge.left <= area.right
                        && merge.right >= area.left)
                })
                .copied()
                .collect();
            list.extend(merges);
            Some(list)
        };
        Ok((edits, all_merges, area))
    }

    /// What cells show, as rows. Trailing empties don't count.
    pub(crate) fn shown_rows(
        &mut self,
        sheet: u32,
        area: Area,
        locale: &Locale,
    ) -> Vec<Vec<String>> {
        let date1904 = self.book.date1904;
        let options = cgs_numfmt::Options { locale, date1904 };
        let mut rows = Vec::new();
        for row in area.top..=area.bottom {
            let mut cells = Vec::new();
            for col in area.left..=area.right {
                let input = crate::tile::TileInput {
                    book: &self.book,
                    calc: &self.calc,
                    options: &options,
                    today: self.today,
                    invalid: None,
                    errors: 0,
                };
                cells.push(crate::tile::display_text(
                    &input,
                    &mut self.cache,
                    sheet,
                    row,
                    col,
                ));
            }
            rows.push(cells);
        }
        rows
    }
}

/// HTML for cells, with their numbers exact, for other programs to paste.
pub(crate) fn rows_to_html(rows: &[Vec<String>], numbers: &[Vec<Option<f64>>]) -> String {
    let mut out = String::from("<table>");
    for (row, cells) in rows.iter().enumerate() {
        out.push_str("<tr>");
        for (col, cell) in cells.iter().enumerate() {
            let mut escaped = Vec::new();
            cgs_xlsx::scan::escape_text(cell, &mut escaped);
            let text = String::from_utf8_lossy(&escaped);
            match numbers
                .get(row)
                .and_then(|line| line.get(col))
                .copied()
                .flatten()
            {
                Some(number) => out.push_str(&format!("<td x:num=\"{number}\">{text}</td>")),
                None => out.push_str(&format!("<td>{text}</td>")),
            }
        }
        out.push_str("</tr>");
    }
    out.push_str("</table>");
    out
}

impl crate::Engine {
    /// A new workbook's cells from CSV text. Each field is read as if typed, as Excel does.
    pub fn load_csv(&mut self, text: &str) -> Result<(), String> {
        let locale = self.locale.clone();
        let Some(session) = self.session.as_mut() else {
            return Err("No workbook is open.".to_string());
        };
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let delimiter = csv_delimiter(text);
        let mut edits = Vec::new();
        for (r, cells) in parse_delimited(text, delimiter).iter().enumerate() {
            for (c, cell) in cells.iter().enumerate() {
                if cell.is_empty() {
                    continue;
                }
                let at = (r as u32 + 1, c as u32 + 1);
                if at.0 > cgs_model::MAX_ROWS || at.1 > cgs_model::MAX_COLS {
                    return Err("This file is too large for a sheet.".to_string());
                }
                edits.push(session.pasted_cell(0, at, cell, &locale));
            }
        }
        session.load_cells(edits)
    }

    /// A sheet's CSV text, as its cells show. With a byte order mark, as Excel needs for Arabic.
    pub fn csv_text(&mut self, sheet: u32) -> String {
        let locale = self.locale.clone();
        let Some(session) = self.session.as_mut() else {
            return String::new();
        };
        let Some(info) = session.book.sheets.get(sheet as usize) else {
            return String::new();
        };
        let area = Area {
            top: 1,
            left: 1,
            bottom: info.used.0.max(1),
            right: info.used.1.max(1),
        };
        let rows = session.shown_rows(sheet, area, &locale);
        format!("\u{feff}{}", join_delimited(&rows, ',', "\r\n"))
    }

    /// What the clipboard gets for these cells: tab-separated text, and HTML with exact numbers.
    pub fn copy_text(&mut self, sheet: u32, area: Area) -> (String, String) {
        let locale = self.locale.clone();
        let Some(session) = self.session.as_mut() else {
            return (String::new(), String::new());
        };
        if sheet as usize >= session.book.sheets.len() {
            return (String::new(), String::new());
        }
        // A whole row or column only reaches as far as the sheet is used.
        let used = session.book.sheets[sheet as usize].used;
        let area = Area {
            top: area.top,
            left: area.left,
            bottom: area.bottom.min(used.0.max(area.top)),
            right: area.right.min(used.1.max(area.left)),
        };
        let rows = session.shown_rows(sheet, area, &locale);
        let mut numbers = Vec::with_capacity(rows.len());
        for row in area.top..=area.bottom {
            let mut line = Vec::new();
            for col in area.left..=area.right {
                let value = session.calc.cell_input(CellRef { sheet, row, col });
                line.push(match value {
                    cgs_calc::CellInput::Value(CalcValue::Number(number)) => Some(number),
                    cgs_calc::CellInput::Formula {
                        cached: Some(CalcValue::Number(number)),
                        ..
                    } => Some(number),
                    _ => None,
                });
            }
            numbers.push(line);
        }
        let text = join_delimited(&rows, '\t', "\r\n");
        (text, rows_to_html(&rows, &numbers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_text_with_quotes_and_line_breaks() {
        assert_eq!(
            parse_delimited("a\tb\r\n\"c\nd\"\t\"e\"\"f\"\n", '\t'),
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["c\nd".to_string(), "e\"f".to_string()]
            ]
        );
        assert_eq!(
            parse_delimited("a,,b", ','),
            vec![vec!["a".to_string(), String::new(), "b".to_string()]]
        );
        assert_eq!(parse_delimited("", ','), Vec::<Vec<String>>::new());
    }

    #[test]
    fn finds_a_csv_files_separator() {
        assert_eq!(csv_delimiter("a,b,c\n1,2,3\n"), ',');
        assert_eq!(csv_delimiter("a;b;c\n1,5;2;3\n"), ';');
        assert_eq!(csv_delimiter("a\tb\n1\t2\n"), '\t');
        assert_eq!(csv_delimiter("just one column\nhere\n"), ',');
    }

    #[test]
    fn writes_csv_with_quotes_where_needed() {
        let rows = vec![vec!["a,b".to_string(), "c\"d".to_string(), "e".to_string()]];
        assert_eq!(
            join_delimited(&rows, ',', "\r\n"),
            "\"a,b\",\"c\"\"d\",e\r\n"
        );
    }

    const EXCEL: &str = r#"<html><head><style><!--
td {padding-top:1px; color:black; font-size:11.0pt; font-family:Calibri, sans-serif; mso-number-format:General;}
.xl65 {mso-number-format:"0\.00"; font-weight:700; color:#FF0000; background:#FFFF00; mso-pattern:black none; text-align:center; border-top:.5pt solid windowtext;}
.xl66 {mso-number-format:"\0022$\0022\#\,\#\#0"; font-style:italic;}
--></style></head><body><table>
<!--StartFragment--><tr height=20><td class=xl65 x:num="1.5">1.50</td><td>plain &amp; text</td></tr>
<tr><td class=xl66 x:num="1234">$1,234</td><td colspan=2 x:str="=A1">=A1</td></tr><!--EndFragment--></table></body></html>"#;

    #[test]
    fn reads_excels_html() {
        let rows = html_rows(EXCEL);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].number, Some(1.5));
        assert_eq!(rows[0][0].class.as_deref(), Some("xl65"));
        assert_eq!(rows[0][1].text, "plain & text");
        assert_eq!(rows[1][1].cols, 2);
        assert!(rows[1][1].is_text);
        assert_eq!(rows[1][1].text, "=A1");
    }

    #[test]
    fn reads_excels_style_sheet() {
        let rules = style_rules(EXCEL);
        let values = declarations(&rules[".xl65"]);
        let new = style_from_css(&values);
        assert_eq!(new.fill.as_deref(), Some("FFFF00"));
        assert_eq!(new.number_format.as_deref(), Some("0.00"));
        assert_eq!(new.alignment.as_deref(), Some(" horizontal=\"center\""));
        assert!(new.font.as_deref().unwrap().contains("<b/>"));
        assert!(new.font.as_deref().unwrap().contains("rgb=\"FFFF0000\""));
        assert!(new
            .border
            .as_deref()
            .unwrap()
            .contains("<top style=\"thin\">"));
        let values = declarations(&rules[".xl66"]);
        assert_eq!(
            style_from_css(&values).number_format.as_deref(),
            Some("\"$\"#,##0")
        );
    }
}
