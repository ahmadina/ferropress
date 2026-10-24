//! Arithmetic operators: `+`, `-`, `*`, `/`, `%` and `**`.

use super::convert::{int_operand, number_operand};
use super::numeric::Number;
use super::{Context, Diagnostic, OpError, Thrown};
use crate::{PhpArray, Value};

/// The error for operands `operator` does not accept.
pub(super) fn unsupported(left: &Value, operator: &'static str, right: &Value) -> OpError {
    OpError::Throw(Thrown::UnsupportedOperands {
        left: left.type_name(),
        operator,
        right: right.type_name(),
    })
}

/// [`OpError::Object`] when either operand is an object.
pub(super) fn reject_objects(left: &Value, right: &Value) -> Result<(), OpError> {
    if matches!(left, Value::Object(_)) || matches!(right, Value::Object(_)) {
        Err(OpError::Object)
    } else {
        Ok(())
    }
}

/// Both operands as numbers, converted left first; an operand that is not accepted throws before the
/// next one is converted.
fn numbers(
    left: &Value,
    operator: &'static str,
    right: &Value,
    cx: &mut impl Context,
) -> Result<(Number, Number), OpError> {
    reject_objects(left, right)?;
    let Some(a) = number_operand(left, cx)? else {
        return Err(unsupported(left, operator, right));
    };
    let Some(b) = number_operand(right, cx)? else {
        return Err(unsupported(left, operator, right));
    };
    Ok((a, b))
}

/// Both operands as integers, converted left first, as `%` and the shifts and bitwise operators take
/// them.
pub(super) fn integers(
    left: &Value,
    operator: &'static str,
    right: &Value,
    cx: &mut impl Context,
) -> Result<(i64, i64), OpError> {
    reject_objects(left, right)?;
    let Some(a) = int_operand(left, cx)? else {
        return Err(unsupported(left, operator, right));
    };
    let Some(b) = int_operand(right, cx)? else {
        return Err(unsupported(left, operator, right));
    };
    Ok((a, b))
}

/// Applies an integer operation that may overflow, falling back to the float operation on the
/// operands converted to floats.
fn combine(
    a: Number,
    b: Number,
    checked: fn(i64, i64) -> Option<i64>,
    float: fn(f64, f64) -> f64,
) -> Value {
    match (a, b) {
        (Number::Int(x), Number::Int(y)) => checked(x, y).map_or_else(
            || Value::Float(float(a.as_float(), b.as_float())),
            Value::Int,
        ),
        _ => Value::Float(float(a.as_float(), b.as_float())),
    }
}

/// `left + right`: the union of two arrays, or the sum of two numbers.
///
/// The union holds every element of `left`, then each element of `right` whose key `left` lacks.
/// Integer sums that overflow become floats.
///
/// # Errors
///
/// Throws `TypeError` for operands that are not numbers or numeric strings (or two arrays);
/// [`OpError::Object`] for an object operand.
pub fn add(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    if let (Value::Array(a), Value::Array(b)) = (left, right) {
        return Ok(Value::Array(union(a, b)));
    }
    let (a, b) = numbers(left, "+", right, cx)?;
    Ok(combine(a, b, i64::checked_add, |x, y| x + y))
}

/// `a + b` for arrays.
fn union(a: &PhpArray, b: &PhpArray) -> PhpArray {
    if b.is_empty() || a.ptr_eq(b) {
        return a.clone();
    }
    let mut out = a.clone();
    for (key, slot) in b.iter() {
        if !out.contains_key(key) {
            match slot.as_reference() {
                Some(reference) => {
                    out.bind(key.clone(), reference.clone());
                }
                None => {
                    out.assign(key.clone(), slot.value());
                }
            }
        }
    }
    out
}

/// `left - right`.
///
/// # Errors
///
/// As [`add`], arrays included.
pub fn sub(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    let (a, b) = numbers(left, "-", right, cx)?;
    Ok(combine(a, b, i64::checked_sub, |x, y| x - y))
}

/// `left * right`. Unary `-x` and `+x` are `x * -1` and `x * 1`.
///
/// # Errors
///
/// As [`sub`].
pub fn mul(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    let (a, b) = numbers(left, "*", right, cx)?;
    Ok(combine(a, b, i64::checked_mul, |x, y| x * y))
}

/// `left / right`: an integer when two integers divide exactly, otherwise a float.
///
/// # Errors
///
/// As [`sub`], and throws `DivisionByZeroError` for a zero divisor.
pub fn div(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    let (a, b) = numbers(left, "/", right, cx)?;
    if b.as_float() == 0.0 {
        return Err(Thrown::DivisionByZero.into());
    }
    Ok(match (a, b) {
        (Number::Int(x), Number::Int(y)) if x.checked_rem(y) == Some(0) => Value::Int(x / y),
        _ => Value::Float(a.as_float() / b.as_float()),
    })
}

/// `left % right` on the operands as integers; the result takes the sign of `left`.
///
/// # Errors
///
/// Throws `TypeError` for operands without an integer value, `DivisionByZeroError` for a zero
/// divisor; [`OpError::Object`] for an object operand.
pub fn modulo(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    let (a, b) = integers(left, "%", right, cx)?;
    if b == 0 {
        return Err(Thrown::ModuloByZero.into());
    }
    Ok(Value::Int(a.checked_rem(b).unwrap_or(0)))
}

/// `base ** exponent` on floats, with the deprecation for a zero base and negative exponent.
fn float_pow(base: f64, exponent: f64, cx: &mut impl Context) -> f64 {
    if base == 0.0 && exponent < 0.0 {
        cx.raise(Diagnostic::ZeroToNegativePower);
    }
    base.powf(exponent)
}

/// `left ** right`.
///
/// Two integers with a non-negative exponent multiply by repeated squaring; when a step overflows,
/// the remaining power is taken in floats from the values reached so far. Everything else is a float
/// power.
///
/// # Errors
///
/// As [`sub`].
pub fn pow(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    let (a, b) = numbers(left, "**", right, cx)?;
    Ok(match (a, b) {
        (Number::Int(base), Number::Int(exponent)) if exponent >= 0 => int_pow(base, exponent, cx),
        _ => Value::Float(float_pow(a.as_float(), b.as_float(), cx)),
    })
}

/// `base ** exponent` for integers with `exponent >= 0`.
fn int_pow(base: i64, exponent: i64, cx: &mut impl Context) -> Value {
    let float = |number: i64| Number::Int(number).as_float();
    if exponent == 0 {
        return Value::Int(1);
    }
    if base == 0 {
        return Value::Int(0);
    }
    let (mut product, mut square, mut remaining) = (1_i64, base, exponent);
    while remaining >= 1 {
        if remaining % 2 == 1 {
            remaining -= 1;
            let Some(next) = product.checked_mul(square) else {
                let reached = float(product) * float(square);
                let rest = float_pow(float(square), float(remaining), cx);
                return Value::Float(reached * rest);
            };
            product = next;
        } else {
            remaining /= 2;
            let Some(next) = square.checked_mul(square) else {
                let squared = float(square) * float(square);
                let rest = float_pow(squared, float(remaining), cx);
                return Value::Float(float(product) * rest);
            };
            square = next;
        }
    }
    Value::Int(product)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Collect;
    use crate::{ArrayKey, ObjectStore, Reference};

    fn run(
        op: fn(&Value, &Value, &mut Collect) -> Result<Value, OpError>,
        a: impl Into<Value>,
        b: impl Into<Value>,
    ) -> (Result<Value, OpError>, Vec<Diagnostic>) {
        let mut cx = Collect::default();
        let result = op(&a.into(), &b.into(), &mut cx);
        (result, cx.diagnostics)
    }

    fn int(result: &Result<Value, OpError>) -> Option<i64> {
        match result {
            Ok(Value::Int(n)) => Some(*n),
            _ => None,
        }
    }

    fn float(result: &Result<Value, OpError>) -> Option<u64> {
        match result {
            Ok(Value::Float(x)) => Some(x.to_bits()),
            _ => None,
        }
    }

    #[test]
    fn integer_overflow_becomes_float() {
        assert_eq!(
            float(&run(add, i64::MAX, 1).0),
            Some(9.223_372_036_854_776e18_f64.to_bits())
        );
        assert_eq!(
            float(&run(sub, i64::MIN, 1).0),
            Some((-9.223_372_036_854_776e18_f64).to_bits())
        );
        assert_eq!(
            float(&run(mul, i64::MIN, -1).0),
            Some(9.223_372_036_854_776e18_f64.to_bits())
        );
        assert_eq!(int(&run(add, 2, 3).0), Some(5));
        assert_eq!(float(&run(add, 2, 0.5).0), Some(2.5_f64.to_bits()));
    }

    #[test]
    fn division_is_exact_or_float() {
        assert_eq!(int(&run(div, 6, 3).0), Some(2));
        assert_eq!(float(&run(div, 7, 2).0), Some(3.5_f64.to_bits()));
        assert_eq!(
            float(&run(div, i64::MIN, -1).0),
            Some(9.223_372_036_854_776e18_f64.to_bits())
        );
        assert_eq!(
            run(div, 1, -0.0).0.err(),
            Some(Thrown::DivisionByZero.into())
        );
        let (result, diagnostics) = run(div, "1x", 0);
        assert_eq!(result.err(), Some(Thrown::DivisionByZero.into()));
        assert_eq!(diagnostics, [Diagnostic::NonNumeric]);
    }

    #[test]
    fn modulo_truncates_toward_zero() {
        assert_eq!(int(&run(modulo, -7, 2).0), Some(-1));
        assert_eq!(int(&run(modulo, i64::MIN, -1).0), Some(0));
        assert_eq!(
            run(modulo, 1, 0.5).0.err(),
            Some(Thrown::ModuloByZero.into())
        );
        assert_eq!(
            run(modulo, PhpArray::new(), 1).0.err(),
            Some(unsupported(&PhpArray::new().into(), "%", &Value::Int(1)))
        );
        assert_eq!(
            run(modulo, 1, "x").0.err(),
            Some(unsupported(&Value::Int(1), "%", &"x".into()))
        );
    }

    #[test]
    fn powers_overflow_into_floats() {
        assert_eq!(int(&run(pow, 2, 62).0), Some(1 << 62));
        assert_eq!(int(&run(pow, -3, 3).0), Some(-27));
        assert_eq!(int(&run(pow, 0, 5).0), Some(0));
        assert_eq!(int(&run(pow, 5, 0).0), Some(1));
        assert_eq!(
            float(&run(pow, 2, 64).0),
            Some(1.844_674_407_370_955_2e19_f64.to_bits())
        );
        assert_eq!(float(&run(pow, 3, 41).0), Some(3.0_f64.powi(41).to_bits()));
        assert_eq!(float(&run(pow, 2, -1).0), Some(0.5_f64.to_bits()));
        let (result, diagnostics) = run(pow, 0, -1);
        assert_eq!(float(&result), Some(f64::INFINITY.to_bits()));
        assert_eq!(diagnostics, [Diagnostic::ZeroToNegativePower]);
    }

    #[test]
    fn operand_errors_name_both_types_and_stop_conversion() {
        let (result, diagnostics) = run(add, "abc", "1x");
        assert_eq!(
            result.err(),
            Some(unsupported(&"abc".into(), "+", &"1x".into()))
        );
        assert!(diagnostics.is_empty());
        let (result, diagnostics) = run(mul, "1x", PhpArray::new());
        assert_eq!(
            result.err(),
            Some(unsupported(&"1x".into(), "*", &PhpArray::new().into()))
        );
        assert_eq!(diagnostics, [Diagnostic::NonNumeric]);
        let store = ObjectStore::new();
        let object = Value::from(store.create("C".into()).unwrap());
        assert_eq!(run(add, object.clone(), 1).0.err(), Some(OpError::Object));
        assert_eq!(run(modulo, "x", object).0.err(), Some(OpError::Object));
    }

    #[test]
    fn array_union_keeps_left_keys_and_references() {
        let mut a = PhpArray::new();
        a.assign(ArrayKey::Int(0), Value::from("a"));
        let mut b = PhpArray::new();
        b.assign(ArrayKey::Int(0), Value::from("b0"));
        let reference = Reference::new(Value::from("b1"));
        b.bind(ArrayKey::Int(1), reference.clone());
        let Ok(Value::Array(sum)) = run(add, a.clone(), b.clone()).0 else {
            panic!("two arrays add up to an array");
        };
        assert_eq!(sum.len(), 2);
        assert!(
            matches!(sum.get(&ArrayKey::Int(0)).unwrap().value(), Value::String(s) if s.as_bytes() == b"a")
        );
        assert!(
            sum.get(&ArrayKey::Int(1))
                .unwrap()
                .as_reference()
                .unwrap()
                .ptr_eq(&reference)
        );
        let Ok(Value::Array(same)) = run(add, a.clone(), a.clone()).0 else {
            panic!("an array added to itself is an array");
        };
        assert!(same.ptr_eq(&a));
        let Ok(Value::Array(left)) = run(add, a.clone(), PhpArray::new()).0 else {
            panic!("adding the empty array gives an array");
        };
        assert!(left.ptr_eq(&a));
    }
}
