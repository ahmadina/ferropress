//! Converting a value to the array key it names, as array literals and array offsets do.

use super::convert::float_to_int_operand;
use super::{Context, Diagnostic};
use crate::{ArrayKey, PhpString, Value};

/// The array key `value` names, or `None` for an array or object, which cannot be keys.
///
/// `null` is the empty string, with a deprecation; booleans are 0 and 1; a float converts as an
/// integer operand (truncated, with a deprecation when a fraction is lost and a warning when it is
/// not representable); a string that is a canonical integer is that integer.
///
/// ```
/// use php_value::{ArrayKey, Value};
/// use php_value::ops::{Collect, array_key};
///
/// let mut cx = Collect::default();
/// assert_eq!(array_key(&Value::from("5"), &mut cx), Some(ArrayKey::Int(5)));
/// assert_eq!(array_key(&Value::Bool(true), &mut cx), Some(ArrayKey::Int(1)));
/// assert!(cx.diagnostics.is_empty());
/// ```
pub fn array_key(value: &Value, cx: &mut impl Context) -> Option<ArrayKey> {
    Some(match value {
        Value::Null => {
            cx.raise(Diagnostic::NullOffset);
            ArrayKey::String(PhpString::new(b""))
        }
        Value::Bool(value) => ArrayKey::Int(i64::from(*value)),
        Value::Int(value) => ArrayKey::Int(*value),
        Value::Float(value) => ArrayKey::Int(float_to_int_operand(*value, cx)),
        Value::String(text) => ArrayKey::from_string(text.clone()),
        Value::Array(_) | Value::Object(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::array_key;
    use crate::ops::{Collect, Diagnostic};
    use crate::{ArrayKey, ObjectStore, PhpArray, PhpString, Value};

    #[test]
    fn keys_convert_with_the_reference_diagnostics() {
        let cases = [
            (
                Value::Null,
                ArrayKey::String(PhpString::new(b"")),
                vec![Diagnostic::NullOffset],
            ),
            (Value::Bool(false), ArrayKey::Int(0), vec![]),
            (Value::Int(-4), ArrayKey::Int(-4), vec![]),
            (Value::Float(2.0), ArrayKey::Int(2), vec![]),
            (
                Value::Float(1.5),
                ArrayKey::Int(1),
                vec![Diagnostic::FloatToIntLosesPrecision(1.5)],
            ),
            (
                Value::Float(f64::INFINITY),
                ArrayKey::Int(0),
                vec![Diagnostic::FloatNotRepresentable(f64::INFINITY)],
            ),
            (Value::from("8"), ArrayKey::Int(8), vec![]),
            (
                Value::from("08"),
                ArrayKey::String(PhpString::new(b"08")),
                vec![],
            ),
        ];
        for (value, key, diagnostics) in cases {
            let mut cx = Collect::default();
            assert_eq!(array_key(&value, &mut cx), Some(key), "{value:?}");
            assert_eq!(cx.diagnostics, diagnostics, "{value:?}");
        }
    }

    #[test]
    fn nan_keys_warn_and_lose_precision() {
        let mut cx = Collect::default();
        assert_eq!(
            array_key(&Value::Float(f64::NAN), &mut cx),
            Some(ArrayKey::Int(0))
        );
        assert!(matches!(
            cx.diagnostics.as_slice(),
            [Diagnostic::FloatNotRepresentable(a), Diagnostic::FloatToIntLosesPrecision(b)]
                if a.is_nan() && b.is_nan()
        ));
    }

    #[test]
    fn arrays_and_objects_are_not_keys() {
        let mut cx = Collect::default();
        let object = ObjectStore::new().create("C".into()).unwrap();
        assert_eq!(array_key(&Value::from(PhpArray::new()), &mut cx), None);
        assert_eq!(array_key(&Value::from(object), &mut cx), None);
        assert!(cx.diagnostics.is_empty());
    }
}
