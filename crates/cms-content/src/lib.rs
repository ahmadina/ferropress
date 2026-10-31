//! Load and write one post row on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/content/get.md`, `contracts/content/write.md`. Hooks, sanitizing,
//! unique slugs and PHP-visible names are not here.
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
mod draft;
mod error;
mod id;
mod post;
mod quote;
mod sql;
mod write;

pub use bag::Posts;
pub use draft::{Draft, Field};
pub use error::{Error, IdError};
pub use id::PostId;
pub use post::Post;
pub use write::Write;
