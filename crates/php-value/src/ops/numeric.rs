//! Numbers spelled by strings.

/// Bytes PHP skips around a number: space, `\t`, `\n`, `\r`, `\v` and `\f`.
const fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// The digits of `2^63`, the magnitude of the most negative integer.
const MIN_MAGNITUDE: &[u8] = b"9223372036854775808";

/// An integer or a float.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Number {
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
}

impl Number {
    /// The number as a float.
    #[expect(
        clippy::cast_precision_loss,
        reason = "PHP converts integers to the nearest float"
    )]
    pub const fn as_float(self) -> f64 {
        match self {
            Self::Int(number) => number as f64,
            Self::Float(number) => number,
        }
    }
}

impl From<i64> for Number {
    fn from(number: i64) -> Self {
        Self::Int(number)
    }
}

impl From<f64> for Number {
    fn from(number: f64) -> Self {
        Self::Float(number)
    }
}

/// Whether an integer string was too large for an integer, and on which side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    /// The string is not an integer out of range.
    None,
    /// An integer above `PHP_INT_MAX`, read as a float.
    Above,
    /// An integer below `PHP_INT_MIN`, read as a float.
    Below,
}

/// The number at the start of a string.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumericPrefix {
    /// The number.
    pub number: Number,
    /// Whether something other than whitespace follows it, making the string leading-numeric.
    pub trailing: bool,
    /// Whether the number is an integer string out of the integer range.
    pub overflow: Overflow,
}

/// The number a string starts with, as PHP reads numeric strings.
///
/// Leading whitespace is skipped; then come an optional sign and either digits, optionally followed by
/// `.` and more digits, or `.` and digits; then an optional exponent (`e` or `E`, an optional sign
/// and at least one digit). Digits alone that fit an integer give an integer, everything else a
/// float. Trailing whitespace is allowed; anything else after the number sets
/// [`NumericPrefix::trailing`]. Hexadecimal, octal and binary forms, `INF` and `NAN` are not numbers.
///
/// ```
/// use php_value::ops::{Number, numeric_prefix};
///
/// let prefix = numeric_prefix(b" 12 apples").unwrap();
/// assert_eq!((prefix.number, prefix.trailing), (Number::Int(12), true));
/// assert_eq!(numeric_prefix(b"1e3").unwrap().number, Number::Float(1000.0));
/// assert_eq!(numeric_prefix(b"0x1A").unwrap().number, Number::Int(0));
/// assert!(numeric_prefix(b"apples").is_none());
/// ```
pub fn numeric_prefix(bytes: &[u8]) -> Option<NumericPrefix> {
    let start = bytes
        .iter()
        .position(|&byte| !is_space(byte))
        .unwrap_or(bytes.len());
    let text = &bytes[start..];
    let unsigned = match text.first() {
        Some(b'-' | b'+') => &text[1..],
        _ => text,
    };
    let integer_digits = unsigned.iter().take_while(|b| b.is_ascii_digit()).count();
    let starts_number = integer_digits > 0
        || (unsigned.first() == Some(&b'.') && unsigned.get(1).is_some_and(u8::is_ascii_digit));
    if !starts_number {
        return None;
    }
    let after_digits = &unsigned[integer_digits..];
    let is_float = match after_digits.first() {
        Some(b'.') => true,
        Some(b'e' | b'E') => {
            let exponent = match after_digits.get(1) {
                Some(b'-' | b'+') => &after_digits[2..],
                _ => &after_digits[1..],
            };
            exponent.first().is_some_and(u8::is_ascii_digit)
        }
        _ => false,
    } || integer_digits == 0;
    let negative = text.first() == Some(&b'-');
    let leading_zeros = unsigned.iter().take_while(|&&b| b == b'0').count();
    let side = if negative {
        Overflow::Below
    } else {
        Overflow::Above
    };
    let (number, end, overflow) = if integer_digits - leading_zeros > MIN_MAGNITUDE.len() {
        let (value, len) = float_prefix(text);
        (Number::Float(value), len, side)
    } else if is_float {
        let (value, len) = float_prefix(text);
        (Number::Float(value), len, Overflow::None)
    } else {
        let digits = &unsigned[..integer_digits];
        let end = text.len() - after_digits.len();
        if let Some(value) = integer(digits, negative, after_digits) {
            (Number::Int(value), end, Overflow::None)
        } else {
            let (value, len) = float_prefix(text);
            (Number::Float(value), len, side)
        }
    };
    let trailing = text[end..].iter().any(|&byte| !is_space(byte));
    Some(NumericPrefix {
        number,
        trailing,
        overflow,
    })
}

/// The number a whole string spells, allowing surrounding whitespace: [`numeric_prefix`] without
/// trailing data.
pub fn whole_numeric(bytes: &[u8]) -> Option<NumericPrefix> {
    numeric_prefix(bytes).filter(|prefix| !prefix.trailing)
}

/// The integer `digits` (at most 19 after leading zeros) spell with their sign, if it is in range.
///
/// `-9223372036854775808` is in range only when nothing but a NUL byte follows it: the reference
/// compares the digits and everything after them against the digits of `2^63` as a C string.
fn integer(digits: &[u8], negative: bool, rest: &[u8]) -> Option<i64> {
    let significant = &digits[digits.iter().take_while(|&&b| b == b'0').count()..];
    if significant.len() == MIN_MAGNITUDE.len() && significant >= MIN_MAGNITUDE {
        let exactly_min = significant == MIN_MAGNITUDE && rest.first().is_none_or(|&b| b == 0);
        return (negative && exactly_min).then_some(i64::MIN);
    }
    let magnitude = significant
        .iter()
        .fold(0_i64, |value, digit| value * 10 + i64::from(digit - b'0'));
    Some(if negative { -magnitude } else { magnitude })
}

/// The float a string starts with, as PHP's `(float)` cast reads it: [`numeric_prefix`]'s grammar
/// without the integer rule, ignoring whatever follows, and 0 when there is no number.
///
/// ```
/// use php_value::ops::parse_float;
///
/// assert_eq!(parse_float(b" -1.5e3 apples"), -1500.0);
/// assert_eq!(parse_float(b"apples"), 0.0);
/// assert_eq!(parse_float(b"1e1000"), f64::INFINITY);
/// ```
pub fn parse_float(bytes: &[u8]) -> f64 {
    let start = bytes
        .iter()
        .position(|&byte| !is_space(byte))
        .unwrap_or(bytes.len());
    float_prefix(&bytes[start..]).0
}

/// The float at the very start of `text` (no leading whitespace) and how many bytes it spans; `(0.0,
/// 0)` when there is none.
fn float_prefix(text: &[u8]) -> (f64, usize) {
    let sign = usize::from(matches!(text.first(), Some(b'-' | b'+')));
    let digits_from = |at: usize| {
        text.get(at..).map_or(0, |rest| {
            rest.iter().take_while(|b| b.is_ascii_digit()).count()
        })
    };
    let integer = digits_from(sign);
    let mut end = sign + integer;
    if text.get(end) == Some(&b'.') {
        let fraction = digits_from(end + 1);
        if integer == 0 && fraction == 0 {
            return (0.0, 0);
        }
        end += 1 + fraction;
    } else if integer == 0 {
        return (0.0, 0);
    }
    if matches!(text.get(end), Some(b'e' | b'E')) {
        let exponent_sign = usize::from(matches!(text.get(end + 1), Some(b'-' | b'+')));
        let exponent = digits_from(end + 1 + exponent_sign);
        if exponent > 0 {
            end += 1 + exponent_sign + exponent;
        }
    }
    let value = std::str::from_utf8(&text[..end])
        .ok()
        .and_then(|literal| literal.parse::<f64>().ok())
        .unwrap_or_default();
    (value, end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    type Parsed = Option<(Number, bool, Overflow)>;

    fn prefix(text: &[u8]) -> Parsed {
        numeric_prefix(text).map(|p| (p.number, p.trailing, p.overflow))
    }

    #[test]
    fn integers_and_floats_are_told_apart() {
        let none = Overflow::None;
        let cases: [(&[u8], Parsed); 24] = [
            (b"", None),
            (b" ", None),
            (b"-", None),
            (b".", None),
            (b"abc", None),
            (b"INF", None),
            (b"0", Some((Number::Int(0), false, none))),
            (b" \t\n\r\x0b\x0c7 \n", Some((Number::Int(7), false, none))),
            (b"-0", Some((Number::Int(0), false, none))),
            (b"+12", Some((Number::Int(12), false, none))),
            (b"012", Some((Number::Int(12), false, none))),
            (b"1abc", Some((Number::Int(1), true, none))),
            (b"1\0", Some((Number::Int(1), true, none))),
            (b"1e", Some((Number::Int(1), true, none))),
            (b"1e+", Some((Number::Int(1), true, none))),
            (b"0x1A", Some((Number::Int(0), true, none))),
            (b"1.", Some((Number::Float(1.0), false, none))),
            (b".5", Some((Number::Float(0.5), false, none))),
            (b"-.5e1", Some((Number::Float(-5.0), false, none))),
            (b"1e3", Some((Number::Float(1000.0), false, none))),
            (b"1E-3x", Some((Number::Float(0.001), true, none))),
            (b"1.5 ", Some((Number::Float(1.5), false, none))),
            (b"- 1", None),
            (b"1e1000", Some((Number::Float(f64::INFINITY), false, none))),
        ];
        for (text, expected) in cases {
            assert_eq!(
                prefix(text),
                expected,
                "{:?}",
                text.escape_ascii().to_string()
            );
        }
    }

    #[test]
    fn integers_out_of_range_become_floats_and_record_the_side() {
        let max = 9_223_372_036_854_775_807;
        let cases: [(&[u8], Number, Overflow); 9] = [
            (b"9223372036854775807", Number::Int(max), Overflow::None),
            (
                b"-9223372036854775808",
                Number::Int(i64::MIN),
                Overflow::None,
            ),
            (
                b"-9223372036854775808\0",
                Number::Int(i64::MIN),
                Overflow::None,
            ),
            (
                b"000009223372036854775807",
                Number::Int(max),
                Overflow::None,
            ),
            (
                b"9223372036854775808",
                Number::Float(9.223_372_036_854_776e18),
                Overflow::Above,
            ),
            (
                b"-9223372036854775808 ",
                Number::Float(-9.223_372_036_854_776e18),
                Overflow::Below,
            ),
            (
                b"+9223372036854775808",
                Number::Float(9.223_372_036_854_776e18),
                Overflow::Above,
            ),
            (
                b"-9223372036854775809",
                Number::Float(-9.223_372_036_854_776e18),
                Overflow::Below,
            ),
            (
                b"99999999999999999999",
                Number::Float(1e20),
                Overflow::Above,
            ),
        ];
        for (text, number, overflow) in cases {
            let parsed = numeric_prefix(text).unwrap();
            assert_eq!(
                (parsed.number, parsed.overflow),
                (number, overflow),
                "{text:?}"
            );
        }
        assert!(whole_numeric(b"-9223372036854775808 ").is_some());
        assert!(whole_numeric(b"1x").is_none());
        assert_eq!(whole_numeric(b" 2 ").unwrap().number, Number::Int(2));
    }

    #[test]
    fn float_casts_read_the_longest_prefix() {
        let cases: [(&[u8], f64); 13] = [
            (b"", 0.0),
            (b"abc", 0.0),
            (b"-abc", 0.0),
            (b"+", 0.0),
            (b".", 0.0),
            (b" 1.5x", 1.5),
            (b"-0", -0.0),
            (b"0x1A", 0.0),
            (b"5.", 5.0),
            (b"1e", 1.0),
            (b"1e-2", 0.01),
            (b"--1", 0.0),
            (b"9223372036854775808", 9.223_372_036_854_776e18),
        ];
        for (text, expected) in cases {
            assert_eq!(parse_float(text).to_bits(), expected.to_bits(), "{text:?}");
        }
        assert_eq!(parse_float(b"-.0").to_bits(), (-0.0_f64).to_bits());
    }

    #[test]
    fn numbers_convert_to_floats() {
        assert_eq!(Number::from(3).as_float().to_bits(), 3.0_f64.to_bits());
        assert_eq!(Number::from(0.5).as_float().to_bits(), 0.5_f64.to_bits());
    }

    proptest! {
        #[test]
        fn every_integer_reads_back(number in any::<i64>()) {
            let text = number.to_string();
            prop_assert_eq!(numeric_prefix(text.as_bytes()).map(|p| p.number), Some(Number::Int(number)));
        }

        #[test]
        fn every_float_rendering_reads_back(bits in any::<u64>()) {
            let value = f64::from_bits(bits);
            prop_assume!(value.is_finite());
            let text = format!("{value:e}");
            prop_assert_eq!(parse_float(text.as_bytes()).to_bits(), value.to_bits());
        }
    }
}
