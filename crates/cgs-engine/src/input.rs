//! What typing into a cell means, as Excel reads it: a formula, a number, a date or text, and which
//! number format the cell should take on if it has none of its own. This follows the user's
//! regional conventions. Formulas are always in English with comma separators, as files store them.

use cgs_calc::CalcValue;
use cgs_numfmt::Locale;

/// What a cell's text comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Typed {
    Clear,
    Value(CalcValue),
    /// With no leading `=`.
    Formula(String),
    /// A value with the format it was typed in, which a General cell takes on.
    Formatted(f64, Format),
}

/// The format typed text suggests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// A built-in format's id.
    Builtin(u32),
}

const ERRORS: &[&str] = &[
    "#NULL!",
    "#DIV/0!",
    "#VALUE!",
    "#REF!",
    "#NAME?",
    "#NUM!",
    "#N/A",
    "#GETTING_DATA",
];

/// Reads what someone typed. `today` is today's date serial, for dates with no year.
pub fn parse_input(text: &str, locale: &Locale, date1904: bool, today: f64) -> Typed {
    if text.is_empty() {
        return Typed::Clear;
    }
    if let Some(rest) = text.strip_prefix('\'') {
        return Typed::Value(CalcValue::Text(rest.to_string()));
    }
    if let Some(rest) = text.strip_prefix('=') {
        if !rest.is_empty() {
            return Typed::Formula(rest.trim_start().to_string());
        }
        return Typed::Value(CalcValue::Text("=".to_string()));
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Typed::Value(CalcValue::Text(text.to_string()));
    }
    if trimmed.eq_ignore_ascii_case("TRUE") {
        return Typed::Value(CalcValue::Bool(true));
    }
    if trimmed.eq_ignore_ascii_case("FALSE") {
        return Typed::Value(CalcValue::Bool(false));
    }
    let upper = trimmed.to_ascii_uppercase();
    if let Some(&error) = ERRORS.iter().find(|&&error| error == upper) {
        return Typed::Value(CalcValue::Error(error.to_string()));
    }
    if let Some(number) = number(trimmed, locale) {
        return number;
    }
    if let Some(moment) = date_time(trimmed, locale, date1904, today) {
        return moment;
    }
    // `+A1` and `-A1` are formulas. `-` or `+` alone are text, as is anything that can't be read.
    if (trimmed.starts_with('+') || trimmed.starts_with('-')) && trimmed.len() > 1 {
        let rest = &trimmed[1..];
        if rest
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '(' | '$' | '-' | '+'))
        {
            return Typed::Formula(trimmed.to_string());
        }
    }
    Typed::Value(CalcValue::Text(text.to_string()))
}

/// A number, with whatever its spelling says about its format.
fn number(text: &str, locale: &Locale) -> Option<Typed> {
    let mut body = text;
    let mut negative = false;
    if let Some(inner) = body
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        // Brackets already say it's negative, so `(-1)` isn't a number.
        if inner.trim_start().starts_with('-') {
            return None;
        }
        negative = true;
        body = inner.trim();
    }
    if let Some(rest) = body.strip_prefix('-') {
        negative = !negative;
        body = rest.trim_start();
    } else if let Some(rest) = body.strip_prefix('+') {
        body = rest.trim_start();
    }
    let mut percent = false;
    if let Some(rest) = body.strip_suffix('%') {
        percent = true;
        body = rest.trim_end();
    } else if let Some(rest) = body.strip_prefix('%') {
        percent = true;
        body = rest.trim_start();
    }
    let mut currency = false;
    for symbol in [locale.currency.as_str(), "$"] {
        if symbol.is_empty() {
            continue;
        }
        if let Some(rest) = body.strip_prefix(symbol) {
            currency = true;
            body = rest.trim_start();
            break;
        }
    }
    if currency && percent {
        return None;
    }
    if currency {
        if let Some(rest) = body.strip_prefix('-') {
            negative = !negative;
            body = rest.trim_start();
        }
    }
    // A fraction with a whole part, `1 1/2`. A plain `1/2` is a date.
    if let Some((whole, fraction)) = body.split_once(' ') {
        if let (Ok(whole), Some((top, bottom))) =
            (whole.trim().parse::<u64>(), fraction.trim().split_once('/'))
        {
            if let (Ok(top), Ok(bottom)) = (top.parse::<u64>(), bottom.parse::<u64>()) {
                if bottom > 0 {
                    let value = whole as f64 + top as f64 / bottom as f64;
                    let value = if negative { -value } else { value };
                    let code = if bottom < 10 { "# ?/?" } else { "# ??/??" };
                    return Some(Typed::Formatted(
                        value,
                        Format::Builtin(if code == "# ?/?" { 12 } else { 13 }),
                    ));
                }
            }
        }
    }
    let (plain, grouped) = plain_number(body, locale)?;
    let value: f64 = plain.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    // Too small to hold, which Excel leaves as text rather than calling it zero.
    if value == 0.0
        && plain
            .split(['e', 'E'])
            .next()
            .is_some_and(|m| m.bytes().any(|b| matches!(b, b'1'..=b'9')))
    {
        return None;
    }
    let mut value = if negative { -value } else { value };
    if percent {
        value /= 100.0;
    }
    let decimals = plain.split_once('.').map_or(0, |(_, rest)| {
        rest.bytes().take_while(u8::is_ascii_digit).count()
    });
    let exponent = plain.contains('e') || plain.contains('E');
    Some(if exponent {
        Typed::Formatted(value, Format::Builtin(11))
    } else if percent {
        Typed::Formatted(value, Format::Builtin(if decimals > 0 { 10 } else { 9 }))
    } else if currency {
        Typed::Formatted(value, Format::Builtin(if decimals > 0 { 8 } else { 6 }))
    } else if grouped {
        Typed::Formatted(value, Format::Builtin(if decimals > 0 { 4 } else { 3 }))
    } else {
        Typed::Value(CalcValue::Number(value))
    })
}

/// Takes the user's separators out, and says whether it had group separators. As in Excel, a group
/// can be long but not short: `1,2345` and `1000,000` are numbers, and `1,23` is not.
fn plain_number(text: &str, locale: &Locale) -> Option<(String, bool)> {
    let decimal = locale.decimal.chars().next().unwrap_or('.');
    let group = locale.group.chars().next().unwrap_or(',');
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    let (whole, fraction) = match mantissa.split_once(decimal) {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (mantissa, None),
    };
    if whole.is_empty() && fraction.is_none_or(str::is_empty) {
        return None;
    }
    let grouped = whole.contains(group);
    let mut digits = String::with_capacity(text.len());
    if grouped {
        let mut parts = whole.split(group);
        let first = parts.next()?;
        if first.is_empty() || !first.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.push_str(first);
        for part in parts {
            if part.len() < 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            digits.push_str(part);
        }
    } else {
        if !whole.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.push_str(whole);
    }
    if let Some(fraction) = fraction {
        if !fraction.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.push('.');
        digits.push_str(fraction);
    }
    if let Some(exponent) = exponent {
        let body = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.push('e');
        digits.push_str(exponent);
    }
    Some((digits, grouped))
}

/// Days from 1900's day 0 as Excel counts them, counting its leap day that never was.
pub fn date_serial(year: i32, month: u32, day: u32, date1904: bool) -> Option<f64> {
    if !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    let days_in = |year: i32, month: u32| -> u32 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            _ if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
            _ => 28,
        }
    };
    if !(1900..=9999).contains(&year) {
        return None;
    }
    if year == 1900 && month == 2 && day == 29 {
        return (!date1904).then_some(60.0);
    }
    if day > days_in(year, month) {
        return None;
    }
    // Civil days since 1970, then shifted to Excel's count.
    let adjusted = i64::from(if month <= 2 { year - 1 } else { year });
    let era = adjusted.div_euclid(400);
    let of_era = adjusted - era * 400;
    let shifted = i64::from(if month > 2 { month - 3 } else { month + 9 });
    let of_year = (153 * shifted + 2) / 5 + i64::from(day) - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    let days = era * 146_097 + of_cycle - 719_468;
    let mut serial = days + 25_569;
    if serial < 61 {
        serial -= 1;
    }
    if date1904 {
        serial -= 1462;
        if serial < 0 {
            return None;
        }
    }
    Some(serial as f64)
}

fn year_of(serial: f64, date1904: bool) -> i32 {
    // Good enough for the year of today's date.
    let days = serial as i64 + if date1904 { 1462 } else { 0 } - 25_569 + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days - era * 146_097;
    let of_cycle = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * of_cycle + of_cycle / 4 - of_cycle / 100);
    let shifted = (5 * of_year + 2) / 153;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    (of_cycle + era * 400 + i64::from(month <= 2)) as i32
}

fn month_named(word: &str, locale: &Locale) -> Option<u32> {
    let lower = word.trim_end_matches('.').to_lowercase();
    if lower.is_empty() {
        return None;
    }
    let english = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    for (index, name) in english.iter().enumerate() {
        if *name == lower || (lower.len() == 3 && name.starts_with(&lower)) {
            return Some(index as u32 + 1);
        }
    }
    for (index, (long, short)) in locale.months.iter().zip(&locale.months_short).enumerate() {
        if long.to_lowercase() == lower || short.to_lowercase() == lower {
            return Some(index as u32 + 1);
        }
    }
    None
}

/// Which order the user's short dates go in, from their format: month first unless it says not.
fn day_comes_first(locale: &Locale) -> bool {
    let code = locale.short_date.to_lowercase();
    match (code.find('d'), code.find('m')) {
        (Some(day), Some(month)) => day < month,
        _ => false,
    }
}

fn year_comes_first(locale: &Locale) -> bool {
    let code = locale.short_date.to_lowercase();
    match (code.find('y'), code.find('m')) {
        (Some(year), Some(month)) => year < month,
        _ => false,
    }
}

fn full_year(text: &str) -> Option<i32> {
    let number: i32 = text.parse().ok()?;
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(match text.len() {
        1 | 2 => {
            if number < 30 {
                2000 + number
            } else {
                1900 + number
            }
        }
        4 => number,
        _ => return None,
    })
}

/// A date's value and format, from its words and numbers.
fn date_only(text: &str, locale: &Locale, date1904: bool, today: f64) -> Option<(f64, Format)> {
    let current = year_of(today, date1904);
    let parts: Vec<&str> = text
        .split(|c: char| c == '/' || c == '-' || c == ',' || c.is_whitespace() || c == '.')
        .filter(|part| !part.is_empty())
        .collect();
    // A comma only separates a day from a year after a month's name.
    if text.contains(',') && !parts.iter().any(|part| month_named(part, locale).is_some()) {
        return None;
    }
    let uses_only_dots = text.contains('.') && !text.contains('/') && !text.contains('-');
    if uses_only_dots && !locale.short_date.contains('.') {
        return None;
    }
    let number = |part: &str| -> Option<u32> {
        (part.bytes().all(|b| b.is_ascii_digit()) && part.len() <= 4)
            .then(|| part.parse().ok())
            .flatten()
    };
    // Numbers alone only stand for a date with marks between them, so `1 1/2` is a fraction or text.
    let has_month = parts.iter().any(|part| month_named(part, locale).is_some());
    if !has_month && text.contains(char::is_whitespace) {
        return None;
    }
    match parts.as_slice() {
        [a, b] => {
            if let Some(month) = month_named(a, locale) {
                // `Jan 5` or `Jan-2020`.
                if b.len() == 4 || number(b)? > 31 {
                    let year = full_year(b)?;
                    return Some((date_serial(year, month, 1, date1904)?, Format::Builtin(17)));
                }
                return Some((
                    date_serial(current, month, number(b)?, date1904)?,
                    Format::Builtin(16),
                ));
            }
            if let Some(month) = month_named(b, locale) {
                return Some((
                    date_serial(current, month, number(a)?, date1904)?,
                    Format::Builtin(16),
                ));
            }
            let (x, y) = (number(a)?, number(b)?);
            // Two numbers: month and day in the user's order, or a month and a year.
            let (month, day) = if day_comes_first(locale) {
                (y, x)
            } else {
                (x, y)
            };
            if let Some(serial) = date_serial(current, month, day, date1904) {
                return Some((serial, Format::Builtin(16)));
            }
            if (1..=12).contains(&x) {
                let year = full_year(b)?;
                return Some((date_serial(year, x, 1, date1904)?, Format::Builtin(17)));
            }
            None
        }
        [a, b, c] => {
            if let Some(month) = month_named(a, locale) {
                // `Mar 4, 2021`: the comma is what Excel goes by.
                if !text.contains(',') {
                    return None;
                }
                return Some((
                    date_serial(full_year(c)?, month, number(b)?, date1904)?,
                    Format::Builtin(15),
                ));
            }
            if let Some(month) = month_named(b, locale) {
                return Some((
                    date_serial(full_year(c)?, month, number(a)?, date1904)?,
                    Format::Builtin(15),
                ));
            }
            // A four-digit first part is a year whatever the conventions.
            let (year, month, day) = if a.len() == 4 || year_comes_first(locale) {
                (full_year(a)?, number(b)?, number(c)?)
            } else if day_comes_first(locale) {
                (full_year(c)?, number(b)?, number(a)?)
            } else {
                (full_year(c)?, number(a)?, number(b)?)
            };
            Some((
                date_serial(year, month, day, date1904)?,
                Format::Builtin(14),
            ))
        }
        _ => None,
    }
}

/// A time of day as a fraction, with its format. Excel reads `1:60` as the time it comes to, but
/// doesn't call it a time, so that takes no format.
fn time_only(text: &str, locale: &Locale) -> Option<(f64, Option<Format>)> {
    let lower = text.to_lowercase();
    let am = locale.am.to_lowercase();
    let pm = locale.pm.to_lowercase();
    let (clock, half) = if let Some(rest) = lower.strip_suffix(&pm).filter(|_| !pm.is_empty()) {
        (rest.trim_end().to_string(), Some(true))
    } else if let Some(rest) = lower.strip_suffix(&am).filter(|_| !am.is_empty()) {
        (rest.trim_end().to_string(), Some(false))
    } else if let Some(rest) = lower.strip_suffix("pm") {
        (rest.trim_end().to_string(), Some(true))
    } else if let Some(rest) = lower.strip_suffix("am") {
        (rest.trim_end().to_string(), Some(false))
    } else {
        (lower.clone(), None)
    };
    let mut parts = clock.split(':');
    let hours: f64 = parts.next()?.trim().parse().ok()?;
    let minutes: f64 = match parts.next() {
        Some(part) => part.trim().parse().ok()?,
        // Only `3 PM` goes without minutes.
        None => {
            half?;
            0.0
        }
    };
    let seconds_text = parts.next();
    let seconds: f64 = match seconds_text {
        Some(part) => part.trim().parse().ok()?,
        None => 0.0,
    };
    if parts.next().is_some() || hours < 0.0 || hours.fract() != 0.0 || hours >= 10_000.0 {
        return None;
    }
    // `12:30.5` is minutes and seconds, down to a fraction of a second.
    if minutes.fract() != 0.0 {
        if half.is_some() || seconds_text.is_some() || !(0.0..60.0).contains(&minutes) {
            return None;
        }
        return Some((
            (hours * 60.0 + minutes) / 86_400.0,
            Some(Format::Builtin(47)),
        ));
    }
    if minutes < 0.0 || seconds < 0.0 {
        return None;
    }
    if half.is_none() && (minutes >= 60.0 || seconds >= 60.0) {
        return Some(((hours * 3600.0 + minutes * 60.0 + seconds) / 86_400.0, None));
    }
    if minutes >= 60.0 || seconds >= 60.0 {
        return None;
    }
    let hours = match half {
        Some(pm) => {
            if !(1.0..=12.0).contains(&hours) {
                return None;
            }
            (hours % 12.0) + if pm { 12.0 } else { 0.0 }
        }
        None => hours,
    };
    let value = (hours * 3600.0 + minutes * 60.0 + seconds) / 86_400.0;
    // Past a day, it's a length of time rather than a time of day.
    if half.is_none() && hours >= 24.0 {
        return Some((value, Some(Format::Builtin(46))));
    }
    if half.is_none() && seconds.fract() != 0.0 {
        return Some((value, Some(Format::Builtin(47))));
    }
    let format = match (half.is_some(), seconds_text.is_some()) {
        (true, true) => 19,
        (true, false) => 18,
        (false, true) => 21,
        (false, false) => 20,
    };
    Some((value, Some(Format::Builtin(format))))
}

fn date_time(text: &str, locale: &Locale, date1904: bool, today: f64) -> Option<Typed> {
    // A letter that isn't part of a month or AM/PM, or an unusual character, rules a date out fast.
    if !text.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if let Some((value, format)) = time_only(text, locale) {
        return Some(match format {
            Some(format) => Typed::Formatted(value, format),
            None => Typed::Value(CalcValue::Number(value)),
        });
    }
    if let Some((value, format)) = date_only(text, locale, date1904, today) {
        return Some(Typed::Formatted(value, format));
    }
    // A date and then a time.
    let at = text.rfind(' ')?;
    for split in [at, text[..at].rfind(' ').unwrap_or(at)] {
        let (date, time) = text.split_at(split);
        if let (Some((day, _)), Some((clock, _))) = (
            date_only(date.trim(), locale, date1904, today),
            time_only(time.trim(), locale),
        ) {
            return Some(Typed::Formatted(day + clock, Format::Builtin(22)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: f64 = 46_331.0; // 2026-11-05

    /// Each read off Excel by `bench/run-excel-input.mjs`.
    #[test]
    fn the_odd_ones_excel_reads_its_own_way() {
        assert_eq!(formatted("1,2345"), (12_345.0, Format::Builtin(3)));
        assert_eq!(formatted("1000,000"), (1_000_000.0, Format::Builtin(3)));
        is_text("1,23");
        is_text("12,34,56");
        assert_eq!(formatted("%5"), (0.05, Format::Builtin(9)));
        assert_eq!(formatted("1e3%"), (10.0, Format::Builtin(11)));
        assert_eq!(formatted("$1e3"), (1000.0, Format::Builtin(11)));
        assert_eq!(formatted("$5"), (5.0, Format::Builtin(6)));
        assert_eq!(formatted("$5.50"), (5.5, Format::Builtin(8)));
        is_text("$5%");
        is_text("(-1)");
        is_text("1E-400");
        is_text("1 1/0");
        is_text("mar 4 2021");
        assert_eq!(formatted("1/0"), (36_526.0, Format::Builtin(17)));
        assert_eq!(formatted("25:00").1, Format::Builtin(46));
        assert_eq!(formatted("12:30.5").1, Format::Builtin(47));
        assert_eq!(formatted("1:02:03.5").1, Format::Builtin(47));
        assert!((plain("1:60") - 2.0 / 24.0).abs() < 1e-12);
        is_text("10000:00");
    }

    fn read(text: &str) -> Typed {
        parse_input(text, &Locale::en_us(), false, TODAY)
    }

    fn formatted(text: &str) -> (f64, Format) {
        match read(text) {
            Typed::Formatted(value, format) => (value, format),
            other => panic!("{text}: {other:?}"),
        }
    }

    fn plain(text: &str) -> f64 {
        match read(text) {
            Typed::Value(CalcValue::Number(value)) => value,
            other => panic!("{text}: {other:?}"),
        }
    }

    fn is_text(text: &str) {
        assert_eq!(
            read(text),
            Typed::Value(CalcValue::Text(text.to_string())),
            "{text}"
        );
    }

    #[test]
    fn plain_numbers() {
        assert_eq!(plain("12"), 12.0);
        assert_eq!(plain("-12.5"), -12.5);
        assert_eq!(plain("+3"), 3.0);
        assert_eq!(plain(".5"), 0.5);
        assert_eq!(plain("5."), 5.0);
        assert_eq!(plain(" 7 "), 7.0);
        assert_eq!(plain("(5)"), -5.0);
    }

    #[test]
    fn numbers_with_a_format_in_them() {
        assert_eq!(formatted("1,234"), (1234.0, Format::Builtin(3)));
        assert_eq!(formatted("1,234.5"), (1234.5, Format::Builtin(4)));
        assert_eq!(formatted("50%"), (0.5, Format::Builtin(9)));
        assert_eq!(formatted("12.5%"), (0.125, Format::Builtin(10)));
        assert_eq!(formatted("1e3"), (1000.0, Format::Builtin(11)));
        assert_eq!(formatted("1 1/2"), (1.5, Format::Builtin(12)));
        assert_eq!(formatted("$5").0, 5.0);
        assert_eq!(formatted("-$5.50").0, -5.5);
    }

    #[test]
    fn text_that_only_looks_close() {
        for text in ["1,23", "abc", "12a", "1.2.3", "$", "%", "-", "+", "1e"] {
            is_text(text);
        }
    }

    #[test]
    fn formulas_booleans_errors_and_quotes() {
        assert_eq!(read("=1+1"), Typed::Formula("1+1".to_string()));
        assert_eq!(read("+A1"), Typed::Formula("+A1".to_string()));
        assert_eq!(read("-A1"), Typed::Formula("-A1".to_string()));
        assert_eq!(read("--5"), Typed::Formula("--5".to_string()));
        assert_eq!(read("true"), Typed::Value(CalcValue::Bool(true)));
        assert_eq!(read("False"), Typed::Value(CalcValue::Bool(false)));
        assert_eq!(
            read("#n/a"),
            Typed::Value(CalcValue::Error("#N/A".to_string()))
        );
        assert_eq!(read("'12"), Typed::Value(CalcValue::Text("12".to_string())));
        assert_eq!(read(""), Typed::Clear);
    }

    #[test]
    fn dates_in_the_users_order() {
        assert_eq!(formatted("1/2/2020"), (43_832.0, Format::Builtin(14)));
        assert_eq!(formatted("12/31/99"), (36_525.0, Format::Builtin(14)));
        assert_eq!(formatted("2020-01-02"), (43_832.0, Format::Builtin(14)));
        assert_eq!(formatted("1/2"), (46_024.0, Format::Builtin(16)));
        assert_eq!(formatted("Jan 2"), (46_024.0, Format::Builtin(16)));
        assert_eq!(formatted("2 Jan"), (46_024.0, Format::Builtin(16)));
        assert_eq!(formatted("Jan 2020"), (43_831.0, Format::Builtin(17)));
        assert_eq!(formatted("Jan 2, 2020"), (43_832.0, Format::Builtin(15)));
        assert_eq!(formatted("2-Jan-20"), (43_832.0, Format::Builtin(15)));
        // 13 can't be a month, and 31 isn't a day in February.
        is_text("13/13/2020");
        is_text("2/31/2020");
    }

    #[test]
    fn dates_where_the_day_comes_first() {
        let mut locale = Locale::en_us();
        locale.short_date = "dd/mm/yyyy".to_string();
        assert_eq!(
            parse_input("2/1/2020", &locale, false, TODAY),
            Typed::Formatted(43_832.0, Format::Builtin(14))
        );
    }

    #[test]
    fn excels_day_numbers() {
        assert_eq!(date_serial(1900, 1, 1, false), Some(1.0));
        assert_eq!(date_serial(1900, 2, 28, false), Some(59.0));
        assert_eq!(date_serial(1900, 2, 29, false), Some(60.0));
        assert_eq!(date_serial(1900, 3, 1, false), Some(61.0));
        assert_eq!(date_serial(2026, 11, 5, false), Some(46_331.0));
        assert_eq!(date_serial(2026, 11, 5, true), Some(44_869.0));
        assert_eq!(year_of(46_331.0, false), 2026);
        assert_eq!(year_of(44_869.0, true), 2026);
    }

    #[test]
    fn times() {
        assert_eq!(formatted("12:30"), (12.5 / 24.0, Format::Builtin(20)));
        assert_eq!(formatted("1:30 PM").1, Format::Builtin(18));
        assert_eq!(formatted("1:30 pm").0, 13.5 / 24.0);
        assert_eq!(formatted("12:00 AM").0, 0.0);
        assert_eq!(formatted("3 PM").0, 15.0 / 24.0);
        assert_eq!(formatted("1:02:03").1, Format::Builtin(21));
        assert_eq!(formatted("1/2/2020 12:00").0, 43_832.5);
        assert_eq!(formatted("1/2/2020 12:00").1, Format::Builtin(22));
        assert!((plain("1:99") - 2.65 / 24.0).abs() < 1e-12);
    }
}
