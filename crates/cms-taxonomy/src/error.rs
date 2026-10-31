//! Failures from the term bag.

use cms_db::NameError as TableNameError;

/// A term id was zero.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdError {
    /// Zero is not a stored term id.
    #[error("a term id must be greater than zero")]
    Zero,
    /// Zero is not a stored term taxonomy id.
    #[error("a term taxonomy id must be greater than zero")]
    TaxonomyZero,
}

/// The store could not be reached, or a table prefix was rejected.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// No usable connection.
    #[error("taxonomy store is unavailable")]
    Unavailable,
    /// The site prefix could not form a terms table name.
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
            "a term id must be greater than zero"
        );
        assert_eq!(
            IdError::TaxonomyZero.to_string(),
            "a term taxonomy id must be greater than zero"
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
            "taxonomy store is unavailable"
        );
    }
}
