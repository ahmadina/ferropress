//! Request-local current user, stored caps, role-merged `allcaps`, primitive `has`,
//! single-site mapping, and a current-user capability check.
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/auth/current.md`, `contracts/auth/caps.md`, `contracts/auth/roles.md`,
//! `contracts/auth/has.md`, `contracts/auth/map.md`, `contracts/auth/current-can.md`,
//! `contracts/auth/links.md`. Cookies, password checks, post object-id mapping
//! and PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_auth::{Actor, Error, GrantMap, RoleTable, allcaps, map_meta, map_user};
//!
//! assert!(Error::Unavailable.is_unavailable());
//! assert!(!GrantMap::empty().granted(b"manage_options"));
//! assert!(GrantMap::empty().has(&[b"exist"]));
//! assert!(!allcaps(&GrantMap::empty(), &RoleTable::empty()).granted(b"read"));
//! let mapped = map_meta(b"customize", Actor::Anonymous);
//! let names: Vec<&[u8]> = mapped.names().collect();
//! assert_eq!(names, [b"edit_theme_options".as_slice()]);
//! assert_eq!(map_user(b"remove_user", Actor::Anonymous, None).names().count(), 1);
//! ```

mod caps;
mod current;
mod error;
mod grants;
mod map;
mod roles;

pub use caps::Caps;
pub use current::Current;
pub use error::Error;
pub use grants::GrantMap;
pub use map::{Actor, Flags, Mapped, map_meta, map_on, map_user};
pub use roles::{RoleTable, Roles, allcaps};
