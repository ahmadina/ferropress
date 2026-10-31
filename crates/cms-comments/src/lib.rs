//! Load one comment row by id on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contract:
//! `contracts/comments/get.md`. Hooks, writes, sanitizing and PHP-visible names
//! are not here.
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
mod error;
mod id;
mod row;
mod sql;

pub use bag::Comments;
pub use error::{Error, IdError};
pub use id::CommentId;
pub use row::Comment;
