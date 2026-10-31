//! Outcome of attach or detach.

/// What a relationship write did. A missing pair is data, not a Rust error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// A row was inserted.
    Attached,
    /// One or more rows were deleted.
    Detached,
    /// The pair already existed, no ids were given, or the server rejected the statement.
    Unchanged,
}

impl Link {
    /// Whether this is not [`Self::Unchanged`].
    #[must_use]
    pub const fn changed(self) -> bool {
        !matches!(self, Self::Unchanged)
    }
}

#[cfg(test)]
mod tests {
    use super::Link;

    #[test]
    fn changed_is_only_attached_or_detached() {
        assert!(Link::Attached.changed());
        assert!(Link::Detached.changed());
        assert!(!Link::Unchanged.changed());
    }
}
