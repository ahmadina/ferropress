//! Request-local current user, stored caps, and role-merged `allcaps`.
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/auth/current.md`, `contracts/auth/caps.md`, `contracts/auth/roles.md`.
//! Cookies, password checks, `user_has_cap` and PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_auth::{Error, GrantMap, RoleTable, allcaps};
//!
//! assert!(Error::Unavailable.is_unavailable());
//! assert!(!GrantMap::empty().granted(b"manage_options"));
//! assert!(!allcaps(&GrantMap::empty(), &RoleTable::empty()).granted(b"read"));
//! ```

mod caps;
mod current;
mod error;
mod grants;
mod roles;

pub use caps::Caps;
pub use current::Current;
pub use error::Error;
pub use grants::GrantMap;
pub use roles::{RoleTable, Roles, allcaps};
