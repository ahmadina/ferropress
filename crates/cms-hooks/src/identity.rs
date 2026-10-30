//! How two stored callables are told apart.

use php_value::{ObjectId, Value};

/// The key a callback is stored under at one priority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Identity {
    /// A function name, compared as exact bytes.
    Name(Box<[u8]>),
    /// An object (typically a `Closure`), compared by identity.
    Object(ObjectId),
    /// A value that cannot be named again for removal.
    Unique(u64),
}

impl Identity {
    pub(crate) fn of(value: &Value, next: &mut u64) -> Self {
        if let Some(id) = Self::named(value) {
            return id;
        }
        let id = *next;
        *next = next.saturating_add(1);
        Self::Unique(id)
    }

    pub(crate) fn named(value: &Value) -> Option<Self> {
        match value {
            Value::String(name) => Some(Self::Name(name.as_bytes().into())),
            Value::Object(object) => Some(Self::Object(object.id())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Identity;
    use php_value::{ObjectStore, PhpString, Value};

    #[test]
    fn strings_and_objects_are_stable() -> Result<(), String> {
        let mut next = 1;
        let name = Identity::of(&Value::from("inc"), &mut next);
        assert_eq!(name, Identity::of(&Value::from("inc"), &mut next));
        let store = ObjectStore::new();
        let object = store
            .create(PhpString::from("Closure"))
            .map_err(|error| error.to_string())?;
        let first = Identity::of(&Value::Object(object.clone()), &mut next);
        let second = Identity::of(&Value::Object(object), &mut next);
        assert_eq!(first, second);
        let a = Identity::of(&Value::Int(1), &mut next);
        let b = Identity::of(&Value::Int(1), &mut next);
        assert_ne!(a, b);
        Ok(())
    }
}
