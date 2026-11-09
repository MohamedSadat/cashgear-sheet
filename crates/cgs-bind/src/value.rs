//! Wire values to cell values, by field type (contract, "Values on the wire"). Text stays text,
//! decimals keep their scale and refuse more digits than a cell's double holds, dates become date
//! serials and `dateTime`s the wall-clock time in the ERP host's own offset.

use crate::schema::FieldKind;
use crate::wire::WireValue;

/// What a bound cell holds. Empty is no cell at all.
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Empty,
    Text(String),
    Number(f64),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueError {
    /// Not the field's type on the wire.
    Type,
    /// More significant digits than a cell's double keeps exactly.
    Digits,
    /// Not a date that exists, or outside Excel's years.
    Date,
}

impl ValueError {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Type => "isn't the type the schema gives it",
            Self::Digits => "has more digits than a cell holds",
            Self::Date => "isn't a date a sheet can show",
        }
    }
}

/// Excel keeps 15 significant digits, so a decimal with more would change on the way in.
pub const MAX_DIGITS: usize = 15;

pub fn cell_value(kind: &FieldKind, wire: Option<&WireValue>) -> Result<CellValue, ValueError> {
    let Some(wire) = wire else {
        return Ok(CellValue::Empty);
    };
    match (kind, wire) {
        (FieldKind::Text | FieldKind::Lookup(_), WireValue::Text(text)) => Ok(if text.is_empty() {
            CellValue::Empty
        } else {
            CellValue::Text(text.clone())
        }),
        (FieldKind::Integer, WireValue::Number(n)) => {
            if n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 {
                Ok(CellValue::Number(*n))
            } else {
                Err(ValueError::Type)
            }
        }
        (FieldKind::Decimal(scale), WireValue::Text(text)) => decimal(text, *scale),
        (FieldKind::Boolean, WireValue::Bool(b)) => Ok(CellValue::Bool(*b)),
        (FieldKind::Date, WireValue::Text(text)) => date(text).map(CellValue::Number),
        (FieldKind::DateTime, WireValue::Text(text)) => date_time(text).map(CellValue::Number),
        _ => Err(ValueError::Type),
    }
}

fn decimal(text: &str, scale: u32) -> Result<CellValue, ValueError> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = match digits.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (digits, None),
    };
    let plain = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !plain(whole) || (whole.len() > 1 && whole.starts_with('0')) {
        return Err(ValueError::Type);
    }
    if let Some(fraction) = fraction {
        if !plain(fraction) || fraction.len() > scale as usize {
            return Err(ValueError::Type);
        }
    }
    let significant = whole
        .bytes()
        .chain(fraction.unwrap_or("").bytes())
        .skip_while(|&b| b == b'0')
        .count();
    if significant > MAX_DIGITS {
        return Err(ValueError::Digits);
    }
    text.parse::<f64>()
        .map(CellValue::Number)
        .map_err(|_| ValueError::Type)
}

/// Days from 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        _ => 28,
    }
}

fn number(text: &str) -> Option<i64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `YYYY-MM-DD` as an Excel date serial in the 1900 date system, which counts 1900-02-29.
pub fn date(text: &str) -> Result<f64, ValueError> {
    let mut parts = text.split('-');
    let (Some(y), Some(m), Some(d), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(ValueError::Date);
    };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return Err(ValueError::Date);
    }
    let (Some(year), Some(month), Some(day)) = (number(y), number(m), number(d)) else {
        return Err(ValueError::Date);
    };
    if !(1900..=9999).contains(&year) || !(1..=12).contains(&month) {
        return Err(ValueError::Date);
    }
    if day < 1 || day > days_in(year, month) {
        return Err(ValueError::Date);
    }
    // Excel's day 1 is 1900-01-01, and it counts a 1900-02-29 that never was.
    let serial = days_from_civil(year, month, day) - days_from_civil(1899, 12, 31);
    Ok((if serial >= 60 { serial + 1 } else { serial }) as f64)
}

/// The clock part of an ISO 8601 instant, without its zone, or `None` if the zone is missing.
fn strip_zone(rest: &str) -> Option<&str> {
    if let Some(clock) = rest.strip_suffix('Z') {
        return Some(clock);
    }
    let cut = rest.len().checked_sub(6)?;
    if !rest.is_char_boundary(cut) {
        return None;
    }
    let (clock, zone) = rest.split_at(cut);
    let b = zone.as_bytes();
    let ok = (b[0] == b'+' || b[0] == b'-')
        && b[3] == b':'
        && number(&zone[1..3]).is_some_and(|h| h <= 14)
        && number(&zone[4..6]).is_some_and(|m| m < 60);
    ok.then_some(clock)
}

/// An ISO 8601 instant with an offset, as the wall-clock time it names in that offset: the ERP
/// writes its host's local time and shows it so (decisions, 2026-11-09, Phase 14).
pub fn date_time(text: &str) -> Result<f64, ValueError> {
    let (day, rest) = text.split_once('T').ok_or(ValueError::Date)?;
    let serial = date(day)?;
    let clock = strip_zone(rest).ok_or(ValueError::Date)?;
    let (hms, fraction) = match clock.split_once('.') {
        Some((hms, fraction)) => (hms, Some(fraction)),
        None => (clock, None),
    };
    let mut parts = hms.split(':');
    let (Some(h), Some(m), Some(s), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(ValueError::Date);
    };
    if h.len() != 2 || m.len() != 2 || s.len() != 2 {
        return Err(ValueError::Date);
    }
    let (Some(h), Some(m), Some(s)) = (number(h), number(m), number(s)) else {
        return Err(ValueError::Date);
    };
    if h > 23 || m > 59 || s > 59 {
        return Err(ValueError::Date);
    }
    let mut seconds = (h * 3600 + m * 60 + s) as f64;
    if let Some(fraction) = fraction {
        if fraction.len() > 7 || number(fraction).is_none() {
            return Err(ValueError::Date);
        }
        seconds += format!("0.{fraction}").parse::<f64>().unwrap_or(0.0);
    }
    Ok(serial + seconds / 86_400.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> WireValue {
        WireValue::Text(s.to_string())
    }

    #[test]
    fn text_stays_text_with_its_leading_zeros() {
        for s in ["01001234567", "001", "0045", "مؤسسة النور للتوريدات"] {
            assert_eq!(
                cell_value(&FieldKind::Text, Some(&text(s))),
                Ok(CellValue::Text(s.to_string()))
            );
        }
        assert_eq!(
            cell_value(&FieldKind::Text, Some(&text(""))),
            Ok(CellValue::Empty)
        );
        assert_eq!(cell_value(&FieldKind::Text, None), Ok(CellValue::Empty));
        assert_eq!(
            cell_value(&FieldKind::Lookup("x".into()), Some(&text("NET30"))),
            Ok(CellValue::Text("NET30".into()))
        );
        assert_eq!(
            cell_value(&FieldKind::Text, Some(&WireValue::Number(1.0))),
            Err(ValueError::Type)
        );
    }

    #[test]
    fn decimals_keep_their_scale_and_refuse_what_a_double_loses() {
        let d = FieldKind::Decimal(2);
        let num = |s: &str| cell_value(&d, Some(&text(s)));
        assert_eq!(num("15000.00"), Ok(CellValue::Number(15000.0)));
        assert_eq!(num("2500.5"), Ok(CellValue::Number(2500.5)));
        assert_eq!(num("-0.05"), Ok(CellValue::Number(-0.05)));
        assert_eq!(
            num("1234567890123.45"),
            Ok(CellValue::Number(1234567890123.45))
        );
        assert_eq!(num("12345678901234.56"), Err(ValueError::Digits));
        for bad in ["1.234", "01.00", "1e3", "", ".5", "5.", "\u{661}\u{662}"] {
            assert_eq!(num(bad), Err(ValueError::Type), "{bad}");
        }
        assert_eq!(
            cell_value(&d, Some(&WireValue::Number(1.0))),
            Err(ValueError::Type)
        );
    }

    #[test]
    fn dates_are_excels_serials() {
        assert_eq!(date("1900-01-01"), Ok(1.0));
        assert_eq!(date("1900-02-28"), Ok(59.0));
        assert_eq!(date("1900-03-01"), Ok(61.0));
        assert_eq!(date("2026-11-09"), Ok(46335.0));
        for bad in [
            "2026-02-29",
            "2026-13-01",
            "1899-12-31",
            "2026-1-01",
            "2026-11-09T00:00:00Z",
        ] {
            assert_eq!(date(bad), Err(ValueError::Date), "{bad}");
        }
    }

    #[test]
    fn date_times_keep_their_wall_clock() {
        let noon = 46335.5;
        assert_eq!(date_time("2026-11-09T12:00:00Z"), Ok(noon));
        assert_eq!(date_time("2026-11-09T12:00:00+02:00"), Ok(noon));
        assert_eq!(date_time("2026-11-09T12:00:00-05:30"), Ok(noon));
        let expected = 46325.0 + (8.0 * 3600.0 + 12.0 * 60.0 + 0.5) / 86400.0;
        assert!((date_time("2026-10-30T08:12:00.5Z").unwrap() - expected).abs() < 1e-9);
        for bad in [
            "2026-11-09T12:00:00",
            "2026-11-09 12:00:00Z",
            "2026-11-09T24:00:00Z",
            "2026-11-09T12:00Z",
            "2026-11-09T12:00:00.Z",
        ] {
            assert_eq!(date_time(bad), Err(ValueError::Date), "{bad}");
        }
    }

    #[test]
    fn booleans_and_integers() {
        assert_eq!(
            cell_value(&FieldKind::Boolean, Some(&WireValue::Bool(true))),
            Ok(CellValue::Bool(true))
        );
        assert_eq!(
            cell_value(&FieldKind::Boolean, Some(&text("true"))),
            Err(ValueError::Type)
        );
        assert_eq!(
            cell_value(&FieldKind::Integer, Some(&WireValue::Number(42.0))),
            Ok(CellValue::Number(42.0))
        );
        assert_eq!(
            cell_value(&FieldKind::Integer, Some(&WireValue::Number(4.2))),
            Err(ValueError::Type)
        );
    }
}
