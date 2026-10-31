//! Table prefix, inventory stem and physical table name.

use crate::NameError;

/// Site table prefix. Empty is allowed; a NUL byte is not.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TablePrefix(Box<[u8]>);

impl TablePrefix {
    /// A prefix with these bytes.
    ///
    /// # Errors
    ///
    /// [`NameError::Nul`] when `bytes` contains a NUL.
    pub fn new(bytes: impl AsRef<[u8]>) -> Result<Self, NameError> {
        let bytes = bytes.as_ref();
        if bytes.contains(&0) {
            return Err(NameError::Nul);
        }
        Ok(Self(bytes.into()))
    }

    /// The bytes as stored.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The physical name of `stem` on a site that uses this prefix.
    #[must_use]
    pub fn table(&self, stem: &TableStem) -> TableName {
        let mut name = Vec::with_capacity(self.0.len() + stem.0.len());
        name.extend_from_slice(&self.0);
        name.extend_from_slice(&stem.0);
        TableName(name.into())
    }
}

impl AsRef<[u8]> for TablePrefix {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

/// Inventory table name without a site prefix (`options`, `posts`, …).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TableStem(Box<[u8]>);

impl TableStem {
    /// A stem with these bytes.
    ///
    /// # Errors
    ///
    /// [`NameError::Empty`] when `bytes` is empty; [`NameError::Nul`] when it contains a NUL.
    pub fn new(bytes: impl AsRef<[u8]>) -> Result<Self, NameError> {
        let bytes = bytes.as_ref();
        if bytes.is_empty() {
            return Err(NameError::Empty);
        }
        if bytes.contains(&0) {
            return Err(NameError::Nul);
        }
        Ok(Self(bytes.into()))
    }

    /// The bytes as stored.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for TableStem {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

/// Physical table name: prefix bytes followed by stem bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TableName(Box<[u8]>);

impl TableName {
    /// The bytes as stored.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for TableName {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::{TablePrefix, TableStem};
    use crate::NameError;

    #[test]
    fn prefix_joins_the_stem() {
        let prefix = TablePrefix::new("fp_");
        let stem = TableStem::new("options");
        assert!(prefix.is_ok() && stem.is_ok());
        if let (Ok(prefix), Ok(stem)) = (prefix, stem) {
            assert_eq!(prefix.table(&stem).as_bytes(), b"fp_options");
            assert_eq!(prefix.as_bytes(), b"fp_");
            assert_eq!(stem.as_bytes(), b"options");
        }
    }

    #[test]
    fn an_empty_prefix_is_the_stem() {
        let prefix = TablePrefix::new("");
        let stem = TableStem::new("posts");
        assert!(prefix.is_ok() && stem.is_ok());
        if let (Ok(prefix), Ok(stem)) = (prefix, stem) {
            assert_eq!(prefix.table(&stem).as_bytes(), b"posts");
        }
    }

    #[test]
    fn nul_and_empty_stems_are_rejected() {
        assert!(matches!(TablePrefix::new("a\0b"), Err(NameError::Nul)));
        assert!(matches!(TableStem::new(""), Err(NameError::Empty)));
        assert!(matches!(TableStem::new("a\0"), Err(NameError::Nul)));
    }

    proptest::proptest! {
        #[test]
        fn prefix_then_stem_is_concatenation(
            prefix in "[^\0]{0,16}",
            stem in "[^\0]{1,16}",
        ) {
            let Ok(named) = TablePrefix::new(prefix.as_bytes()) else {
                return Ok(());
            };
            let Ok(table) = TableStem::new(stem.as_bytes()) else {
                return Ok(());
            };
            let mut joined = prefix.into_bytes();
            joined.extend_from_slice(stem.as_bytes());
            let physical = named.table(&table);
            proptest::prop_assert_eq!(physical.as_bytes(), joined.as_slice());
        }
    }
}
