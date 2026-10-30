//! PHP-visible hook names that delegate to [`cms_hooks::Hooks`] (ADR-0003).
//!
//! Contract: `contracts/hooks/dispatch.md`. No dispatch behavior is implemented here.

mod convert;
mod host;

pub use host::HookHost;
