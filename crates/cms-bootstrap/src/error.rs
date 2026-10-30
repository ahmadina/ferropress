//! Errors returned by `cms-bootstrap`.

/// Why plugin load stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BootError<L, I> {
    /// The plugin file could not be loaded.
    #[error(transparent)]
    Load(L),
    /// A load action could not invoke its callbacks.
    #[error(transparent)]
    Invoke(I),
}
