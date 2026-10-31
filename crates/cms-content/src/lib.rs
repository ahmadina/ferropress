//! Load one post row by id on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contract:
//! `contracts/content/get.md`. Hooks, writes, sanitizing and PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_content::PostId;
//!
//! let id = PostId::new(12)?;
//! assert_eq!(id.as_u64(), 12);
//! # Ok::<(), cms_content::IdError>(())
//! ```

mod bag;
mod error;
mod id;
mod post;
mod sql;

pub use bag::Posts;
pub use error::{Error, IdError};
pub use id::PostId;
pub use post::Post;
