//! SQL store port: one statement in, a server reply or a platform error out.
//!
//! Domain crates depend on [`Store`], never on a driver (ADR-0004). Contract:
//! `contracts/db/store.md`. The `MySQL` adapter and the PHP-visible database object are not here.
//!
//! # Examples
//!
//! ```
//! use cms_db::{Sql, TablePrefix, TableStem};
//!
//! let prefix = TablePrefix::new("fp_")?;
//! let options = TableStem::new("options")?;
//! assert_eq!(prefix.table(&options).as_bytes(), b"fp_options");
//! assert_eq!(Sql::from("SELECT 1").as_bytes(), b"SELECT 1");
//! # Ok::<(), cms_db::NameError>(())
//! ```

mod error;
mod names;
mod reply;
mod sql;
mod store;

#[cfg(feature = "test-util")]
mod scripted;

pub use error::{NameError, StoreError};
pub use names::{TableName, TablePrefix, TableStem};
pub use reply::{Cell, Column, Reply, ResultSet, Row, ServerError};
pub use sql::Sql;
pub use store::Store;

#[cfg(feature = "test-util")]
pub use scripted::Scripted;

#[cfg(feature = "test-util")]
pub use store::store_contract;
