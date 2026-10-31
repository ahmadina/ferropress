//! Load the post row named by a singular id.

use cms_content::{Post, PostId, Posts};
use cms_db::{Store, TablePrefix};

use crate::attachment_var::AttachmentVar;
use crate::error::Error;
use crate::page_var::PageVar;
use crate::post_var::PostVar;
use crate::singular::Singular;

/// Request-local post hits for a parsed singular id (`contracts/query/load-id.md`).
pub struct Query<S> {
    posts: Posts<S>,
}

impl<S: Store> Query<S> {
    /// A runner that reads `{prefix}posts`.
    ///
    /// # Errors
    ///
    /// [`Error::Table`] when the prefix contains a NUL.
    pub fn new(store: S, prefix: &TablePrefix) -> Result<Self, Error> {
        Ok(Self {
            posts: Posts::new(store, prefix)?,
        })
    }

    /// The post row for the winning singular id, or none.
    ///
    /// Attachment id wins, then `p`, then `page_id`. A name-only or slug-only
    /// winner issues no SQL.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn load_id(
        &mut self,
        flags: Singular,
        post: PostVar,
        page: PageVar,
        attachment: &AttachmentVar,
    ) -> Result<Vec<Post>, Error> {
        let Some(id) = singular_id(flags, post, page, attachment) else {
            return Ok(Vec::new());
        };
        match self.posts.get(id)? {
            Some(row) => Ok(vec![row]),
            None => Ok(Vec::new()),
        }
    }
}

impl<S> std::fmt::Debug for Query<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Query").finish_non_exhaustive()
    }
}

fn singular_id(
    flags: Singular,
    post: PostVar,
    page: PageVar,
    attachment: &AttachmentVar,
) -> Option<PostId> {
    match flags {
        Singular::Attachment if attachment.id() > 0 => post_id(attachment.id()),
        Singular::Single if post.is_single() => post_id(post.value()),
        Singular::Page if page.is_page() => post_id(page.value()),
        Singular::None | Singular::Attachment | Singular::Single | Singular::Page => None,
    }
}

fn post_id(value: i64) -> Option<PostId> {
    u64::try_from(value)
        .ok()
        .and_then(|id| PostId::new(id).ok())
}

#[cfg(test)]
mod tests {
    use super::singular_id;
    use crate::{AttachmentInput, AttachmentVar, NameVar, PageNameVar, PageVar, PostVar, Singular};

    fn flags(
        attachment: &AttachmentVar,
        name: Option<&[u8]>,
        post: PostVar,
        page: PageVar,
    ) -> Singular {
        Singular::resolve(
            attachment,
            &NameVar::parse(name),
            post,
            &PageNameVar::parse(None),
            page,
        )
    }

    #[test]
    fn ids_follow_the_winner() {
        let attach = AttachmentVar::parse(AttachmentInput::new().attachment_id(b"9"));
        let post = PostVar::parse(Some(b"12"));
        let page = PageVar::parse(Some(b"4"));
        let id = singular_id(flags(&attach, None, post, page), post, page, &attach);
        assert!(id.is_some_and(|id| id.as_u64() == 9));
        let none = AttachmentVar::parse(AttachmentInput::new());
        let id = singular_id(flags(&none, None, post, page), post, page, &none);
        assert!(id.is_some_and(|id| id.as_u64() == 12));
        let empty_post = PostVar::parse(None);
        let id = singular_id(
            flags(&none, None, empty_post, page),
            empty_post,
            page,
            &none,
        );
        assert!(id.is_some_and(|id| id.as_u64() == 4));
        let id = singular_id(
            flags(&none, Some(b"hello"), empty_post, page),
            empty_post,
            page,
            &none,
        );
        assert!(id.is_none());
        let slug = AttachmentVar::parse(AttachmentInput::new().attachment(b"file"));
        let id = singular_id(
            flags(&slug, None, empty_post, page),
            empty_post,
            page,
            &slug,
        );
        assert!(id.is_none());
    }

    #[test]
    fn post_id_rejects_non_positive() {
        assert!(super::post_id(-1).is_none());
        assert!(super::post_id(0).is_none());
        assert!(super::post_id(12).is_some_and(|id| id.as_u64() == 12));
    }
}
