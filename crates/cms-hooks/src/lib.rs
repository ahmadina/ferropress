//! Priority-ordered filter and action dispatch.
//!
//! [`Hooks`] stores live callables (`php_value::Value`) and runs them through the invoke port
//! (ADR-0003). Behavior is `contracts/hooks/dispatch.md`. PHP-visible names are registered by
//! `php-bindings`, not here.

mod identity;
mod registry;

pub use registry::{Dispatch, DispatchKind, Hooks};
