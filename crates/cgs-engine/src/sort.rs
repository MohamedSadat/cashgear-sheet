//! Sorting, as Excel 2016 sorts: numbers, then text, then FALSE and TRUE, then errors, with blanks
//! last both ways. Text compares as Windows' word sort does, ignoring case and accents until they
//! are all that differ, and hyphens and apostrophes until nothing else does. Hidden rows stay where
//! they are, and each cell moves as if copied, so relative references move with it. Read off Excel
//! over COM.

use std::cmp::Ordering;

use cgs_calc::{CalcEngine, CalcValue, CellInput, CellRef};
use serde::Deserialize;

use crate::command::{Applied, Area, Op, PasteMode};
use crate::edit::EditCell;
use crate::Session;

/// What a sort key orders by.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SortOn {
    #[default]
    Value,
    /// Cells filled with this `RRGGBB` first, or with no fill for none.
    CellColor {
        #[serde(default)]
        color: Option<String>,
    },
    FontColor {
        #[serde(default)]
        color: Option<String>,
    },
    /// Cells showing this conditional format icon first: its set's name and place from 0.
    Icon { set: String, id: u32 },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortKeySpec {
    /// The sheet's column, or its row for a sort left to right.
    pub index: u32,
    #[serde(default)]
    pub descending: bool,
    #[serde(default)]
    pub by: SortOn,
    /// A custom order, such as months: these first, in this order.
    #[serde(default)]
    pub list: Option<Vec<String>>,
}

/// Where a character sorts, before accents and case: symbols, then digits, then letters.
fn primary(ch: char, out: &mut Vec<u32>) {
    const SYMBOLS: &str = " !\"#$%&()*,./:;?@[\\]^_`{|}~+<=>";
    if let Some(at) = SYMBOLS.find(ch) {
        out.push(1 + at as u32);
        return;
    }
    if ch.is_ascii_digit() {
        out.push(100 + ch as u32 - '0' as u32);
        return;
    }
    let lower: char = ch.to_lowercase().next().unwrap_or(ch);
    let base = match lower {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' | 'ħ' => 'h',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' | 'ŧ' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        // Alef with a hamza is alef, as far as order goes. With a madda it comes just after.
        'أ' | 'إ' | 'ٱ' => 'ا',
        'آ' => {
            out.push(200 + 2 * 'ا' as u32 + 1);
            return;
        }
        other => other,
    };
    if base == 'ß' {
        out.push(200 + 2 * 's' as u32);
        out.push(200 + 2 * 's' as u32);
        return;
    }
    out.push(200 + 2 * base as u32);
}

/// Text's order, as keys compared one after another.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TextKey {
    primary: Vec<u32>,
    /// Accents, then case, then how many hyphens and apostrophes were left out.
    accents: Vec<u32>,
    case: Vec<u8>,
    /// Hyphens and apostrophes left out, by where they were: later ones sort first, and an
    /// apostrophe before a hyphen.
    ignored: Vec<(std::cmp::Reverse<usize>, u8)>,
}

pub(crate) fn text_key(text: &str, case_sensitive: bool) -> TextKey {
    let mut key = TextKey {
        primary: Vec::with_capacity(text.len()),
        accents: Vec::new(),
        case: Vec::new(),
        ignored: Vec::new(),
    };
    let _ = case_sensitive;
    for ch in text.chars() {
        if ch == '-' || ch == '\'' {
            key.ignored
                .push((std::cmp::Reverse(key.primary.len()), u8::from(ch == '-')));
            continue;
        }
        let before = key.primary.len();
        primary(ch, &mut key.primary);
        let lower = ch.to_lowercase().next().unwrap_or(ch);
        let plain = key.primary[before] == 200 + 2 * lower as u32 || ch.is_ascii() || ch == 'ß';
        for _ in before..key.primary.len() {
            key.accents.push(if plain { 0 } else { lower as u32 });
            // Lower case first, which only counts when case does.
            key.case.push(u8::from(ch.is_uppercase()));
        }
    }
    key
}

/// A cell's place in a sort by value.
#[derive(Debug, Clone)]
enum SortValue {
    Number(f64),
    Text(TextKey),
    Bool(bool),
    Error,
    Blank,
}

impl SortValue {
    fn class(&self) -> u8 {
        match self {
            Self::Number(_) => 0,
            Self::Text(_) => 1,
            Self::Bool(_) => 2,
            Self::Error => 3,
            Self::Blank => 4,
        }
    }
}

/// How two values compare, ascending, with blanks to be put last by the caller.
fn compare(a: &SortValue, b: &SortValue, case_sensitive: bool) -> Ordering {
    match (a, b) {
        (SortValue::Number(x), SortValue::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (SortValue::Text(x), SortValue::Text(y)) => {
            let order = x
                .primary
                .cmp(&y.primary)
                .then_with(|| x.accents.cmp(&y.accents));
            let order = if case_sensitive {
                order.then_with(|| x.case.cmp(&y.case))
            } else {
                order
            };
            order
                .then_with(|| x.ignored.len().cmp(&y.ignored.len()))
                .then_with(|| x.ignored.cmp(&y.ignored))
        }
        (SortValue::Bool(x), SortValue::Bool(y)) => x.cmp(y),
        _ => a.class().cmp(&b.class()),
    }
}

fn parse_rgb(text: &str) -> Option<u32> {
    u32::from_str_radix(text.trim_start_matches('#'), 16).ok()
}

impl Session {
    fn sort_value(&self, sheet: u32, row: u32, col: u32, case_sensitive: bool) -> SortValue {
        let value = match self.calc.cell_input(CellRef { sheet, row, col }) {
            CellInput::Value(value) => value,
            CellInput::Formula { cached, .. } => cached.unwrap_or(CalcValue::Empty),
            CellInput::Clear | CellInput::Spill { .. } => CalcValue::Empty,
        };
        match value {
            CalcValue::Number(number) => SortValue::Number(number),
            CalcValue::Text(text) if text.is_empty() => SortValue::Blank,
            CalcValue::Text(text) => SortValue::Text(text_key(&text, case_sensitive)),
            CalcValue::Bool(flag) => SortValue::Bool(flag),
            CalcValue::Error(_) => SortValue::Error,
            CalcValue::Empty => SortValue::Blank,
        }
    }

    /// A cell's fill or font colour as it shows in a light theme, for sorting and filtering by it.
    /// A reader of what the sheet's conditional formats make of its cells.
    pub(crate) fn cf_reader(&self, sheet: u32) -> crate::cf::CfReader<'_> {
        let info = &self.book.sheets[sheet as usize];
        crate::cf::CfReader::new(
            &self.calc,
            &self.book.styles,
            sheet,
            &info.name,
            &info.cf,
            info.used,
            self.today,
        )
    }

    /// The fill or text colour a cell shows, a conditional format's first, as Excel sorts and
    /// filters by it.
    pub(crate) fn shown_color_with(
        &self,
        cf: &crate::cf::CfReader<'_>,
        (sheet, row, col): (u32, u32, u32),
        font: bool,
    ) -> Option<u32> {
        let styles = &self.book.styles;
        if let Some(dxf) = cf.cell(row, col).and_then(|cell| cell.dxf) {
            if font {
                if let Some(color) = dxf.font.color {
                    return styles.colors.resolve(color, None);
                }
            } else if let Some(fill) = dxf.fill.filter(|f| f.pattern != cgs_model::Pattern::None) {
                return styles.colors.resolve(fill.color, None);
            }
        }
        self.shown_color(sheet, row, col, font)
    }

    pub(crate) fn shown_color(&self, sheet: u32, row: u32, col: u32, font: bool) -> Option<u32> {
        let styles = &self.book.styles;
        let xf = styles.xf(self.book.sheets[sheet as usize].style_at(row, col));
        if font {
            return styles
                .colors
                .resolve(styles.font(xf).color, None)
                .or(Some(0));
        }
        let fill = styles.fill(xf);
        match fill.pattern {
            cgs_model::Pattern::None => None,
            cgs_model::Pattern::Solid => styles.colors.resolve(fill.fg, None),
            cgs_model::Pattern::Hatch(_) => styles.colors.resolve(fill.bg, None),
        }
    }

    /// Sorts an area by its keys, as Data > Sort does.
    pub(crate) fn sort_command(
        &mut self,
        sheet: u32,
        area: Area,
        keys: &[SortKeySpec],
        header: bool,
        case_sensitive: bool,
        by_columns: bool,
    ) -> Result<Applied, String> {
        let info = self.sheet_for_edit(sheet)?;
        if keys.is_empty() {
            return Err("Pick something to sort by.".to_string());
        }
        if keys.len() > 64 {
            return Err("A sort can have at most 64 levels.".to_string());
        }
        // Whole columns or rows only reach as far as what's in them.
        let area = Area {
            bottom: area.bottom.min(info.used.0.max(area.top)),
            right: area.right.min(info.used.1.max(area.left)),
            ..area
        };
        if info.merges.iter().any(|merge| {
            merge.top <= area.bottom
                && merge.bottom >= area.top
                && merge.left <= area.right
                && merge.right >= area.left
        }) {
            return Err("To do this, all the merged cells need to be the same size.".to_string());
        }
        if self.cuts_an_array(sheet, area)
            || self.calc.spill_areas().iter().any(|spill| {
                spill.sheet == sheet
                    && spill.rect.top <= area.bottom
                    && spill.rect.bottom >= area.top
                    && spill.rect.left <= area.right
                    && spill.rect.right >= area.left
            })
        {
            return Err("You can't change part of an array.".to_string());
        }
        let (first, last, across) = if by_columns {
            (area.left, area.right, (area.top, area.bottom))
        } else {
            (area.top, area.bottom, (area.left, area.right))
        };
        let first = first + u32::from(header);
        if first > last {
            return Ok(Applied::default());
        }
        for key in keys {
            if key.index < across.0 || key.index > across.1 {
                return Err("A sort key must be inside what's sorted.".to_string());
            }
        }
        // Lines that show, which are all that move.
        let lines: Vec<u32> = (first..=last)
            .filter(|&line| {
                if by_columns {
                    info.cols.px(line) > 0
                } else {
                    info.rows.px(line) > 0
                }
            })
            .collect();
        let cell = |line: u32, index: u32| {
            if by_columns {
                (index, line)
            } else {
                (line, index)
            }
        };
        let lists: Vec<Option<Vec<TextKey>>> = keys
            .iter()
            .map(|key| {
                key.list.as_ref().map(|list| {
                    list.iter()
                        .map(|item| text_key(item, case_sensitive))
                        .collect()
                })
            })
            .collect();
        // Each line's keys, read once.
        let cf = self.cf_reader(sheet);
        let mut rows: Vec<(u32, Vec<(SortValue, u32)>)> = lines
            .iter()
            .map(|&line| {
                let values = keys
                    .iter()
                    .zip(&lists)
                    .map(|(key, list)| {
                        let (row, col) = cell(line, key.index);
                        let value = self.sort_value(sheet, row, col, case_sensitive);
                        // A colour key or a custom list ranks; a value key ranks 0.
                        let rank = match &key.by {
                            SortOn::Value => match (list, &value) {
                                (Some(list), SortValue::Text(text)) => list
                                    .iter()
                                    .position(|item| item.primary == text.primary)
                                    .map_or(list.len() as u32, |at| at as u32),
                                (Some(list), _) => list.len() as u32,
                                (None, _) => 0,
                            },
                            SortOn::CellColor { color } | SortOn::FontColor { color } => {
                                let font = matches!(key.by, SortOn::FontColor { .. });
                                let shown = self.shown_color_with(&cf, (sheet, row, col), font);
                                let wanted = color.as_deref().and_then(parse_rgb);
                                u32::from(shown != wanted)
                            }
                            SortOn::Icon { set, id } => {
                                let icon = cf.cell(row, col).and_then(|cell| cell.icon);
                                u32::from(icon.as_ref() != Some(&(set.clone(), *id)))
                            }
                        };
                        (value, rank)
                    })
                    .collect();
                (line, values)
            })
            .collect();
        rows.sort_by(|(_, a), (_, b)| {
            for ((key, (x, x_rank)), (y, y_rank)) in keys.iter().zip(a).zip(b) {
                let order = match key.by {
                    SortOn::Value => {
                        let ranked = x_rank.cmp(y_rank);
                        let ranked = if key.descending {
                            ranked.reverse()
                        } else {
                            ranked
                        };
                        // Blanks go last whichever way.
                        let blanks =
                            matches!(x, SortValue::Blank).cmp(&matches!(y, SortValue::Blank));
                        let plain = compare(x, y, case_sensitive);
                        let plain = if key.descending {
                            plain.reverse()
                        } else {
                            plain
                        };
                        ranked.then(blanks).then(plain)
                    }
                    _ => {
                        let ranked = x_rank.cmp(y_rank);
                        if key.descending {
                            ranked.reverse()
                        } else {
                            ranked
                        }
                    }
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
            Ordering::Equal
        });
        // Each line takes the one sorted into its place, as a copy, formats too.
        let (from_index, to_index) = across;
        let mut edits = Vec::new();
        for (target, (source, _)) in lines.iter().zip(&rows) {
            if target == source {
                continue;
            }
            let shift = i64::from(*target) - i64::from(*source);
            for index in from_index..=to_index {
                let (row, col) = cell(*source, index);
                let (to_row, to_col) = cell(*target, index);
                let (rows_by, cols_by) = if by_columns { (0, shift) } else { (shift, 0) };
                let input = self.copied_input(sheet, row, col, rows_by, cols_by, PasteMode::All);
                edits.push(EditCell {
                    sheet,
                    row: to_row,
                    col: to_col,
                    input,
                    style: Some(self.book.sheets[sheet as usize].style_at(row, col)),
                });
            }
        }
        if edits.is_empty() {
            return Ok(Applied {
                selection: Some((sheet, area)),
                replaced: 0,
            });
        }
        self.perform("sorting", Op::Cells(edits), Some((sheet, area)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(items: &[&str]) -> Vec<String> {
        let mut list: Vec<(TextKey, String)> = items
            .iter()
            .map(|item| (text_key(item, false), item.to_string()))
            .collect();
        list.sort_by(|a, b| {
            compare(
                &SortValue::Text(a.0.clone()),
                &SortValue::Text(b.0.clone()),
                false,
            )
        });
        list.into_iter().map(|(_, text)| text).collect()
    }

    /// Each list was sorted by Excel 2016 over COM.
    #[test]
    fn text_sorts_as_excel_sorts_it() {
        let items = [
            "b", "B", "a", "A", "ab", "a-b", "ab-", "a'b", "a b", "a_b", "a.b", "a1", "a10", "a2",
            "1", "10", "2", "-1", "!", " ", "#", "+", "<", "~", "z", "Z", "é", "e", "f", "É", "ß",
            "ss", "co-op", "coop", "cop",
        ];
        assert_eq!(
            sorted(&items),
            [
                " ", "!", "#", "~", "+", "<", "1", "-1", "10", "2", "a", "A", "a b", "a.b", "a_b",
                "a1", "a10", "a2", "ab", "ab-", "a'b", "a-b", "b", "B", "coop", "co-op", "cop",
                "e", "é", "É", "f", "ß", "ss", "z", "Z"
            ]
        );
    }

    #[test]
    fn types_sort_numbers_text_logicals_errors() {
        let mut values = [
            SortValue::Error,
            SortValue::Bool(true),
            SortValue::Text(text_key("x", false)),
            SortValue::Number(5.0),
            SortValue::Bool(false),
            SortValue::Number(-3.0),
        ];
        values.sort_by(|a, b| compare(a, b, false));
        let classes: Vec<u8> = values.iter().map(SortValue::class).collect();
        assert_eq!(classes, [0, 0, 1, 2, 2, 3]);
        assert!(matches!(values[3], SortValue::Bool(false)));
    }
}
