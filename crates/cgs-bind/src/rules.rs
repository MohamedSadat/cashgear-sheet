//! Local rules (plan B.3 and E.4): the schema's constraints, checked as the user edits, on the
//! fields that differ from what the ERP sent. The server checks only changed fields too, so a
//! record may keep a value the rules would refuse, such as an inactive code (contract, "Commit").
//! Phase 17's Validate adds the server's own checks.

use serde::Serialize;

use crate::guard::Content;
use crate::schema::{BoundField, FieldKind, LookupEntry};
use crate::value::{CellValue, MAX_DIGITS};

/// A broken rule, named by the contract's error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum Rule {
    /// Empty, where the field is required.
    #[serde(rename = "required")]
    Required,
    /// Longer than the field's `maxLength`, counted as the ERP counts: UTF-16 code units.
    #[serde(rename = "maxLength")]
    MaxLength,
    /// Not the field's type: text in a number, a time in a date, an error value.
    #[serde(rename = "type")]
    Type,
    /// Outside the field's `min` and `max`.
    #[serde(rename = "range")]
    Range,
    /// More decimals than the field's scale, or more digits than a cell's double keeps exactly.
    #[serde(rename = "scale")]
    Scale,
    /// Not an active code of the field's list.
    #[serde(rename = "lookup.notFound")]
    Lookup,
}

impl Rule {
    /// The contract's error code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::MaxLength => "maxLength",
            Self::Type => "type",
            Self::Range => "range",
            Self::Scale => "scale",
            Self::Lookup => "lookup.notFound",
        }
    }
}

/// The largest whole number a double holds exactly, and so the largest integer a cell keeps.
const MAX_SAFE: f64 = 9_007_199_254_740_991.0;

/// Excel's last date, 9999-12-31.
const LAST_SERIAL: f64 = 2_958_465.0;

/// A number's significant digits and its digits after the point, as its shortest exact text has
/// them. Rust writes a double in plain decimal with the fewest digits that read back to it.
pub fn digits(n: f64) -> (usize, usize) {
    let text = format!("{}", n.abs());
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let significant = whole
        .bytes()
        .chain(fraction.bytes())
        .skip_while(|&b| b == b'0')
        .count();
    // Trailing zeros of a whole number aren't significant.
    let significant = if fraction.is_empty() {
        significant - whole.bytes().rev().take_while(|&b| b == b'0').count()
    } else {
        significant
    };
    (significant, fraction.len())
}

fn text_of(now: &Content) -> Option<&str> {
    match now {
        Content::Text(text) if !text.is_empty() => Some(text),
        _ => None,
    }
}

fn is_empty(now: &Content) -> bool {
    matches!(now, Content::Empty) || matches!(now, Content::Text(text) if text.is_empty())
}

fn in_range(field: &BoundField, n: f64) -> bool {
    field.min.is_none_or(|min| n >= min) && field.max.is_none_or(|max| n <= max)
}

/// The rule a field's value breaks, if any. `original` is what the ERP sent, which a lookup may
/// keep though it is no longer active. Formulas and spills aren't checked: they are left out of a
/// commit until converted to values.
pub fn check(
    field: &BoundField,
    now: &Content,
    original: &CellValue,
    list: Option<&[LookupEntry]>,
) -> Option<Rule> {
    if matches!(now, Content::Formula(_) | Content::Spill) {
        return None;
    }
    if is_empty(now) {
        return field.required.then_some(Rule::Required);
    }
    if matches!(now, Content::Error(_)) {
        return Some(Rule::Type);
    }
    match &field.kind {
        FieldKind::Text => {
            let text = text_of(now)?;
            if !matches!(now, Content::Text(_)) {
                return Some(Rule::Type);
            }
            let length = text.encode_utf16().count() as u32;
            field
                .max_length
                .is_some_and(|max| length > max)
                .then_some(Rule::MaxLength)
        }
        FieldKind::Lookup(_) => {
            let Some(text) = text_of(now) else {
                return Some(Rule::Type);
            };
            if field
                .max_length
                .is_some_and(|max| text.encode_utf16().count() as u32 > max)
            {
                return Some(Rule::MaxLength);
            }
            let active = list
                .unwrap_or_default()
                .iter()
                .any(|item| item.active && item.code == text);
            let kept = matches!(original, CellValue::Text(old) if old == text);
            (!active && !kept).then_some(Rule::Lookup)
        }
        FieldKind::Integer => {
            let Content::Number(bits) = now else {
                return Some(Rule::Type);
            };
            let n = f64::from_bits(*bits);
            if n.fract() != 0.0 || n.abs() > MAX_SAFE {
                return Some(Rule::Type);
            }
            (!in_range(field, n)).then_some(Rule::Range)
        }
        FieldKind::Decimal(scale) => {
            let Content::Number(bits) = now else {
                return Some(Rule::Type);
            };
            let n = f64::from_bits(*bits);
            if !n.is_finite() {
                return Some(Rule::Type);
            }
            let (significant, decimals) = digits(n);
            if significant > MAX_DIGITS || decimals > *scale as usize {
                return Some(Rule::Scale);
            }
            (!in_range(field, n)).then_some(Rule::Range)
        }
        FieldKind::Boolean => (!matches!(now, Content::Bool(_))).then_some(Rule::Type),
        FieldKind::Date => {
            let Content::Number(bits) = now else {
                return Some(Rule::Type);
            };
            let n = f64::from_bits(*bits);
            (n.fract() != 0.0 || !(1.0..=LAST_SERIAL).contains(&n)).then_some(Rule::Type)
        }
        FieldKind::DateTime => {
            let Content::Number(bits) = now else {
                return Some(Rule::Type);
            };
            let n = f64::from_bits(*bits);
            (!(1.0..LAST_SERIAL + 1.0).contains(&n)).then_some(Rule::Type)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(kind: FieldKind) -> BoundField {
        BoundField {
            name: "f".into(),
            label: "F".into(),
            kind,
            editable: true,
            edit_reason: None,
            required: false,
            max_length: None,
            min: None,
            max: None,
        }
    }

    fn text(s: &str) -> Content {
        Content::Text(s.into())
    }

    const NONE: &CellValue = &CellValue::Empty;

    #[test]
    fn required_and_lengths_count_as_the_erp_does() {
        let mut name = field(FieldKind::Text);
        name.required = true;
        name.max_length = Some(5);
        assert_eq!(
            check(&name, &Content::Empty, NONE, None),
            Some(Rule::Required)
        );
        assert_eq!(check(&name, &text(""), NONE, None), Some(Rule::Required));
        assert_eq!(check(&name, &text("12345"), NONE, None), None);
        assert_eq!(
            check(&name, &text("123456"), NONE, None),
            Some(Rule::MaxLength)
        );
        // Arabic letters are one code unit each; a symbol beyond the BMP is two.
        assert_eq!(check(&name, &text("النيل"), NONE, None), None);
        assert_eq!(
            check(&name, &text("ab😀cd"), NONE, None),
            Some(Rule::MaxLength)
        );
        name.required = false;
        assert_eq!(check(&name, &Content::Empty, NONE, None), None);
        assert_eq!(
            check(&name, &Content::Error("#N/A".into()), NONE, None),
            Some(Rule::Type)
        );
    }

    #[test]
    fn formulas_are_left_to_converting() {
        let mut name = field(FieldKind::Text);
        name.required = true;
        assert_eq!(
            check(&name, &Content::Formula("=A1".into()), NONE, None),
            None
        );
        assert_eq!(check(&name, &Content::Spill, NONE, None), None);
    }

    #[test]
    fn lookups_take_active_codes_and_keep_an_old_one() {
        let list = [
            LookupEntry {
                code: "NET30".into(),
                label: "30 days".into(),
                active: true,
            },
            LookupEntry {
                code: "OLD".into(),
                label: "Old".into(),
                active: false,
            },
        ];
        let pay = field(FieldKind::Lookup("paymentModes".into()));
        assert_eq!(check(&pay, &text("NET30"), NONE, Some(&list)), None);
        assert_eq!(
            check(&pay, &text("NET60"), NONE, Some(&list)),
            Some(Rule::Lookup)
        );
        assert_eq!(
            check(&pay, &text("OLD"), NONE, Some(&list)),
            Some(Rule::Lookup)
        );
        assert_eq!(
            check(
                &pay,
                &text("OLD"),
                &CellValue::Text("OLD".into()),
                Some(&list)
            ),
            None
        );
        assert_eq!(
            check(&pay, &Content::number(30.0), NONE, Some(&list)),
            Some(Rule::Type)
        );
    }

    #[test]
    fn numbers_keep_their_scale_range_and_digits() {
        let mut limit = field(FieldKind::Decimal(2));
        limit.min = Some(0.0);
        assert_eq!(check(&limit, &Content::number(15000.5), NONE, None), None);
        assert_eq!(check(&limit, &Content::number(0.07), NONE, None), None);
        assert_eq!(
            check(&limit, &Content::number(1.005), NONE, None),
            Some(Rule::Scale)
        );
        assert_eq!(
            check(&limit, &Content::number(-1.0), NONE, None),
            Some(Rule::Range)
        );
        assert_eq!(
            check(&limit, &Content::number(1234567890123456.0), NONE, None),
            Some(Rule::Scale)
        );
        assert_eq!(
            check(&limit, &Content::number(123456789012345.0), NONE, None),
            None
        );
        assert_eq!(check(&limit, &text("12"), NONE, None), Some(Rule::Type));
        let count = field(FieldKind::Integer);
        assert_eq!(check(&count, &Content::number(3.0), NONE, None), None);
        assert_eq!(
            check(&count, &Content::number(3.5), NONE, None),
            Some(Rule::Type)
        );
    }

    #[test]
    fn booleans_and_dates_need_their_types() {
        let blocked = field(FieldKind::Boolean);
        assert_eq!(check(&blocked, &Content::Bool(true), NONE, None), None);
        assert_eq!(check(&blocked, &text("yes"), NONE, None), Some(Rule::Type));
        let since = field(FieldKind::Date);
        assert_eq!(check(&since, &Content::number(45292.0), NONE, None), None);
        assert_eq!(
            check(&since, &Content::number(45292.5), NONE, None),
            Some(Rule::Type)
        );
        assert_eq!(
            check(&since, &text("2024-01-01"), NONE, None),
            Some(Rule::Type)
        );
    }

    #[test]
    fn digits_count_as_a_cell_shows_them() {
        assert_eq!(digits(15000.0), (2, 0));
        assert_eq!(digits(0.07), (1, 2));
        assert_eq!(digits(1234.5), (5, 1));
        assert_eq!(digits(0.1 + 0.2), (17, 17));
    }
}
