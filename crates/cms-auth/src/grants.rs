//! Stored capability names and grant flags.

use php_serialize::{Key, Value, decode};

/// One user's stored `caps` map (not role-merged `allcaps`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantMap {
    grants: Vec<(Box<[u8]>, bool)>,
}

impl GrantMap {
    /// No stored names.
    #[must_use]
    pub const fn empty() -> Self {
        Self { grants: Vec::new() }
    }

    /// The stored flag for `name`, or `None` when the name is absent.
    #[must_use]
    pub fn get(&self, name: &[u8]) -> Option<bool> {
        self.grants
            .iter()
            .rev()
            .find(|(stored, _)| stored.as_ref() == name)
            .map(|(_, granted)| *granted)
    }

    /// Whether `name` is present and granted.
    #[must_use]
    pub fn granted(&self, name: &[u8]) -> bool {
        self.get(name) == Some(true)
    }

    /// Stored pairs in written order.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], bool)> {
        self.grants
            .iter()
            .map(|(name, granted)| (name.as_ref(), *granted))
    }

    /// Whether no names were stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }

    /// Overlay `top` onto this map (PHP `array_merge` for string keys).
    #[must_use]
    pub(crate) fn overlay(&self, top: &Self) -> Self {
        let mut grants = self.grants.clone();
        for (name, granted) in &top.grants {
            if let Some((_, slot)) = grants.iter_mut().find(|(stored, _)| stored == name) {
                *slot = *granted;
            } else {
                grants.push((name.clone(), *granted));
            }
        }
        Self { grants }
    }
}

/// Decode a serialized array, or empty when the bytes are not one.
#[must_use]
pub(crate) fn grants_from_bytes(bytes: &[u8]) -> GrantMap {
    let Ok(document) = decode(bytes) else {
        return GrantMap::empty();
    };
    grants_from_value(&document.root())
}

/// Grant flags from a decoded array value.
#[must_use]
pub(crate) fn grants_from_value(value: &Value<'_, '_>) -> GrantMap {
    let Value::Array(entries) = value else {
        return GrantMap::empty();
    };
    let mut grants = Vec::new();
    for (key, nested) in entries.clone() {
        let Key::String(name) = key else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        grants.push((Box::from(name), is_granted(&nested)));
    }
    GrantMap { grants }
}

fn is_granted(value: &Value<'_, '_>) -> bool {
    match value {
        Value::Bool(granted) => *granted,
        Value::Null | Value::Int(0) => false,
        Value::Float(number) => *number != 0.0,
        Value::String(bytes) => !bytes.is_empty() && bytes != b"0",
        Value::Array(entries) => entries.len() != 0,
        Value::Int(_)
        | Value::Object { .. }
        | Value::Custom { .. }
        | Value::Enum { .. }
        | Value::Reference(_)
        | Value::ObjectReference(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{GrantMap, grants_from_bytes};

    #[test]
    fn serialized_pairs_and_empty_values() {
        let map = grants_from_bytes(b"a:2:{s:13:\"administrator\";b:1;s:10:\"edit_posts\";b:0;}");
        assert!(map.granted(b"administrator"));
        assert_eq!(map.get(b"edit_posts"), Some(false));
        assert!(!map.granted(b"edit_posts"));
        assert!(map.get(b"manage_options").is_none());
        assert!(!map.is_empty());
        let names: Vec<(&[u8], bool)> = map.iter().collect();
        assert_eq!(
            names,
            vec![
                (b"administrator".as_slice(), true),
                (b"edit_posts".as_slice(), false)
            ]
        );

        assert!(grants_from_bytes(b"").is_empty());
        assert!(grants_from_bytes(b"N;").is_empty());
        assert!(grants_from_bytes(b"s:3:\"yes\";").is_empty());
        assert!(grants_from_bytes(b"not-serialized").is_empty());

        let mixed = grants_from_bytes(
            b"a:8:{i:0;b:1;s:0:\"\";b:1;s:1:\"a\";b:1;s:1:\"b\";i:0;s:1:\"c\";i:1;s:1:\"d\";s:1:\"0\";s:1:\"e\";s:0:\"\";s:1:\"f\";N;}",
        );
        assert!(mixed.granted(b"a"));
        assert_eq!(mixed.get(b"b"), Some(false));
        assert!(mixed.granted(b"c"));
        assert!(!mixed.granted(b"d"));
        assert!(!mixed.granted(b"e"));
        assert!(!mixed.granted(b"f"));
        assert!(GrantMap::empty().is_empty());
        assert!(!GrantMap::empty().granted(b"a"));

        let objects = grants_from_bytes(
            b"a:4:{s:3:\"obj\";O:8:\"stdClass\":0:{}s:3:\"cst\";C:1:\"X\":0:{}s:3:\"enm\";E:3:\"A:B\";s:3:\"flt\";d:0;}",
        );
        assert!(objects.granted(b"obj"));
        assert!(objects.granted(b"cst"));
        assert!(objects.granted(b"enm"));
        assert!(!objects.granted(b"flt"));
        let nonzero = grants_from_bytes(b"a:1:{s:1:\"n\";d:1;}");
        assert!(nonzero.granted(b"n"));
        let arrays = grants_from_bytes(b"a:2:{s:4:\"full\";a:1:{i:0;i:1;}s:5:\"empty\";a:0:{}}");
        assert!(arrays.granted(b"full"));
        assert!(!arrays.granted(b"empty"));
        let refs = grants_from_bytes(b"a:2:{s:1:\"a\";b:1;s:1:\"b\";R:2;}");
        assert!(refs.granted(b"a"));
        assert!(refs.granted(b"b"));
    }

    #[test]
    fn later_entries_replace_earlier_ones() {
        let map = grants_from_bytes(b"a:2:{s:3:\"cap\";b:1;s:3:\"cap\";b:0;}");
        assert_eq!(map.get(b"cap"), Some(false));
        let base = grants_from_bytes(b"a:2:{s:1:\"a\";b:1;s:1:\"b\";b:1;}");
        let top = grants_from_bytes(b"a:2:{s:1:\"b\";b:0;s:1:\"c\";b:1;}");
        let merged = base.overlay(&top);
        assert!(merged.granted(b"a"));
        assert_eq!(merged.get(b"b"), Some(false));
        assert!(merged.granted(b"c"));
        let names: Vec<(&[u8], bool)> = merged.iter().collect();
        assert_eq!(
            names,
            vec![
                (b"a".as_slice(), true),
                (b"b".as_slice(), false),
                (b"c".as_slice(), true)
            ]
        );
    }
}
