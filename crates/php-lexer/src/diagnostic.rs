//! Scanner diagnostics.

use std::fmt;

/// A scanner error. Reading continues after one; when the source is compiled, PHP reports the first
/// error as a `ParseError` with exactly this message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LexError {
    /// A closing bracket with no bracket open.
    #[error("Unmatched '{}'", char::from(*.0))]
    Unmatched(u8),
    /// A closing bracket that does not match the innermost open one.
    #[error("Unclosed '{}'{} does not match '{}'", char::from(*.opening), OnLine(*.opening_line, *.line), char::from(*.closing))]
    Mismatched {
        /// The open bracket, as written (`{` for `${` and `{$`, `[` for `#[`).
        opening: u8,
        /// Line of the open bracket.
        opening_line: u32,
        /// The closing bracket.
        closing: u8,
        /// Line of the closing bracket.
        line: u32,
    },
    /// A bracket still open at the end of the source.
    #[error("Unclosed '{}'{}", char::from(*.opening), OnLine(*.opening_line, *.line))]
    Unclosed {
        /// The open bracket.
        opening: u8,
        /// Line of the open bracket.
        opening_line: u32,
        /// Line of the end of the source.
        line: u32,
    },
    /// A `/*` comment without `*/`.
    #[error("Unterminated comment starting line {0}")]
    UnterminatedComment(u32),
    /// A decimal literal with a leading zero and a digit 8 or 9.
    #[error("Invalid numeric literal")]
    InvalidNumericLiteral,
    /// `\u{…}` that is empty, not hexadecimal or not closed.
    #[error("Invalid UTF-8 codepoint escape sequence")]
    InvalidCodepointEscape,
    /// `\u{…}` above U+10FFFF.
    #[error("Invalid UTF-8 codepoint escape sequence: Codepoint too large")]
    CodepointTooLarge,
    /// Heredoc indentation mixing tabs and spaces.
    #[error("Invalid indentation - tabs and spaces cannot be mixed")]
    MixedIndentation,
    /// A heredoc whose first line interpolates and is indented less than the closing label.
    #[error("Invalid body indentation level (expecting an indentation level of at least {0})")]
    BodyIndentation(usize),
}

/// ` on line N` when the bracket was opened on another line than the one reporting it.
struct OnLine(u32, u32);

impl fmt::Display for OnLine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == self.1 {
            Ok(())
        } else {
            write!(formatter, " on line {}", self.0)
        }
    }
}

/// A scanner warning, reported as an `E_WARNING` while the source is read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LexWarning {
    /// A three-digit octal escape above `\377`; the value wraps to one byte.
    #[error("Octal escape sequence overflow \\{} is greater than \\377", String::from_utf8_lossy(.0))]
    OctalEscapeOverflow([u8; 3]),
}

/// A diagnostic and the source offset of the token that raised it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    /// A scanner error.
    Error {
        /// The error.
        error: LexError,
        /// Offset of the token being read when it was raised.
        offset: u32,
    },
    /// A scanner warning.
    Warning {
        /// The warning.
        warning: LexWarning,
        /// Offset of the token being read when it was raised.
        offset: u32,
        /// Line of the escape sequence that raised it.
        line: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::{LexError, LexWarning};

    #[test]
    fn messages_match_php() {
        let cases = [
            (LexError::Unmatched(b')'), "Unmatched ')'"),
            (
                LexError::Mismatched {
                    opening: b'(',
                    opening_line: 2,
                    closing: b']',
                    line: 2,
                },
                "Unclosed '(' does not match ']'",
            ),
            (
                LexError::Mismatched {
                    opening: b'{',
                    opening_line: 1,
                    closing: b')',
                    line: 3,
                },
                "Unclosed '{' on line 1 does not match ')'",
            ),
            (
                LexError::Unclosed {
                    opening: b'[',
                    opening_line: 4,
                    line: 4,
                },
                "Unclosed '['",
            ),
            (
                LexError::Unclosed {
                    opening: b'[',
                    opening_line: 4,
                    line: 5,
                },
                "Unclosed '[' on line 4",
            ),
            (
                LexError::UnterminatedComment(7),
                "Unterminated comment starting line 7",
            ),
            (LexError::InvalidNumericLiteral, "Invalid numeric literal"),
            (
                LexError::InvalidCodepointEscape,
                "Invalid UTF-8 codepoint escape sequence",
            ),
            (
                LexError::CodepointTooLarge,
                "Invalid UTF-8 codepoint escape sequence: Codepoint too large",
            ),
            (
                LexError::MixedIndentation,
                "Invalid indentation - tabs and spaces cannot be mixed",
            ),
            (
                LexError::BodyIndentation(4),
                "Invalid body indentation level (expecting an indentation level of at least 4)",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
        assert_eq!(
            LexWarning::OctalEscapeOverflow(*b"400").to_string(),
            "Octal escape sequence overflow \\400 is greater than \\377"
        );
    }
}
