//! Errors returned by `test-orchestrator`.

use std::io;
use std::path::PathBuf;

use test_harness::{ReplayExhausted, ScenarioError, Side, Slug};

/// Why a scenario could not be loaded, observed or compared.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OrchestratorError {
    /// No known Ferropress target for this scenario id.
    #[error("scenario `{0}` is not a plugin-load hook-trace")]
    Unsupported(Slug),
    /// The scenario document could not be read.
    #[error("reading {}: {source}", path.display())]
    Read {
        /// File that could not be read.
        path: PathBuf,
        /// The filesystem error.
        #[source]
        source: io::Error,
    },
    /// The scenario document is not valid.
    #[error(transparent)]
    Scenario(#[from] ScenarioError),
    /// A lifecycle or plugin file could not be parsed.
    #[error("{path}: {message}")]
    Parse {
        /// File that failed to parse.
        path: String,
        /// Parser or compiler message.
        message: String,
    },
    /// A plugin file could not be located or run.
    #[error("loading {path}: {message}")]
    Load {
        /// Hook path that failed.
        path: String,
        /// Why it failed.
        message: String,
    },
    /// The reference lifecycle had no plugin-load events.
    #[error("lifecycle `{0}` has no plugin-load events")]
    EmptyLifecycle(String),
    /// A callback ran during a load action and this runner has no PHP session for it.
    #[error("unexpected plugin callback during load")]
    UnexpectedCallback,
    /// A replay ran out of recorded observations.
    #[error(transparent)]
    Replay(#[from] ReplayExhausted),
    /// A target reported an observation of a different request than the step sent.
    #[error("the {side} target observed a different request than step {step} sent")]
    WrongRequest {
        /// The misbehaving side.
        side: Side,
        /// Step index, from 0.
        step: usize,
    },
}
