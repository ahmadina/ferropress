//! Load one user row by id on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contract:
//! `contracts/users/get.md`. Hooks, writes, sanitizing, caps and PHP-visible
//! names are not here.
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
mod error;
mod id;
mod row;
mod sql;

pub use bag::Users;
pub use error::{Error, IdError};
pub use id::UserId;
pub use row::User;
