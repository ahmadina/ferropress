//! Platform failures from the `MySQL` adapter.

use cms_db::StoreError;

/// The adapter could not reach a server or the connection is unusable.
///
/// Server-reported SQL failures are [`cms_db::Reply::Failed`], not this type.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// No usable connection (I/O, protocol, or URL).
    #[error("MySQL store is unavailable")]
    Unavailable,
}

impl Error {
    /// Whether this is [`Self::Unavailable`].
    #[must_use]
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "matches cms_db::StoreError::is_unavailable"
    )]
    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable)
    }
}

impl From<Error> for StoreError {
    fn from(error: Error) -> Self {
        match error {
            Error::Unavailable => Self::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use cms_db::StoreError;

    #[test]
    fn unavailable_maps_to_the_store_port() {
        let error = Error::Unavailable;
        assert!(error.is_unavailable());
        assert_eq!(error.to_string(), "MySQL store is unavailable");
        assert!(StoreError::from(error).is_unavailable());
    }
}
