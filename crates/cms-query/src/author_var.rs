//! The `author` and `author_name` query vars after `parse_query`.

use php_value::PhpString;
use php_value::Value;
use php_value::ops::Collect;
use php_value::ops::loose_equals;

/// The parsed author query vars (`contracts/query/author.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorVar {
    ids: Box<[u8]>,
    name: Box<[u8]>,
}

impl AuthorVar {
    /// Parse optional raw bytes the way `parse_query` reads the two keys.
    ///
    /// `author` keeps digits, comma and minus. `author_name` is stored as-is.
    #[must_use]
    pub fn parse(author: Option<&[u8]>, name: Option<&[u8]>) -> Self {
        Self {
            ids: author.map(keep_id_list).unwrap_or_default(),
            name: name.map(Box::from).unwrap_or_default(),
        }
    }

    /// The stored `author` bytes after the id-list filter.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.ids
    }

    /// The stored `author_name` bytes (not trimmed).
    #[must_use]
    pub fn name(&self) -> &[u8] {
        &self.name
    }

    /// Whether this is an author archive (`is_author`).
    #[must_use]
    pub fn is_author(&self) -> bool {
        author_ids(&self.ids) || !self.name.is_empty()
    }
}

fn keep_id_list(bytes: &[u8]) -> Box<[u8]> {
    bytes
        .iter()
        .copied()
        .filter(|byte| byte.is_ascii_digit() || *byte == b',' || *byte == b'-')
        .collect()
}

fn author_ids(bytes: &[u8]) -> bool {
    !php_empty(bytes) && !loosely_zero(bytes)
}

fn php_empty(bytes: &[u8]) -> bool {
    bytes.is_empty() || bytes == b"0"
}

fn loosely_zero(bytes: &[u8]) -> bool {
    let zero = Value::from("0");
    let value = Value::from(PhpString::from(bytes));
    let cx = Collect::default();
    loose_equals(&zero, &value, &cx).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::AuthorVar;

    #[test]
    fn missing_empty_and_zero() {
        let missing = AuthorVar::parse(None, None);
        assert_eq!(missing.as_bytes(), b"");
        assert_eq!(missing.name(), b"");
        assert!(!missing.is_author());
        assert!(!AuthorVar::parse(Some(b""), None).is_author());
        assert!(!AuthorVar::parse(Some(b"0"), None).is_author());
        assert!(!AuthorVar::parse(Some(b"00"), None).is_author());
        assert!(!AuthorVar::parse(Some(b"abc"), None).is_author());
    }

    #[test]
    fn lists_and_name() {
        let ids = AuthorVar::parse(Some(b"1,2"), None);
        assert_eq!(ids.as_bytes(), b"1,2");
        assert!(ids.is_author());
        assert!(AuthorVar::parse(Some(b"-3"), None).is_author());
        assert!(AuthorVar::parse(Some(b"01"), None).is_author());
        assert_eq!(AuthorVar::parse(Some(b"a1b,c2"), None).as_bytes(), b"1,2");
        assert!(AuthorVar::parse(Some(b"0,1"), None).is_author());
        assert!(!AuthorVar::parse(Some(b"-0"), None).is_author());
        let named = AuthorVar::parse(Some(b"0"), Some(b" alice "));
        assert_eq!(named.name(), b" alice ");
        assert!(named.is_author());
        assert!(AuthorVar::parse(None, Some(b"0")).is_author());
    }
}
