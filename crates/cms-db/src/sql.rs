//! A SQL statement as the caller wrote it.

/// One statement. The store does not rewrite these bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sql(Box<[u8]>);

impl Sql {
    /// A statement with these bytes.
    #[must_use]
    pub fn new(bytes: impl AsRef<[u8]>) -> Self {
        Self(bytes.as_ref().into())
    }

    /// The bytes as stored.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for Sql {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl From<&[u8]> for Sql {
    fn from(bytes: &[u8]) -> Self {
        Self::new(bytes)
    }
}

impl From<&str> for Sql {
    fn from(text: &str) -> Self {
        Self::new(text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::Sql;

    #[test]
    fn sql_keeps_the_bytes_as_given() {
        let sql = Sql::from("SELECT 1");
        assert_eq!(sql.as_bytes(), b"SELECT 1");
        assert_eq!(Sql::from(&b"INSERT 2"[..]).as_bytes(), b"INSERT 2");
    }
}
