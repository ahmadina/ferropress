//! Singular flags after `parse_query` precedence.

use crate::{AttachmentVar, NameVar, PageNameVar, PageVar, PostVar};

/// The winning singular request (`contracts/query/singular.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Singular {
    /// No attachment, single-post, or page key claimed the query.
    None,
    /// An attachment slug or a positive attachment id.
    Attachment,
    /// A non-empty `name` or a positive `p`.
    Single,
    /// A non-empty `pagename` or a positive `page_id`.
    Page,
}

impl Singular {
    /// Pick one singular request in `parse_query` order.
    ///
    /// Attachment wins over `name` and `p`, which win over `pagename` and
    /// `page_id`.
    #[must_use]
    pub fn resolve(
        attachment: &AttachmentVar,
        name: &NameVar,
        post: PostVar,
        page_name: &PageNameVar,
        page: PageVar,
    ) -> Self {
        if attachment.is_attachment() {
            Self::Attachment
        } else if name.is_single() || post.is_single() {
            Self::Single
        } else if page_name.is_page() || page.is_page() {
            Self::Page
        } else {
            Self::None
        }
    }

    /// Whether this is an attachment request.
    #[must_use]
    pub const fn is_attachment(self) -> bool {
        matches!(self, Self::Attachment)
    }

    /// Whether this is a single-post request (attachment or name/`p`).
    #[must_use]
    pub const fn is_single(self) -> bool {
        matches!(self, Self::Attachment | Self::Single)
    }

    /// Whether this is a page request.
    #[must_use]
    pub const fn is_page(self) -> bool {
        matches!(self, Self::Page)
    }

    /// Whether this is any singular request (`is_single` or `is_page`).
    #[must_use]
    pub const fn is_singular(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[cfg(test)]
mod tests {
    use super::Singular;
    use crate::attachment_var::{AttachmentInput, AttachmentVar};
    use crate::name_var::NameVar;
    use crate::page_name_var::PageNameVar;
    use crate::page_var::PageVar;
    use crate::post_var::PostVar;

    fn resolve(
        attachment: Option<&[u8]>,
        name: Option<&[u8]>,
        post: Option<&[u8]>,
        page_name: Option<&[u8]>,
        page: Option<&[u8]>,
    ) -> Singular {
        let attach = match attachment {
            Some(bytes) => AttachmentVar::parse(AttachmentInput::new().attachment(bytes)),
            None => AttachmentVar::parse(AttachmentInput::new()),
        };
        Singular::resolve(
            &attach,
            &NameVar::parse(name),
            PostVar::parse(post),
            &PageNameVar::parse(page_name),
            PageVar::parse(page),
        )
    }

    #[test]
    fn order() {
        let empty = resolve(None, None, None, None, None);
        assert!(!empty.is_singular());
        assert!(!empty.is_single());
        assert!(!empty.is_page());
        assert!(!empty.is_attachment());
        let attach = resolve(
            Some(b"file"),
            Some(b"n"),
            Some(b"1"),
            Some(b"p"),
            Some(b"2"),
        );
        assert!(attach.is_attachment());
        assert!(attach.is_single());
        assert!(!attach.is_page());
        assert!(attach.is_singular());
        let name = resolve(None, Some(b"n"), Some(b"1"), Some(b"p"), Some(b"2"));
        assert!(name.is_single());
        assert!(!name.is_page());
        assert!(!name.is_attachment());
        let post = resolve(None, None, Some(b"1"), Some(b"p"), Some(b"2"));
        assert!(post.is_single());
        assert!(!post.is_page());
        let page = resolve(None, None, Some(b"-1"), Some(b"about"), Some(b"2"));
        assert!(page.is_page());
        assert!(!page.is_single());
        assert!(page.is_singular());
    }
}
