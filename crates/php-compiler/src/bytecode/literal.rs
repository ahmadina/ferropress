//! Values known while compiling, held in a code object's literal pool.

/// A literal: owned data with no shared ownership, so compiled units can be shared between
/// threads and the VM builds its runtime values from them.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
    /// A byte string.
    String(Box<[u8]>),
}

impl From<&[u8]> for Literal {
    fn from(bytes: &[u8]) -> Self {
        Self::String(bytes.into())
    }
}

#[cfg(test)]
mod tests {
    use super::Literal;

    #[test]
    fn byte_strings_convert() {
        assert_eq!(
            Literal::from(&b"a"[..]),
            Literal::String(Box::from(&b"a"[..]))
        );
    }
}
