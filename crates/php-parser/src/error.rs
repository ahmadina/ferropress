//! Parse errors and warnings.

use std::fmt;

/// The PHP exception class a parse failure is reported as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    /// `ParseError`: syntax errors and scanner errors.
    Parse,
    /// `CompileError`: invalid modifiers and other checks made while the tree is built.
    Compile,
}

impl ErrorClass {
    /// The PHP class name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Parse => "ParseError",
            Self::Compile => "CompileError",
        }
    }
}

/// Why a source could not be parsed, as PHP reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// The exception class.
    pub class: ErrorClass,
    /// The message. It is bytes because it quotes source text, which need not be UTF-8.
    pub message: Vec<u8>,
    /// The line PHP attributes the error to; `None` when PHP reports no line of the source.
    pub line: Option<u32>,
}

impl ParseError {
    pub(crate) fn new(class: ErrorClass, message: impl Into<Vec<u8>>, line: Option<u32>) -> Self {
        Self {
            class,
            message: message.into(),
            line,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {}",
            self.class.name(),
            String::from_utf8_lossy(&self.message)
        )?;
        if let Some(line) = self.line {
            write!(formatter, " on line {line}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

/// How serious a warning is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// `E_WARNING` or `E_COMPILE_WARNING`, displayed as "Warning".
    Warning,
    /// `E_DEPRECATED`, displayed as "Deprecated".
    Deprecated,
}

impl Severity {
    /// The label PHP displays.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Warning => "Warning",
            Self::Deprecated => "Deprecated",
        }
    }
}

/// A warning raised while parsing; parsing continues after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// How serious it is.
    pub severity: Severity,
    /// The message.
    pub message: String,
    /// The line it is reported on.
    pub line: u32,
}

impl Warning {
    pub(crate) fn new(severity: Severity, message: impl Into<String>, line: u32) -> Self {
        Self {
            severity,
            message: message.into(),
            line,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorClass, ParseError, Severity, Warning};

    #[test]
    fn names_follow_php() {
        assert_eq!(ErrorClass::Parse.name(), "ParseError");
        assert_eq!(ErrorClass::Compile.name(), "CompileError");
        assert_eq!(Severity::Warning.name(), "Warning");
        assert_eq!(Severity::Deprecated.name(), "Deprecated");
    }

    #[test]
    fn display_includes_the_line_when_known() {
        let error = ParseError::new(ErrorClass::Parse, "syntax error", Some(3));
        assert_eq!(error.to_string(), "ParseError: syntax error on line 3");
        let error = ParseError::new(ErrorClass::Compile, &b"bad \xff"[..], None);
        assert_eq!(error.to_string(), "CompileError: bad \u{fffd}");
        let warning = Warning::new(Severity::Deprecated, "old", 2);
        assert_eq!((warning.message.as_str(), warning.line), ("old", 2));
    }
}
