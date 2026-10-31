//! Outcome of insert, update or delete.

use crate::{TermId, TermTaxonomyId};

/// What a write did. A missing row or a rejected statement is data, not a Rust error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Write {
    /// Both rows were inserted.
    Inserted {
        /// The new `term_id`.
        term_id: TermId,
        /// The new `term_taxonomy_id`.
        taxonomy_id: TermTaxonomyId,
    },
    /// An existing pair was rewritten.
    Replaced,
    /// A taxonomy row was deleted, and the terms row when nothing else used it.
    Removed,
    /// An `insert_id` was zero, no rows were affected, or the server rejected a statement.
    Unchanged,
}

impl Write {
    /// Whether this is not [`Self::Unchanged`].
    #[must_use]
    pub const fn changed(self) -> bool {
        !matches!(self, Self::Unchanged)
    }

    /// Both new ids when this is [`Self::Inserted`].
    #[must_use]
    pub const fn inserted_ids(self) -> Option<(TermId, TermTaxonomyId)> {
        match self {
            Self::Inserted {
                term_id,
                taxonomy_id,
            } => Some((term_id, taxonomy_id)),
            Self::Replaced | Self::Removed | Self::Unchanged => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Write;
    use crate::{TermId, TermTaxonomyId};

    #[test]
    fn changed_and_inserted_ids_match_the_variant() {
        let term = TermId::new(7);
        let taxonomy = TermTaxonomyId::new(9);
        assert!(term.is_ok() && taxonomy.is_ok());
        if let (Ok(term_id), Ok(taxonomy_id)) = (term, taxonomy) {
            let written = Write::Inserted {
                term_id,
                taxonomy_id,
            };
            assert!(written.changed());
            assert_eq!(written.inserted_ids(), Some((term_id, taxonomy_id)));
        }
        assert!(Write::Replaced.changed());
        assert!(Write::Removed.changed());
        assert!(!Write::Unchanged.changed());
        assert!(Write::Replaced.inserted_ids().is_none());
        assert!(Write::Removed.inserted_ids().is_none());
        assert!(Write::Unchanged.inserted_ids().is_none());
    }
}
