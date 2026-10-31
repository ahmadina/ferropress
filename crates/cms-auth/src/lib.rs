//! Request-local current user, stored caps, role-merged `allcaps`, and primitive `has`.
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/auth/current.md`, `contracts/auth/caps.md`, `contracts/auth/roles.md`,
//! `contracts/auth/has.md`. Cookies, password checks, `map_meta_cap` and
//! PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_auth::{Error, GrantMap, RoleTable, allcaps};
//!
//! assert!(Error::Unavailable.is_unavailable());
//! assert!(!GrantMap::empty().granted(b"manage_options"));
//! assert!(GrantMap::empty().has(&[b"exist"]));
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
