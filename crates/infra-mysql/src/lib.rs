//! `MySQL` adapter for the [`cms_db::Store`] port.
//!
//! Domain crates depend on [`cms_db::Store`], not this crate (ADR-0004). Contract:
//! `contracts/db/store.md`. Placeholders, transactions, multiple result sets, typed cells
//! and the PHP-visible database object are not here.
//!
//! # Examples
//!
//! ```
//! use infra_mysql::Endpoint;
//!
//! let endpoint = Endpoint::new("127.0.0.1", 3306, "reference", "secret", "reference");
//! let shown = format!("{endpoint:?}");
//! assert!(shown.contains("<redacted>"));
//! assert!(!shown.contains("secret"));
//! ```

mod convert;
mod endpoint;
mod error;
mod session;

pub use endpoint::Endpoint;
pub use error::Error;
pub use session::Mysql;
