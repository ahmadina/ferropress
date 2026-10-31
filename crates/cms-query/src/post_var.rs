//! The `p` query var after `parse_query`.

use php_value::PhpString;
use php_value::Value;
use php_value::ops::Collect;

/// The parsed `p` query var (`contracts/query/p.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostVar {
    value: i64,
    not_found: bool,
}

impl PostVar {
    /// Parse optional raw bytes the way `parse_query` reads `p`.
    ///
    /// Missing or empty bytes are `0` and not a 404. A PHP `(int)` less than
    /// zero is `0` and a 404. Any other `(int)` is stored as-is.
    #[must_use]
    pub fn parse(raw: Option<&[u8]>) -> Self {
        let value = raw.map_or(0, php_int);
        if value < 0 {
            Self {
                value: 0,
                not_found: true,
            }
        } else {
            Self {
                value,
                not_found: false,
            }
        }
    }

    /// The stored integer after parsing (0 when missing or a 404).
    #[must_use]
    pub const fn value(self) -> i64 {
        self.value
    }

    /// Whether this is a single-post request (`p` greater than zero).
    #[must_use]
    pub const fn is_single(self) -> bool {
        self.value > 0
    }

    /// Whether parse set the 404 error (`p` was a negative integer).
    #[must_use]
    pub const fn is_not_found(self) -> bool {
        self.not_found
    }
}

fn php_int(bytes: &[u8]) -> i64 {
    let value = Value::from(PhpString::from(bytes));
    let mut cx = Collect::default();
    value.to_int(&mut cx).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::PostVar;

    #[test]
    fn missing_empty_and_zero() {
        let missing = PostVar::parse(None);
        assert_eq!(missing.value(), 0);
        assert!(!missing.is_single());
        assert!(!missing.is_not_found());
        let empty = PostVar::parse(Some(b""));
        assert_eq!(empty.value(), 0);
        assert!(!empty.is_single());
        assert!(!empty.is_not_found());
        let zero = PostVar::parse(Some(b"0"));
        assert_eq!(zero.value(), 0);
        assert!(!zero.is_single());
        assert!(!zero.is_not_found());
    }

    #[test]
    fn positive_and_leading_numeric() {
        let parsed = PostVar::parse(Some(b"12"));
        assert_eq!(parsed.value(), 12);
        assert!(parsed.is_single());
        assert!(!parsed.is_not_found());
        assert_eq!(PostVar::parse(Some(b"12abc")).value(), 12);
        assert_eq!(PostVar::parse(Some(b" 7")).value(), 7);
        assert!(PostVar::parse(Some(b"1")).is_single());
    }

    #[test]
    fn negative_is_not_found() {
        let parsed = PostVar::parse(Some(b"-1"));
        assert_eq!(parsed.value(), 0);
        assert!(!parsed.is_single());
        assert!(parsed.is_not_found());
        assert!(PostVar::parse(Some(b"-12")).is_not_found());
        assert!(!PostVar::parse(Some(b"foo")).is_not_found());
        assert_eq!(PostVar::parse(Some(b"foo")).value(), 0);
    }
}
