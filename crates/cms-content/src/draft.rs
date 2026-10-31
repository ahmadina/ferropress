//! Compact writable cells for one posts-table row.

use std::fmt;

/// A writable posts-table column from the insert compact list.
///
/// `ID` and `comment_count` are not here. Contract: `contracts/content/write.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Field {
    /// `post_author` (`%d`).
    Author,
    /// `post_date`.
    Date,
    /// `post_date_gmt`.
    DateGmt,
    /// `post_content`.
    Content,
    /// `post_content_filtered`.
    ContentFiltered,
    /// `post_title`.
    Title,
    /// `post_excerpt`.
    Excerpt,
    /// `post_status`.
    Status,
    /// `post_type`.
    TypeName,
    /// `comment_status`.
    CommentStatus,
    /// `ping_status`.
    PingStatus,
    /// `post_password`.
    Password,
    /// `post_name`.
    Name,
    /// `to_ping`.
    ToPing,
    /// `pinged`.
    Pinged,
    /// `post_modified`.
    Modified,
    /// `post_modified_gmt`.
    ModifiedGmt,
    /// `post_parent` (`%d`).
    Parent,
    /// `menu_order` (`%d`).
    MenuOrder,
    /// `post_mime_type`.
    MimeType,
    /// `guid`.
    Guid,
}

impl Field {
    /// Compact insert order used by the reference `INSERT` / `UPDATE`.
    pub const ALL: [Self; 21] = [
        Self::Author,
        Self::Date,
        Self::DateGmt,
        Self::Content,
        Self::ContentFiltered,
        Self::Title,
        Self::Excerpt,
        Self::Status,
        Self::TypeName,
        Self::CommentStatus,
        Self::PingStatus,
        Self::Password,
        Self::Name,
        Self::ToPing,
        Self::Pinged,
        Self::Modified,
        Self::ModifiedGmt,
        Self::Parent,
        Self::MenuOrder,
        Self::MimeType,
        Self::Guid,
    ];

    /// Inventory column name.
    #[must_use]
    pub const fn column(self) -> &'static [u8] {
        match self {
            Self::Author => b"post_author",
            Self::Date => b"post_date",
            Self::DateGmt => b"post_date_gmt",
            Self::Content => b"post_content",
            Self::ContentFiltered => b"post_content_filtered",
            Self::Title => b"post_title",
            Self::Excerpt => b"post_excerpt",
            Self::Status => b"post_status",
            Self::TypeName => b"post_type",
            Self::CommentStatus => b"comment_status",
            Self::PingStatus => b"ping_status",
            Self::Password => b"post_password",
            Self::Name => b"post_name",
            Self::ToPing => b"to_ping",
            Self::Pinged => b"pinged",
            Self::Modified => b"post_modified",
            Self::ModifiedGmt => b"post_modified_gmt",
            Self::Parent => b"post_parent",
            Self::MenuOrder => b"menu_order",
            Self::MimeType => b"post_mime_type",
            Self::Guid => b"guid",
        }
    }

    /// Whether the reference formats this column as `%d`.
    #[must_use]
    pub const fn is_integer(self) -> bool {
        matches!(self, Self::Author | Self::Parent | Self::MenuOrder)
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Caller-supplied cells for one insert or update.
///
/// Empty by default. Password is redacted in [`Debug`].
#[derive(Clone, PartialEq, Eq)]
pub struct Draft {
    cells: [Box<[u8]>; 21],
}

impl Draft {
    /// Every cell empty.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cells: [(); 21].map(|()| Box::from([])),
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
            .field("status", &String::from_utf8_lossy(self.get(Field::Status)))
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
        assert!(draft.get(Field::Title).is_empty());
        draft.set(Field::Title, b"Hello");
        assert_eq!(draft.get(Field::Title), b"Hello");
        assert_eq!(Draft::default().get(Field::Author), b"");
        assert_eq!(Field::ALL.len(), 21);
        assert!(Field::Author.is_integer());
        assert!(Field::Parent.is_integer());
        assert!(Field::MenuOrder.is_integer());
        assert!(!Field::Title.is_integer());
        assert_eq!(Field::Author.column(), b"post_author");
        assert_eq!(Field::Guid.column(), b"guid");
        let shown = format!("{:?}", draft.set(Field::Password, b"secret"));
        assert!(shown.contains("Draft"));
        assert!(!shown.contains("secret"));
    }

    #[test]
    fn every_compact_column_is_named() {
        for field in Field::ALL {
            assert!(!field.column().is_empty());
        }
    }
}
