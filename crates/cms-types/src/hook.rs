//! Hook name, priority and accepted-argument count.

use std::fmt;

/// The exact byte name of a hook. `the_content` and `The_Content` are different names.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HookName(Box<[u8]>);

impl HookName {
    /// The reserved name whose callbacks run before every other hook.
    pub const ALL: &[u8] = b"all";

    /// A name with these bytes.
    #[must_use]
    pub fn new(name: impl AsRef<[u8]>) -> Self {
        Self(name.as_ref().into())
    }

    /// The bytes as stored.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Whether this is the reserved `all` hook.
    #[must_use]
    pub fn is_all(&self) -> bool {
        self.0.as_ref() == Self::ALL
    }
}

impl AsRef<[u8]> for HookName {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl From<&[u8]> for HookName {
    fn from(name: &[u8]) -> Self {
        Self::new(name)
    }
}

impl From<&str> for HookName {
    fn from(name: &str) -> Self {
        Self::new(name.as_bytes())
    }
}

impl fmt::Display for HookName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.0))
    }
}

/// The order a callback runs in on one hook. Lower values run first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Priority(i32);

impl Priority {
    /// The default used when a caller does not name a priority.
    pub const DEFAULT: Self = Self(10);

    /// The value a PHP `null` priority becomes.
    pub const NULL: Self = Self(0);

    /// A priority with this integer.
    #[must_use]
    pub const fn new(value: i32) -> Self {
        Self(value)
    }

    /// The integer as stored.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<i32> for Priority {
    fn from(value: i32) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// How many arguments a callback opted to receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArgCount(u32);

impl ArgCount {
    /// The default used when a caller does not name a count.
    pub const DEFAULT: Self = Self(1);

    /// The callback is invoked with no arguments.
    pub const NONE: Self = Self(0);

    /// A count of this many arguments.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// The count as stored.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Default for ArgCount {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<u32> for ArgCount {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{ArgCount, HookName, Priority};

    #[test]
    fn hook_names_are_exact_bytes() {
        let lower = HookName::from("the_content");
        let mixed = HookName::from("The_Content");
        assert_ne!(lower, mixed);
        assert_eq!(lower.as_bytes(), b"the_content");
        assert!(HookName::from("all").is_all());
        assert!(!lower.is_all());
        assert_eq!(format!("{lower}"), "the_content");
    }

    #[test]
    fn priorities_order_numerically() {
        assert!(Priority::new(-5) < Priority::NULL);
        assert!(Priority::NULL < Priority::DEFAULT);
        assert!(Priority::DEFAULT < Priority::new(20));
        assert_eq!(Priority::DEFAULT.get(), 10);
        assert_eq!(Priority::default(), Priority::DEFAULT);
        assert_eq!(format!("{}", Priority::DEFAULT), "10");
    }

    #[test]
    fn argument_counts_include_none() {
        assert_eq!(ArgCount::DEFAULT.get(), 1);
        assert_eq!(ArgCount::NONE.get(), 0);
        assert_eq!(ArgCount::from(3).get(), 3);
        assert_eq!(ArgCount::default(), ArgCount::DEFAULT);
    }
}
