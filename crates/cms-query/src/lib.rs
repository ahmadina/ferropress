//! Parse query vars, resolve the singular request, and load a row by id.
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/query/p.md`, `contracts/query/page-id.md`,
//! `contracts/query/name.md`, `contracts/query/pagename.md`,
//! `contracts/query/attachment.md`, `contracts/query/singular.md`,
//! `contracts/query/load-id.md`, `contracts/query/load-path.md`,
//! `contracts/query/load-name.md`, `contracts/query/date.md`,
//! `contracts/query/time.md`. Status filters, hooks and PHP-visible names
//! are not here.
//!
//! # Examples
//!
//! ```
//! use cms_query::{
//!     AttachmentInput, AttachmentVar, NameVar, PageNameVar, PageVar, PostVar, Singular,
//! };
//!
//! let parsed = PostVar::parse(Some(b"12"));
//! assert!(parsed.is_single());
//! assert_eq!(parsed.value(), 12);
//! assert!(!parsed.is_not_found());
//! assert!(PageVar::parse(Some(b"-4")).is_page());
//! assert!(NameVar::parse(Some(b" hello ")).is_single());
//! assert!(PageNameVar::parse(Some(b" about ")).is_page());
//! let attach = AttachmentVar::parse(AttachmentInput::new().attachment(b"file"));
//! assert!(attach.is_attachment());
//! let flags = Singular::resolve(
//!     &attach,
//!     &NameVar::parse(Some(b"n")),
//!     PostVar::parse(Some(b"1")),
//!     &PageNameVar::parse(None),
//!     PageVar::parse(None),
//! );
//! assert!(flags.is_attachment());
//! assert!(flags.is_singular());
//! let date = cms_query::DateFlags::resolve(cms_query::DateParts::parse(
//!     Some(b"2026"),
//!     Some(b"5"),
//!     None,
//! ));
//! assert!(date.is_month());
//! assert!(!date.is_year());
//! let time = cms_query::DateFlags::resolve_all(
//!     &cms_query::DateParts::parse(None, None, None),
//!     &cms_query::StampVar::parse(Some(b"2026051512")),
//!     cms_query::WeekVar::parse(None),
//!     cms_query::TimeParts::parse(None, None, None),
//! );
//! assert!(time.is_time());
//! assert!(time.is_date());
//! ```

mod attachment_var;
mod basename;
mod date_var;
mod error;
mod load;
mod name_var;
mod page_name_var;
mod page_var;
mod post_var;
mod singular;

pub use attachment_var::{AttachmentInput, AttachmentVar};
pub use cms_content::Post;
pub use date_var::{DateArchive, DateFlags, DateParts, StampVar, TimeParts, WeekVar};
pub use error::Error;
pub use load::Query;
pub use name_var::NameVar;
pub use page_name_var::PageNameVar;
pub use page_var::PageVar;
pub use post_var::PostVar;
pub use singular::Singular;
