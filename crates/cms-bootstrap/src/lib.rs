//! Plugin load order for one request.
//!
//! [`plan`] decides which files run and in what order. [`run`] loads each file through
//! [`LoadPlugin`] and fires the matching actions on the request's hook table.
//! Contract: `contracts/bootstrap/plugin-load.md`.

mod error;
mod path;
mod plan;
mod run;

pub use error::BootError;
pub use path::PluginFile;
pub use plan::{Layout, LoadKind, LoadStep, Snapshot, plan, sort_active};
pub use run::{LoadPlugin, run};
