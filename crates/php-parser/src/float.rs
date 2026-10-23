//! Float to text conversion at a given number of significant digits.

use std::fmt::Write as _;

/// The digits PHP's `precision` setting gives by default.
pub(crate) const PRECISION: usize = 14;

/// Formats `value` with at most `precision` significant digits the way PHP's `precision`-based
/// conversion does: plain notation unless the exponent is below -4 or at least `precision`,
/// otherwise `d.dddE+x` with at least one digit after the point; `INF`, `-INF` and `NAN` for
/// non-finite values.
pub(crate) fn format(value: f64, precision: usize) -> String {
    if value.is_nan() {
        return "NAN".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-INF" } else { "INF" }.to_owned();
    }
    let precision = precision.max(1);
    let mut out = String::new();
    if value.is_sign_negative() {
        out.push('-');
    }
    let (digits, point) = significant_digits(value.abs(), precision);
    let point_limit = i64::try_from(precision).unwrap_or(i64::MAX);
    if point < -3 || point > point_limit {
        let mut digits = digits.chars();
        if let Some(first) = digits.next() {
            out.push(first);
        }
        out.push('.');
        let rest = digits.as_str();
        out.push_str(if rest.is_empty() { "0" } else { rest });
        let exponent = point - 1;
        let sign = if exponent < 0 { '-' } else { '+' };
        let _ = write!(out, "E{sign}{}", exponent.unsigned_abs());
    } else if point <= 0 {
        out.push_str("0.");
        for _ in point..0 {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let whole = usize::try_from(point).unwrap_or(usize::MAX);
        if digits.len() <= whole {
            out.push_str(&digits);
            for _ in digits.len()..whole {
                out.push('0');
            }
        } else {
            out.push_str(&digits[..whole]);
            out.push('.');
            out.push_str(&digits[whole..]);
        }
    }
    out
}

/// The decimal digits of a finite, non-negative `value` rounded to `precision` significant digits
/// with trailing zeros removed, and the position of the decimal point relative to the first digit
/// (`0.d1d2… × 10^point`). Zero is `("0", 1)`.
fn significant_digits(value: f64, precision: usize) -> (String, i64) {
    if value == 0.0 {
        return ("0".to_owned(), 1);
    }
    let scientific = format!("{value:.prec$e}", prec = precision - 1);
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let mut digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    let exponent: i64 = exponent.parse().unwrap_or(0);
    (digits, exponent + 1)
}

/// How a float literal is rendered in an exported expression: [`PRECISION`] digits, with `.0`
/// appended to finite values written without a point.
pub(crate) fn export(value: f64) -> String {
    let mut text = format(value, PRECISION);
    if value.is_finite() && !text.contains('.') {
        text.push_str(".0");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{PRECISION, export, format};

    #[test]
    fn plain_and_exponent_notation() {
        let cases = [
            (0.1, "0.1"),
            (1.5, "1.5"),
            (100.0, "100"),
            (0.0001, "0.0001"),
            (0.000_01, "1.0E-5"),
            (1e14, "1.0E+14"),
            (1e13, "10000000000000"),
            (123_456_789_012_345.0, "1.2345678901234E+14"),
            (1.5e25, "1.5E+25"),
            (1e100, "1.0E+100"),
            (2.5e-300, "2.5E-300"),
            (0.5, "0.5"),
            (0.001_25, "0.00125"),
            (-2.0, "-2"),
            (-0.0, "-0"),
            (0.0, "0"),
            (1.0 / 3.0, "0.33333333333333"),
            (9_007_199_254_740_993.0, "9.007199254741E+15"),
            (f64::INFINITY, "INF"),
            (f64::NEG_INFINITY, "-INF"),
            (f64::NAN, "NAN"),
        ];
        for (value, text) in cases {
            assert_eq!(format(value, PRECISION), text, "{value}");
        }
        assert_eq!(format(123.456, 0), "1.0E+2");
        assert_eq!(format(0.5, 1), "0.5");
    }

    #[test]
    fn exported_floats_keep_a_point() {
        assert_eq!(export(1.0), "1.0");
        assert_eq!(export(-0.0), "-0.0");
        assert_eq!(export(1e20), "1.0E+20");
        assert_eq!(export(0.25), "0.25");
        assert_eq!(export(f64::INFINITY), "INF");
        assert_eq!(export(f64::NAN), "NAN");
    }
}
