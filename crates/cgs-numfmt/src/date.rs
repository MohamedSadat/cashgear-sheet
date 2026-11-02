//! Date serials and the calendars Excel shows them in.

/// A serial as a calendar date and a time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTimeParts {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millisecond: u32,
}

/// The last serial with a date: 31 December 9999.
pub(crate) const MAX_1900: i64 = 2_958_465;
pub(crate) const MAX_1904: i64 = 2_957_003;

/// A date in whichever calendar, plus what stays the same across calendars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// 0 is Sunday.
    pub weekday: u32,
}

/// Days since 1 January 1970 in the proleptic Gregorian calendar.
pub(crate) fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year } as i64;
    let era = year.div_euclid(400);
    let of_era = year.rem_euclid(400);
    let shifted = if month > 2 { month - 3 } else { month + 9 } as i64;
    let of_year = (153 * shifted + 2) / 5 + day as i64 - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_cycle - 719_468
}

pub(crate) fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let of_cycle = shifted.rem_euclid(146_097);
    let of_era = (of_cycle - of_cycle / 1460 + of_cycle / 36_524 - of_cycle / 146_096) / 365;
    let of_year = of_cycle - (365 * of_era + of_era / 4 - of_era / 100);
    let index = (5 * of_year + 2) / 153;
    let day = (of_year - (153 * index + 2) / 5 + 1) as u32;
    let month = if index < 10 { index + 3 } else { index - 9 } as u32;
    let year = of_era + era * 400 + if month <= 2 { 1 } else { 0 };
    (year as i32, month, day)
}

/// The real day a whole serial stands for, as days since 1970.
fn real_days(whole: i64, date1904: bool) -> i64 {
    if date1904 {
        days_from_civil(1904, 1, 1) + whole
    } else if whole > 60 {
        days_from_civil(1899, 12, 30) + whole
    } else {
        days_from_civil(1899, 12, 31) + whole
    }
}

/// The weekday of the real day, which `b2` uses where Excel's own count is off before March 1900.
pub(crate) fn real_weekday(whole: i64, date1904: bool) -> u32 {
    let used = if !date1904 && whole == 0 { 1 } else { whole };
    (real_days(used, date1904) + 4).rem_euclid(7) as u32
}

pub(crate) fn weekday(whole: i64, date1904: bool) -> u32 {
    let shifted = if date1904 { whole + 5 } else { whole - 1 };
    shifted.rem_euclid(7) as u32
}

/// Excel's own calendar: serial 0 is 0 January 1900 and serial 60 is the 29 February 1900 that
/// never happened.
pub(crate) fn excel_day(whole: i64, date1904: bool) -> Day {
    let weekday = weekday(whole, date1904);
    if !date1904 {
        if whole == 0 {
            return Day {
                year: 1900,
                month: 1,
                day: 0,
                weekday,
            };
        }
        if whole == 60 {
            return Day {
                year: 1900,
                month: 2,
                day: 29,
                weekday,
            };
        }
    }
    let (year, month, day) = civil_from_days(real_days(whole, date1904));
    Day {
        year,
        month,
        day,
        weekday,
    }
}

/// The Gregorian calendar as Windows counts it, for a tag that names a calendar. It has no day 0
/// and no 29 February 1900: serial 0 shows as serial 1, and serial 60 has no date at all.
pub(crate) fn system_day(whole: i64, date1904: bool) -> Option<Day> {
    if !date1904 && whole == 60 {
        return None;
    }
    let weekday = weekday(whole, date1904);
    let used = if !date1904 && whole == 0 { 1 } else { whole };
    let (year, month, day) = civil_from_days(real_days(used, date1904));
    Some(Day {
        year,
        month,
        day,
        weekday,
    })
}

/// The tabular Hijri calendar with the astronomical epoch: the "Kuwaiti algorithm" Windows uses.
pub(crate) fn hijri_day(whole: i64, date1904: bool) -> Option<Day> {
    if !date1904 && whole == 60 {
        return None;
    }
    let weekday = weekday(whole, date1904);
    let used = if !date1904 && whole == 0 { 1 } else { whole };
    // Julian day number 2440588 is 1 January 1970, and 1948084 is the day before 1 Muharram 1.
    let mut z = real_days(used, date1904) + 2_440_588 - 1_948_084;
    let cycle = z.div_euclid(10_631);
    z -= cycle * 10_631;
    let year_of_cycle = (30 * z - 5).div_euclid(10_631);
    z -= (10_631 * year_of_cycle + 4).div_euclid(30);
    let month = ((2 * z + 57) / 59).min(12);
    let day = z - (59 * month / 2 - 29);
    Some(Day {
        year: (30 * cycle + year_of_cycle) as i32,
        month: month as u32,
        day: day as u32,
        weekday,
    })
}

/// A Japanese era: its first day, its letter, first character and name.
struct Era {
    start: (i32, u32, u32),
    letter: &'static str,
    short: &'static str,
    name: &'static str,
}

const ERAS: [Era; 5] = [
    Era {
        start: (2019, 5, 1),
        letter: "R",
        short: "\u{4ee4}",
        name: "\u{4ee4}\u{548c}",
    },
    Era {
        start: (1989, 1, 8),
        letter: "H",
        short: "\u{5e73}",
        name: "\u{5e73}\u{6210}",
    },
    Era {
        start: (1926, 12, 25),
        letter: "S",
        short: "\u{662d}",
        name: "\u{662d}\u{548c}",
    },
    Era {
        start: (1912, 7, 30),
        letter: "T",
        short: "\u{5927}",
        name: "\u{5927}\u{6b63}",
    },
    Era {
        start: (1868, 9, 8),
        letter: "M",
        short: "\u{660e}",
        name: "\u{660e}\u{6cbb}",
    },
];

/// The year of the Japanese era holding this date, and the era's name at this many `g`.
pub(crate) fn japanese_era(day: &Day, letters: usize) -> (i32, &'static str) {
    let days = days_from_civil(day.year, day.month, day.day.max(1));
    for era in &ERAS {
        let (year, month, first) = era.start;
        if days >= days_from_civil(year, month, first) {
            let name = match letters {
                1 => era.letter,
                2 => era.short,
                _ => era.name,
            };
            return (day.year - year + 1, name);
        }
    }
    (day.year, "")
}

/// A serial's time of day, rounded to this many decimals of a second. A carry moves the date.
pub(crate) fn split_time(serial: f64, places: u32) -> (i64, u64) {
    let scale = 10u64.pow(places);
    let whole = serial.floor();
    let units = ((serial - whole) * 86_400.0 * scale as f64 + 0.5).floor() as u64;
    if units >= 86_400 * scale {
        (whole as i64 + 1, 0)
    } else {
        (whole as i64, units)
    }
}

/// Date serial to its parts, with Excel's 1900 leap year bug: serial 60 is 29 February 1900.
pub fn serial_to_datetime(serial: f64, date1904: bool) -> Option<DateTimeParts> {
    if !serial.is_finite() || serial < 0.0 {
        return None;
    }
    let max = if date1904 { MAX_1904 } else { MAX_1900 };
    let (whole, millis) = split_time(serial, 3);
    if whole > max {
        return None;
    }
    let day = excel_day(whole, date1904);
    let seconds = millis / 1000;
    Some(DateTimeParts {
        year: day.year,
        month: day.month,
        day: day.day,
        hour: (seconds / 3600) as u32,
        minute: (seconds / 60 % 60) as u32,
        second: (seconds % 60) as u32,
        millisecond: (millis % 1000) as u32,
    })
}
