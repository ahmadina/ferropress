//! Load one joined term row by id on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contract:
//! `contracts/taxonomy/get.md`. Hooks, sanitizing, the taxonomy registry and
//! PHP-visible names are not here.
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
mod error;
mod found;
mod id;
mod sql;
mod term;

pub use bag::Terms;
pub use error::{Error, IdError};
pub use found::Found;
pub use id::TermId;
pub use term::Term;
