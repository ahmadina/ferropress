//! Outcome of add, update or delete.

/// What a write did. Protected names are data, not a Rust error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Write {
    /// A row was inserted, updated or deleted.
    Changed,
    /// The name already existed, the value was unchanged, or the row was missing.
    Unchanged,
    /// `alloptions` or `notoptions`.
    Protected,
}

impl Write {
    /// Whether this is [`Self::Changed`].
    #[must_use]
    pub const fn changed(self) -> bool {
        matches!(self, Self::Changed)
    }
}

#[cfg(test)]
mod tests {
    use super::Write;

    #[test]
    fn changed_is_only_the_changed_variant() {
        assert!(Write::Changed.changed());
        assert!(!Write::Unchanged.changed());
        assert!(!Write::Protected.changed());
    }
}
