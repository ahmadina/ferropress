//! Errors from [`crate::Store`] and table-name constructors.

/// The adapter could not perform the statement.
///
/// Server-reported SQL failures are [`crate::Reply::Failed`], not this type.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StoreError {
    /// No usable connection, or the scripted store has no matching reply.
    #[error("database store is unavailable")]
    Unavailable,
}

impl StoreError {
    /// Whether this is [`Self::Unavailable`].
    #[must_use]
    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::{NameError, StoreError};

    #[test]
    fn store_errors_are_unavailable() {
        let error = StoreError::Unavailable;
        assert!(error.is_unavailable());
        assert_eq!(error.to_string(), "database store is unavailable");
        assert_eq!(
            NameError::Nul.to_string(),
            "a table name must not contain a nul byte"
        );
        assert_eq!(
            NameError::Empty.to_string(),
            "a table stem must not be empty"
        );
    }
}

/// A table prefix or stem was rejected.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NameError {
    /// A NUL byte is not a valid part of a table name.
    #[error("a table name must not contain a nul byte")]
    Nul,
    /// A table stem must name a table.
    #[error("a table stem must not be empty")]
    Empty,
}
