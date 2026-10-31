//! Outcome of a term get.

use crate::Term;

/// What a get found. A shared id is data, not a Rust error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// Exactly one usable join row, or a filter matched one row.
    One(Term),
    /// More than one join row and no taxonomy filter.
    Shared,
    /// No usable row, or the filter matched none.
    Miss,
}

impl Found {
    /// The term when this is [`Self::One`].
    #[must_use]
    pub const fn term(&self) -> Option<&Term> {
        match self {
            Self::One(term) => Some(term),
            Self::Shared | Self::Miss => None,
        }
    }

    /// Whether this is [`Self::Shared`].
    #[must_use]
    pub const fn is_shared(&self) -> bool {
        matches!(self, Self::Shared)
    }

    /// Whether this is [`Self::Miss`].
    #[must_use]
    pub const fn is_miss(&self) -> bool {
        matches!(self, Self::Miss)
    }
}

#[cfg(test)]
mod tests {
    use super::Found;

    #[test]
    fn miss_and_shared_have_no_term() {
        assert!(Found::Miss.is_miss());
        assert!(!Found::Miss.is_shared());
        assert!(Found::Miss.term().is_none());
        assert!(Found::Shared.is_shared());
        assert!(!Found::Shared.is_miss());
        assert!(Found::Shared.term().is_none());
    }
}
