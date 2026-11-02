//! Numbers as decimal digits, which is what Excel rounds, and General's rules.

use std::fmt::Write;

/// Digits go through this when a tag asks for another numeral shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Shape {
    pub zero: Option<char>,
}

impl Shape {
    /// The zero of a Windows numeral shape, the `NN` in `[$-NNCCLLLL]`.
    pub fn from_code(code: u8) -> Self {
        let zero = match code {
            0x02 => '\u{660}',
            0x03 => '\u{6f0}',
            0x04 => '\u{966}',
            0x05 => '\u{9e6}',
            0x06 => '\u{a66}',
            0x07 => '\u{ae6}',
            0x08 => '\u{b66}',
            0x09 => '\u{be6}',
            0x0a => '\u{c66}',
            0x0b => '\u{ce6}',
            0x0c => '\u{d66}',
            0x0d => '\u{e50}',
            0x0e => '\u{ed0}',
            0x0f => '\u{f20}',
            0x10 => '\u{1040}',
            0x12 => '\u{17e0}',
            0x13 => '\u{1810}',
            _ => return Self::default(),
        };
        Self { zero: Some(zero) }
    }

    pub fn push(self, out: &mut String, digit: u8) {
        match self.zero {
            Some(zero) => out.push(char::from_u32(zero as u32 + digit as u32).unwrap_or(zero)),
            None => out.push((b'0' + digit) as char),
        }
    }

    pub fn push_u64(self, out: &mut String, value: u64, width: usize) {
        let mut digits = [0u8; 20];
        let mut count = 0;
        let mut rest = value;
        while rest > 0 {
            digits[count] = (rest % 10) as u8;
            rest /= 10;
            count += 1;
        }
        for _ in count..width.max(1) {
            self.push(out, 0);
        }
        for at in (0..count).rev() {
            self.push(out, digits[at]);
        }
    }
}

struct Stack {
    bytes: [u8; 32],
    len: usize,
}

impl Write for Stack {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let end = self.len + text.len();
        if end > self.bytes.len() {
            return Err(std::fmt::Error);
        }
        self.bytes[self.len..end].copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// A number's magnitude as Excel holds it for display: 15 significant digits. Rounding works on
/// these, half away from zero, so 1.005 shows as 1.01 whatever its binary value.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Decimal {
    digits: [u8; 15],
    /// Digits in use, with no trailing zeros. Zero means the value is zero.
    len: usize,
    /// The value is 0.d1d2d3... times ten to this power.
    point: i32,
}

impl Decimal {
    pub fn new(value: f64) -> Self {
        let mut stack = Stack {
            bytes: [0; 32],
            len: 0,
        };
        let mut this = Self {
            digits: [0; 15],
            len: 0,
            point: 0,
        };
        if !value.is_finite() || write!(stack, "{:.14e}", value.abs()).is_err() {
            return this;
        }
        // The text is d.dddddddddddddde[-]x.
        let text = &stack.bytes[..stack.len];
        this.digits[0] = text[0] - b'0';
        for at in 0..14 {
            this.digits[at + 1] = text[at + 2] - b'0';
        }
        let mut exponent = 0;
        let negative = text[17] == b'-';
        for &byte in &text[if negative { 18 } else { 17 }..] {
            exponent = exponent * 10 + (byte - b'0') as i32;
        }
        this.point = if negative { -exponent } else { exponent } + 1;
        this.len = 15;
        this.trim();
        this
    }

    fn trim(&mut self) {
        while self.len > 0 && self.digits[self.len - 1] == 0 {
            self.len -= 1;
        }
        if self.len == 0 {
            self.point = 0;
        }
    }

    pub fn is_zero(&self) -> bool {
        self.len == 0
    }

    /// Multiplies by a power of ten.
    pub fn shift(&mut self, by: i32) {
        if self.len > 0 {
            self.point += by;
        }
    }

    /// Keeps this many digits after the decimal point.
    pub fn round(&mut self, places: i32) {
        self.round_digits(self.point + places);
    }

    /// Keeps this many significant digits.
    pub fn round_digits(&mut self, keep: i32) {
        if self.len == 0 || keep >= self.len as i32 {
            return;
        }
        if keep < 0 {
            self.len = 0;
            self.point = 0;
            return;
        }
        let keep = keep as usize;
        let up = self.digits[keep] >= 5;
        self.len = keep;
        if up {
            let mut at = keep;
            loop {
                if at == 0 {
                    self.digits[0] = 1;
                    self.len = 1;
                    self.point += 1;
                    break;
                }
                at -= 1;
                if self.digits[at] < 9 {
                    self.digits[at] += 1;
                    self.len = at + 1;
                    break;
                }
            }
        }
        self.trim();
    }

    /// The digit worth this power of ten: 0 for units, -1 for tenths.
    pub fn digit(&self, power: i32) -> u8 {
        let at = self.point - 1 - power;
        if at < 0 || at >= self.len as i32 {
            0
        } else {
            self.digits[at as usize]
        }
    }

    /// How many digits sit before the decimal point.
    pub fn int_len(&self) -> i32 {
        self.point.max(0)
    }

    /// The power of ten of the last digit that isn't zero.
    pub fn lowest(&self) -> i32 {
        self.point - self.len as i32
    }

    pub fn point(&self) -> i32 {
        self.point
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

/// General: at most 11 characters, not counting a minus sign, and scientific form beyond that.
pub(crate) fn write_general(value: f64, decimal: &str, shape: Shape, out: &mut String) {
    let mut number = Decimal::new(value);
    if number.is_zero() {
        shape.push(out, 0);
        return;
    }
    let exponent = number.point - 1;
    let fixed = match exponent {
        11.. => false,
        -9..=-5 => (-exponent - 1) + number.len as i32 <= 9,
        ..=-10 => false,
        _ => {
            number.round(if exponent >= 0 { 9 - exponent } else { 9 }.max(0));
            number.int_len() <= 11
        }
    };
    if !fixed {
        write_general_scientific(Decimal::new(value), decimal, shape, out);
        return;
    }
    if number.point <= 0 {
        shape.push(out, 0);
    } else {
        for power in (0..number.point).rev() {
            shape.push(out, number.digit(power));
        }
    }
    let lowest = number.lowest();
    if lowest < 0 {
        out.push_str(decimal);
        for power in (lowest..0).rev() {
            shape.push(out, number.digit(power));
        }
    }
}

fn write_general_scientific(mut number: Decimal, decimal: &str, shape: Shape, out: &mut String) {
    // The whole text is 11 characters, so an exponent near three digits costs a digit.
    number.round_digits(6);
    if (number.point - 1).abs() >= 99 {
        number.round_digits(5);
    }
    let exponent = number.point - 1;
    shape.push(out, number.digit(number.point - 1));
    if number.len > 1 {
        out.push_str(decimal);
        for at in 1..number.len {
            shape.push(out, number.digits[at]);
        }
    }
    out.push('E');
    out.push(if exponent < 0 { '-' } else { '+' });
    shape.push_u64(out, exponent.unsigned_abs() as u64, 2);
}
