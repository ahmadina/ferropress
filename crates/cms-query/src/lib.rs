//! Parse the `p` query var into a single-post key.
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contract:
//! `contracts/query/p.md`. SQL, other query vars, hooks and PHP-visible names
//! are not here.
//!
//! # Examples
//!
//! ```
//! use cms_query::PostVar;
//!
//! let parsed = PostVar::parse(Some(b"12"));
//! assert!(parsed.is_single());
//! assert_eq!(parsed.value(), 12);
//! assert!(!parsed.is_not_found());
//! ```

mod post_var;

pub use post_var::PostVar;
