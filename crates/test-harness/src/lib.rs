//! Differential test support for comparing Ferropress against the reference platform.
//!
//! This crate owns the [`Observation`] model: everything one request makes observable, namely the HTTP
//! response, the ordered hook trace, SQL statements, outgoing mail, the cron schedule and database row
//! changes. Reference observations are recorded by `cargo xtask lab capture`; Ferropress produces the
//! same model so both sides compare field by field.
//!
//! It is test support only: production crates may depend on it solely as a dev-dependency, which
//! `cargo xtask check-layers` enforces.

mod error;
mod observation;

pub use error::{InvalidRequestPath, InvalidSlug};
pub use observation::{
    CronEvent, DatabaseDiff, HookEvent, HookKind, Mail, Method, Observation, Request, RequestPath,
    Response, Slug, TableDiff, Trace,
};
