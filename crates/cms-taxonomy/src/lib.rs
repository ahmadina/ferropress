//! Load and write term rows on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/taxonomy/get.md`, `contracts/taxonomy/write.md`. Hooks, sanitizing,
//! unique slugs, the taxonomy registry and PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_taxonomy::TermId;
//!
//! let id = TermId::new(12)?;
//! assert_eq!(id.as_u64(), 12);
//! # Ok::<(), cms_taxonomy::IdError>(())
//! ```

mod bag;
mod draft;
mod error;
mod found;
mod id;
mod quote;
mod sql;
mod term;
mod write;

pub use bag::Terms;
pub use draft::{Draft, Field};
pub use error::{Error, IdError};
pub use found::Found;
pub use id::{TermId, TermTaxonomyId};
pub use term::Term;
pub use write::Write;
