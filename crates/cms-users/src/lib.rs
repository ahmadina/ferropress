//! Load and write one user row on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/users/get.md`, `contracts/users/write.md`. Hooks, sanitizing,
//! password hashing, caps and PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_users::UserId;
//!
//! let id = UserId::new(12)?;
//! assert_eq!(id.as_u64(), 12);
//! # Ok::<(), cms_users::IdError>(())
//! ```

mod bag;
mod draft;
mod error;
mod id;
mod quote;
mod row;
mod sql;
mod write;

pub use bag::Users;
pub use draft::{Draft, Field};
pub use error::{Error, IdError};
pub use id::UserId;
pub use row::User;
pub use write::Write;
