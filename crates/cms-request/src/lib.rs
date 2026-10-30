//! One request's native context.
//!
//! [`Request`] holds the hook table and the flags the plugin-load planner reads
//! (`contracts/hooks/dispatch.md`, `contracts/entries/plugin-load-order.toml`). It is request-affine
//! and not [`Send`]. HTTP, query parsing and PHP-visible names live elsewhere.

use std::rc::Rc;

use cms_hooks::Hooks;

/// Flags that change which plugins load on this request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// The platform is installing; site plugins do not load.
    pub installing: bool,
    /// Recovery mode; paused plugins are skipped.
    pub recovery: bool,
    /// The request is on a network; network-wide plugins load.
    pub network: bool,
}

/// The native state of one request.
///
/// Contract: `contracts/hooks/dispatch.md`. Plugin load order is planned by `cms-bootstrap`.
#[derive(Debug, Default)]
pub struct Request {
    hooks: Rc<Hooks>,
    flags: Flags,
}

impl Request {
    /// An empty request: no callbacks, all flags clear.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A request with these flags and an empty hook table.
    #[must_use]
    pub fn with_flags(flags: Flags) -> Self {
        Self {
            hooks: Rc::new(Hooks::new()),
            flags,
        }
    }

    /// The hook table for this request.
    #[must_use]
    pub fn hooks(&self) -> &Hooks {
        &self.hooks
    }

    /// A shared handle to the hook table, for host bindings.
    #[must_use]
    pub fn shared_hooks(&self) -> Rc<Hooks> {
        Rc::clone(&self.hooks)
    }

    /// The boot flags for this request.
    #[must_use]
    pub fn flags(&self) -> Flags {
        self.flags
    }

    /// Replaces the boot flags.
    pub fn set_flags(&mut self, flags: Flags) {
        self.flags = flags;
    }
}

#[cfg(test)]
mod tests {
    use super::{Flags, Request};

    #[test]
    fn a_new_request_has_clear_flags() {
        let request = Request::new();
        assert_eq!(request.flags(), Flags::default());
    }

    #[test]
    fn flags_can_be_replaced() {
        let mut request = Request::with_flags(Flags {
            installing: true,
            recovery: false,
            network: true,
        });
        assert!(request.flags().installing);
        request.set_flags(Flags {
            installing: false,
            recovery: true,
            network: true,
        });
        assert!(request.flags().recovery);
        assert!(!request.flags().installing);
    }
}
