//! The step operators `++` and `--`.

use super::numeric::{Number, whole_numeric};
use super::{Context, Diagnostic, OpError, Step, Thrown};
use crate::{PhpString, Value};

/// `number ± 1`, becoming a float past the integer range.
fn step_int(number: i64, step: Step) -> Value {
    let (stepped, by) = match step {
        Step::Increment => (number.checked_add(1), 1.0),
        Step::Decrement => (number.checked_sub(1), -1.0),
    };
    stepped.map_or_else(
        || Value::Float(Number::Int(number).as_float() + by),
        Value::Int,
    )
}

fn step_float(number: f64, step: Step) -> Value {
    Value::Float(match step {
        Step::Increment => number + 1.0,
        Step::Decrement => number - 1.0,
    })
}

/// The next string in the sequence `a`…`z`, `A`…`Z` and `0`…`9`, counted from the last byte: a byte at
/// the end of its range wraps and carries into the byte before it. A carry out of the first byte adds
/// a new first byte (`1`, `A` or `a`, after the range of the byte it left). A byte outside these
/// ranges stops the count where it is, and is left as it is.
fn increment_text(text: &[u8]) -> PhpString {
    let mut out = text.to_vec();
    let mut carry = None;
    for byte in out.iter_mut().rev() {
        let (first, last) = match *byte {
            b'a'..=b'z' => (b'a', b'z'),
            b'A'..=b'Z' => (b'A', b'Z'),
            b'0'..=b'9' => (b'0', b'9'),
            _ => return PhpString::from(out),
        };
        if *byte != last {
            *byte += 1;
            return PhpString::from(out);
        }
        *byte = first;
        carry = Some(first);
    }
    match carry {
        Some(b'0') => out.insert(0, b'1'),
        Some(first) => out.insert(0, first),
        None => out.push(b'1'),
    }
    PhpString::from(out)
}

/// The result of `++` on a value: `null` becomes 1, numbers and numeric strings grow by one, other
/// strings take the next string in sequence, and booleans stay as they are.
///
/// # Errors
///
/// Throws `TypeError` for an array; [`OpError::Object`] for an object.
pub fn increment(value: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    Ok(match value {
        Value::Null => Value::Int(1),
        Value::Bool(_) => {
            cx.raise(Diagnostic::StepHasNoEffect(Step::Increment, "bool"));
            value.clone()
        }
        Value::Int(number) => step_int(*number, Step::Increment),
        Value::Float(number) => step_float(*number, Step::Increment),
        Value::String(text) => match whole_numeric(text.as_bytes()).map(|p| p.number) {
            Some(Number::Int(number)) => step_int(number, Step::Increment),
            Some(Number::Float(number)) => step_float(number, Step::Increment),
            None => {
                cx.raise(Diagnostic::IncrementNonNumericString);
                Value::String(increment_text(text.as_bytes()))
            }
        },
        Value::Array(_) => return Err(Thrown::CannotStep(Step::Increment, "array").into()),
        Value::Object(_) => return Err(OpError::Object),
    })
}

/// The result of `--` on a value: numbers and numeric strings shrink by one, the empty string becomes
/// -1, and `null`, booleans and other strings stay as they are.
///
/// # Errors
///
/// Throws `TypeError` for an array; [`OpError::Object`] for an object.
pub fn decrement(value: &Value, cx: &mut impl Context) -> Result<Value, OpError> {
    Ok(match value {
        Value::Null => {
            cx.raise(Diagnostic::StepHasNoEffect(Step::Decrement, "null"));
            Value::Null
        }
        Value::Bool(_) => {
            cx.raise(Diagnostic::StepHasNoEffect(Step::Decrement, "bool"));
            value.clone()
        }
        Value::Int(number) => step_int(*number, Step::Decrement),
        Value::Float(number) => step_float(*number, Step::Decrement),
        Value::String(text) if text.is_empty() => {
            cx.raise(Diagnostic::DecrementEmptyString);
            Value::Int(-1)
        }
        Value::String(text) => match whole_numeric(text.as_bytes()).map(|p| p.number) {
            Some(Number::Int(number)) => step_int(number, Step::Decrement),
            Some(Number::Float(number)) => step_float(number, Step::Decrement),
            None => {
                cx.raise(Diagnostic::DecrementNonNumericString);
                value.clone()
            }
        },
        Value::Array(_) => return Err(Thrown::CannotStep(Step::Decrement, "array").into()),
        Value::Object(_) => return Err(OpError::Object),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObjectStore;
    use crate::ops::Collect;

    fn text_after(text: &str) -> String {
        String::from_utf8(increment_text(text.as_bytes()).as_bytes().to_vec()).unwrap()
    }

    #[test]
    fn strings_count_through_letters_and_digits() {
        let cases = [
            ("", "1"),
            ("a", "b"),
            ("z", "aa"),
            ("Az", "Ba"),
            ("zZ9", "aaA0"),
            ("Zz", "AAa"),
            ("9", "10"),
            ("a9", "b0"),
            ("a-z", "a-a"),
            ("z-", "z-"),
            ("-z", "-a"),
        ];
        for (text, next) in cases {
            assert_eq!(text_after(text), next, "{text:?}");
        }
    }

    #[test]
    fn increment_follows_the_type() {
        let mut cx = Collect::default();
        let inc = |value: Value, cx: &mut Collect| increment(&value, cx).unwrap();
        assert!(matches!(inc(Value::Null, &mut cx), Value::Int(1)));
        assert!(matches!(inc(Value::Int(1), &mut cx), Value::Int(2)));
        assert!(
            matches!(inc(Value::Int(i64::MAX), &mut cx), Value::Float(x) if x.to_bits() == 9.223_372_036_854_776e18_f64.to_bits())
        );
        assert!(
            matches!(inc(Value::Float(0.5), &mut cx), Value::Float(x) if x.to_bits() == 1.5_f64.to_bits())
        );
        assert!(matches!(inc(Value::from(" 5 "), &mut cx), Value::Int(6)));
        assert!(matches!(
            inc(Value::from("9223372036854775807"), &mut cx),
            Value::Float(_)
        ));
        assert!(
            matches!(inc(Value::from("1.5"), &mut cx), Value::Float(x) if x.to_bits() == 2.5_f64.to_bits())
        );
        assert!(cx.diagnostics.is_empty());
        assert!(matches!(inc(Value::Bool(true), &mut cx), Value::Bool(true)));
        assert!(
            matches!(inc(Value::from("5a"), &mut cx), Value::String(s) if s.as_bytes() == b"5b")
        );
        assert_eq!(
            cx.diagnostics,
            [
                Diagnostic::StepHasNoEffect(Step::Increment, "bool"),
                Diagnostic::IncrementNonNumericString
            ]
        );
        assert_eq!(
            increment(&Value::Array(crate::PhpArray::new()), &mut cx).err(),
            Some(Thrown::CannotStep(Step::Increment, "array").into())
        );
    }

    #[test]
    fn decrement_follows_the_type() {
        let mut cx = Collect::default();
        let dec = |value: Value, cx: &mut Collect| decrement(&value, cx).unwrap();
        assert!(matches!(dec(Value::Int(1), &mut cx), Value::Int(0)));
        assert!(
            matches!(dec(Value::Int(i64::MIN), &mut cx), Value::Float(x) if x.to_bits() == (-9.223_372_036_854_776e18_f64).to_bits())
        );
        assert!(
            matches!(dec(Value::Float(0.5), &mut cx), Value::Float(x) if x.to_bits() == (-0.5_f64).to_bits())
        );
        assert!(matches!(
            dec(Value::from("-9223372036854775808"), &mut cx),
            Value::Float(_)
        ));
        assert!(
            matches!(dec(Value::from("1e1"), &mut cx), Value::Float(x) if x.to_bits() == 9.0_f64.to_bits())
        );
        assert!(matches!(dec(Value::from("7"), &mut cx), Value::Int(6)));
        assert!(cx.diagnostics.is_empty());
        assert!(matches!(dec(Value::Null, &mut cx), Value::Null));
        assert!(matches!(
            dec(Value::Bool(false), &mut cx),
            Value::Bool(false)
        ));
        assert!(matches!(dec(Value::from(""), &mut cx), Value::Int(-1)));
        assert!(matches!(dec(Value::from("b"), &mut cx), Value::String(s) if s.as_bytes() == b"b"));
        assert_eq!(
            cx.diagnostics,
            [
                Diagnostic::StepHasNoEffect(Step::Decrement, "null"),
                Diagnostic::StepHasNoEffect(Step::Decrement, "bool"),
                Diagnostic::DecrementEmptyString,
                Diagnostic::DecrementNonNumericString
            ]
        );
        assert_eq!(
            decrement(&Value::Array(crate::PhpArray::new()), &mut cx).err(),
            Some(Thrown::CannotStep(Step::Decrement, "array").into())
        );
    }

    #[test]
    fn objects_step_through_their_class() {
        let store = ObjectStore::new();
        let object = Value::from(store.create("C".into()).unwrap());
        let mut cx = Collect::default();
        assert_eq!(increment(&object, &mut cx).err(), Some(OpError::Object));
        assert_eq!(decrement(&object, &mut cx).err(), Some(OpError::Object));
    }
}
