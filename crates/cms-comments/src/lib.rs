//! Load and write one comment row on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/comments/get.md`, `contracts/comments/write.md`. Hooks, sanitizing
//! and PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_comments::CommentId;
//!
//! let id = CommentId::new(12)?;
//! assert_eq!(id.as_u64(), 12);
//! # Ok::<(), cms_comments::IdError>(())
//! ```

mod bag;
mod draft;
mod error;
mod id;
mod quote;
mod row;
mod sql;
mod write;

pub use bag::Comments;
pub use draft::{Draft, Field};
pub use error::{Error, IdError};
pub use id::CommentId;
pub use row::Comment;
pub use write::Write;
