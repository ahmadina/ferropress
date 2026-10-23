//! PHP strings: immutable byte sequences shared on clone.

use std::borrow::Borrow;
use std::fmt;
use std::rc::Rc;

/// A PHP string: bytes in no particular encoding, immutable and shared between clones.
///
/// Cloning only increments a reference count; a changed string is a new `PhpString`.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PhpString(Rc<[u8]>);

impl PhpString {
    /// Copies `bytes` into a new string.
    pub fn new(bytes: &[u8]) -> Self {
        Self(Rc::from(bytes))
    }

    /// The string's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the string has no bytes.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Default for PhpString {
    fn default() -> Self {
        Self::new(b"")
    }
}

impl From<&[u8]> for PhpString {
    fn from(bytes: &[u8]) -> Self {
        Self::new(bytes)
    }
}

impl From<&str> for PhpString {
    fn from(text: &str) -> Self {
        Self::new(text.as_bytes())
    }
}

impl From<Vec<u8>> for PhpString {
    fn from(bytes: Vec<u8>) -> Self {
        Self(Rc::from(bytes))
    }
}

impl AsRef<[u8]> for PhpString {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Borrow<[u8]> for PhpString {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for PhpString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "b\"{}\"", self.0.escape_ascii())
    }
}

#[cfg(test)]
mod tests {
    use super::PhpString;
    use std::collections::HashSet;

    #[test]
    fn strings_hold_arbitrary_bytes() {
        let text = PhpString::new(b"a\0\xff");
        assert_eq!(text.as_bytes(), b"a\0\xff");
        assert_eq!(text.len(), 3);
        assert!(!text.is_empty());
        assert!(PhpString::default().is_empty());
        assert_eq!(format!("{text:?}"), r#"b"a\x00\xff""#);
    }

    #[test]
    fn conversions_agree_and_clones_share_storage() {
        let from_str = PhpString::from("key");
        assert_eq!(from_str, PhpString::from(&b"key"[..]));
        assert_eq!(from_str, PhpString::from(b"key".to_vec()));
        assert_eq!(from_str.as_ref(), b"key");
        let clone = from_str.clone();
        assert!(std::ptr::eq(clone.as_bytes(), from_str.as_bytes()));
        let set: HashSet<PhpString> = [from_str].into_iter().collect();
        assert!(set.contains(&b"key"[..]));
        assert!(PhpString::from("a") < PhpString::from("b"));
    }
}
