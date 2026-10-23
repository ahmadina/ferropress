//! Errors returned by `test-harness`.

use crate::normalize::Location;
use crate::scenario::Aspect;

/// A string that is not a valid [`Slug`](crate::Slug).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` must be 1-64 lowercase letters, digits or hyphens, starting with a letter or digit")]
pub struct InvalidSlug(pub String);

/// A string that is not a valid [`RequestPath`](crate::RequestPath).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` must start with a single `/` and contain no spaces, control characters or `#`")]
pub struct InvalidRequestPath(pub String);

/// A scenario document that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ScenarioError {
    /// The document is not valid TOML or does not match the scenario schema.
    #[error("scenario does not match the schema: {0}")]
    Syntax(#[source] toml::de::Error),
    /// The `schema` version is not one this crate reads.
    #[error("scenario schema {found} is not supported")]
    UnsupportedSchema {
        /// The version found in the document.
        found: u32,
    },
    /// The description is empty or whitespace.
    #[error("scenario description is empty")]
    EmptyDescription,
    /// The scenario has no steps.
    #[error("scenario has no steps")]
    NoSteps,
    /// The scenario compares nothing.
    #[error("scenario compares no aspects")]
    NoAspects,
    /// An aspect is listed more than once.
    #[error("aspect `{0}` is listed more than once")]
    DuplicateAspect(Aspect),
    /// A normalization location is not one the harness can rewrite.
    #[error("`{0}` is not a normalizable location")]
    InvalidLocation(String),
    /// A normalization has no reason.
    #[error("normalization of `{0}` has no reason")]
    MissingReason(Location),
    /// A normalization has no reviewer.
    #[error("normalization of `{0}` has no reviewer")]
    MissingReviewer(Location),
    /// A normalization targets an aspect the scenario does not compare.
    #[error("normalization of `{0}` targets an aspect the scenario does not compare")]
    NormalizationNotCompared(Location),
    /// The same location is normalized twice.
    #[error("`{0}` is normalized more than once")]
    DuplicateNormalization(Location),
}
