//! User identifiers greater than zero.

use crate::IdError;

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

#[cfg(test)]
mod tests {
    use super::UserId;
    use crate::IdError;

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
    }
}
