//! Differential test support for comparing Ferropress against the reference platform.
//!
//! This crate owns the [`Observation`] model: everything one request makes observable, namely the HTTP
//! response, the ordered hook trace, SQL statements, outgoing mail, the cron schedule and database row
//! changes. Reference observations are recorded by `cargo xtask lab capture`; Ferropress produces the
//! same model so both sides compare field by field.
//!
//! A [`Scenario`] lists the steps to run on both sides, the [`Aspect`]s to compare and the
//! [`Normalization`]s accepted in review; every normalization states a reason and a reviewer.
//! [`run`] performs a scenario on two [`Target`]s and [`compare`]s their observations step by step.
//! The `test-util` feature adds the `fakes` module with a scripted target.
//!
//! It is test support only: production crates may depend on it solely as a dev-dependency, which
//! `cargo xtask check-layers` enforces.

mod compare;
mod error;
#[cfg(any(test, feature = "test-util"))]
pub mod fakes;
mod normalize;
mod observation;
mod run;
mod scenario;

pub use compare::{Mismatch, compare};
pub use error::{InvalidRequestPath, InvalidSlug, ScenarioError, UnknownProfile};
pub use normalize::{Location, NORMALIZED, Normalization};
pub use observation::{
    CronEvent, DatabaseDiff, FileLoad, HookEvent, HookKind, Mail, Method, Observation, Request,
    RequestPath, Response, Slug, TableDiff, Trace,
};
pub use run::{Replay, ReplayExhausted, Report, RunError, Side, StepReport, Target, run};
pub use scenario::{Aspect, Profile, Scenario, Step};
