//! Foundation newtypes shared by the native kernel.
//!
//! This crate owns hook names, priorities and accepted-argument counts
//! (`contracts/hooks/dispatch.md`). Callables, request state and PHP-visible names live elsewhere.

mod hook;

pub use hook::{ArgCount, HookName, Priority};
