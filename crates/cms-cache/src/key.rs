//! Cache keys after the reference's integer / non-empty-string check.

use crate::KeyError;

/// Bytes the reference's `trim` treats as empty for key validity.
const TRIM: &[u8] = b" \t\n\r\x0b\0";

/// A cache key. `0` and `"0"` are the same stored id.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey(Box<[u8]>);

impl CacheKey {
    /// Reject a string that is empty after PHP `trim`. The stored bytes are not trimmed.
    ///
    /// # Errors
    ///
    /// [`KeyError::Empty`] when every byte is a trim byte, or there are no bytes.
    pub fn new(bytes: impl AsRef<[u8]>) -> Result<Self, KeyError> {
        let bytes = bytes.as_ref();
        if !bytes.iter().any(|b| !TRIM.contains(b)) {
            return Err(KeyError::Empty);
        }
        Ok(Self(bytes.into()))
    }

    /// An integer key, including `0`. Stored as its decimal digits.
    #[must_use]
    pub fn from_int(value: i64) -> Self {
        Self(value.to_string().into_bytes().into())
    }

    /// The bytes as stored.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for CacheKey {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::CacheKey;
    use crate::KeyError;

    #[test]
    fn phpunit_key_cases_match() {
        assert!(matches!(CacheKey::new(""), Err(KeyError::Empty)));
        assert!(matches!(CacheKey::new(" "), Err(KeyError::Empty)));
        assert!(matches!(CacheKey::new("  "), Err(KeyError::Empty)));
        assert!(matches!(CacheKey::new("\n"), Err(KeyError::Empty)));
        assert!(matches!(CacheKey::new("\0"), Err(KeyError::Empty)));
        assert!(CacheKey::new("0").is_ok());
        assert!(CacheKey::new("key").is_ok());
        assert_eq!(CacheKey::from_int(0).as_bytes(), b"0");
        assert_eq!(CacheKey::from_int(1).as_bytes(), b"1");
        let padded = CacheKey::new(" a ");
        assert!(padded.is_ok());
        if let Ok(padded) = padded {
            assert_eq!(padded.as_bytes(), b" a ");
            assert_eq!(padded.as_ref(), b" a ");
        }
    }

    proptest::proptest! {
        #[test]
        fn a_non_trim_byte_makes_a_key(
            text in "[A-Za-z0-9._-]{1,32}",
        ) {
            let key = CacheKey::new(text.as_bytes());
            proptest::prop_assert!(key.is_ok());
            if let Ok(key) = key {
                proptest::prop_assert_eq!(key.as_bytes(), text.as_bytes());
            }
        }
    }
}
