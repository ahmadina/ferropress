//! Failures from the option bag.

use cms_db::NameError as TableNameError;

/// An option name was empty after trim (or was `0`).
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NameError {
    /// Nothing usable remained.
    #[error("an option name must not be empty")]
    Empty,
}

/// The store could not be reached, or a table prefix was rejected.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// No usable connection.
    #[error("option store is unavailable")]
    Unavailable,
    /// The site prefix could not form the options table name.
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
    use super::Error;
    use super::NameError;
    use cms_db::NameError as TableNameError;

    #[test]
    fn empty_names_and_table_failures_are_distinct() {
        assert_eq!(
            NameError::Empty.to_string(),
            "an option name must not be empty"
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
            "option store is unavailable"
        );
    }
}
