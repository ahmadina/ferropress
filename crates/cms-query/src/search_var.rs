//! The `s` query var after `parse_query`.

/// Byte cap `parse_query` applies to the `s` query var.
const SEARCH_MAX: usize = 1600;

/// The parsed `s` query var (`contracts/query/search.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchVar {
    value: Box<[u8]>,
    present: bool,
}

impl SearchVar {
    /// Parse optional raw bytes the way `parse_query` reads `s`.
    ///
    /// Missing bytes are empty and not a search. A supplied scalar is stored
    /// as-is when its length is at most 1600 bytes; a longer value is stored
    /// empty. Either supplied case is a search.
    #[must_use]
    pub fn parse(raw: Option<&[u8]>) -> Self {
        match raw {
            None => Self {
                value: Box::default(),
                present: false,
            },
            Some(bytes) => Self {
                value: if bytes.len() > SEARCH_MAX {
                    Box::default()
                } else {
                    Box::from(bytes)
                },
                present: true,
            },
        }
    }

    /// The stored search bytes after the length cap.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.value
    }

    /// Whether the caller supplied `s` (`is_search`).
    #[must_use]
    pub const fn is_search(&self) -> bool {
        self.present
    }
}

impl AsRef<[u8]> for SearchVar {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::{SEARCH_MAX, SearchVar};

    #[test]
    fn missing_is_not_search() {
        let missing = SearchVar::parse(None);
        assert_eq!(missing.as_bytes(), b"");
        assert!(!missing.is_search());
    }

    #[test]
    fn present_is_search_even_when_empty() {
        let empty = SearchVar::parse(Some(b""));
        assert_eq!(empty.as_bytes(), b"");
        assert!(empty.is_search());
        let zero = SearchVar::parse(Some(b"0"));
        assert_eq!(zero.as_bytes(), b"0");
        assert!(zero.is_search());
    }

    #[test]
    fn length_cap_keeps_then_wipes() {
        let kept = vec![b'a'; SEARCH_MAX];
        let parsed = SearchVar::parse(Some(&kept));
        assert_eq!(parsed.as_bytes(), kept.as_slice());
        assert!(parsed.is_search());
        let wiped = vec![b'a'; SEARCH_MAX + 1];
        let parsed = SearchVar::parse(Some(&wiped));
        assert_eq!(parsed.as_bytes(), b"");
        assert!(parsed.is_search());
        assert_eq!(SearchVar::parse(Some(b" hello ")).as_ref(), b" hello ");
    }
}
