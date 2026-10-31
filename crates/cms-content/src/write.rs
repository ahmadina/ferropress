//! Outcome of insert, update or delete.

use crate::PostId;

/// What a write did. A missing row or a rejected statement is data, not a Rust error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Write {
    /// A row was inserted; the value is the new id.
    Inserted(PostId),
    /// An existing row was rewritten.
    Replaced,
    /// A row was deleted.
    Removed,
    /// `insert_id` was zero, no rows were affected, or the server rejected the statement.
    Unchanged,
}

impl Write {
    /// Whether this is not [`Self::Unchanged`].
    #[must_use]
    pub const fn changed(self) -> bool {
        !matches!(self, Self::Unchanged)
    }

    /// The new id when this is [`Self::Inserted`].
    #[must_use]
    pub const fn inserted_id(self) -> Option<PostId> {
        match self {
            Self::Inserted(id) => Some(id),
            Self::Replaced | Self::Removed | Self::Unchanged => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Write;
    use crate::PostId;

    #[test]
    fn changed_and_inserted_id_match_the_variant() {
        let id = PostId::new(7);
        assert!(id.is_ok());
        if let Ok(id) = id {
            assert!(Write::Inserted(id).changed());
            assert_eq!(Write::Inserted(id).inserted_id(), Some(id));
        }
        assert!(Write::Replaced.changed());
        assert!(Write::Removed.changed());
        assert!(!Write::Unchanged.changed());
        assert!(Write::Replaced.inserted_id().is_none());
        assert!(Write::Removed.inserted_id().is_none());
        assert!(Write::Unchanged.inserted_id().is_none());
    }
}
