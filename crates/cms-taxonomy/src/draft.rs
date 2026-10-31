//! Compact writable cells for one terms / `term_taxonomy` pair.

/// A writable column from the insert compact lists.
///
/// Contract: `contracts/taxonomy/write.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Field {
    /// `name`.
    Name,
    /// `slug`.
    Slug,
    /// `term_group` (`%d`).
    TermGroup,
    /// `taxonomy`.
    Taxonomy,
    /// `description`.
    Description,
    /// `parent` (`%d`).
    Parent,
    /// `count` (`%d`).
    Count,
}

impl Field {
    /// Every writable cell, in declaration order.
    pub const ALL: [Self; 7] = [
        Self::Name,
        Self::Slug,
        Self::TermGroup,
        Self::Taxonomy,
        Self::Description,
        Self::Parent,
        Self::Count,
    ];

    /// Inventory column name.
    #[must_use]
    pub const fn column(self) -> &'static [u8] {
        match self {
            Self::Name => b"name",
            Self::Slug => b"slug",
            Self::TermGroup => b"term_group",
            Self::Taxonomy => b"taxonomy",
            Self::Description => b"description",
            Self::Parent => b"parent",
            Self::Count => b"count",
        }
    }

    /// Whether the reference formats this column as `%d`.
    #[must_use]
    pub const fn is_integer(self) -> bool {
        matches!(self, Self::TermGroup | Self::Parent | Self::Count)
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Caller-supplied cells for one insert or update.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    cells: [Box<[u8]>; 7],
}

impl Draft {
    /// Every cell empty.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cells: [(); 7].map(|()| Box::from([])),
        }
    }

    /// Replace one cell. Bytes are stored as given.
    pub fn set(&mut self, field: Field, value: impl AsRef<[u8]>) -> &mut Self {
        self.cells[field.index()] = Box::from(value.as_ref());
        self
    }

    /// Stored bytes for `field`.
    #[must_use]
    pub fn get(&self, field: Field) -> &[u8] {
        &self.cells[field.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::{Draft, Field};

    #[test]
    fn empty_draft_and_setters_round_trip() {
        let mut draft = Draft::new();
        assert!(draft.get(Field::Name).is_empty());
        draft.set(Field::Name, b"News");
        assert_eq!(draft.get(Field::Name), b"News");
        assert_eq!(Draft::default().get(Field::Slug), b"");
        assert_eq!(Field::ALL.len(), 7);
        assert!(Field::TermGroup.is_integer());
        assert!(Field::Parent.is_integer());
        assert!(Field::Count.is_integer());
        assert!(!Field::Name.is_integer());
        assert_eq!(Field::Name.column(), b"name");
        assert_eq!(Field::Count.column(), b"count");
        for field in Field::ALL {
            assert!(!field.column().is_empty());
        }
    }
}
