//! Request-local current user on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contract:
//! `contracts/auth/current.md`. Cookies, password checks, capabilities and
//! PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_auth::Error;
//!
//! assert!(Error::Unavailable.is_unavailable());
//! ```

mod current;
mod error;

pub use current::Current;
pub use error::Error;
