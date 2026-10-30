//! Errors returned by `php-abi-bridge`.

use php_vm::{SessionError, VmError};

/// Why the bridge could not start a request or finish an invoke.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BridgeError {
    /// The VM itself failed.
    #[error(transparent)]
    Vm(#[from] VmError),
    /// PHP threw and nothing in the callable caught it, or the engine failed during invoke.
    #[error(transparent)]
    Session(#[from] SessionError),
}
