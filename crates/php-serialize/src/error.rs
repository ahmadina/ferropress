//! Diagnostics and the decoding error of `php-serialize`.

use std::fmt;

/// How PHP reports a [`Diagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Reported as `E_WARNING`.
    Warning,
    /// Reported as `E_DEPRECATED`.
    Deprecated,
}

/// A message PHP emits while it reads serialized data, in the order it emits them.
///
/// The runtime turns these into PHP-visible errors. Messages for which
/// [`Diagnostic::has_function_prefix`] is true are prefixed with the calling function, as in
/// `unserialize(): Numerical result out of range`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Diagnostic<'a> {
    /// An integer, element count or payload length did not fit in a signed 64-bit integer and was
    /// clamped to the nearest limit.
    NumberOutOfRange,
    /// The legacy escaped string token `S:` was read.
    EscapedStringDeprecated,
    /// A `}` appeared where another value or key was expected.
    UnexpectedEnd,
    /// An object header ended too close to the end of the input to hold its property count.
    BadData,
    /// A `C:` payload length exceeded the remaining input or was negative.
    InsufficientData {
        /// The length the token declared.
        required: i64,
        /// The bytes left after the opening brace.
        present: usize,
    },
    /// An `E:` token's text had no `:` between the enum name and the case name.
    MissingEnumColon {
        /// The token's text.
        name: &'a [u8],
    },
    /// Arrays and objects were nested deeper than the configured limit.
    MaxDepthExceeded {
        /// The configured limit.
        max_depth: usize,
    },
}

impl Diagnostic<'_> {
    /// How PHP reports this diagnostic.
    pub const fn severity(&self) -> Severity {
        match self {
            Self::EscapedStringDeprecated => Severity::Deprecated,
            _ => Severity::Warning,
        }
    }

    /// Whether PHP prefixes the message with the calling function's name.
    pub const fn has_function_prefix(&self) -> bool {
        !matches!(self, Self::BadData | Self::InsufficientData { .. })
    }

    /// The exact message bytes PHP emits, without the function prefix.
    pub fn message(&self) -> Vec<u8> {
        match self {
            Self::NumberOutOfRange => b"Numerical result out of range".to_vec(),
            Self::EscapedStringDeprecated => b"Unserializing the 'S' format is deprecated".to_vec(),
            Self::UnexpectedEnd => b"Unexpected end of serialized data".to_vec(),
            Self::BadData => b"Bad unserialize data".to_vec(),
            Self::InsufficientData { required, present } => format!(
                "Insufficient data for unserializing - {required} required, {present} present"
            )
            .into_bytes(),
            Self::MissingEnumColon { name } => {
                let shown = name.split(|&b| b == 0).next().unwrap_or_default();
                let mut message = b"Invalid enum name '".to_vec();
                message.extend_from_slice(shown);
                message.extend_from_slice(b"' (missing colon)");
                message
            }
            Self::MaxDepthExceeded { max_depth } => format!(
                "Maximum depth of {max_depth} exceeded. The depth limit can be changed using the \
                 max_depth unserialize() option or the unserialize_max_depth ini setting"
            )
            .into_bytes(),
        }
    }
}

impl fmt::Display for Diagnostic<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.message()))
    }
}

/// Serialized data that PHP would refuse to read.
///
/// [`DecodeError::offset`] is the byte offset PHP reports in its `Error at offset` warning. PHP
/// returns `false` for empty input without any warning; that case is reported here with offset 0.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid serialized data at offset {offset}")]
pub struct DecodeError<'a> {
    offset: usize,
    diagnostics: Vec<Diagnostic<'a>>,
}

impl<'a> DecodeError<'a> {
    pub(crate) const fn new(offset: usize, diagnostics: Vec<Diagnostic<'a>>) -> Self {
        Self {
            offset,
            diagnostics,
        }
    }

    /// The byte offset at which PHP stops reading.
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// The diagnostics PHP emits before giving up, in order.
    pub fn diagnostics(&self) -> &[Diagnostic<'a>] {
        &self.diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::{DecodeError, Diagnostic, Severity};

    #[test]
    fn messages_match_php_text() {
        let cases: [(Diagnostic<'_>, &str, Severity, bool); 7] = [
            (
                Diagnostic::NumberOutOfRange,
                "Numerical result out of range",
                Severity::Warning,
                true,
            ),
            (
                Diagnostic::EscapedStringDeprecated,
                "Unserializing the 'S' format is deprecated",
                Severity::Deprecated,
                true,
            ),
            (
                Diagnostic::UnexpectedEnd,
                "Unexpected end of serialized data",
                Severity::Warning,
                true,
            ),
            (
                Diagnostic::BadData,
                "Bad unserialize data",
                Severity::Warning,
                false,
            ),
            (
                Diagnostic::InsufficientData {
                    required: -1,
                    present: 3,
                },
                "Insufficient data for unserializing - -1 required, 3 present",
                Severity::Warning,
                false,
            ),
            (
                Diagnostic::MissingEnumColon {
                    name: b"Suit\0Hearts",
                },
                "Invalid enum name 'Suit' (missing colon)",
                Severity::Warning,
                true,
            ),
            (
                Diagnostic::MaxDepthExceeded { max_depth: 2 },
                "Maximum depth of 2 exceeded. The depth limit can be changed using the max_depth \
                 unserialize() option or the unserialize_max_depth ini setting",
                Severity::Warning,
                true,
            ),
        ];
        for (diagnostic, message, severity, prefixed) in cases {
            assert_eq!(diagnostic.to_string(), message);
            assert_eq!(diagnostic.severity(), severity, "{message}");
            assert_eq!(diagnostic.has_function_prefix(), prefixed, "{message}");
        }
    }

    #[test]
    fn errors_expose_offset_and_diagnostics() {
        let error = DecodeError::new(4, vec![Diagnostic::UnexpectedEnd]);
        assert_eq!(error.offset(), 4);
        assert_eq!(error.diagnostics(), [Diagnostic::UnexpectedEnd]);
        assert_eq!(error.to_string(), "invalid serialized data at offset 4");
    }
}
