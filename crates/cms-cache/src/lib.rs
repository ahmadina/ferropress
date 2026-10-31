//! Request-local object cache: keys, groups and site prefixes.
//!
//! Domain crates depend on this crate, not on a persistent driver. Contract:
//! `contracts/cache/memory.md`. PHP-visible names, incr/decr and drop-in backends are not here.
//!
//! # Examples
//!
//! ```
//! use cms_cache::{CacheKey, Group, Memory};
//!
//! let key = CacheKey::new("home")?;
//! let group = Group::new("");
//! let mut cache = Memory::new();
//! assert!(cache.add(&key, b"http://example.test", &group));
//! assert_eq!(
//!     cache.get(&key, &group).as_deref(),
//!     Some(b"http://example.test".as_slice())
//! );
//! # Ok::<(), cms_cache::KeyError>(())
//! ```

mod bag;
mod error;
mod group;
mod key;

pub use bag::Memory;
pub use error::KeyError;
pub use group::Group;
pub use key::CacheKey;
