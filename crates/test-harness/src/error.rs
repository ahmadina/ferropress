//! Errors returned by `test-harness`.

/// A string that is not a valid [`Slug`](crate::Slug).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` must be 1-64 lowercase letters, digits or hyphens, starting with a letter or digit")]
pub struct InvalidSlug(pub String);

/// A string that is not a valid [`RequestPath`](crate::RequestPath).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` must start with a single `/` and contain no spaces, control characters or `#`")]
pub struct InvalidRequestPath(pub String);
