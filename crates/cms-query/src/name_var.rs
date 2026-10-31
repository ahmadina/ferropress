//! The `name` query var after `parse_query`.

/// Bytes PHP `trim` strips from both ends (space, tab, LF, CR, VT, NUL).
const TRIM: &[u8] = b" \t\n\r\x0b\0";

/// The parsed `name` query var (`contracts/query/name.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameVar {
    value: Box<[u8]>,
}

impl NameVar {
    /// Parse optional raw bytes the way `parse_query` reads `name`.
    ///
    /// Missing bytes are empty. A scalar is stored after PHP `trim`. Empty
    /// after trim is not a single-post request; any other bytes are.
    #[must_use]
    pub fn parse(raw: Option<&[u8]>) -> Self {
        Self {
            value: raw.map_or(&[][..], php_trim).into(),
        }
    }

    /// The stored slug bytes after trim.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.value
    }

    /// Whether this is a single-post request (`name` is not empty).
    #[must_use]
    pub fn is_single(&self) -> bool {
        !self.value.is_empty()
    }
}

impl AsRef<[u8]> for NameVar {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

fn php_trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !TRIM.contains(byte))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !TRIM.contains(byte))
        .map_or(start, |index| index + 1);
    if start >= end {
        &[]
    } else {
        &bytes[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::NameVar;

    #[test]
    fn missing_empty_and_whitespace() {
        let missing = NameVar::parse(None);
        assert_eq!(missing.as_bytes(), b"");
        assert!(!missing.is_single());
        let empty = NameVar::parse(Some(b""));
        assert_eq!(empty.as_bytes(), b"");
        assert!(!empty.is_single());
        let spaces = NameVar::parse(Some(b" \t\n\r\x0b\0"));
        assert_eq!(spaces.as_bytes(), b"");
        assert!(!spaces.is_single());
    }

    #[test]
    fn trimmed_and_zero() {
        let parsed = NameVar::parse(Some(b" hello "));
        assert_eq!(parsed.as_bytes(), b"hello");
        assert!(parsed.is_single());
        let zero = NameVar::parse(Some(b"0"));
        assert_eq!(zero.as_bytes(), b"0");
        assert!(zero.is_single());
        assert_eq!(NameVar::parse(Some(b"a b")).as_bytes(), b"a b");
        assert_eq!(NameVar::parse(Some(b"x")).as_ref(), b"x");
    }
}
