//! Failures from the post bag.

use cms_db::NameError as TableNameError;

/// A post id was zero.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdError {
    /// Zero is not a stored post id.
    #[error("a post id must be greater than zero")]
    Zero,
}

/// The store could not be reached, or a table prefix was rejected.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// No usable connection.
    #[error("content store is unavailable")]
    Unavailable,
    /// The site prefix could not form the posts table name.
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
    use super::{Error, IdError};
    use cms_db::NameError as TableNameError;

    #[test]
    fn zero_ids_and_table_failures_are_distinct() {
        assert_eq!(
            IdError::Zero.to_string(),
            "a post id must be greater than zero"
        );
        let error = Error::from(TableNameError::Nul);
        assert!(!error.is_unavailable());
        assert_eq!(
            error.to_string(),
            "a table name must not contain a nul byte"
        );
        assert!(Error::Unavailable.is_unavailable());
        assert_eq!(
            Error::Unavailable.to_string(),
            "content store is unavailable"
        );
    }
}
