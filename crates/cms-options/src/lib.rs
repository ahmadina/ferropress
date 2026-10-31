//! Autoloaded option bag, get-by-name, and add / update / delete on a [`cms_db::Store`].
//!
//! Domain crates depend on this crate, not on a driver (ADR-0004). Contracts:
//! `contracts/options/load.md` and `contracts/options/write.md`. Hooks, serialization
//! and PHP-visible names are not here.
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

mod autoload;
mod bag;
mod error;
mod name;
mod quote;
mod sql;
mod write;

pub use autoload::Autoload;
pub use bag::Options;
pub use error::{Error, NameError};
pub use name::OptionName;
pub use write::Write;
