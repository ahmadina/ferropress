//! User-meta keys. Empty keys (the "all keys" form) are not bound.

use crate::KeyError;

/// A stored `meta_key`. `Nickname` and `nickname` are different keys.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MetaKey(Box<[u8]>);

impl MetaKey {
    /// A non-empty key, as stored. Bytes are not trimmed.
    ///
    /// # Errors
    ///
    /// [`KeyError::Empty`] when `bytes` is empty.
    pub fn new(bytes: impl AsRef<[u8]>) -> Result<Self, KeyError> {
        let bytes = bytes.as_ref();
        if bytes.is_empty() {
            return Err(KeyError::Empty);
        }
        Ok(Self(bytes.into()))
    }

    /// The bytes as stored.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// `{prefix}capabilities` — the stored capability-map key.
    ///
    /// The suffix is never empty, so the result is always a usable key.
    #[must_use]
    pub fn capabilities(prefix: impl AsRef<[u8]>) -> Self {
        let prefix = prefix.as_ref();
        let mut bytes = Vec::with_capacity(prefix.len().saturating_add(12));
        bytes.extend_from_slice(prefix);
        bytes.extend_from_slice(b"capabilities");
        Self(bytes.into())
    }
}

#[cfg(test)]
mod tests {
    use super::MetaKey;
    use crate::KeyError;

    #[test]
    fn empty_is_rejected_and_other_bytes_are_kept() {
        assert!(matches!(MetaKey::new(""), Err(KeyError::Empty)));
        assert!(matches!(MetaKey::new([]), Err(KeyError::Empty)));
        let spaced = MetaKey::new(" nickname ");
        assert!(spaced.is_ok());
        if let Ok(spaced) = spaced {
            assert_eq!(spaced.as_bytes(), b" nickname ");
        }
        let zero = MetaKey::new("0");
        assert!(zero.is_ok());
        if let Ok(zero) = zero {
            assert_eq!(zero.as_bytes(), b"0");
        }
        let nick = MetaKey::new("nickname");
        let other = MetaKey::new("Nickname");
        assert!(nick.is_ok() && other.is_ok());
        if let (Ok(nick), Ok(other)) = (nick, other) {
            assert_ne!(nick, other);
        }
        assert_eq!(MetaKey::capabilities("wp_").as_bytes(), b"wp_capabilities");
        assert_eq!(MetaKey::capabilities("").as_bytes(), b"capabilities");
    }
}
