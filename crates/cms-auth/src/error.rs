//! Failures from the current-user bag.

use cms_db::NameError as TableNameError;
use cms_options::Error as OptionsError;
use cms_users::Error as UsersError;

/// The store could not be reached, or a table prefix was rejected.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// No usable connection.
    #[error("auth store is unavailable")]
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

impl From<UsersError> for Error {
    fn from(error: UsersError) -> Self {
        if let UsersError::Table(name) = error {
            Self::Table(name)
        } else {
            Self::Unavailable
        }
    }
}

impl From<OptionsError> for Error {
    fn from(error: OptionsError) -> Self {
        if let OptionsError::Table(name) = error {
            Self::Table(name)
        } else {
            Self::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use cms_db::NameError as TableNameError;
    use cms_options::Error as OptionsError;
    use cms_users::Error as UsersError;

    #[test]
    fn unavailable_and_table_failures_are_distinct() {
        let error = Error::from(TableNameError::Nul);
        assert!(!error.is_unavailable());
        assert_eq!(
            error.to_string(),
            "a table name must not contain a nul byte"
        );
        assert!(Error::Unavailable.is_unavailable());
        assert_eq!(Error::Unavailable.to_string(), "auth store is unavailable");
        assert!(Error::from(UsersError::Unavailable).is_unavailable());
        let table = Error::from(UsersError::from(TableNameError::Nul));
        assert!(!table.is_unavailable());
        assert!(Error::from(OptionsError::Unavailable).is_unavailable());
        let options = Error::from(OptionsError::from(TableNameError::Nul));
        assert!(!options.is_unavailable());
    }
}
