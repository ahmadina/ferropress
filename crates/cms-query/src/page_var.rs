//! The `page_id` query var after `parse_query`.

use php_value::PhpString;
use php_value::Value;
use php_value::ops::Collect;

/// The parsed `page_id` query var (`contracts/query/page-id.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageVar {
    value: i64,
}

impl PageVar {
    /// Parse optional raw bytes the way `parse_query` reads `page_id`.
    ///
    /// Missing or empty bytes are `0`. A scalar is `absint`: PHP `(int)` then
    /// the absolute value. Negatives are not a 404.
    #[must_use]
    pub fn parse(raw: Option<&[u8]>) -> Self {
        Self {
            value: raw.map_or(0, absint),
        }
    }

    /// The stored non-negative integer after parsing.
    #[must_use]
    pub const fn value(self) -> i64 {
        self.value
    }

    /// Whether this is a page request (`page_id` greater than zero).
    #[must_use]
    pub const fn is_page(self) -> bool {
        self.value > 0
    }
}

fn absint(bytes: &[u8]) -> i64 {
    php_int(bytes).saturating_abs()
}

fn php_int(bytes: &[u8]) -> i64 {
    let value = Value::from(PhpString::from(bytes));
    let mut cx = Collect::default();
    value.to_int(&mut cx).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::PageVar;

    #[test]
    fn missing_empty_and_zero() {
        let missing = PageVar::parse(None);
        assert_eq!(missing.value(), 0);
        assert!(!missing.is_page());
        let empty = PageVar::parse(Some(b""));
        assert_eq!(empty.value(), 0);
        assert!(!empty.is_page());
        let zero = PageVar::parse(Some(b"0"));
        assert_eq!(zero.value(), 0);
        assert!(!zero.is_page());
    }

    #[test]
    fn positive_and_absolute() {
        let parsed = PageVar::parse(Some(b"12"));
        assert_eq!(parsed.value(), 12);
        assert!(parsed.is_page());
        let negative = PageVar::parse(Some(b"-4"));
        assert_eq!(negative.value(), 4);
        assert!(negative.is_page());
        assert_eq!(PageVar::parse(Some(b"12abc")).value(), 12);
        assert_eq!(PageVar::parse(Some(b" 7")).value(), 7);
        assert_eq!(PageVar::parse(Some(b"-3foo")).value(), 3);
    }
}
