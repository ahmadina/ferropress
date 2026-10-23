//! Float text as PHP writes it inside a `d:` token under the default `serialize_precision` (-1).
//!
//! The digits are the shortest decimal string that reads back as the same `f64`; when two strings of
//! that length read back, the one nearer the exact binary value wins, and an exact tie goes to the
//! even last digit. They are laid out in plain notation while the decimal exponent stays within
//! -3..=17 and in `d.dddE±x` notation outside it; non-finite values are written as `NAN`, `INF` and
//! `-INF`.

/// Largest decimal exponent written in plain notation.
const PLAIN_MAX_EXPONENT: i32 = 17;
/// Smallest decimal exponent written in plain notation.
const PLAIN_MIN_EXPONENT: i32 = -3;
/// Upper bound on the significant digits of a shortest round-trip `f64` (17).
const MAX_DIGITS: usize = 17;
/// Room for `-d.ddddddddddddddddde-324`.
const SCIENTIFIC_CAPACITY: usize = MAX_DIGITS + 8;

/// A finite value's significant digits and decimal exponent: `0.d1d2… × 10^point`.
#[derive(Clone, Copy)]
struct Decimal {
    negative: bool,
    digits: [u8; MAX_DIGITS],
    len: usize,
    point: i32,
}

impl Decimal {
    /// Reads Rust's `{:e}` / `{:.Ne}` rendering, `[-]d[.ddd]e[-]x`, dropping trailing zero digits.
    fn parse(text: &str) -> Self {
        let mut decimal = Self {
            negative: false,
            digits: [b'0'; MAX_DIGITS],
            len: 0,
            point: 0,
        };
        let mut exponent = 0_i32;
        let mut negative_exponent = false;
        let mut in_exponent = false;
        for byte in text.bytes() {
            match (in_exponent, byte) {
                (false, b'-') => decimal.negative = true,
                (false, b'e') => in_exponent = true,
                (false, digit @ b'0'..=b'9') if decimal.len < MAX_DIGITS => {
                    decimal.digits[decimal.len] = digit;
                    decimal.len += 1;
                }
                (true, b'-') => negative_exponent = true,
                (true, digit @ b'0'..=b'9') => {
                    exponent = exponent * 10 + i32::from(digit - b'0');
                }
                _ => {}
            }
        }
        while decimal.len > 1 && decimal.digits[decimal.len - 1] == b'0' {
            decimal.len -= 1;
        }
        decimal.point = if negative_exponent {
            -exponent
        } else {
            exponent
        } + 1;
        decimal
    }

    fn digits(&self) -> &[u8] {
        &self.digits[..self.len]
    }

    /// Whether this decimal reads back as exactly `value`.
    fn reads_back(&self, value: f64) -> bool {
        let mut text = [0_u8; SCIENTIFIC_CAPACITY];
        let mut len = 0;
        let mut push = |byte: u8| {
            text[len] = byte;
            len += 1;
        };
        if self.negative {
            push(b'-');
        }
        push(b'.');
        self.digits().iter().copied().for_each(&mut push);
        push(b'e');
        if self.point < 0 {
            push(b'-');
        }
        let magnitude = self.point.unsigned_abs();
        let mut started = false;
        for divisor in [100, 10, 1] {
            let digit = magnitude / divisor % 10;
            if started || digit != 0 || divisor == 1 {
                started = true;
                push(b'0' + u8::try_from(digit).unwrap_or_default());
            }
        }
        std::str::from_utf8(&text[..len])
            .ok()
            .and_then(|text| text.parse::<f64>().ok())
            .is_some_and(|parsed| parsed.to_bits() == value.to_bits())
    }

    /// Whether changing the last digit by one also reads back as `value`.
    fn has_round_trip_neighbor(&self, value: f64) -> bool {
        let last = self.len - 1;
        [b'1'..=b'9', b'0'..=b'8']
            .into_iter()
            .zip([-1_i8, 1])
            .any(|(range, step)| {
                range.contains(&self.digits[last]) && {
                    let mut neighbor = *self;
                    neighbor.digits[last] = self.digits[last].wrapping_add_signed(step);
                    neighbor.reads_back(value)
                }
            })
    }
}

/// Appends `value` in PHP's serialized float notation.
pub(crate) fn write_float(out: &mut Vec<u8>, value: f64) {
    if value.is_nan() {
        out.extend_from_slice(b"NAN");
        return;
    }
    if value.is_infinite() {
        out.extend_from_slice(if value < 0.0 { b"-INF" } else { b"INF" });
        return;
    }
    // `{:e}` renders the shortest round-trip digits; among those it may not pick the nearest.
    let shortest = Decimal::parse(&format!("{value:e}"));
    let decimal = if shortest.has_round_trip_neighbor(value) {
        // `{:.Ne}` is correctly rounded, ties to even.
        let nearest = Decimal::parse(&format!("{value:.*e}", shortest.len - 1));
        if nearest.len == shortest.len && nearest.reads_back(value) {
            nearest
        } else {
            shortest
        }
    } else {
        shortest
    };
    write_decimal(out, &decimal);
}

fn write_decimal(out: &mut Vec<u8>, decimal: &Decimal) {
    if decimal.negative {
        out.push(b'-');
    }
    let digits = decimal.digits();
    let point = decimal.point;
    if !(PLAIN_MIN_EXPONENT..=PLAIN_MAX_EXPONENT).contains(&point) {
        write_scientific(out, digits, point - 1);
    } else if point <= 0 {
        out.extend_from_slice(b"0.");
        out.extend(std::iter::repeat_n(b'0', point.unsigned_abs() as usize));
        out.extend_from_slice(digits);
    } else {
        let whole = point.unsigned_abs() as usize;
        if digits.len() > whole {
            out.extend_from_slice(&digits[..whole]);
            out.push(b'.');
            out.extend_from_slice(&digits[whole..]);
        } else {
            out.extend_from_slice(digits);
            out.extend(std::iter::repeat_n(b'0', whole - digits.len()));
        }
    }
}

/// Appends `d.dddE±x`, using `d.0` when there is a single digit.
fn write_scientific(out: &mut Vec<u8>, digits: &[u8], exponent: i32) {
    let (first, rest) = digits.split_at(1);
    out.extend_from_slice(first);
    out.push(b'.');
    if rest.is_empty() {
        out.push(b'0');
    } else {
        out.extend_from_slice(rest);
    }
    out.push(b'E');
    out.push(if exponent < 0 { b'-' } else { b'+' });
    crate::write::push_decimal(out, u64::from(exponent.unsigned_abs()));
}

#[cfg(test)]
mod tests {
    use super::{Decimal, write_float};
    use proptest::prelude::*;

    fn text(value: f64) -> String {
        let mut out = Vec::new();
        write_float(&mut out, value);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn layout_follows_the_decimal_exponent() {
        let cases = [
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1, "0.1"),
            (0.0001, "0.0001"),
            (0.00001, "1.0E-5"),
            (1e16, "10000000000000000"),
            (1e17, "1.0E+17"),
            (1e25, "1.0E+25"),
            (123_456_789_012_345_680.0, "1.2345678901234568E+17"),
            (0.300_000_000_000_000_04, "0.30000000000000004"),
            (5e-324, "5.0E-324"),
            (1.797_693_134_862_315_7e308, "1.7976931348623157E+308"),
            (f64::NAN, "NAN"),
            (f64::INFINITY, "INF"),
            (f64::NEG_INFINITY, "-INF"),
        ];
        for (value, expected) in cases {
            assert_eq!(text(value), expected, "{value:?}");
        }
    }

    #[test]
    fn exact_ties_take_the_even_digit() {
        // 2059555018444622.25 and 2^50 + 0.75: exactly halfway between two 17-digit candidates.
        let below_even = f64::from_bits(0x431d_449e_1514_3d39);
        let above_even = f64::from_bits(0x4310_0000_0000_0003);
        assert_eq!(text(below_even), "2059555018444622.2");
        assert_eq!(text(-below_even), "-2059555018444622.2");
        assert_eq!(text(above_even), "1125899906842624.8");
    }

    #[test]
    fn parsing_drops_trailing_zero_digits() {
        let decimal = Decimal::parse("-1.2500e-3");
        assert!(decimal.negative);
        assert_eq!(decimal.digits(), b"125");
        assert_eq!(decimal.point, -2);
        assert_eq!(Decimal::parse("0e0").digits(), b"0");
    }

    #[test]
    fn reads_back_checks_the_exact_value() {
        assert!(Decimal::parse("1e-300").reads_back(1e-300));
        assert!(Decimal::parse("-5e-324").reads_back(-5e-324));
        assert!(!Decimal::parse("1e0").reads_back(1.000_000_000_000_000_2));
        assert!(Decimal::parse("1.7976931348623157e308").reads_back(f64::MAX));
    }

    proptest! {
        #[test]
        fn finite_values_read_back_exactly(bits in any::<u64>()) {
            let value = f64::from_bits(bits);
            prop_assume!(value.is_finite());
            let parsed: f64 = text(value).parse().unwrap();
            prop_assert_eq!(parsed.to_bits(), value.to_bits());
        }
    }
}
