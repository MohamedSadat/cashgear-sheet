//! Format code text to sections. What counts as readable follows what Excel opens in a file: a
//! workbook holding a code Excel refuses here fails to open there.

use crate::locale::lcid_from_name;
use crate::FormatColor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    Zero,
    Hash,
    Space,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Gt,
    Ge,
    Lt,
    Le,
    Eq,
    Ne,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cond {
    pub op: Op,
    pub limit: f64,
}

impl Cond {
    pub fn test(self, value: f64) -> bool {
        match self.op {
            Op::Gt => value > self.limit,
            Op::Ge => value >= self.limit,
            Op::Lt => value < self.limit,
            Op::Le => value <= self.limit,
            Op::Eq => value == self.limit,
            Op::Ne => value != self.limit,
        }
    }

    /// A section only negative numbers can reach shows them without their minus sign.
    pub fn only_negative(self) -> bool {
        match self.op {
            Op::Lt => self.limit <= 0.0,
            Op::Le | Op::Eq => self.limit < 0.0,
            _ => false,
        }
    }

    /// Whether the section that catches what this condition leaves also drops the minus sign.
    pub fn else_drops_sign(self) -> bool {
        self.op != Op::Eq && self.limit <= 0.0
    }
}

/// One date or time token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    /// Two digits, or four.
    Year(u8),
    /// 1 and 2 are digits, 3 the short name, 4 the name and 5 its first letter.
    Month(u8),
    /// 1 and 2 are digits, 3 the short name and 4 the name.
    Day(u8),
    Hour(u8),
    Minute(u8),
    Second(u8),
    /// Decimals of a second.
    SubSecond(u8),
    AmPm,
    /// `A/P` as written, such as `a` and `P`.
    AP(char, char),
    /// `[h]`, `[m]` and `[s]` with their width.
    ElapsedHours(u8),
    ElapsedMinutes(u8),
    ElapsedSeconds(u8),
    /// `e`: the year, or the era's year where the locale has eras.
    EraYear(u8),
    /// `g` to `ggg`.
    Era(u8),
    /// `b`: the Buddhist year in two digits, or four.
    BuddhistYear(u8),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Item {
    Lit(Box<str>),
    Fill(char),
    Digit(Place),
    Point,
    /// `E+` or `E-`, with the width of the exponent.
    Exp {
        plus: bool,
        width: u8,
    },
    /// `@`
    Text,
    General,
    Date(Part),
    DateSep,
    TimeSep,
    /// The locale's currency symbol, in the built-ins that use it.
    Currency,
    /// `,`, `%`, `/`, `:` and the digits 1 to 9 until the section knows what kind it is.
    Comma,
    Percent,
    Slash,
    Colon,
    Fixed(u8),
    /// `E` before its kind is known.
    ExpMark(bool, bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Den {
    Fixed(u32),
    Places,
}

/// A fraction's items are laid out as prefix and whole number, separator, numerator, literals,
/// slash, literals, denominator, suffix. These are the positions in between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fraction {
    pub whole_end: usize,
    pub num_start: usize,
    pub num_end: usize,
    pub slash: usize,
    pub den_start: usize,
    pub den_end: usize,
    pub den: Den,
    pub mixed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Number {
    pub int_places: i32,
    pub frac_places: i32,
    pub grouping: bool,
    /// Power of ten from `%` and trailing commas.
    pub scale: i32,
    pub scientific: bool,
    pub fraction: Option<Fraction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DateInfo {
    /// Decimals of a second shown.
    pub places: u32,
    /// AM/PM is shown, so hours count to 12.
    pub twelve: bool,
    /// Only elapsed time is shown, so a negative or very large number still has a text.
    pub elapsed_only: bool,
    pub elapsed: bool,
}

/// `[$-F800]`, `[$-F400]` and the built-ins that take their whole form from the locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum System {
    ShortDate,
    ShortDateTime,
    LongDate,
    LongTime,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    Literal,
    General,
    Text,
    Number(Number),
    Date(DateInfo),
    System(System),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Section {
    pub cond: Option<Cond>,
    pub color: Option<FormatColor>,
    pub kind: Kind,
    pub items: Vec<Item>,
}

/// A locale tag: the parts of `[$-NNCCLLLL]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Tag {
    pub lcid: u32,
    pub numeral: u8,
    pub calendar: u8,
    /// `b2` came first, so dates are Hijri unless a calendar says otherwise.
    pub hijri_prefix: bool,
    pub present: bool,
}

impl Tag {
    pub fn is_japanese(&self) -> bool {
        self.lcid & 0x3ff == 0x11
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Code {
    pub sections: Vec<Section>,
    pub text: Option<Section>,
    pub tag: Tag,
}

struct Parser {
    lower_exponent: bool,
    chars: Vec<char>,
    at: usize,
    tag: Tag,
    /// This section started with `b1` or `b2`.
    prefix: bool,
    section_index: usize,
}

const COLORS: [(&str, FormatColor); 8] = [
    ("black", FormatColor::Black),
    ("blue", FormatColor::Blue),
    ("cyan", FormatColor::Cyan),
    ("green", FormatColor::Green),
    ("magenta", FormatColor::Magenta),
    ("red", FormatColor::Red),
    ("white", FormatColor::White),
    ("yellow", FormatColor::Yellow),
];

pub(crate) fn parse_code(code: &str) -> Option<Code> {
    parse_code_with(code, false)
}

/// `lower_exponent` is for `TEXT`, which takes `0.00e+00` where a file's formats can't.
pub(crate) fn parse_code_with(code: &str, lower_exponent: bool) -> Option<Code> {
    let mut parser = Parser {
        chars: code.chars().collect(),
        at: 0,
        tag: Tag::default(),
        prefix: false,
        section_index: 0,
        lower_exponent,
    };
    let mut sections = Vec::new();
    loop {
        sections.push(parser.section()?);
        if parser.at >= parser.chars.len() {
            break;
        }
        parser.at += 1; // The semicolon.
        parser.section_index += 1;
        if sections.len() == 4 {
            return None;
        }
    }
    let last = sections.len() - 1;
    if sections[..last].iter().any(|s| s.kind == Kind::Text) {
        return None;
    }
    let text = if sections[last].kind == Kind::Text || sections.len() == 4 {
        sections.pop()
    } else {
        None
    };
    // A third section takes whatever is left, so it can't have a condition of its own. The fourth
    // is only for text, so it can't either.
    if sections.get(2).is_some_and(|s| s.cond.is_some())
        || text.as_ref().is_some_and(|s| s.cond.is_some())
    {
        return None;
    }
    Some(Code {
        sections,
        text,
        tag: parser.tag,
    })
}

impl Parser {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    /// How many times this letter repeats here, in either case.
    fn run(&self, letter: char) -> usize {
        self.chars[self.at..]
            .iter()
            .take_while(|c| c.eq_ignore_ascii_case(&letter))
            .count()
    }

    fn looking_at(&self, text: &str) -> bool {
        let mut chars = self.chars[self.at..].iter();
        text.chars()
            .all(|c| chars.next().is_some_and(|d| d.eq_ignore_ascii_case(&c)))
    }

    fn section(&mut self) -> Option<Section> {
        let mut items = Vec::new();
        let mut cond = None;
        let mut color = None;
        let mut system = None;
        let tag_before = self.tag;
        self.prefix = false;
        while let Some(c) = self.peek(0) {
            if c == ';' {
                break;
            }
            self.at += 1;
            match c {
                '"' => {
                    let start = self.at;
                    while self.peek(0)? != '"' {
                        self.at += 1;
                    }
                    let text: String = self.chars[start..self.at].iter().collect();
                    self.at += 1;
                    items.push(Item::Lit(text.into()));
                }
                '\\' => {
                    let next = self.peek(0)?;
                    self.at += 1;
                    items.push(Item::Lit(next.to_string().into()));
                }
                '_' => {
                    self.peek(0)?;
                    self.at += 1;
                    items.push(Item::Lit(" ".into()));
                }
                '*' => {
                    let next = self.peek(0)?;
                    self.at += 1;
                    items.push(Item::Fill(next));
                }
                '[' => self.bracket(&mut items, &mut cond, &mut color, &mut system)?,
                '0' => items.push(Item::Digit(Place::Zero)),
                '#' => items.push(Item::Digit(Place::Hash)),
                '?' => items.push(Item::Digit(Place::Space)),
                '1'..='9' => items.push(Item::Fixed(c as u8 - b'0')),
                '.' => items.push(Item::Point),
                ',' => items.push(Item::Comma),
                '%' => items.push(Item::Percent),
                '/' => items.push(Item::Slash),
                ':' => items.push(Item::Colon),
                '@' => items.push(Item::Text),
                _ if c.is_ascii_alphabetic() => {
                    self.at -= 1;
                    self.letters(c, &mut items)?;
                }
                _ => items.push(Item::Lit(c.to_string().into())),
            }
        }
        let section = finish_section(items, cond, color, system, self.prefix)?;
        // A tag on a text section means nothing.
        if section.kind == Kind::Text {
            self.tag = tag_before;
        }
        Some(section)
    }

    fn bracket(
        &mut self,
        items: &mut Vec<Item>,
        cond: &mut Option<Cond>,
        color: &mut Option<FormatColor>,
        system: &mut Option<System>,
    ) -> Option<()> {
        let start = self.at;
        while self.peek(0)? != ']' {
            self.at += 1;
        }
        let body: String = self.chars[start..self.at].iter().collect();
        self.at += 1;
        if let Some(rest) = body.strip_prefix('$') {
            return self.locale_tag(rest, items, system);
        }
        if body.starts_with(['<', '>', '=']) {
            if cond.is_some() {
                return None;
            }
            *cond = Some(parse_condition(&body));
            return Some(());
        }
        let lower = body.to_ascii_lowercase();
        if !lower.is_empty() && lower.chars().all(|c| c == lower.as_bytes()[0] as char) {
            let width = lower.len() as u8;
            let part = match lower.as_bytes()[0] {
                b'h' => Some(Part::ElapsedHours(width)),
                b'm' => Some(Part::ElapsedMinutes(width)),
                b's' => Some(Part::ElapsedSeconds(width)),
                _ => None,
            };
            if let Some(part) = part {
                items.push(Item::Date(part));
                return Some(());
            }
        }
        // Accepted, but the numerals themselves are a known gap: `[DBNum1][$-804]` asks for Chinese
        // ones, and these show Western digits.
        if let Some(rest) = lower.strip_prefix("dbnum") {
            rest.parse::<u8>().ok().filter(|n| (1..=9).contains(n))?;
            return Some(());
        }
        if let Some(rest) = lower.strip_prefix("color") {
            let index: u8 = rest.trim().parse().ok()?;
            if !(1..=56).contains(&index) {
                return None;
            }
            if color.replace(FormatColor::Indexed(index)).is_some() {
                return None;
            }
            return Some(());
        }
        let named = COLORS.iter().find(|(name, _)| *name == lower)?;
        if color.replace(named.1).is_some() {
            return None;
        }
        Some(())
    }

    fn locale_tag(
        &mut self,
        body: &str,
        items: &mut Vec<Item>,
        system: &mut Option<System>,
    ) -> Option<()> {
        let (symbol, rest) = match body.find('-') {
            Some(at) => (&body[..at], &body[at + 1..]),
            None => (body, ""),
        };
        if !symbol.is_empty() {
            items.push(Item::Lit(symbol.into()));
        }
        if rest.is_empty() {
            return Some(());
        }
        let raw = match u32::from_str_radix(rest, 16) {
            Ok(raw) => raw,
            Err(_) => match rest.to_ascii_lowercase().as_str() {
                "x-sysdate" => 0xF800,
                "x-systime" => 0xF400,
                _ => {
                    // A language tag, with a calendar after a comma: ar-SA,1.
                    let named = match rest.split_once(',') {
                        Some((name, calendar)) => calendar
                            .parse::<u32>()
                            .ok()
                            .and_then(|c| Some((c << 16) | lcid_from_name(name)?)),
                        None => lcid_from_name(rest),
                    };
                    match named {
                        Some(raw) => raw,
                        // After a currency symbol, a tag that can't be read only loses the tag.
                        None if !symbol.is_empty() => 0,
                        None => return None,
                    }
                }
            },
        };
        match raw {
            0xF800 => *system = Some(System::LongDate),
            0xF400 => *system = Some(System::LongTime),
            _ => {}
        }
        // The first section's tag holds for the whole code, and a later section's means nothing.
        if self.section_index == 0 && !self.tag.present && raw != 0 {
            self.tag.lcid = raw & 0xffff;
            self.tag.calendar = (raw >> 16) as u8;
            self.tag.numeral = (raw >> 24) as u8;
            self.tag.present = true;
        }
        Some(())
    }

    fn letters(&mut self, c: char, items: &mut Vec<Item>) -> Option<()> {
        let lower = c.to_ascii_lowercase();
        if self.looking_at("general") {
            self.at += 7;
            items.push(Item::General);
            return Some(());
        }
        if self.looking_at("am/pm") {
            self.at += 5;
            items.push(Item::Date(Part::AmPm));
            return Some(());
        }
        if self.looking_at("a/p") {
            let a = self.chars[self.at];
            let p = self.chars[self.at + 2];
            self.at += 3;
            items.push(Item::Date(Part::AP(a, p)));
            return Some(());
        }
        match lower {
            'y' => {
                let run = self.run('y');
                self.at += run;
                items.push(Item::Date(Part::Year(if run < 3 { 2 } else { 4 })));
            }
            'm' => {
                let run = self.run('m');
                self.at += run;
                items.push(Item::Date(Part::Month(if run > 5 { 4 } else { run as u8 })));
            }
            'd' => {
                let run = self.run('d');
                self.at += run;
                items.push(Item::Date(Part::Day(run.min(4) as u8)));
            }
            'h' => {
                let run = self.run('h');
                self.at += run;
                items.push(Item::Date(Part::Hour(run.min(2) as u8)));
            }
            's' => {
                let run = self.run('s');
                self.at += run;
                items.push(Item::Date(Part::Second(run.min(2) as u8)));
            }
            'e' => {
                // For `TEXT`, `e-` after digit places is an exponent, as `E-` is. On its own it
                // is the era year of a date, as in `e-mm-dd`.
                let after_digits = items.iter().any(|item| matches!(item, Item::Digit(_)));
                if c == 'E' || (self.lower_exponent && after_digits) {
                    if let Some(sign @ ('+' | '-')) = self.peek(1) {
                        self.at += 2;
                        items.push(Item::ExpMark(sign == '+', c == 'e'));
                        return Some(());
                    }
                    if c == 'E' {
                        return None;
                    }
                }
                let run = self.run('e');
                self.at += run;
                items.push(Item::Date(Part::EraYear(run.min(2) as u8)));
            }
            'g' => {
                let run = self.run('g');
                self.at += run;
                items.push(Item::Date(Part::Era(run.min(3) as u8)));
            }
            'b' => {
                let run = self.run('b');
                if run == 1 {
                    if let Some(which @ ('1' | '2')) = self.peek(1) {
                        // A calendar prefix: it must come before everything but a tag and has no
                        // effect after a tag, which already says which calendar.
                        if items.iter().any(|item| !matches!(item, Item::Lit(_))) {
                            return None;
                        }
                        self.at += 2;
                        self.prefix = true;
                        if which == '2' && !self.tag.present {
                            self.tag.hijri_prefix = true;
                        }
                        return Some(());
                    }
                    if c == 'B' || self.peek(1).is_some_and(|d| d.is_ascii_digit()) {
                        return None;
                    }
                }
                self.at += run;
                items.push(Item::Date(Part::BuddhistYear(if run < 3 { 2 } else { 4 })));
            }
            'n' => return None,
            _ => {
                self.at += 1;
                items.push(Item::Lit(c.to_string().into()));
            }
        }
        Some(())
    }
}

/// `[>a]` reads as `[>0]` in Excel, so a limit it can't read is not a failure.
fn parse_condition(body: &str) -> Cond {
    let (op, rest) = if let Some(rest) = body.strip_prefix(">=") {
        (Op::Ge, rest)
    } else if let Some(rest) = body.strip_prefix("<=") {
        (Op::Le, rest)
    } else if let Some(rest) = body.strip_prefix("<>") {
        (Op::Ne, rest)
    } else if let Some(rest) = body.strip_prefix('>') {
        (Op::Gt, rest)
    } else if let Some(rest) = body.strip_prefix('<') {
        (Op::Lt, rest)
    } else {
        (Op::Eq, &body[1..])
    };
    Cond {
        op,
        limit: rest.trim().parse().unwrap_or(0.0),
    }
}

fn finish_section(
    mut items: Vec<Item>,
    cond: Option<Cond>,
    color: Option<FormatColor>,
    system: Option<System>,
    prefix: bool,
) -> Option<Section> {
    let has = |wanted: fn(&Item) -> bool, items: &[Item]| items.iter().any(wanted);
    let has_date = has(|i| matches!(i, Item::Date(_)), &items);
    let has_digit = has(|i| matches!(i, Item::Digit(_)), &items);
    let has_text = has(|i| matches!(i, Item::Text), &items);
    let has_general = has(|i| matches!(i, Item::General), &items);
    let has_exp = has(|i| matches!(i, Item::ExpMark(..)), &items);
    let has_fixed = has(|i| matches!(i, Item::Fixed(_)), &items);
    let has_point = has(|i| matches!(i, Item::Point), &items);
    let generals = items.iter().filter(|i| matches!(i, Item::General)).count();

    if has_exp && !has_digit && !has_fixed {
        return None;
    }
    if generals > 1 || (prefix && has_digit) || (has_fixed && (has_text || has_date)) {
        return None;
    }
    if system.is_some() {
        // The tag replaces the whole section, whatever it says.
        return Some(Section {
            cond,
            color,
            kind: Kind::System(system?),
            items: Vec::new(),
        });
    }
    let has_slash = has(|i| matches!(i, Item::Slash), &items);
    let has_percent = has(|i| matches!(i, Item::Percent), &items);
    let kind = if has_text {
        if has_digit || has_date || has_general || has_point || has_slash || has_percent {
            return None;
        }
        literalize(&mut items);
        Kind::Text
    } else if has_general && !has_date {
        if has_digit || has_point || has_slash {
            return None;
        }
        literalize(&mut items);
        Kind::General
    } else if has_date {
        // Excel takes General beside a date and shows something odd. We show the date.
        items.retain(|item| !matches!(item, Item::General));
        Kind::Date(finish_date(&mut items)?)
    } else if has_digit || has_fixed || has_point {
        Kind::Number(finish_number(&mut items)?)
    } else if items.is_empty() && (cond.is_some() || color.is_some()) {
        // An empty section with a condition or a colour is General.
        items.push(Item::General);
        Kind::General
    } else {
        literalize(&mut items);
        Kind::Literal
    };
    Some(Section {
        cond,
        color,
        kind,
        items,
    })
}

/// In a section with no digits, these characters only stand for themselves.
fn literalize(items: &mut [Item]) {
    for item in items {
        let text = match item {
            Item::Point => ".".to_string(),
            Item::Comma => ",".to_string(),
            Item::Percent => "%".to_string(),
            Item::Slash => "/".to_string(),
            Item::Colon => ":".to_string(),
            Item::Fixed(digit) => digit.to_string(),
            _ => continue,
        };
        *item = Item::Lit(text.into());
    }
}

fn finish_date(items: &mut Vec<Item>) -> Option<DateInfo> {
    let mut info = DateInfo::default();
    let mut out = Vec::with_capacity(items.len());
    let mut at = 0;
    while at < items.len() {
        match &items[at] {
            Item::Point => {
                let mut end = at + 1;
                while matches!(items.get(end), Some(Item::Digit(Place::Zero))) {
                    end += 1;
                }
                if matches!(items.get(end), Some(Item::Digit(_))) || end - at - 1 > 3 {
                    return None;
                }
                if end > at + 1 {
                    let places = (end - at - 1) as u8;
                    info.places = info.places.max(places as u32);
                    out.push(Item::Date(Part::SubSecond(places)));
                } else {
                    out.push(Item::Lit(".".into()));
                }
                at = end;
                continue;
            }
            Item::Digit(_) | Item::Percent | Item::ExpMark(..) => return None,
            Item::Comma => out.push(Item::Lit(",".into())),
            Item::Slash => {
                if !out.iter().any(|item| matches!(item, Item::Date(_))) {
                    return None;
                }
                out.push(Item::DateSep);
            }
            Item::Colon => out.push(Item::TimeSep),
            Item::Fixed(digit) => out.push(Item::Lit(digit.to_string().into())),
            other => out.push(other.clone()),
        }
        at += 1;
    }
    *items = out;

    // After [h] a plain h is elapsed hours too, and likewise for [s] and s.
    let mut elapsed_hours = false;
    let mut elapsed_seconds = false;
    for item in items.iter_mut() {
        if let Item::Date(part) = item {
            match *part {
                Part::ElapsedHours(_) => elapsed_hours = true,
                Part::ElapsedSeconds(_) => elapsed_seconds = true,
                Part::Hour(letters) if elapsed_hours => *part = Part::ElapsedHours(letters),
                Part::Second(letters) if elapsed_seconds => *part = Part::ElapsedSeconds(letters),
                _ => {}
            }
        }
    }
    let parts: Vec<Part> = items
        .iter()
        .filter_map(|item| match item {
            Item::Date(part) => Some(*part),
            _ => None,
        })
        .collect();
    let is_elapsed = |part: &Part| {
        matches!(
            part,
            Part::ElapsedHours(_) | Part::ElapsedMinutes(_) | Part::ElapsedSeconds(_)
        )
    };
    let elapsed: Vec<Part> = parts.iter().filter(|p| is_elapsed(p)).copied().collect();
    info.elapsed = !elapsed.is_empty();
    info.twelve = parts
        .iter()
        .any(|part| matches!(part, Part::AmPm | Part::AP(..)));
    if info.elapsed && info.twelve {
        return None;
    }
    // One unit of elapsed time, and nothing larger beside a smaller one.
    if let Some(first) = elapsed.first() {
        if elapsed
            .iter()
            .any(|p| std::mem::discriminant(p) != std::mem::discriminant(first))
        {
            return None;
        }
        let seen_before = parts.iter().take_while(|p| !is_elapsed(p));
        let smaller_before = seen_before.clone().any(|p| match first {
            Part::ElapsedMinutes(_) => matches!(p, Part::Hour(_)),
            Part::ElapsedSeconds(_) => matches!(p, Part::Hour(_) | Part::Minute(_)),
            _ => false,
        });
        if smaller_before {
            return None;
        }
    }
    info.elapsed_only = info.elapsed && parts.iter().all(is_elapsed);

    // `m` is minutes straight after hours or seconds, or straight before seconds. Minutes use up
    // the hours before them, so a later `m` is a month again.
    let mut after_time = false;
    let mut minutes_seen = false;
    let mut index = 0;
    for item in items.iter_mut() {
        let Item::Date(part) = item else {
            continue;
        };
        let next = parts[index + 1..]
            .iter()
            .find(|p| !matches!(p, Part::AmPm | Part::AP(..)));
        index += 1;
        match *part {
            Part::Hour(_) | Part::ElapsedHours(_) => after_time = true,
            Part::Second(_) | Part::ElapsedSeconds(_) if !minutes_seen => after_time = true,
            Part::Month(letters) if letters <= 2 => {
                let before_seconds = matches!(
                    next,
                    Some(Part::Second(_) | Part::ElapsedSeconds(_) | Part::SubSecond(_))
                );
                if after_time || before_seconds {
                    *part = Part::Minute(letters);
                    after_time = false;
                    minutes_seen = true;
                }
            }
            _ => {}
        }
    }
    Some(info)
}

fn finish_number(items: &mut Vec<Item>) -> Option<Number> {
    let mut number = Number::default();
    let mut out: Vec<Item> = Vec::with_capacity(items.len());
    let mut scale_commas = 0;
    let mut percents = 0;
    let mut seen_point = false;
    let mut after_exp = false;
    let mut at = 0;
    while at < items.len() {
        match &items[at] {
            Item::Comma => {
                let mut end = at;
                while matches!(items.get(end), Some(Item::Comma)) {
                    end += 1;
                }
                let prev_digit = matches!(out.last(), Some(Item::Digit(_) | Item::Exp { .. }))
                    || (seen_point && matches!(out.last(), Some(Item::Point)));
                let next_digit = matches!(items.get(end), Some(Item::Digit(_)));
                if prev_digit && next_digit {
                    if !seen_point {
                        number.grouping = true;
                    }
                } else if prev_digit {
                    scale_commas += end - at;
                } else {
                    for _ in at..end {
                        out.push(Item::Lit(",".into()));
                    }
                }
                at = end;
                continue;
            }
            Item::Percent => {
                percents += 1;
                out.push(Item::Lit("%".into()));
            }
            Item::Colon => return None,
            Item::ExpMark(plus, lower) => {
                if after_exp {
                    return None;
                }
                if !out
                    .iter()
                    .any(|item| matches!(item, Item::Digit(_) | Item::Fixed(_)))
                {
                    return None;
                }
                // Literals can stand between the mark and its digits. The letter stays where
                // it was written, and the sign goes with the digits.
                let mut first = at + 1;
                while matches!(items.get(first), Some(Item::Lit(_))) {
                    first += 1;
                }
                let mut end = first;
                while matches!(items.get(end), Some(Item::Digit(_))) {
                    end += 1;
                }
                if end == first {
                    return None;
                }
                out.push(Item::Lit(if *lower { "e" } else { "E" }.into()));
                out.extend(items[at + 1..first].iter().cloned());
                out.push(Item::Exp {
                    plus: *plus,
                    width: (end - first) as u8,
                });
                number.scientific = true;
                after_exp = true;
                at = end;
                continue;
            }
            Item::Point if after_exp => out.push(Item::Lit(".".into())),
            Item::Digit(_) if after_exp => out.push(Item::Lit("0".into())),
            Item::Point if seen_point => out.push(Item::Lit(".".into())),
            Item::Point => {
                seen_point = true;
                out.push(Item::Point);
            }
            other => out.push(other.clone()),
        }
        at += 1;
    }
    *items = out;

    let slash_literal = !items.iter().any(|item| matches!(item, Item::Digit(_)))
        && items.iter().any(|item| matches!(item, Item::Fixed(_)));
    if slash_literal {
        // Only fixed digits, as in 1/2: nothing there to fill in.
        literalize(items);
    } else if items.iter().any(|item| matches!(item, Item::Slash)) {
        number.fraction = Some(finish_fraction(items)?);
    }
    for item in items.iter_mut() {
        if let Item::Fixed(digit) = item {
            *item = Item::Lit(digit.to_string().into());
        }
    }
    let mut past_point = false;
    for item in items.iter() {
        match item {
            Item::Point => past_point = true,
            Item::Digit(_) if past_point => number.frac_places += 1,
            Item::Digit(_) => number.int_places += 1,
            _ => {}
        }
    }
    if let Some(fraction) = &number.fraction {
        number.int_places = items[..fraction.whole_end]
            .iter()
            .filter(|item| matches!(item, Item::Digit(_)))
            .count() as i32;
    }
    number.scale = if number.scientific {
        0
    } else {
        percents * 2 - scale_commas as i32 * 3
    };
    Some(number)
}

fn finish_fraction(items: &mut [Item]) -> Option<Fraction> {
    if items
        .iter()
        .any(|item| matches!(item, Item::Point | Item::Exp { .. }))
    {
        return None;
    }
    let slash = items.iter().position(|item| matches!(item, Item::Slash))?;
    let is_digit = |item: &Item| matches!(item, Item::Digit(_) | Item::Fixed(_));

    let den_start = slash + 1 + items[slash + 1..].iter().position(is_digit)?;
    let mut den_end = den_start;
    while items.get(den_end).is_some_and(is_digit) {
        den_end += 1;
    }
    let den = match items[den_start] {
        Item::Fixed(_) => {
            let mut value = 0u32;
            for item in &items[den_start..den_end] {
                value = value.saturating_mul(10).saturating_add(match item {
                    Item::Fixed(digit) => *digit as u32,
                    Item::Digit(Place::Zero) => 0,
                    _ => return None,
                });
            }
            Den::Fixed(value)
        }
        _ => {
            if items[den_start..den_end]
                .iter()
                .any(|item| matches!(item, Item::Fixed(_)))
            {
                return None;
            }
            Den::Places
        }
    };

    let num_end = 1 + items[..slash]
        .iter()
        .rposition(|item| matches!(item, Item::Digit(_)))?;
    let mut num_start = num_end - 1;
    while num_start > 0 && matches!(items[num_start - 1], Item::Digit(_)) {
        num_start -= 1;
    }
    let whole_end = items[..num_start]
        .iter()
        .rposition(|item| matches!(item, Item::Digit(_)))
        .map(|at| at + 1);
    if items[slash + 1..]
        .iter()
        .any(|item| matches!(item, Item::Slash))
    {
        return None;
    }
    Some(Fraction {
        whole_end: whole_end.unwrap_or(num_start),
        num_start,
        num_end,
        slash,
        den_start,
        den_end,
        den,
        mixed: whole_end.is_some(),
    })
}
