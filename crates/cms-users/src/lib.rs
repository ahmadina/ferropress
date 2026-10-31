//! Load and write user rows and usermeta cells on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/users/get.md`, `contracts/users/write.md`, `contracts/users/meta.md`.
//! Hooks, sanitizing, password hashing, caps and PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_users::{MetaKey, UserId};
//!
//! let id = UserId::new(12)?;
//! assert_eq!(id.as_u64(), 12);
//! let key = MetaKey::new("nickname")?;
//! assert_eq!(key.as_bytes(), b"nickname");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod bag;
mod draft;
mod error;
mod id;
mod key;
mod meta;
mod quote;
mod row;
mod sql;
mod write;

pub use bag::Users;
pub use draft::{Draft, Field};
pub use error::{Error, IdError, KeyError};
pub use id::UserId;
pub use key::MetaKey;
pub use meta::UserMeta;
pub use row::User;
pub use write::Write;
