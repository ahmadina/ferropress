//! Parse the `p`, `page_id` and `name` query vars into post and page keys.
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/query/p.md`, `contracts/query/page-id.md`,
//! `contracts/query/name.md`. SQL, other query vars, hooks and PHP-visible
//! names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_query::{NameVar, PageVar, PostVar};
//!
//! let parsed = PostVar::parse(Some(b"12"));
//! assert!(parsed.is_single());
//! assert_eq!(parsed.value(), 12);
//! assert!(!parsed.is_not_found());
//! assert!(PageVar::parse(Some(b"-4")).is_page());
//! assert!(NameVar::parse(Some(b" hello ")).is_single());
//! ```

mod name_var;
mod page_var;
mod post_var;

pub use name_var::NameVar;
pub use page_var::PageVar;
pub use post_var::PostVar;
