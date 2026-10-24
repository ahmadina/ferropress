//! Conversions between value types: the casts, and the operand conversions operators apply.

use super::numeric::{Number, numeric_prefix, parse_float};
use super::{Context, Diagnostic, NanTarget, OpError};
use crate::{ArrayKey, PhpArray, PhpString, Value};

/// `2^63` as a float: the first float above the integer range.
const INT_RANGE_END: f64 = 9_223_372_036_854_775_808.0;
/// `2^64` as a float.
const TWO_POW_64: f64 = 18_446_744_073_709_551_616.0;

/// Whether `value` truncates to an integer in range (false for infinities, true for `NAN`).
fn fits_int(value: f64) -> bool {
    value.is_nan() || (-INT_RANGE_END..INT_RANGE_END).contains(&value)
}

/// `value` truncated toward zero, known to fit.
#[expect(
    clippy::cast_possible_truncation,
    reason = "callers check the range; truncation is the conversion"
)]
const fn truncate(value: f64) -> i64 {
    value as i64
}

/// Whether `number` converts back to exactly `value`.
#[expect(
    clippy::cast_precision_loss,
    clippy::float_cmp,
    reason = "the exact comparison is the reference's test for a lossless conversion"
)]
fn is_exact(value: f64, number: i64) -> bool {
    number as f64 == value
}

/// A float out of the integer range, wrapped modulo `2^64` into it.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "the value is an integral float in 0..2^64 and wraps into the signed range"
)]
fn wrap(value: f64) -> i64 {
    let mut remainder = value % TWO_POW_64;
    if remainder < 0.0 {
        remainder += TWO_POW_64;
    }
    remainder as u64 as i64
}

/// A float as an integer the way `(int)` converts it: truncated, wrapped modulo `2^64` when out of
/// range and 0 when not finite, with a warning in both cases.
pub(super) fn float_to_int(value: f64, cx: &mut impl Context) -> i64 {
    if !value.is_finite() {
        cx.raise(Diagnostic::FloatNotRepresentable(value));
        0
    } else if fits_int(value) {
        truncate(value)
    } else {
        cx.raise(Diagnostic::FloatNotRepresentable(value));
        wrap(value)
    }
}

/// A float as an integer operand: [`float_to_int`], with a deprecation when an in-range value loses a
/// fractional part (or is `NAN`).
pub(super) fn float_to_int_operand(value: f64, cx: &mut impl Context) -> i64 {
    let number = float_to_int(value, cx);
    if !is_exact(value, number) && fits_int(value) {
        cx.raise(Diagnostic::FloatToIntLosesPrecision(value));
    }
    number
}

/// A float read from a string as an integer: truncated, saturated when out of range, 0 when not
/// finite.
fn saturate(value: f64) -> i64 {
    if !value.is_finite() {
        0
    } else if fits_int(value) {
        truncate(value)
    } else if value > 0.0 {
        i64::MAX
    } else {
        i64::MIN
    }
}

/// An arithmetic operand as a number, or `None` when the operator does not accept it.
///
/// `null` and `false` are 0, `true` is 1, a numeric string is its number, and a leading-numeric
/// string is its number with a warning. Other strings and arrays are not accepted.
pub(super) fn number_operand(
    value: &Value,
    cx: &mut impl Context,
) -> Result<Option<Number>, OpError> {
    Ok(match value {
        Value::Null | Value::Bool(false) => Some(Number::Int(0)),
        Value::Bool(true) => Some(Number::Int(1)),
        Value::Int(number) => Some(Number::Int(*number)),
        Value::Float(number) => Some(Number::Float(*number)),
        Value::String(text) => numeric_prefix(text.as_bytes()).map(|prefix| {
            if prefix.trailing {
                cx.raise(Diagnostic::NonNumeric);
            }
            prefix.number
        }),
        Value::Array(_) => None,
        Value::Object(_) => return Err(OpError::Object),
    })
}

/// An integer operand (of `%`, the shifts and the bitwise operators), or `None` when the operator does
/// not accept it.
///
/// Floats convert as [`float_to_int_operand`]. A string's float saturates, with a deprecation when
/// it is not an integer.
pub(super) fn int_operand(value: &Value, cx: &mut impl Context) -> Result<Option<i64>, OpError> {
    Ok(match value {
        Value::Null | Value::Bool(false) => Some(0),
        Value::Bool(true) => Some(1),
        Value::Int(number) => Some(*number),
        Value::Float(number) => Some(float_to_int_operand(*number, cx)),
        Value::String(text) => numeric_prefix(text.as_bytes()).map(|prefix| {
            if prefix.trailing {
                cx.raise(Diagnostic::NonNumeric);
            }
            match prefix.number {
                Number::Int(number) => number,
                Number::Float(float) => {
                    let number = saturate(float);
                    if !is_exact(float, number) {
                        cx.raise(Diagnostic::FloatStringToIntLosesPrecision(text.clone()));
                    }
                    number
                }
            }
        }),
        Value::Array(_) => None,
        Value::Object(_) => return Err(OpError::Object),
    })
}

/// Appends the decimal form of `number`.
pub(super) fn write_int(out: &mut Vec<u8>, number: i64) {
    let mut digits = [0_u8; 20];
    let mut at = digits.len();
    let mut magnitude = number.unsigned_abs();
    loop {
        at -= 1;
        digits[at] = b'0' + u8::try_from(magnitude % 10).unwrap_or_default();
        magnitude /= 10;
        if magnitude == 0 {
            break;
        }
    }
    if number < 0 {
        out.push(b'-');
    }
    out.extend_from_slice(&digits[at..]);
}

/// `left . right`: both operands as `(string)` converts them, left first, joined.
///
/// # Errors
///
/// [`OpError::Object`] for an object operand, before anything is converted.
pub fn concat(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    super::arith::reject_objects(left, right)?;
    let a = left.to_php_string(cx)?;
    let b = right.to_php_string(cx)?;
    Ok(Value::String(if b.is_empty() {
        a
    } else if a.is_empty() {
        b
    } else {
        [a.as_bytes(), b.as_bytes()].concat().into()
    }))
}

impl Value {
    /// Whether the value is true in a condition, as `(bool)`, `!` and `if` see it.
    ///
    /// `null`, `false`, 0, 0.0, `-0.0`, `""`, `"0"` and the empty array are false; everything else,
    /// `NAN` included (with a warning), is true.
    ///
    /// # Errors
    ///
    /// [`OpError::Object`] for an object.
    pub fn to_bool(&self, cx: &mut impl Context) -> Result<bool, OpError> {
        Ok(match self {
            Self::Null => false,
            Self::Bool(value) => *value,
            Self::Int(number) => *number != 0,
            Self::Float(number) => {
                if number.is_nan() {
                    cx.raise(Diagnostic::NanCoerced(NanTarget::Bool));
                }
                *number != 0.0
            }
            Self::String(text) => !matches!(text.as_bytes(), b"" | b"0"),
            Self::Array(array) => !array.is_empty(),
            Self::Object(_) => return Err(OpError::Object),
        })
    }

    /// The value as `(int)` converts it.
    ///
    /// A float is truncated, or wrapped modulo `2^64` with a warning when out of range, and is 0 with
    /// a warning when not finite. A string is the number it starts with (0 if none), its float
    /// saturating at the integer range. An array is 1 when it has elements.
    ///
    /// # Errors
    ///
    /// [`OpError::Object`] for an object.
    pub fn to_int(&self, cx: &mut impl Context) -> Result<i64, OpError> {
        Ok(match self {
            Self::Null | Self::Bool(false) => 0,
            Self::Bool(true) => 1,
            Self::Int(number) => *number,
            Self::Float(number) => float_to_int(*number, cx),
            Self::String(text) => match numeric_prefix(text.as_bytes()) {
                None => 0,
                Some(prefix) => match prefix.number {
                    Number::Int(number) => number,
                    Number::Float(float) => saturate(float),
                },
            },
            Self::Array(array) => i64::from(!array.is_empty()),
            Self::Object(_) => return Err(OpError::Object),
        })
    }

    /// The value as `(float)` converts it: a string is the float it starts with (0 if none), an array
    /// is 1.0 when it has elements.
    ///
    /// # Errors
    ///
    /// [`OpError::Object`] for an object.
    pub fn to_float(&self) -> Result<f64, OpError> {
        Ok(match self {
            Self::Null | Self::Bool(false) => 0.0,
            Self::Bool(true) => 1.0,
            Self::Int(number) => Number::Int(*number).as_float(),
            Self::Float(number) => *number,
            Self::String(text) => parse_float(text.as_bytes()),
            Self::Array(array) => {
                if array.is_empty() {
                    0.0
                } else {
                    1.0
                }
            }
            Self::Object(_) => return Err(OpError::Object),
        })
    }

    /// The value as `(string)` converts it.
    ///
    /// `null` and `false` are `""`, `true` is `"1"`, a float is written under the context's
    /// precision (with a warning for `NAN`), and an array is `"Array"` with a warning.
    ///
    /// # Errors
    ///
    /// [`OpError::Object`] for an object.
    pub fn to_php_string(&self, cx: &mut impl Context) -> Result<PhpString, OpError> {
        let mut out = Vec::new();
        match self {
            Self::Null | Self::Bool(false) => {}
            Self::Bool(true) => out.push(b'1'),
            Self::Int(number) => write_int(&mut out, *number),
            Self::Float(number) => {
                crate::write_float_with(&mut out, *number, cx.precision());
                if number.is_nan() {
                    cx.raise(Diagnostic::NanCoerced(NanTarget::String));
                }
            }
            Self::String(text) => return Ok(text.clone()),
            Self::Array(_) => {
                cx.raise(Diagnostic::ArrayToString);
                out.extend_from_slice(b"Array");
            }
            Self::Object(_) => return Err(OpError::Object),
        }
        Ok(PhpString::from(out))
    }

    /// The value as `(array)` converts it: `null` is the empty array, an array is itself, and any
    /// other value is the array holding it at key 0 (with a warning for `NAN`).
    ///
    /// # Errors
    ///
    /// [`OpError::Object`] for an object.
    pub fn to_array(&self, cx: &mut impl Context) -> Result<PhpArray, OpError> {
        Ok(match self {
            Self::Null => PhpArray::new(),
            Self::Array(array) => array.clone(),
            Self::Object(_) => return Err(OpError::Object),
            scalar => {
                if matches!(scalar, Self::Float(number) if number.is_nan()) {
                    cx.raise(Diagnostic::NanCoerced(NanTarget::Array));
                }
                let mut array = PhpArray::with_capacity(1);
                array.assign(ArrayKey::Int(0), scalar.clone());
                array
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Collect;
    use crate::{ObjectStore, Precision};

    fn object() -> Value {
        let store = ObjectStore::new();
        Value::from(store.create("C".into()).unwrap())
    }

    #[test]
    fn floats_become_integers_by_truncation_wrapping_or_zero() {
        let mut cx = Collect::default();
        assert_eq!(float_to_int(-1.9, &mut cx), -1);
        assert_eq!(float_to_int(-INT_RANGE_END, &mut cx), i64::MIN);
        assert!(cx.diagnostics.is_empty());
        assert_eq!(float_to_int(INT_RANGE_END, &mut cx), i64::MIN);
        assert_eq!(
            float_to_int(-9.223_372_036_854_777e18, &mut cx),
            9_223_372_036_854_773_760
        );
        assert_eq!(float_to_int(1e19, &mut cx), -8_446_744_073_709_551_616);
        assert_eq!(float_to_int(f64::NAN, &mut cx), 0);
        assert_eq!(cx.diagnostics.len(), 4);
        assert!(matches!(cx.diagnostics[3], Diagnostic::FloatNotRepresentable(x) if x.is_nan()));
    }

    #[test]
    fn integer_operands_report_lost_fractions() {
        let mut cx = Collect::default();
        assert_eq!(float_to_int_operand(2.0, &mut cx), 2);
        assert!(cx.diagnostics.is_empty());
        assert_eq!(float_to_int_operand(2.5, &mut cx), 2);
        assert_eq!(
            float_to_int_operand(1e19, &mut cx),
            -8_446_744_073_709_551_616
        );
        assert_eq!(
            cx.diagnostics,
            [
                Diagnostic::FloatToIntLosesPrecision(2.5),
                Diagnostic::FloatNotRepresentable(1e19)
            ]
        );
        let mut cx = Collect::default();
        assert_eq!(int_operand(&Value::from("1e1000"), &mut cx), Ok(Some(0)));
        assert_eq!(
            int_operand(&Value::from("-1e30"), &mut cx),
            Ok(Some(i64::MIN))
        );
        assert_eq!(int_operand(&Value::from("2.0x"), &mut cx), Ok(Some(2)));
        assert_eq!(int_operand(&Value::from("x"), &mut cx), Ok(None));
        assert_eq!(
            int_operand(&Value::from(PhpArray::new()), &mut cx),
            Ok(None)
        );
        assert_eq!(int_operand(&object(), &mut cx), Err(OpError::Object));
        assert_eq!(
            cx.diagnostics,
            [
                Diagnostic::FloatStringToIntLosesPrecision("1e1000".into()),
                Diagnostic::FloatStringToIntLosesPrecision("-1e30".into()),
                Diagnostic::NonNumeric,
            ]
        );
    }

    #[test]
    fn number_operands_accept_scalars_and_numeric_strings() {
        let mut cx = Collect::default();
        let cases = [
            (Value::Null, Some(Number::Int(0))),
            (Value::Bool(true), Some(Number::Int(1))),
            (Value::Float(0.5), Some(Number::Float(0.5))),
            (Value::from(" 7 "), Some(Number::Int(7))),
            (Value::from("abc"), None),
            (Value::from(PhpArray::new()), None),
        ];
        for (value, expected) in cases {
            assert_eq!(number_operand(&value, &mut cx), Ok(expected), "{value:?}");
        }
        assert!(cx.diagnostics.is_empty());
        assert_eq!(
            number_operand(&Value::from("7a"), &mut cx),
            Ok(Some(Number::Int(7)))
        );
        assert_eq!(cx.diagnostics, [Diagnostic::NonNumeric]);
        assert_eq!(number_operand(&object(), &mut cx), Err(OpError::Object));
    }

    #[test]
    fn concatenation_converts_left_then_right() {
        let mut cx = Collect::default();
        let joined = |a: Value, b: Value, cx: &mut Collect| match concat(&a, &b, cx) {
            Ok(Value::String(s)) => s.as_bytes().to_vec(),
            other => panic!("concatenation gives a string, not {other:?}"),
        };
        assert_eq!(joined(Value::Int(1), Value::Float(0.5), &mut cx), b"10.5");
        assert_eq!(joined(Value::from(""), Value::from("b"), &mut cx), b"b");
        assert_eq!(joined(Value::from("a"), Value::Null, &mut cx), b"a");
        assert!(cx.diagnostics.is_empty());
        let array = Value::from(PhpArray::new());
        assert_eq!(
            joined(array.clone(), Value::Float(f64::NAN), &mut cx),
            b"ArrayNAN"
        );
        assert_eq!(
            cx.diagnostics,
            [
                Diagnostic::ArrayToString,
                Diagnostic::NanCoerced(NanTarget::String)
            ]
        );
        assert_eq!(
            concat(&array, &object(), &mut cx).err(),
            Some(OpError::Object)
        );
        assert_eq!(cx.diagnostics.len(), 2);
    }

    #[test]
    fn integers_print_in_decimal() {
        for number in [0, 7, -7, i64::MAX, i64::MIN] {
            let mut out = Vec::new();
            write_int(&mut out, number);
            assert_eq!(out, number.to_string().as_bytes());
        }
    }

    #[test]
    fn casts_follow_php() {
        let mut cx = Collect::default();
        let array = Value::from(PhpArray::new());
        assert_eq!(Value::from("0").to_bool(&mut cx), Ok(false));
        assert_eq!(Value::from("0.0").to_bool(&mut cx), Ok(true));
        assert_eq!(array.to_bool(&mut cx), Ok(false));
        assert_eq!(Value::from(" 12abc").to_int(&mut cx), Ok(12));
        assert_eq!(Value::from("1e30").to_int(&mut cx), Ok(i64::MAX));
        assert_eq!(Value::from("x").to_int(&mut cx), Ok(0));
        assert_eq!(array.to_int(&mut cx), Ok(0));
        assert_eq!(Value::from("1.5x").to_float(), Ok(1.5));
        assert_eq!(Value::Int(3).to_float(), Ok(3.0));
        assert_eq!(array.to_float(), Ok(0.0));
        let string = |value: Value, cx: &mut Collect| value.to_php_string(cx).unwrap();
        assert_eq!(string(Value::Float(0.1 + 0.2), &mut cx).as_bytes(), b"0.3");
        assert_eq!(string(Value::Int(-5), &mut cx).as_bytes(), b"-5");
        assert_eq!(string(Value::Bool(true), &mut cx).as_bytes(), b"1");
        assert!(cx.diagnostics.is_empty());
        cx.precision = Precision::Shortest;
        assert_eq!(
            string(Value::Float(0.1 + 0.2), &mut cx).as_bytes(),
            b"0.30000000000000004"
        );
        assert_eq!(string(array.clone(), &mut cx).as_bytes(), b"Array");
        assert_eq!(cx.diagnostics, [Diagnostic::ArrayToString]);
        assert!(Value::Null.to_array(&mut cx).unwrap().is_empty());
        let wrapped = Value::from("a").to_array(&mut cx).unwrap();
        assert_eq!(wrapped.len(), 1);
        assert!(
            matches!(wrapped.get(&ArrayKey::Int(0)).unwrap().value(), Value::String(s) if s.as_bytes() == b"a")
        );
        assert!(array.to_array(&mut cx).unwrap().is_empty());
        for operation in [
            Value::to_bool,
            |v: &Value, cx: &mut Collect| v.to_int(cx).map(|_| true),
            |v: &Value, cx: &mut Collect| v.to_php_string(cx).map(|_| true),
            |v: &Value, cx: &mut Collect| v.to_array(cx).map(|_| true),
        ] {
            assert_eq!(operation(&object(), &mut cx), Err(OpError::Object));
        }
        assert_eq!(object().to_float(), Err(OpError::Object));
    }
}
