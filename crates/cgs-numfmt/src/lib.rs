//! Excel number format codes and date serials to display text, in pure Rust. The behaviour is
//! Excel's own, checked against its display text by `tests/excel_text.rs`.

mod date;
mod locale;
mod number;
mod parse;
mod render;

pub use date::{serial_to_datetime, DateTimeParts};
pub use locale::Locale;

use parse::{parse_code, parse_code_with, Code, Item, Kind, Part, System};

/// What goes through a format.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    Number(f64),
    Text(&'a str),
    Bool(bool),
    Error(&'a str),
}

#[derive(Debug, Clone, Copy)]
pub struct Options<'a> {
    pub locale: &'a Locale,
    pub date1904: bool,
}

/// A colour named by the format. `Indexed` is `[Color n]`, 1 to 56.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatColor {
    Black,
    Blue,
    Cyan,
    Green,
    Magenta,
    Red,
    White,
    Yellow,
    Indexed(u8),
}

/// What the tile worker needs beyond the text that went into `out`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Display {
    /// From `[Red]`, `[Color 10]` and the like.
    pub color: Option<FormatColor>,
    /// General number: the worker may shorten it to fit the column.
    pub general: bool,
    /// Excel shows `####` whatever the width, such as a negative date. `out` gets nothing.
    pub hashes: bool,
    /// `*x` repeat character and its byte position in `out`. The worker expands it to the column
    /// width.
    pub fill: Option<(char, usize)>,
}

/// A compiled format code, cheap to reuse.
#[derive(Debug, Clone, PartialEq)]
pub struct NumFmt {
    code: Code,
    valid: bool,
}

/// A built-in format's code as it reads in a file, for ids that have a fixed one.
pub fn builtin_code(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 => r##""$"#,##0_);\("$"#,##0\)"##,
        6 => r##""$"#,##0_);[Red]\("$"#,##0\)"##,
        7 => r##""$"#,##0.00_);\("$"#,##0.00\)"##,
        8 => r##""$"#,##0.00_);[Red]\("$"#,##0.00\)"##,
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yyyy h:mm",
        37 => r#"#,##0_);\(#,##0\)"#,
        38 => r#"#,##0_);[Red]\(#,##0\)"#,
        39 => r#"#,##0.00_);\(#,##0.00\)"#,
        40 => r#"#,##0.00_);[Red]\(#,##0.00\)"#,
        41 => r#"_(* #,##0_);_(* \(#,##0\);_(* "-"_);_(@_)"#,
        42 => r##"_("$"* #,##0_);_("$"* \(#,##0\);_("$"* "-"_);_(@_)"##,
        43 => r#"_(* #,##0.00_);_(* \(#,##0.00\);_(* "-"??_);_(@_)"#,
        44 => r##"_("$"* #,##0.00_);_("$"* \(#,##0.00\);_("$"* "-"??_);_(@_)"##,
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mm:ss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// The built-in id of a code, if it has one, as Excel stores those.
pub fn builtin_id(code: &str) -> Option<u32> {
    (0..=49).find(|&id| builtin_code(id) == Some(code))
}

/// A format code with more or fewer decimal places, in each section that shows a number. Nothing if
/// it doesn't have such a place, as with a date. General becomes a code first, starting from what it
/// showed, as Excel does.
pub fn change_decimals(code: &str, step: i32, general_decimals: u32) -> Option<String> {
    if code.eq_ignore_ascii_case("General") {
        let places = (i64::from(general_decimals) + i64::from(step)).max(0) as usize;
        return Some(if places == 0 {
            "0".to_string()
        } else {
            format!("0.{}", "0".repeat(places))
        });
    }
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::with_capacity(code.len() + 2);
    let mut changed = false;
    let mut at = 0;
    // A section at a time: what's inside it is left alone apart from the number.
    while at < chars.len() {
        let mut end = at;
        let mut quoted = false;
        let mut bracket = false;
        while end < chars.len() {
            match chars[end] {
                '"' if !bracket => quoted = !quoted,
                '[' if !quoted => bracket = true,
                ']' if !quoted => bracket = false,
                '\\' | '_' | '*' if !quoted && !bracket => end += 1,
                ';' if !quoted && !bracket => break,
                _ => {}
            }
            end += 1;
        }
        let end = end.min(chars.len());
        let (section, did) = section_decimals(&chars[at..end], step);
        out.push_str(&section);
        changed |= did;
        if end < chars.len() {
            out.push(';');
        }
        at = end + 1;
    }
    changed.then_some(out)
}

/// One section with more or fewer decimal places, and whether it changed.
fn section_decimals(section: &[char], step: i32) -> (String, bool) {
    // Where the number's digits are, outside text and brackets.
    let mut last_digit = None;
    let mut point = None;
    let mut decimals_end = None;
    // A section such as `"-"??`, which only holds room, grows the room.
    let mut only_room = true;
    let mut quoted = false;
    let mut bracket = false;
    let mut at = 0;
    let mut has_date = false;
    while at < section.len() {
        let ch = section[at];
        match ch {
            '"' if !bracket => quoted = !quoted,
            '[' if !quoted => bracket = true,
            ']' if !quoted => bracket = false,
            '\\' | '_' | '*' if !quoted && !bracket => at += 1,
            _ if quoted || bracket => {}
            '0' | '#' | '?' => {
                only_room &= ch == '?';
                last_digit = Some(at);
                if point.is_some() {
                    decimals_end = Some(at);
                }
            }
            '.' if point.is_none() && last_digit.is_some() => point = Some(at),
            'd' | 'D' | 'm' | 'M' | 'y' | 'Y' | 'h' | 'H' | 's' | 'S' => has_date = true,
            _ => {}
        }
        at += 1;
    }
    let text = |list: &[char]| list.iter().collect::<String>();
    let Some(last) = last_digit.filter(|_| !has_date) else {
        return (text(section), false);
    };
    if only_room && point.is_none() {
        let mut out = text(&section[..=last]);
        if step > 0 {
            out.push_str(&"?".repeat(step as usize));
        } else {
            let keep = out.len().saturating_sub((-step) as usize).max(1);
            out.truncate(keep);
        }
        out.push_str(&text(&section[last + 1..]));
        return (out, true);
    }
    if step > 0 {
        let extra = "0".repeat(step as usize);
        return match (point, decimals_end) {
            (Some(_), Some(end)) => {
                let mut out = text(&section[..=end]);
                out.push_str(&extra);
                out.push_str(&text(&section[end + 1..]));
                (out, true)
            }
            (Some(place), None) => {
                let mut out = text(&section[..=place]);
                out.push_str(&extra);
                out.push_str(&text(&section[place + 1..]));
                (out, true)
            }
            _ => {
                let mut out = text(&section[..=last]);
                out.push('.');
                out.push_str(&extra);
                out.push_str(&text(&section[last + 1..]));
                (out, true)
            }
        };
    }
    let (Some(place), Some(end)) = (point, decimals_end) else {
        return (text(section), false);
    };
    let have = end - place;
    let drop = (-step) as usize;
    if drop >= have {
        let mut out = text(&section[..place]);
        out.push_str(&text(&section[end + 1..]));
        (out, true)
    } else {
        let mut out = text(&section[..=end - drop]);
        out.push_str(&text(&section[end + 1..]));
        (out, true)
    }
}

impl NumFmt {
    /// Never fails: an unreadable code behaves as Excel's General, and `is_valid` says so.
    pub fn parse(code: &str) -> Self {
        match parse_code(code) {
            Some(code) => Self { code, valid: true },
            None => Self {
                valid: false,
                ..Self::general()
            },
        }
    }

    pub fn general() -> Self {
        Self::parse("General")
    }

    /// The format behind a built-in id, as Excel has it on an en-US system: 0 to 49, and the
    /// higher ids it treats as one of those. Ids 14 and 22 take their form from the `Locale`, and
    /// the `$` in 5 to 8, 42 and 44 is the `Locale`'s currency symbol.
    pub fn builtin(id: u32) -> Option<Self> {
        let locale_form = |system| {
            let mut format = Self::general();
            format.code.sections[0].kind = Kind::System(system);
            format.code.sections[0].items.clear();
            format
        };
        let code = match id {
            0 | 23..=26 => "General",
            1 | 59 => "0",
            2 | 60 => "0.00",
            3 | 61 => "#,##0",
            4 | 62 => "#,##0.00",
            5 | 63 => r#"$#,##0_);\($#,##0\)"#,
            6 | 64 => r#"$#,##0_);[Red]\($#,##0\)"#,
            7 | 65 => r#"$#,##0.00_);\($#,##0.00\)"#,
            8 | 66 => r#"$#,##0.00_);[Red]\($#,##0.00\)"#,
            9 | 67 => "0%",
            10 | 68 => "0.00%",
            11 => "0.00E+00",
            12 | 69 => "# ?/?",
            13 | 70 => "# ??/??",
            14 | 27..=31 | 36 | 50..=58 | 71 | 72 => return Some(locale_form(System::ShortDate)),
            15 | 73 => "d-mmm-yy",
            16 | 74 => "d-mmm",
            17 | 75 => "mmm-yy",
            18 => "h:mm AM/PM",
            19 => "h:mm:ss AM/PM",
            20 | 76 => "h:mm",
            21 | 32..=35 | 77 => "h:mm:ss",
            22 | 78 => return Some(locale_form(System::ShortDateTime)),
            37 => r#"#,##0_);\(#,##0\)"#,
            38 => r#"#,##0_);[Red]\(#,##0\)"#,
            39 => r#"#,##0.00_);\(#,##0.00\)"#,
            40 => r#"#,##0.00_);[Red]\(#,##0.00\)"#,
            41 => r#"_(* #,##0_);_(* \(#,##0\);_(* "-"_);_(@_)"#,
            42 => r##"_($* #,##0_);_($* \(#,##0\);_($* "-"_);_(@_)"##,
            43 => r#"_(* #,##0.00_);_(* \(#,##0.00\);_(* "-"??_);_(@_)"#,
            44 => r##"_($* #,##0.00_);_($* \(#,##0.00\);_($* "-"??_);_(@_)"##,
            45 | 79 => "mm:ss",
            46 | 80 => "[h]:mm:ss",
            47 | 81 => "mm:ss.0",
            48 => "##0.0E+0",
            49 => "@",
            _ => return None,
        };
        let mut format = Self::parse(code);
        let sections = format.code.sections.iter_mut();
        for section in sections.chain(format.code.text.iter_mut()) {
            for item in &mut section.items {
                if matches!(item, Item::Lit(text) if &**text == "$") {
                    *item = Item::Currency;
                }
            }
        }
        Some(format)
    }

    /// Any section formats a date or time.
    pub fn is_date(&self) -> bool {
        self.code
            .sections
            .iter()
            .any(|section| matches!(section.kind, Kind::Date(_) | Kind::System(_)))
    }

    pub fn is_general(&self) -> bool {
        self.code.sections.len() == 1
            && self.code.text.is_none()
            && self.code.sections[0].kind == Kind::General
            && self.code.sections[0].items.len() == 1
            && self.code.sections[0].cond.is_none()
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Whether the first section shows only a time, so the formula bar leaves out the date.
    fn is_time_only(&self) -> bool {
        self.code
            .sections
            .first()
            .is_some_and(|section| match section.kind {
                Kind::System(system) => system == System::LongTime,
                Kind::Date(_) => !section.items.iter().any(|item| {
                    matches!(
                        item,
                        Item::Date(
                            Part::Year(_)
                                | Part::Month(_)
                                | Part::Day(_)
                                | Part::EraYear(_)
                                | Part::Era(_)
                                | Part::BuddhistYear(_)
                        )
                    )
                }),
                _ => false,
            })
    }

    /// Whether the first section shows a percent, which the formula bar keeps.
    fn is_percent(&self) -> bool {
        self.code.sections.first().is_some_and(|section| {
            matches!(section.kind, Kind::Number(_))
                && section
                    .items
                    .iter()
                    .any(|item| matches!(item, Item::Lit(text) if &**text == "%"))
        })
    }

    /// Appends the text to `out`.
    pub fn format(&self, value: Value<'_>, options: &Options<'_>, out: &mut String) -> Display {
        render::format(&self.code, value, options, out)
    }
}

/// The TEXT() worksheet function: `Err(())` is `#VALUE!`.
#[allow(clippy::result_unit_err)]
pub fn text_function(value: Value<'_>, code: &str, options: &Options<'_>) -> Result<String, ()> {
    let format = NumFmt {
        code: parse_code_with(code, true).ok_or(())?,
        valid: true,
    };
    let mut out = String::new();
    let display = format.format(value, options, &mut out);
    if display.hashes {
        return Err(());
    }
    Ok(out)
}

/// What the formula bar shows for a constant: full precision (15 significant digits), dates in the
/// locale's short date form, percents as percents.
pub fn edit_text(value: Value<'_>, format: &NumFmt, options: &Options<'_>, out: &mut String) {
    match value {
        Value::Text(text) | Value::Error(text) => out.push_str(text),
        Value::Bool(flag) => out.push_str(if flag { "TRUE" } else { "FALSE" }),
        Value::Number(number) => {
            if format.is_date() && render::edit_date(number, format.is_time_only(), options, out) {
                return;
            }
            if format.is_percent() {
                render::edit_number(number * 100.0, options.locale, out);
                out.push('%');
            } else {
                render::edit_number(number, options.locale, out);
            }
        }
    }
}

#[cfg(test)]
mod decimals_tests {
    use super::*;

    #[test]
    fn decimals_change_in_each_section_that_shows_a_number() {
        let more = |code| change_decimals(code, 1, 0);
        let fewer = |code| change_decimals(code, -1, 0);
        assert_eq!(more("0").as_deref(), Some("0.0"));
        assert_eq!(more("0.00").as_deref(), Some("0.000"));
        assert_eq!(fewer("0.00").as_deref(), Some("0.0"));
        assert_eq!(fewer("0.0").as_deref(), Some("0"));
        assert_eq!(fewer("0"), None);
        assert_eq!(more("0%").as_deref(), Some("0.0%"));
        assert_eq!(more("#,##0").as_deref(), Some("#,##0.0"));
        assert_eq!(
            more(r#"_(* #,##0.00_);_(* \(#,##0.00\);_(* "-"??_);_(@_)"#).as_deref(),
            Some(r#"_(* #,##0.000_);_(* \(#,##0.000\);_(* "-"???_);_(@_)"#)
        );
        assert_eq!(more("0.00E+00").as_deref(), Some("0.000E+00"));
        assert_eq!(more("d-mmm-yy"), None);
        assert_eq!(more("@"), None);
        assert_eq!(more(r#""$"0"x.0""#).as_deref(), Some(r#""$"0.0"x.0""#));
    }

    #[test]
    fn general_starts_from_what_it_showed() {
        assert_eq!(change_decimals("General", 1, 2).as_deref(), Some("0.000"));
        assert_eq!(change_decimals("General", -1, 2).as_deref(), Some("0.0"));
        assert_eq!(change_decimals("General", -1, 0).as_deref(), Some("0"));
    }

    #[test]
    fn built_in_codes_map_to_their_ids() {
        assert_eq!(builtin_id("0.00"), Some(2));
        assert_eq!(builtin_id("@"), Some(49));
        assert_eq!(builtin_id("0.000"), None);
        for id in 0..=49 {
            if let Some(code) = builtin_code(id) {
                assert!(NumFmt::parse(code).is_valid(), "{code}");
            }
        }
    }
}
