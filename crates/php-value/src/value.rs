//! PHP values.

use crate::{Object, PhpArray, PhpString};

/// A PHP value.
///
/// Cloning has PHP's assignment semantics: strings and arrays are shared until one side is changed
/// (copy-on-write), and objects are handles, so a clone names the same object.
#[derive(Clone, Debug, Default)]
pub enum Value {
    /// `null`.
    #[default]
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A signed 64-bit integer.
    Int(i64),
    /// A 64-bit float.
    Float(f64),
    /// A byte string.
    String(PhpString),
    /// An ordered array.
    Array(PhpArray),
    /// An object handle.
    Object(Object),
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<PhpString> for Value {
    fn from(value: PhpString) -> Self {
        Self::String(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}

impl From<PhpArray> for Value {
    fn from(value: PhpArray) -> Self {
        Self::Array(value)
    }
}

impl From<Object> for Value {
    fn from(value: Object) -> Self {
        Self::Object(value)
    }
}

#[cfg(test)]
mod tests {
    use super::Value;
    use crate::{ObjectStore, PhpArray, PhpString};

    #[test]
    fn conversions_pick_the_matching_variant() {
        assert!(matches!(Value::default(), Value::Null));
        assert!(matches!(Value::from(true), Value::Bool(true)));
        assert!(matches!(Value::from(3), Value::Int(3)));
        assert!(matches!(Value::from(1.5), Value::Float(x) if x.to_bits() == 1.5_f64.to_bits()));
        assert!(matches!(Value::from("a"), Value::String(s) if s.as_bytes() == b"a"));
        assert!(matches!(
            Value::from(PhpString::from("b")),
            Value::String(_)
        ));
        assert!(matches!(Value::from(PhpArray::new()), Value::Array(a) if a.is_empty()));
        let store = ObjectStore::new();
        let object = store.create("stdClass".into()).unwrap();
        let Value::Object(handle) = Value::from(object.clone()) else {
            panic!("an object converts to an object value");
        };
        assert_eq!(handle, object);
    }
}
