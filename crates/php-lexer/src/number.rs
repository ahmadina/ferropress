//! Numeric literals: where they end and whether they are integers or floats.

use crate::token::TokenKind;

/// A numeric literal read from the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Number {
    /// Offset just past the literal.
    pub(crate) end: usize,
    /// [`TokenKind::Integer`] or [`TokenKind::Float`].
    pub(crate) kind: TokenKind,
    /// A leading-zero decimal literal containing 8 or 9.
    pub(crate) invalid_octal: bool,
}

/// The base a literal is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Base {
    Binary,
    Octal,
    Decimal,
    Hex,
}

impl Base {
    const fn admits(self, byte: u8) -> bool {
        match self {
            Self::Binary => matches!(byte, b'0' | b'1'),
            Self::Octal => matches!(byte, b'0'..=b'7'),
            Self::Decimal => byte.is_ascii_digit(),
            Self::Hex => byte.is_ascii_hexdigit(),
        }
    }

    const fn radix(self) -> u32 {
        match self {
            Self::Binary => 2,
            Self::Octal => 8,
            Self::Decimal => 10,
            Self::Hex => 16,
        }
    }
}

/// End of a digit run in `base` starting at `start`, where single underscores may separate digits;
/// `None` when no digit starts there.
fn digits(source: &[u8], start: usize, base: Base) -> Option<usize> {
    let admits = |index: usize| source.get(index).is_some_and(|&byte| base.admits(byte));
    if !admits(start) {
        return None;
    }
    let mut end = start + 1;
    loop {
        if admits(end) {
            end += 1;
        } else if source.get(end) == Some(&b'_') && admits(end + 1) {
            end += 2;
        } else {
            return Some(end);
        }
    }
}

/// End of a prefixed literal (`0x…`, `0b…`, `0o…`, any letter case) starting at `start`.
fn prefixed(source: &[u8], start: usize) -> Option<(Base, usize)> {
    if source.get(start) != Some(&b'0') {
        return None;
    }
    let base = match source.get(start + 1)?.to_ascii_lowercase() {
        b'x' => Base::Hex,
        b'b' => Base::Binary,
        b'o' => Base::Octal,
        _ => return None,
    };
    digits(source, start + 2, base).map(|end| (base, end))
}

/// End of the integer-looking literal an interpolated `"$a[…]"` offset accepts at `start`: decimal,
/// hexadecimal, binary or octal, never a float.
pub(crate) fn scan_offset(source: &[u8], start: usize) -> Option<usize> {
    let decimal = digits(source, start, Base::Decimal);
    let prefixed = prefixed(source, start).map(|(_, end)| end);
    decimal.max(prefixed)
}

/// Reads the numeric literal at `start`, which must be a digit or a `.` followed by a digit.
pub(crate) fn scan(source: &[u8], start: usize) -> Option<Number> {
    let integer = digits(source, start, Base::Decimal);
    let fraction_start = integer.unwrap_or(start);
    let float = if source.get(fraction_start) == Some(&b'.') {
        match digits(source, fraction_start + 1, Base::Decimal) {
            Some(end) => Some(end),
            None => integer.map(|_| fraction_start + 1),
        }
    } else {
        None
    };
    let mantissa = float.or(integer)?;
    let exponent = exponent(source, mantissa);
    let float = exponent.or(float);
    let prefixed = prefixed(source, start);

    let number = |end, kind| Number {
        end,
        kind,
        invalid_octal: false,
    };
    let decimal_end = float.or(integer).unwrap_or(start);
    Some(match prefixed {
        Some((base, end)) if end > decimal_end => {
            number(end, classify_prefixed(&source[start + 2..end], base))
        }
        _ => match float {
            Some(end) => number(end, TokenKind::Float),
            None => classify_decimal(&source[start..decimal_end], decimal_end),
        },
    })
}

/// End of an exponent (`e`, optional sign, digits) at `start`.
fn exponent(source: &[u8], start: usize) -> Option<usize> {
    if !matches!(source.get(start), Some(b'e' | b'E')) {
        return None;
    }
    let digits_start = start + 1 + usize::from(matches!(source.get(start + 1), Some(b'+' | b'-')));
    digits(source, digits_start, Base::Decimal)
}

/// Whether a run of digits in `base` (underscores ignored) exceeds the largest integer.
fn overflows(digits: impl Iterator<Item = u8>, base: Base) -> bool {
    let limit = u128::from(i64::MAX.unsigned_abs());
    let mut value: u128 = 0;
    for digit in digits {
        let Some(digit) = char::from(digit).to_digit(base.radix()) else {
            break;
        };
        value = value * u128::from(base.radix()) + u128::from(digit);
        if value > limit {
            return true;
        }
    }
    false
}

/// Kind of a prefixed literal given its digits after the prefix.
fn classify_prefixed(text: &[u8], base: Base) -> TokenKind {
    let significant: Vec<u8> = text
        .iter()
        .copied()
        .skip_while(|&byte| byte == b'0' || byte == b'_')
        .filter(|&byte| byte != b'_')
        .collect();
    let integer = match base {
        Base::Hex => significant.len() < 16 || (significant.len() == 16 && significant[0] <= b'7'),
        Base::Binary => significant.len() < 64,
        Base::Octal | Base::Decimal => !overflows(significant.into_iter(), base),
    };
    if integer {
        TokenKind::Integer
    } else {
        TokenKind::Float
    }
}

/// Kind of a decimal integer literal; a leading zero makes it octal.
fn classify_decimal(text: &[u8], end: usize) -> Number {
    let digits = || text.iter().copied().filter(|&byte| byte != b'_');
    let octal = text.first() == Some(&b'0');
    let invalid = octal
        .then(|| digits().position(|byte| matches!(byte, b'8' | b'9')))
        .flatten();
    let length = invalid.unwrap_or_else(|| digits().count());
    let base = if octal { Base::Octal } else { Base::Decimal };
    let kind = if length < 19 || !overflows(digits(), base) {
        TokenKind::Integer
    } else {
        TokenKind::Float
    };
    Number {
        end,
        kind,
        invalid_octal: invalid.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Number, scan, scan_offset};
    use crate::token::TokenKind::{self, Float, Integer};

    fn read(text: &str) -> (usize, TokenKind, bool) {
        let source = text.as_bytes();
        let start = 0;
        match scan(source, start) {
            Some(Number {
                end,
                kind,
                invalid_octal,
            }) => (end, kind, invalid_octal),
            None => panic!("no number in {text:?}"),
        }
    }

    #[test]
    fn literal_extent() {
        assert_eq!(read("123;"), (3, Integer, false));
        assert_eq!(read("1_000_000+"), (9, Integer, false));
        assert_eq!(read("1__0"), (1, Integer, false));
        assert_eq!(read("1_"), (1, Integer, false));
        assert_eq!(read("1.5."), (3, Float, false));
        assert_eq!(read("1."), (2, Float, false));
        assert_eq!(read(".5"), (2, Float, false));
        assert_eq!(read("1e5"), (3, Float, false));
        assert_eq!(read("1.5E-3x"), (6, Float, false));
        assert_eq!(read("1.e+3"), (5, Float, false));
        assert_eq!(read(".5e3"), (4, Float, false));
        assert_eq!(read("1e"), (1, Integer, false));
        assert_eq!(read("1e+"), (1, Integer, false));
        assert_eq!(read("0x1F"), (4, Integer, false));
        assert_eq!(read("0XaB_cD"), (7, Integer, false));
        assert_eq!(read("0x"), (1, Integer, false));
        assert_eq!(read("0b101"), (5, Integer, false));
        assert_eq!(read("0B2"), (1, Integer, false));
        assert_eq!(read("0o17"), (4, Integer, false));
        assert_eq!(read("0O8"), (1, Integer, false));
        assert_eq!(read("0b1.5"), (3, Integer, false));
        assert_eq!(read("0.5"), (3, Float, false));
        assert_eq!(read("0e1"), (3, Float, false));
        assert_eq!(scan(b".x", 0), None);
    }

    #[test]
    fn decimal_overflow_becomes_float() {
        assert_eq!(read("9223372036854775807").1, Integer);
        assert_eq!(read("9223372036854775808").1, Float);
        assert_eq!(read("999999999999999999").1, Integer);
        assert_eq!(read("9_223_372_036_854_775_808").1, Float);
        assert_eq!(read("0000000000000000000001").1, Integer);
    }

    #[test]
    fn leading_zero_is_octal() {
        assert_eq!(read("0777"), (4, Integer, false));
        assert_eq!(read("0777777777777777777777").1, Integer);
        assert_eq!(read("01777777777777777777777").1, Float);
        assert_eq!(read("09"), (2, Integer, true));
        assert_eq!(read("0_8"), (3, Integer, true));
        assert_eq!(read("01777777777777777777777779").1, Float);
        assert_eq!(read("0177777777777777777779").1, Integer);
    }

    #[test]
    fn prefixed_overflow() {
        assert_eq!(read("0x7FFFFFFFFFFFFFFF").1, Integer);
        assert_eq!(read("0x8000000000000000").1, Float);
        assert_eq!(read("0x00_7FFFFFFFFFFFFFFF").1, Integer);
        assert_eq!(read("0x10000000000000000").1, Float);
        let ones = format!("0b{}", "1".repeat(63));
        assert_eq!(read(&ones).1, Integer);
        let wide = format!("0b{}", "1".repeat(64));
        assert_eq!(read(&wide).1, Float);
        assert_eq!(read("0o777777777777777777777").1, Integer);
        assert_eq!(read("0o1777777777777777777777").1, Float);
        assert_eq!(read("0o0_0").1, Integer);
    }

    #[test]
    fn offsets_accept_integers_only() {
        assert_eq!(scan_offset(b"12]", 0), Some(2));
        assert_eq!(scan_offset(b"0x1f]", 0), Some(4));
        assert_eq!(scan_offset(b"1_2.5", 0), Some(3));
        assert_eq!(scan_offset(b"x", 0), None);
    }
}
