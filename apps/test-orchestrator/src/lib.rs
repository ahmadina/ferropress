//! Differential runner: a recorded lab lifecycle versus a Ferropress plugin-load boot.
//!
//! Composition root for roadmap step 22. Scenario documents live in [`SCENARIO_DIR`]; this crate
//! does not own hook dispatch or plugin-load order. Contract: `contracts/bootstrap/plugin-load.md`.

mod boot;
mod error;
mod lifecycle;
mod run;

pub use error::OrchestratorError;
pub use run::{
    KNOWN, LIFECYCLE_DIR, SCENARIO_DIR, describe, known_ids, load_scenario, run_scenario,
};
