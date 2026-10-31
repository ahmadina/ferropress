//! Compact writable cells for one comments-table row.

use std::fmt;

/// A writable comments-table column from the insert compact list.
///
/// `comment_ID` is not here. Contract: `contracts/comments/write.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Field {
    /// `comment_post_ID` (`%d`).
    Post,
    /// `comment_author_IP`.
    AuthorIp,
    /// `comment_author`.
    Author,
    /// `comment_author_email`.
    AuthorEmail,
    /// `comment_author_url`.
    AuthorUrl,
    /// `comment_date`.
    Date,
    /// `comment_date_gmt`.
    DateGmt,
    /// `comment_content`.
    Content,
    /// `comment_karma` (`%d`).
    Karma,
    /// `comment_approved`.
    Approved,
    /// `comment_agent`.
    Agent,
    /// `comment_type`.
    TypeName,
    /// `comment_parent` (`%d`).
    Parent,
    /// `user_id` (`%d`).
    UserId,
}

impl Field {
    /// Compact insert order used by the reference `INSERT`.
    pub const INSERT: [Self; 14] = [
        Self::Post,
        Self::AuthorIp,
        Self::Author,
        Self::AuthorEmail,
        Self::AuthorUrl,
        Self::Date,
        Self::DateGmt,
        Self::Content,
        Self::Karma,
        Self::Approved,
        Self::Agent,
        Self::TypeName,
        Self::Parent,
        Self::UserId,
    ];

    /// Compact update order used by the reference `UPDATE`.
    pub const UPDATE: [Self; 14] = [
        Self::Post,
        Self::Author,
        Self::AuthorEmail,
        Self::AuthorUrl,
        Self::AuthorIp,
        Self::Date,
        Self::DateGmt,
        Self::Content,
        Self::Karma,
        Self::Approved,
        Self::Agent,
        Self::TypeName,
        Self::Parent,
        Self::UserId,
    ];

    /// Every writable cell. Same as [`Self::INSERT`].
    pub const ALL: [Self; 14] = Self::INSERT;

    /// Inventory column name.
    #[must_use]
    pub const fn column(self) -> &'static [u8] {
        match self {
            Self::Post => b"comment_post_ID",
            Self::AuthorIp => b"comment_author_IP",
            Self::Author => b"comment_author",
            Self::AuthorEmail => b"comment_author_email",
            Self::AuthorUrl => b"comment_author_url",
            Self::Date => b"comment_date",
            Self::DateGmt => b"comment_date_gmt",
            Self::Content => b"comment_content",
            Self::Karma => b"comment_karma",
            Self::Approved => b"comment_approved",
            Self::Agent => b"comment_agent",
            Self::TypeName => b"comment_type",
            Self::Parent => b"comment_parent",
            Self::UserId => b"user_id",
        }
    }

    /// Whether the reference formats this column as `%d`.
    #[must_use]
    pub const fn is_integer(self) -> bool {
        matches!(self, Self::Post | Self::Karma | Self::Parent | Self::UserId)
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Caller-supplied cells for one insert or update.
///
/// Empty by default. Author email is redacted in [`Debug`].
#[derive(Clone, PartialEq, Eq)]
pub struct Draft {
    cells: [Box<[u8]>; 14],
}

impl Draft {
    /// Every cell empty.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cells: [(); 14].map(|()| Box::from([])),
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

impl Default for Draft {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Draft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Draft")
            .field(
                "approved",
                &String::from_utf8_lossy(self.get(Field::Approved)),
            )
            .field(
                "type_name",
                &String::from_utf8_lossy(self.get(Field::TypeName)),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{Draft, Field};

    #[test]
    fn empty_draft_and_setters_round_trip() {
        let mut draft = Draft::new();
        assert!(draft.get(Field::Content).is_empty());
        draft.set(Field::Content, b"hello");
        assert_eq!(draft.get(Field::Content), b"hello");
        assert_eq!(Draft::default().get(Field::Author), b"");
        assert_eq!(Field::ALL.len(), 14);
        assert_eq!(Field::INSERT.len(), 14);
        assert_eq!(Field::UPDATE.len(), 14);
        assert_eq!(Field::INSERT[1], Field::AuthorIp);
        assert_eq!(Field::UPDATE[4], Field::AuthorIp);
        assert!(Field::Post.is_integer());
        assert!(Field::Karma.is_integer());
        assert!(Field::Parent.is_integer());
        assert!(Field::UserId.is_integer());
        assert!(!Field::Approved.is_integer());
        assert_eq!(Field::Post.column(), b"comment_post_ID");
        assert_eq!(Field::UserId.column(), b"user_id");
        let shown = format!("{:?}", draft.set(Field::AuthorEmail, b"ada@example.test"));
        assert!(shown.contains("Draft"));
        assert!(!shown.contains("ada@example.test"));
    }

    #[test]
    fn every_compact_column_is_named() {
        for field in Field::ALL {
            assert!(!field.column().is_empty());
        }
        for field in Field::UPDATE {
            assert!(!field.column().is_empty());
        }
    }
}
