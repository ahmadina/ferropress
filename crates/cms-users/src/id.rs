//! Stored identifiers greater than zero.

use crate::{IdError, MetaIdError};

/// A stored user id. Zero is rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UserId(u64);

impl UserId {
    /// An id that can name a row.
    ///
    /// # Errors
    ///
    /// [`IdError::Zero`] when `id` is `0`.
    pub const fn new(id: u64) -> Result<Self, IdError> {
        if id == 0 {
            return Err(IdError::Zero);
        }
        Ok(Self(id))
    }

    /// Parse a decimal id from the `ID` cell.
    ///
    /// # Errors
    ///
    /// [`IdError::Zero`] when the bytes are not a positive integer.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, IdError> {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Err(IdError::Zero);
        };
        let Ok(id) = text.parse::<u64>() else {
            return Err(IdError::Zero);
        };
        Self::new(id)
    }

    /// The integer as stored.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// A stored `umeta_id`. Zero is rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MetaId(u64);

impl MetaId {
    /// An id that can name a usermeta row.
    ///
    /// # Errors
    ///
    /// [`MetaIdError::Zero`] when `id` is `0`.
    pub const fn new(id: u64) -> Result<Self, MetaIdError> {
        if id == 0 {
            return Err(MetaIdError::Zero);
        }
        Ok(Self(id))
    }

    /// Parse a decimal id from the `umeta_id` cell.
    ///
    /// # Errors
    ///
    /// [`MetaIdError::Zero`] when the bytes are not a positive integer.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MetaIdError> {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Err(MetaIdError::Zero);
        };
        let Ok(id) = text.parse::<u64>() else {
            return Err(MetaIdError::Zero);
        };
        Self::new(id)
    }

    /// The integer as stored.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{MetaId, UserId};
    use crate::{IdError, MetaIdError};

    #[test]
    fn zero_and_junk_are_rejected() {
        assert!(matches!(UserId::new(0), Err(IdError::Zero)));
        assert!(UserId::new(1).is_ok());
        assert!(matches!(UserId::from_bytes(b"0"), Err(IdError::Zero)));
        assert!(matches!(UserId::from_bytes(b""), Err(IdError::Zero)));
        assert!(matches!(UserId::from_bytes(b"x"), Err(IdError::Zero)));
        assert!(matches!(UserId::from_bytes(&[0xff]), Err(IdError::Zero)));
        let one = UserId::from_bytes(b"12");
        assert!(one.is_ok());
        if let Ok(one) = one {
            assert_eq!(one.as_u64(), 12);
        }
        assert!(matches!(MetaId::new(0), Err(MetaIdError::Zero)));
        assert!(MetaId::new(1).is_ok());
        assert!(matches!(MetaId::from_bytes(b"0"), Err(MetaIdError::Zero)));
        assert!(matches!(MetaId::from_bytes(b""), Err(MetaIdError::Zero)));
        assert!(matches!(MetaId::from_bytes(b"x"), Err(MetaIdError::Zero)));
        assert!(matches!(
            MetaId::from_bytes(&[0xff]),
            Err(MetaIdError::Zero)
        ));
        let meta = MetaId::from_bytes(b"7");
        assert!(meta.is_ok());
        if let Ok(meta) = meta {
            assert_eq!(meta.as_u64(), 7);
        }
    }
}
