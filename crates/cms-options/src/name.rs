//! Option names after trim, empty-check and the two renamed keys.

use crate::NameError;

/// Bytes the reference's `trim` strips from both ends of a name.
const TRIM: &[u8] = b" \t\n\r\x0b\0";

/// A stored option name. `Home` and `home` are different names.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OptionName(Box<[u8]>);

impl OptionName {
    /// Trim, reject an empty/`0` name, then apply the two renamed keys.
    ///
    /// # Errors
    ///
    /// [`NameError::Empty`] when nothing remains, or only `0` remains.
    pub fn new(bytes: impl AsRef<[u8]>) -> Result<Self, NameError> {
        let trimmed = trim(bytes.as_ref());
        if trimmed.is_empty() || trimmed == b"0" {
            return Err(NameError::Empty);
        }
        let canonical = match trimmed {
            b"blacklist_keys" => b"disallowed_keys".as_slice(),
            b"comment_whitelist" => b"comment_previously_approved".as_slice(),
            other => other,
        };
        Ok(Self(canonical.into()))
    }

    /// The bytes as stored.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The `siteurl` option. Used when `home` is stored as empty bytes.
    #[must_use]
    pub fn siteurl() -> Self {
        Self((*b"siteurl").into())
    }

    /// The `home` option, which falls back to `siteurl` when empty.
    #[must_use]
    pub fn is_home(&self) -> bool {
        self.0.as_ref() == b"home"
    }

    /// Whether trailing `/` and `\` are stripped from the stored value.
    #[must_use]
    pub fn strips_trailing_slash(&self) -> bool {
        matches!(
            self.0.as_ref(),
            b"siteurl" | b"home" | b"category_base" | b"tag_base"
        )
    }
}

impl AsRef<[u8]> for OptionName {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !TRIM.contains(b))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !TRIM.contains(b))
        .map_or(start, |i| i + 1);
    &bytes[start..end]
}

#[cfg(test)]
mod tests {
    use super::OptionName;
    use crate::NameError;

    #[test]
    fn trim_empty_and_zero_are_rejected() {
        assert!(matches!(OptionName::new(""), Err(NameError::Empty)));
        assert!(matches!(OptionName::new("   "), Err(NameError::Empty)));
        assert!(matches!(OptionName::new("0"), Err(NameError::Empty)));
        assert!(matches!(OptionName::new(" 0\n"), Err(NameError::Empty)));
        assert!(OptionName::new("00").is_ok());
        assert!(OptionName::new("home").is_ok());
    }

    #[test]
    fn deprecated_keys_are_rewritten() {
        let keys = OptionName::new("blacklist_keys");
        let comments = OptionName::new("comment_whitelist");
        assert!(keys.is_ok() && comments.is_ok());
        if let (Ok(keys), Ok(comments)) = (keys, comments) {
            assert_eq!(keys.as_bytes(), b"disallowed_keys");
            assert_eq!(comments.as_bytes(), b"comment_previously_approved");
            assert!(!keys.is_home());
        }
        let home = OptionName::new("home");
        assert!(home.as_ref().is_ok_and(OptionName::is_home));
        if let Ok(home) = home {
            assert!(home.strips_trailing_slash());
        }
        let other = OptionName::new("blogname");
        assert!(other.is_ok());
        if let Ok(other) = other {
            assert!(!other.strips_trailing_slash());
        }
        assert_eq!(OptionName::siteurl().as_bytes(), b"siteurl");
        assert!(OptionName::siteurl().strips_trailing_slash());
    }
}
