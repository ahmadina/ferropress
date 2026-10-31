//! How a row is marked for the autoload `SELECT`.

/// Stored `autoload` column after the reference's boolean / yes-no mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Autoload {
    /// Stored as `on` (also the mapping of `true` and `yes`).
    On,
    /// Stored as `off` (also the mapping of `false` and `no`).
    Off,
    /// Stored as `auto` when the caller did not pick on or off.
    Auto,
}

impl Autoload {
    /// `true` → [`Self::On`], `false` → [`Self::Off`].
    #[must_use]
    pub const fn from_flag(load: bool) -> Self {
        if load { Self::On } else { Self::Off }
    }

    /// Bytes written to the `autoload` column.
    #[must_use]
    pub const fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::On => b"on",
            Self::Off => b"off",
            Self::Auto => b"auto",
        }
    }

    /// Whether this token is in the autoload `IN` list.
    #[must_use]
    pub const fn is_loaded(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Tokens the autoload `SELECT` keeps.
#[must_use]
pub(crate) fn is_loaded_token(bytes: &[u8]) -> bool {
    matches!(bytes, b"yes" | b"on" | b"auto-on" | b"auto")
}

/// Tokens the reference will re-evaluate on update when autoload was omitted.
#[must_use]
pub(crate) fn is_auto_family(bytes: &[u8]) -> bool {
    matches!(bytes, b"auto-on" | b"auto-off" | b"auto")
}

#[cfg(test)]
mod tests {
    use super::{Autoload, is_auto_family, is_loaded_token};

    #[test]
    fn flags_and_tokens_match_the_lab() {
        assert_eq!(Autoload::from_flag(true).as_bytes(), b"on");
        assert_eq!(Autoload::from_flag(false).as_bytes(), b"off");
        assert_eq!(Autoload::Auto.as_bytes(), b"auto");
        assert!(Autoload::On.is_loaded());
        assert!(Autoload::Auto.is_loaded());
        assert!(!Autoload::Off.is_loaded());
        assert!(is_loaded_token(b"yes"));
        assert!(is_loaded_token(b"on"));
        assert!(is_loaded_token(b"auto-on"));
        assert!(is_loaded_token(b"auto"));
        assert!(!is_loaded_token(b"off"));
        assert!(!is_loaded_token(b"auto-off"));
        assert!(is_auto_family(b"auto-on"));
        assert!(is_auto_family(b"auto-off"));
        assert!(is_auto_family(b"auto"));
        assert!(!is_auto_family(b"on"));
    }
}
