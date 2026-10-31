//! Autoloaded option bag and get-by-name on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contract:
//! `contracts/options/load.md`. Hooks, writes, serialization and PHP-visible names are not here.
//!
//! # Examples
//!
//! ```
//! use cms_options::OptionName;
//!
//! let name = OptionName::new(" home ")?;
//! assert_eq!(name.as_bytes(), b"home");
//! assert!(name.is_home());
//! # Ok::<(), cms_options::NameError>(())
//! ```

mod bag;
mod error;
mod name;
mod quote;
mod sql;

pub use bag::Options;
pub use error::{Error, NameError};
pub use name::OptionName;
