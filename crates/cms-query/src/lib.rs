//! Parse the `p` and `page_id` query vars into post and page keys.
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/query/p.md`, `contracts/query/page-id.md`. SQL, other query vars,
//! hooks and PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_query::{PageVar, PostVar};
//!
//! let parsed = PostVar::parse(Some(b"12"));
//! assert!(parsed.is_single());
//! assert_eq!(parsed.value(), 12);
//! assert!(!parsed.is_not_found());
//! assert!(PageVar::parse(Some(b"-4")).is_page());
//! ```

mod page_var;
mod post_var;

pub use page_var::PageVar;
pub use post_var::PostVar;
