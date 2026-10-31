//! Failures from the user bag.

use cms_db::NameError as TableNameError;

/// A user id was zero.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdError {
    /// Zero is not a stored user id.
    #[error("a user id must be greater than zero")]
    Zero,
}

/// A user-meta key was empty.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyError {
    /// Empty keys are the unbound "all keys" form.
    #[error("a user meta key must not be empty")]
    Empty,
}

/// The store could not be reached, or a table prefix was rejected.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// No usable connection.
    #[error("user store is unavailable")]
    Unavailable,
    /// The prefix could not form the users table name.
    #[error(transparent)]
    Table(#[from] TableNameError),
}

impl Error {
    /// Whether this is [`Self::Unavailable`].
    #[must_use]
    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, IdError, KeyError};
    use cms_db::NameError as TableNameError;

    #[test]
    fn zero_ids_and_table_failures_are_distinct() {
        assert_eq!(
            IdError::Zero.to_string(),
            "a user id must be greater than zero"
        );
        assert_eq!(
            KeyError::Empty.to_string(),
            "a user meta key must not be empty"
        );
        let error = Error::from(TableNameError::Nul);
        assert!(!error.is_unavailable());
        assert_eq!(
            error.to_string(),
            "a table name must not contain a nul byte"
        );
        assert!(Error::Unavailable.is_unavailable());
        assert_eq!(Error::Unavailable.to_string(), "user store is unavailable");
    }
}
