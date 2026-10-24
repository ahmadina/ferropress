//! Bitwise operators: `~`, `|`, `&`, `^`, `<<` and `>>`.

use super::arith::integers;
use super::convert::float_to_int_operand;
use super::{Context, OpError, Thrown};
use crate::{PhpString, Value};

/// Bits in an integer.
const INT_BITS: u64 = 64;

/// `~value`: the complement of an integer (a float is converted to one first), or of every byte of a
/// string.
///
/// # Errors
///
/// Throws `TypeError` for `null`, booleans and arrays; [`OpError::Object`] for an object.
pub fn bit_not(value: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    match value {
        Value::Int(number) => Ok(Value::Int(!number)),
        Value::Float(number) => Ok(Value::Int(!float_to_int_operand(*number, cx))),
        Value::String(text) => Ok(Value::String(
            text.as_bytes()
                .iter()
                .map(|byte| !byte)
                .collect::<Vec<_>>()
                .into(),
        )),
        Value::Object(_) => Err(OpError::Object),
        Value::Null | Value::Bool(_) | Value::Array(_) => {
            Err(Thrown::BitwiseNot(value.value_name()).into())
        }
    }
}

/// How `|`, `&` and `^` combine two strings: over the longer length (`|`, the rest copied from the
/// longer) or the shorter one (`&` and `^`).
#[derive(Clone, Copy)]
enum Span {
    Longer,
    Shorter,
}

fn bitwise(
    left: &Value,
    operator: &'static str,
    right: &Value,
    cx: &mut impl Context,
    bits: fn(i64, i64) -> i64,
    bytes: fn(u8, u8) -> u8,
    span: Span,
) -> Result<Value, OpError> {
    if let (Value::String(a), Value::String(b)) = (left, right) {
        return Ok(Value::String(combine_bytes(a, b, bytes, span)));
    }
    let (a, b) = integers(left, operator, right, cx)?;
    Ok(Value::Int(bits(a, b)))
}

fn combine_bytes(a: &PhpString, b: &PhpString, bytes: fn(u8, u8) -> u8, span: Span) -> PhpString {
    let (longer, shorter) = if a.len() >= b.len() {
        (a.as_bytes(), b.as_bytes())
    } else {
        (b.as_bytes(), a.as_bytes())
    };
    let mut out: Vec<u8> = longer
        .iter()
        .zip(shorter)
        .map(|(&x, &y)| bytes(x, y))
        .collect();
    if matches!(span, Span::Longer) {
        out.extend_from_slice(&longer[shorter.len()..]);
    }
    out.into()
}

/// `left | right`: on two strings, byte by byte over the longer length; otherwise on the operands as
/// integers.
///
/// # Errors
///
/// Throws `TypeError` for operands without an integer value; [`OpError::Object`] for an object.
pub fn bit_or(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    bitwise(
        left,
        "|",
        right,
        cx,
        |a, b| a | b,
        |a, b| a | b,
        Span::Longer,
    )
}

/// `left & right`: on two strings, byte by byte over the shorter length; otherwise on the operands as
/// integers.
///
/// # Errors
///
/// As [`bit_or`].
pub fn bit_and(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    bitwise(
        left,
        "&",
        right,
        cx,
        |a, b| a & b,
        |a, b| a & b,
        Span::Shorter,
    )
}

/// `left ^ right`: on two strings, byte by byte over the shorter length; otherwise on the operands as
/// integers.
///
/// # Errors
///
/// As [`bit_or`].
pub fn bit_xor(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    bitwise(
        left,
        "^",
        right,
        cx,
        |a, b| a ^ b,
        |a, b| a ^ b,
        Span::Shorter,
    )
}

/// Both operands as integers and the shift count, or the error for a negative count.
fn shift_operands(
    left: &Value,
    operator: &'static str,
    right: &Value,
    cx: &mut impl Context,
) -> Result<(i64, Option<u32>), OpError> {
    let (value, count) = integers(left, operator, right, cx)?;
    if count < 0 {
        return Err(Thrown::NegativeShift.into());
    }
    Ok((
        value,
        u32::try_from(count)
            .ok()
            .filter(|&c| u64::from(c) < INT_BITS),
    ))
}

/// `left << right` on the operands as integers; bits shifted out are lost, and a count of 64 or more
/// gives 0.
///
/// # Errors
///
/// Throws `ArithmeticError` for a negative count, and as [`bit_or`] otherwise.
pub fn shift_left(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    let (value, count) = shift_operands(left, "<<", right, cx)?;
    Ok(Value::Int(count.map_or(0, |count| value << count)))
}

/// `left >> right` on the operands as integers, keeping the sign; a count of 64 or more gives 0 or
/// -1.
///
/// # Errors
///
/// As [`shift_left`].
pub fn shift_right(left: &Value, right: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    let (value, count) = shift_operands(left, ">>", right, cx)?;
    Ok(Value::Int(
        count.map_or(if value < 0 { -1 } else { 0 }, |count| value >> count),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{Collect, Diagnostic};
    use crate::{ObjectStore, PhpArray};

    fn run(
        op: fn(&Value, &Value, &mut Collect) -> Result<Value, OpError>,
        a: impl Into<Value>,
        b: impl Into<Value>,
    ) -> Result<Value, OpError> {
        op(&a.into(), &b.into(), &mut Collect::default())
    }

    fn int(result: &Result<Value, OpError>) -> Option<i64> {
        match result {
            Ok(Value::Int(n)) => Some(*n),
            _ => None,
        }
    }

    fn bytes(result: &Result<Value, OpError>) -> Option<Vec<u8>> {
        match result {
            Ok(Value::String(s)) => Some(s.as_bytes().to_vec()),
            _ => None,
        }
    }

    #[test]
    fn complement_covers_numbers_and_strings() {
        let mut cx = Collect::default();
        assert!(matches!(
            bit_not(&Value::Int(0), &mut cx),
            Ok(Value::Int(-1))
        ));
        assert!(matches!(
            bit_not(&Value::Float(2.5), &mut cx),
            Ok(Value::Int(-3))
        ));
        assert_eq!(cx.diagnostics, [Diagnostic::FloatToIntLosesPrecision(2.5)]);
        assert_eq!(
            bytes(&bit_not(&Value::from("\x00\x7f"), &mut cx)),
            Some(vec![0xff, 0x80])
        );
        assert_eq!(
            bit_not(&Value::Bool(false), &mut cx).err(),
            Some(Thrown::BitwiseNot("false").into())
        );
        assert_eq!(
            bit_not(&Value::from(PhpArray::new()), &mut cx).err(),
            Some(Thrown::BitwiseNot("array").into())
        );
        let store = ObjectStore::new();
        let object = Value::from(store.create("C".into()).unwrap());
        assert_eq!(bit_not(&object, &mut cx).err(), Some(OpError::Object));
    }

    #[test]
    fn strings_combine_bytewise() {
        assert_eq!(bytes(&run(bit_or, "a", "AB")), Some(b"aB".to_vec()));
        assert_eq!(bytes(&run(bit_or, "AB", "a")), Some(b"aB".to_vec()));
        assert_eq!(bytes(&run(bit_and, "ab", "c")), Some(b"a".to_vec()));
        assert_eq!(bytes(&run(bit_xor, "ab", "ab")), Some(b"\0\0".to_vec()));
        assert_eq!(int(&run(bit_or, 1, "2")), Some(3));
        assert_eq!(int(&run(bit_and, 6, 3.0)), Some(2));
        assert_eq!(int(&run(bit_xor, true, 3)), Some(2));
        assert!(run(bit_or, "a", PhpArray::new()).is_err());
    }

    #[test]
    fn shifts_saturate_at_the_width() {
        assert_eq!(int(&run(shift_left, 1, 63)), Some(i64::MIN));
        assert_eq!(int(&run(shift_left, 1, 64)), Some(0));
        assert_eq!(int(&run(shift_right, -8, 1)), Some(-4));
        assert_eq!(int(&run(shift_right, -8, 64)), Some(-1));
        assert_eq!(int(&run(shift_right, 8, i64::MAX)), Some(0));
        assert_eq!(
            run(shift_left, 1, -1).err(),
            Some(Thrown::NegativeShift.into())
        );
        assert_eq!(
            run(shift_right, 1, -1).err(),
            Some(Thrown::NegativeShift.into())
        );
    }
}
