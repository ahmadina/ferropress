//! Failures from the query runner.

use cms_content::Error as ContentError;
use cms_db::NameError as TableNameError;

/// The store could not be reached, or a table prefix was rejected.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// No usable connection.
    #[error("query store is unavailable")]
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

impl From<ContentError> for Error {
    fn from(error: ContentError) -> Self {
        if let ContentError::Table(name) = error {
            Self::Table(name)
        } else {
            Self::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use cms_content::Error as ContentError;
    use cms_db::NameError as TableNameError;

    #[test]
    fn unavailable_and_table_failures_are_distinct() {
        let error = Error::from(TableNameError::Nul);
        assert!(!error.is_unavailable());
        assert_eq!(
            error.to_string(),
            "a table name must not contain a nul byte"
        );
        assert!(Error::Unavailable.is_unavailable());
        assert_eq!(Error::Unavailable.to_string(), "query store is unavailable");
        assert!(Error::from(ContentError::Unavailable).is_unavailable());
        let table = Error::from(ContentError::from(TableNameError::Nul));
        assert!(!table.is_unavailable());
    }
}
