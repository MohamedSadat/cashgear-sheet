//! Sections and a value to text.

use crate::date::{
    excel_day, hijri_day, japanese_era, real_weekday, split_time, system_day, Day, MAX_1900,
    MAX_1904,
};
use crate::locale::{hijri_names_for, names_for, Names, ARABIC_LEVANT, ENGLISH};
use crate::number::{write_general, Decimal, Shape};
use crate::parse::{
    parse_code, Code, DateInfo, Den, Fraction, Item, Kind, Number, Part, Place, Section, System,
    Tag,
};
use crate::{Display, Locale, Options, Value};

struct Ctx<'a> {
    options: &'a Options<'a>,
    tag: Tag,
    shape: Shape,
}

pub(crate) fn format(
    code: &Code,
    value: Value<'_>,
    options: &Options<'_>,
    out: &mut String,
) -> Display {
    let mut display = Display::default();
    let ctx = Ctx {
        options,
        tag: code.tag,
        shape: Shape::from_code(code.tag.numeral),
    };
    match value {
        Value::Number(number) => number_value(code, number, &ctx, out, &mut display),
        Value::Text(text) => text_value(code, text, out, &mut display),
        Value::Bool(flag) => {
            text_value(code, if flag { "TRUE" } else { "FALSE" }, out, &mut display)
        }
        Value::Error(text) => out.push_str(text),
    }
    display
}

/// Text uses the text section where there is one, and otherwise shows as itself.
fn text_value(code: &Code, text: &str, out: &mut String, display: &mut Display) {
    let Some(section) = &code.text else {
        // A lone General section is also what text goes through.
        if let [only] = code.sections.as_slice() {
            if only.kind == Kind::General {
                display.color = only.color;
            }
        }
        out.push_str(text);
        return;
    };
    display.color = section.color;
    for item in &section.items {
        match item {
            Item::Lit(literal) => out.push_str(literal),
            Item::Fill(fill) => display.fill = Some((*fill, out.len())),
            Item::Text | Item::General => out.push_str(text),
            _ => {}
        }
    }
}

enum Pick<'a> {
    Section(&'a Section, bool),
    General(bool),
    Hashes,
}

/// Chooses the section for a number, and whether that section drops the minus sign. A section
/// that can only get negative numbers drops it, as does the second section of a plain code.
fn pick(code: &Code, value: f64) -> Pick<'_> {
    let sections = &code.sections;
    let drops = |section: &Section| section.cond.is_some_and(|cond| cond.only_negative());
    match sections.len() {
        0 => Pick::General(false),
        1 => {
            let first = &sections[0];
            match first.cond {
                None => Pick::Section(first, false),
                Some(_) if first.kind == Kind::General => {
                    if first.cond.is_some_and(|cond| cond.test(value)) {
                        Pick::Section(first, false)
                    } else {
                        Pick::Hashes
                    }
                }
                Some(cond) if cond.test(value) => Pick::Section(first, cond.only_negative()),
                Some(cond) => Pick::General(cond.else_drops_sign()),
            }
        }
        2 => {
            let (first, second) = (&sections[0], &sections[1]);
            match (first.cond, second.cond) {
                (None, None) if value >= 0.0 => Pick::Section(first, false),
                (None, None) => Pick::Section(second, true),
                (Some(cond), None) if cond.test(value) => Pick::Section(first, drops(first)),
                (Some(cond), None) => Pick::Section(second, cond.else_drops_sign()),
                (None, Some(cond)) => {
                    if value > 0.0 {
                        Pick::Section(first, false)
                    } else if cond.test(value) {
                        Pick::Section(second, cond.only_negative())
                    } else {
                        Pick::Hashes
                    }
                }
                (Some(a), Some(b)) => {
                    if a.test(value) {
                        Pick::Section(first, a.only_negative())
                    } else if b.test(value) {
                        Pick::Section(second, b.only_negative())
                    } else {
                        Pick::Hashes
                    }
                }
            }
        }
        _ => {
            let first = &sections[0];
            let in_first = first.cond.map_or(value > 0.0, |cond| cond.test(value));
            if in_first {
                return Pick::Section(first, drops(first));
            }
            let second = &sections[1];
            match second.cond {
                None if value < 0.0 => Pick::Section(second, true),
                Some(cond) if cond.test(value) => Pick::Section(second, cond.only_negative()),
                _ => Pick::Section(&sections[2], false),
            }
        }
    }
}

fn number_value(code: &Code, value: f64, ctx: &Ctx<'_>, out: &mut String, display: &mut Display) {
    if !value.is_finite() {
        display.hashes = true;
        return;
    }
    match pick(code, value) {
        Pick::Hashes => display.hashes = true,
        Pick::General(drop_sign) => {
            if value < 0.0 && !drop_sign {
                out.push('-');
            }
            let decimal = &ctx.options.locale.decimal;
            write_general(value, decimal, ctx.shape, out);
            display.general = true;
        }
        Pick::Section(section, drop_sign) => {
            display.color = section.color;
            let sign = value < 0.0 && !drop_sign;
            // A negative number that rounds to zero is treated as its positive self, conditions
            // included.
            if let Kind::Number(number) = &section.kind {
                if sign && number.fraction.is_none() && !number.scientific {
                    let mut rounded = Decimal::new(value);
                    rounded.shift(number.scale);
                    rounded.round(number.frac_places);
                    if rounded.is_zero() {
                        display.color = None;
                        return number_value(code, -value, ctx, out, display);
                    }
                }
            }
            match &section.kind {
                Kind::Literal | Kind::Text => {
                    if sign {
                        out.push('-');
                    }
                    write_literals(&section.items, ctx, out, display);
                }
                Kind::General => {
                    if sign {
                        out.push('-');
                    }
                    for item in &section.items {
                        match item {
                            Item::General => {
                                let decimal = &ctx.options.locale.decimal;
                                write_general(value, decimal, ctx.shape, out);
                                display.general = true;
                            }
                            other => write_literal(other, ctx, out, display),
                        }
                    }
                }
                Kind::Number(number) => match number.fraction {
                    Some(fraction) => {
                        write_fraction(section, number, fraction, value, sign, ctx, out, display)
                    }
                    None if number.scientific => {
                        write_scientific(section, number, value, sign, ctx, out, display)
                    }
                    None => write_plain(section, number, value, sign, ctx, out, display),
                },
                Kind::Date(info) => write_date(section, *info, value, sign, ctx, out, display),
                Kind::System(system) => {
                    let locale = ctx.options.locale;
                    let text;
                    let source = match system {
                        System::ShortDate => &locale.short_date,
                        System::LongDate => &locale.long_date,
                        System::LongTime => &locale.long_time,
                        System::ShortDateTime => {
                            text = format!("{} h:mm", locale.short_date);
                            &text
                        }
                    };
                    let color = display.color;
                    if let Some(form) = parse_code(source) {
                        *display = format(&form, Value::Number(value), ctx.options, out);
                    }
                    display.color = color.or(display.color);
                }
            }
        }
    }
}

fn write_literal(item: &Item, ctx: &Ctx<'_>, out: &mut String, display: &mut Display) {
    match item {
        Item::Lit(text) => out.push_str(text),
        Item::Fill(fill) => display.fill = Some((*fill, out.len())),
        Item::Currency => out.push_str(&ctx.options.locale.currency),
        _ => {}
    }
}

fn write_literals(items: &[Item], ctx: &Ctx<'_>, out: &mut String, display: &mut Display) {
    for item in items {
        write_literal(item, ctx, out, display);
    }
}

/// Puts a number's whole digits into its placeholders, right to left. Digits beyond the first
/// placeholder all come out there.
struct Whole<'a> {
    number: &'a Decimal,
    places: i32,
    digits: i32,
    seen: i32,
    printed: bool,
    grouping: bool,
    group: &'a str,
    shape: Shape,
}

impl<'a> Whole<'a> {
    fn new(number: &'a Decimal, places: i32, grouping: bool, ctx: &'a Ctx<'_>) -> Self {
        Self {
            number,
            places,
            digits: number.int_len(),
            seen: 0,
            printed: false,
            grouping,
            group: &ctx.options.locale.group,
            shape: ctx.shape,
        }
    }

    fn separator(&self, power: i32) -> bool {
        self.grouping && power > 0 && power % 3 == 0
    }

    fn overflow(&mut self, out: &mut String) {
        for power in (self.places..self.digits).rev() {
            self.shape.push(out, self.number.digit(power));
            self.printed = true;
            if self.separator(power) {
                out.push_str(self.group);
            }
        }
    }

    fn place(&mut self, place: Place, out: &mut String) {
        let power = self.places - 1 - self.seen;
        if self.seen == 0 {
            self.overflow(out);
        }
        self.seen += 1;
        if power < self.digits {
            self.shape.push(out, self.number.digit(power));
            self.printed = true;
        } else {
            match place {
                Place::Zero => {
                    self.shape.push(out, 0);
                    self.printed = true;
                }
                Place::Space => out.push(' '),
                Place::Hash => {}
            }
        }
        if self.separator(power) {
            if self.printed {
                out.push_str(self.group);
            } else if place == Place::Space {
                out.push(' ');
            }
        }
    }
}

fn write_frac_digit(number: &Decimal, index: i32, place: Place, shape: Shape, out: &mut String) {
    let power = -(index + 1);
    if power >= number.lowest() && !number.is_zero() {
        shape.push(out, number.digit(power));
    } else {
        match place {
            Place::Zero => shape.push(out, 0),
            Place::Space => out.push(' '),
            Place::Hash => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_plain(
    section: &Section,
    spec: &Number,
    value: f64,
    sign: bool,
    ctx: &Ctx<'_>,
    out: &mut String,
    display: &mut Display,
) {
    let mut number = Decimal::new(value);
    number.shift(spec.scale);
    number.round(spec.frac_places);
    if sign && !number.is_zero() {
        out.push('-');
    }
    let mut whole = Whole::new(&number, spec.int_places, spec.grouping, ctx);
    let mut past_point = false;
    let mut index = 0;
    for item in &section.items {
        match item {
            Item::Digit(place) if past_point => {
                write_frac_digit(&number, index, *place, ctx.shape, out);
                index += 1;
            }
            Item::Digit(place) => whole.place(*place, out),
            Item::Point => {
                if spec.int_places == 0 {
                    whole.overflow(out);
                }
                out.push_str(&ctx.options.locale.decimal);
                past_point = true;
            }
            other => write_literal(other, ctx, out, display),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_scientific(
    section: &Section,
    spec: &Number,
    value: f64,
    sign: bool,
    ctx: &Ctx<'_>,
    out: &mut String,
    display: &mut Display,
) {
    let mut number = Decimal::new(value);
    let zero = number.is_zero();
    let places = spec.int_places;
    let mut exponent = 0;
    if !zero {
        let natural = number.point() - 1;
        exponent = if places >= 1 {
            natural.div_euclid(places) * places
        } else {
            natural + 1
        };
        number.shift(-exponent);
        number.round(spec.frac_places);
        // Rounding can carry into a new digit: 9.99 at one decimal is 1.0E+1.
        if number.point() > places {
            let step = places.max(1);
            exponent += step;
            number.shift(-step);
        }
    }
    if sign && !zero {
        out.push('-');
    }
    let mut whole = Whole::new(&number, places, spec.grouping, ctx);
    let mut past_point = false;
    let mut index = 0;
    for item in &section.items {
        match item {
            // With a value of zero every whole placeholder shows a zero: `##0.0E+0` gives 000.0E+0.
            Item::Digit(place) if !past_point => {
                whole.place(if zero { Place::Zero } else { *place }, out)
            }
            Item::Digit(place) => {
                write_frac_digit(&number, index, *place, ctx.shape, out);
                index += 1;
            }
            Item::Point => {
                if places == 0 {
                    whole.overflow(out);
                }
                out.push_str(&ctx.options.locale.decimal);
                past_point = true;
            }
            Item::Exp { plus, width } => {
                out.push('E');
                if exponent < 0 {
                    out.push('-');
                } else if *plus {
                    out.push('+');
                }
                let size = exponent.unsigned_abs() as u64;
                ctx.shape.push_u64(out, size, *width as usize);
            }
            other => write_literal(other, ctx, out, display),
        }
    }
}

/// Excel's fraction: the last continued fraction convergent with a denominator up to `max`. So
/// 0.1 at one digit is 0, since 1/10 doesn't fit and 1/9 is no convergent of it.
fn best_fraction(value: f64, max: u64) -> (u64, u64) {
    let (mut p0, mut q0, mut p1, mut q1) = (0u64, 1u64, 1u64, 0u64);
    let mut rest = value;
    for _ in 0..64 {
        let term = rest.floor();
        let (p, q) = (term as u64 * p1 + p0, term as u64 * q1 + q0);
        if q > max {
            return (p1, q1.max(1));
        }
        (p0, q0, p1, q1) = (p1, q1, p, q);
        let left = rest - term;
        if left < 5e-8 {
            return (p1, q1.max(1));
        }
        rest = 1.0 / left;
    }
    (p1, q1.max(1))
}

fn count_chars(items: &[Item]) -> usize {
    items
        .iter()
        .map(|item| match item {
            Item::Lit(text) => text.chars().count(),
            _ => 0,
        })
        .sum()
}

fn places_of(items: &[Item]) -> impl DoubleEndedIterator<Item = Place> + '_ {
    items.iter().filter_map(|item| match item {
        Item::Digit(place) => Some(*place),
        _ => None,
    })
}

#[allow(clippy::too_many_arguments)]
fn write_fraction(
    section: &Section,
    spec: &Number,
    fraction: Fraction,
    value: f64,
    sign: bool,
    ctx: &Ctx<'_>,
    out: &mut String,
    display: &mut Display,
) {
    let items = &section.items;
    let num_items = &items[fraction.num_start..fraction.num_end];
    let den_items = &items[fraction.den_start..fraction.den_end];
    let magnitude = value.abs() * 10f64.powi(spec.scale);
    if !magnitude.is_finite() {
        display.hashes = true;
        return;
    }
    let mut whole = magnitude.floor();
    let rest = magnitude - whole;
    let (mut n, d) = match fraction.den {
        Den::Fixed(den) => {
            // In decimal, so that 0.285 of 100 is 29.
            let mut count = Decimal::new(rest * den as f64);
            count.round(0);
            let mut n = 0;
            for power in (0..count.int_len()).rev() {
                n = n * 10 + count.digit(power) as u64;
            }
            (n, den as u64)
        }
        Den::Places => {
            let max = 10u64.pow(den_items.len().min(9) as u32) - 1;
            best_fraction(rest, max)
        }
    };
    if n >= d {
        whole += 1.0;
        n = 0;
    }
    let d = if n == 0 && fraction.den == Den::Places {
        1
    } else {
        d
    };

    let has_zero = places_of(num_items).any(|place| place == Place::Zero);
    if fraction.mixed && n == 0 && !has_zero {
        // No fraction to show: its characters become spaces, if `?` asks for them.
        if sign {
            out.push('-');
        }
        let shown = write_whole_part(items, spec, fraction, whole, ctx, out, display);
        if whole == 0.0 && !shown {
            // Excel writes this zero without the tag's numeral shape.
            out.push('0');
        }
        let spaces = places_of(num_items)
            .chain(places_of(den_items))
            .any(|place| place == Place::Space);
        if spaces {
            let count = count_chars(&items[fraction.whole_end..fraction.num_start])
                + num_items.len()
                + count_chars(&items[fraction.num_end..fraction.slash])
                + 1
                + count_chars(&items[fraction.slash + 1..fraction.den_start])
                + den_items.len();
            out.extend(std::iter::repeat_n(' ', count));
        }
        write_literals(&items[fraction.den_end..], ctx, out, display);
        return;
    }

    let numerator = if fraction.mixed {
        Decimal::new(n as f64)
    } else {
        // Excel only has room for so large a numerator.
        let top = whole * d as f64 + n as f64;
        let too_large = match fraction.den {
            Den::Fixed(_) => top > 32_767.0,
            Den::Places => magnitude >= 2_147_483_647.0,
        };
        if too_large {
            display.hashes = true;
            return;
        }
        Decimal::new(top)
    };
    if sign && (fraction.mixed || !numerator.is_zero()) {
        out.push('-');
    }
    if fraction.mixed {
        let shown = write_whole_part(items, spec, fraction, whole, ctx, out, display);
        let keep = shown || places_of(num_items).next() == Some(Place::Space);
        if keep {
            write_literals(
                &items[fraction.whole_end..fraction.num_start],
                ctx,
                out,
                display,
            );
        }
    } else {
        write_literals(&items[..fraction.num_start], ctx, out, display);
    }
    // The numerator lines up on the right, and always shows when it is the whole value.
    let count = num_items.len() as i32;
    let mut top = Whole::new(&numerator, count, false, ctx);
    for (index, place) in places_of(num_items).enumerate() {
        let last = index as i32 == count - 1;
        let place = if !fraction.mixed && numerator.is_zero() && last {
            Place::Zero
        } else {
            place
        };
        top.place(place, out);
    }
    write_literals(&items[fraction.num_end..fraction.slash], ctx, out, display);
    out.push('/');
    write_literals(
        &items[fraction.slash + 1..fraction.den_start],
        ctx,
        out,
        display,
    );
    match fraction.den {
        Den::Fixed(den) => ctx.shape.push_u64(out, den as u64, 0),
        Den::Places => {
            // The denominator lines up on the left, unless it pads with zeros.
            let shown = Decimal::new(d as f64);
            let digits = shown.int_len() as usize;
            if places_of(den_items).any(|place| place == Place::Zero) {
                let mut bottom = Whole::new(&shown, den_items.len() as i32, false, ctx);
                for place in places_of(den_items) {
                    bottom.place(place, out);
                }
            } else {
                ctx.shape.push_u64(out, d, 0);
                for place in places_of(den_items).skip(digits) {
                    if place == Place::Space {
                        out.push(' ');
                    }
                }
            }
        }
    }
    write_literals(&items[fraction.den_end..], ctx, out, display);
}

/// Everything up to the end of the whole number. Says whether the number showed.
fn write_whole_part(
    items: &[Item],
    spec: &Number,
    fraction: Fraction,
    whole: f64,
    ctx: &Ctx<'_>,
    out: &mut String,
    display: &mut Display,
) -> bool {
    let number = Decimal::new(whole);
    let mut digits = Whole::new(&number, spec.int_places, spec.grouping, ctx);
    for item in &items[..fraction.whole_end] {
        match item {
            Item::Digit(place) => digits.place(*place, out),
            other => write_literal(other, ctx, out, display),
        }
    }
    digits.printed
}

/// Which calendar a code's dates are in, and whose names they use.
struct Calendar<'a> {
    kind: CalendarKind,
    table: Option<&'static Names>,
    am_pm: Option<&'static Names>,
    locale: &'a Locale,
    named_a_p: bool,
    real_weekdays: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CalendarKind {
    Excel,
    System,
    Hijri,
    Japanese,
    Thai,
}

impl<'a> Calendar<'a> {
    fn new(tag: Tag, locale: &'a Locale) -> Self {
        let lcid = tag.lcid;
        let language = (lcid != 0).then(|| names_for(lcid));
        let japanese = tag.is_japanese();
        let (kind, table) = match tag.calendar {
            0 if tag.hijri_prefix => (CalendarKind::Hijri, Some(hijri_names_for(lcid))),
            0 | 1 => (CalendarKind::Excel, language),
            2 => (CalendarKind::Excel, Some(&ENGLISH)),
            3 => (
                CalendarKind::Japanese,
                if japanese { language } else { Some(&ENGLISH) },
            ),
            6 | 0x0e | 0x17 => (CalendarKind::Hijri, Some(hijri_names_for(lcid))),
            7 => (CalendarKind::Thai, language),
            0x0a => (CalendarKind::System, Some(&ARABIC_LEVANT)),
            0x09 | 0x0b | 0x0c => (CalendarKind::System, language),
            _ => (CalendarKind::Excel, language),
        };
        Self {
            kind,
            table,
            am_pm: language,
            locale,
            named_a_p: lcid != 0,
            // Plain `b2` counts weekdays from the real calendar. With a language they are Excel's.
            real_weekdays: tag.hijri_prefix && lcid == 0,
        }
    }

    fn month(&self, index: usize) -> &str {
        match self.table {
            Some(table) => table.months[index],
            None => &self.locale.months[index],
        }
    }

    fn month_short(&self, index: usize) -> &str {
        match self.table {
            Some(table) => table.months_short[index],
            None => &self.locale.months_short[index],
        }
    }

    fn day(&self, index: usize) -> &str {
        match self.table {
            Some(table) => table.days[index],
            None => &self.locale.days[index],
        }
    }

    fn day_short(&self, index: usize) -> &str {
        match self.table {
            Some(table) => table.days_short[index],
            None => &self.locale.days_short[index],
        }
    }

    fn am_pm(&self, pm: bool) -> &str {
        match (self.am_pm, pm) {
            (Some(names), false) => names.am,
            (Some(names), true) => names.pm,
            (None, false) => &self.locale.am,
            (None, true) => &self.locale.pm,
        }
    }

    fn date(&self, whole: i64, date1904: bool) -> Option<Day> {
        match self.kind {
            CalendarKind::Excel | CalendarKind::Japanese | CalendarKind::Thai => {
                Some(excel_day(whole, date1904))
            }
            CalendarKind::System => system_day(whole, date1904),
            CalendarKind::Hijri => {
                let day = hijri_day(whole, date1904)?;
                Some(if self.real_weekdays {
                    Day {
                        weekday: real_weekday(whole, date1904),
                        ..day
                    }
                } else {
                    day
                })
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_date(
    section: &Section,
    info: DateInfo,
    value: f64,
    sign: bool,
    ctx: &Ctx<'_>,
    out: &mut String,
    display: &mut Display,
) {
    let date1904 = ctx.options.date1904;
    let max = if date1904 { MAX_1904 } else { MAX_1900 };
    let locale = ctx.options.locale;
    let magnitude = value.abs();
    // In the 1904 system a negative date shows as its distance from day 0, with a minus sign.
    if value < 0.0 && !info.elapsed_only && !date1904 {
        display.hashes = true;
        return;
    }
    let large = magnitude >= 1e11;
    let (whole, units) = if large {
        (i64::MAX, 0)
    } else {
        split_time(magnitude, info.places)
    };
    if whole > max && !info.elapsed_only {
        display.hashes = true;
        return;
    }
    let calendar = Calendar::new(ctx.tag, locale);
    let day = if whole <= max {
        match calendar.date(whole, date1904) {
            Some(day) => day,
            None => {
                display.hashes = true;
                return;
            }
        }
    } else {
        Day {
            year: 0,
            month: 1,
            day: 1,
            weekday: 0,
        }
    };
    let japanese = calendar.kind == CalendarKind::Japanese || ctx.tag.is_japanese();

    if sign {
        out.push('-');
    }
    let scale = 10u64.pow(info.places);
    let seconds = units / scale;
    let hours = seconds / 3600;
    let shape = ctx.shape;
    let pair = |number: u64, letters: u8, out: &mut String| {
        shape.push_u64(out, number, letters as usize);
    };
    for item in &section.items {
        let Item::Date(part) = item else {
            match item {
                Item::DateSep => out.push_str(&locale.date_separator),
                Item::TimeSep => out.push_str(&locale.time_separator),
                other => write_literal(other, ctx, out, display),
            }
            continue;
        };
        match *part {
            Part::Year(letters) => {
                let year = match calendar.kind {
                    CalendarKind::Japanese => japanese_era(&day, 1).0,
                    CalendarKind::Thai => day.year + 543,
                    _ => day.year,
                } as i64;
                if calendar.kind == CalendarKind::Japanese {
                    pair(year as u64, 2, out);
                } else if letters == 2 {
                    pair(year.rem_euclid(100) as u64, 2, out);
                } else {
                    pair(year as u64, 4, out);
                }
            }
            Part::Month(letters) => {
                let index = (day.month as usize).saturating_sub(1).min(11);
                match letters {
                    1 | 2 => pair(day.month as u64, letters, out),
                    3 => out.push_str(calendar.month_short(index)),
                    4 => out.push_str(calendar.month(index)),
                    _ => out.extend(calendar.month(index).chars().next()),
                }
            }
            Part::Day(letters) => match letters {
                1 | 2 => pair(day.day as u64, letters, out),
                3 => out.push_str(calendar.day_short(day.weekday as usize)),
                _ => out.push_str(calendar.day(day.weekday as usize)),
            },
            Part::Hour(letters) => {
                let shown = if info.twelve {
                    match hours % 12 {
                        0 => 12,
                        other => other,
                    }
                } else {
                    hours
                };
                pair(shown, letters, out);
            }
            Part::Minute(letters) => pair(seconds / 60 % 60, letters, out),
            Part::Second(letters) => pair(seconds % 60, letters, out),
            Part::SubSecond(places) => {
                out.push_str(&locale.decimal);
                let fraction = units % scale;
                let drop = 10u64.pow(info.places - places as u32);
                pair(fraction / drop, places, out);
            }
            Part::AmPm => out.push_str(calendar.am_pm(hours >= 12)),
            Part::AP(a, p) => {
                let pm = hours >= 12;
                if calendar.named_a_p {
                    out.push_str(calendar.am_pm(pm));
                } else {
                    out.push(if pm { p } else { a });
                }
            }
            Part::ElapsedHours(width) => write_elapsed(
                magnitude, large, whole, units, 3600, scale, width, shape, out,
            ),
            Part::ElapsedMinutes(width) => {
                write_elapsed(magnitude, large, whole, units, 60, scale, width, shape, out)
            }
            Part::ElapsedSeconds(width) => {
                write_elapsed(magnitude, large, whole, units, 1, scale, width, shape, out)
            }
            Part::EraYear(letters) => {
                if japanese {
                    pair(japanese_era(&day, 1).0 as u64, letters, out);
                } else {
                    pair(day.year as u64, 4, out);
                }
            }
            Part::Era(letters) => {
                if japanese {
                    out.push_str(japanese_era(&day, letters as usize).1);
                }
            }
            Part::BuddhistYear(letters) => {
                // With a tag, b is just the year.
                let year = if ctx.tag.present {
                    day.year as i64
                } else {
                    day.year as i64 + 543
                };
                if letters == 2 {
                    pair(year.rem_euclid(100) as u64, 2, out);
                } else {
                    pair(year as u64, 4, out);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_elapsed(
    magnitude: f64,
    large: bool,
    whole: i64,
    units: u64,
    per: u64,
    scale: u64,
    width: u8,
    shape: Shape,
    out: &mut String,
) {
    if large {
        let number = Decimal::new(magnitude * (86_400 / per) as f64);
        if number.is_zero() {
            shape.push_u64(out, 0, width as usize);
        }
        for power in (0..number.int_len()).rev() {
            shape.push(out, number.digit(power));
        }
        return;
    }
    let seconds = whole as u64 * 86_400 + units / scale;
    shape.push_u64(out, seconds / per, width as usize);
}

/// The formula bar's number: 15 significant digits, in plain form up to 21 characters.
pub(crate) fn edit_number(value: f64, locale: &Locale, out: &mut String) {
    if value < 0.0 {
        out.push('-');
    }
    let number = Decimal::new(value);
    if number.is_zero() {
        out.push('0');
        return;
    }
    let exponent = number.point() - 1;
    let shape = Shape::default();
    let decimals = (-number.lowest()).max(0);
    let plain_length = number.point().max(1) + if decimals > 0 { decimals + 1 } else { 0 };
    if plain_length > 21 {
        shape.push(out, number.digit(exponent));
        if number.len() > 1 {
            out.push_str(&locale.decimal);
            for power in (number.lowest()..exponent).rev() {
                shape.push(out, number.digit(power));
            }
        }
        out.push('E');
        out.push(if exponent < 0 { '-' } else { '+' });
        shape.push_u64(out, exponent.unsigned_abs() as u64, 2);
        return;
    }
    if number.point() <= 0 {
        out.push('0');
    } else {
        for power in (0..number.point()).rev() {
            shape.push(out, number.digit(power));
        }
    }
    let lowest = number.lowest();
    if lowest < 0 {
        out.push_str(&locale.decimal);
        for power in (lowest..0).rev() {
            shape.push(out, number.digit(power));
        }
    }
}

/// The formula bar's date: the locale's short date, then the long time if there is a time part.
/// A serial under 1 in a format with no date in it is a time alone. Says false for a serial with
/// no date.
pub(crate) fn edit_date(
    value: f64,
    time_only: bool,
    options: &Options<'_>,
    out: &mut String,
) -> bool {
    let locale = options.locale;
    let max = if options.date1904 { MAX_1904 } else { MAX_1900 };
    if value < 0.0 || value.floor() as i64 > max {
        return false;
    }
    let (whole, units) = split_time(value, 0);
    let source = if whole == 0 && time_only {
        locale.long_time.clone()
    } else if units != 0 {
        format!("{} {}", locale.short_date, locale.long_time)
    } else {
        locale.short_date.clone()
    };
    match parse_code(&source) {
        Some(form) => {
            format(&form, Value::Number(value), options, out);
            true
        }
        None => false,
    }
}
