//! Cache groups, including the empty/`0` rewrite used on read and write.

/// The group used when the caller passed an empty name or `0`.
const DEFAULT: &[u8] = b"default";

/// A cache group name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Group(Box<[u8]>);

impl Group {
    /// Empty or `0` becomes `default` (PHP `empty`). Other bytes stay as given.
    #[must_use]
    pub fn new(bytes: impl AsRef<[u8]>) -> Self {
        let bytes = bytes.as_ref();
        if bytes.is_empty() || bytes == b"0" {
            Self(DEFAULT.into())
        } else {
            Self(bytes.into())
        }
    }

    /// The bytes as stored, with no empty rewrite. Used by [`crate::Memory::flush_group`].
    #[must_use]
    pub fn exact(bytes: impl AsRef<[u8]>) -> Self {
        Self(bytes.as_ref().into())
    }

    /// The group used when the name was empty or `0`.
    #[must_use]
    pub fn default_group() -> Self {
        Self(DEFAULT.into())
    }

    /// The bytes as stored.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for Group {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::Group;

    #[test]
    fn empty_and_zero_become_default() {
        assert_eq!(Group::new("").as_bytes(), b"default");
        assert_eq!(Group::new("0").as_bytes(), b"default");
        assert_eq!(Group::new("00").as_bytes(), b"00");
        assert_eq!(Group::new(" ").as_bytes(), b" ");
        assert_eq!(Group::exact("").as_bytes(), b"");
        assert_eq!(Group::default_group().as_bytes(), b"default");
        assert_eq!(Group::new("posts").as_ref(), b"posts");
    }
}
