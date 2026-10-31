//! Failures from cache key construction.

/// A cache key was not an integer and was empty after PHP `trim`.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyError {
    /// Nothing usable remained.
    #[error("a cache key must not be empty")]
    Empty,
}

#[cfg(test)]
mod tests {
    use super::KeyError;

    #[test]
    fn empty_keys_describe_themselves() {
        assert_eq!(KeyError::Empty.to_string(), "a cache key must not be empty");
    }
}
