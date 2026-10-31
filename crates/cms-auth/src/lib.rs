//! Request-local current user and stored capability maps on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/auth/current.md`, `contracts/auth/caps.md`. Cookies, password
//! checks, role merge and PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_auth::{Error, GrantMap};
//!
//! assert!(Error::Unavailable.is_unavailable());
//! assert!(!GrantMap::empty().granted(b"manage_options"));
//! ```

mod caps;
mod current;
mod error;
mod grants;

pub use caps::Caps;
pub use current::Current;
pub use error::Error;
pub use grants::GrantMap;
