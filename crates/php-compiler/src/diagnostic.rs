//! Compile-time diagnostics, as PHP reports them.

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Stops compilation; nothing of the file runs.
    Fatal,
    /// Reported; compilation continues.
    Warning,
    /// Reported; compilation continues.
    Deprecated,
}

impl Severity {
    /// The label PHP prints before the message.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fatal => "Fatal error",
            Self::Warning => "Warning",
            Self::Deprecated => "Deprecated",
        }
    }
}

/// One diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// How serious it is.
    pub severity: Severity,
    /// The message; names from the source keep their bytes.
    pub message: Vec<u8>,
    /// The line it is reported on.
    pub line: u32,
}

/// Everything compiling a file reported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    /// Warnings and deprecations, in the order they were raised.
    pub warnings: Vec<Diagnostic>,
    /// The fatal error that stopped compilation, raised after every warning.
    pub fatal: Option<Diagnostic>,
}

#[cfg(test)]
mod tests {
    use super::Severity;

    #[test]
    fn labels_follow_php() {
        assert_eq!(Severity::Fatal.label(), "Fatal error");
        assert_eq!(Severity::Warning.label(), "Warning");
        assert_eq!(Severity::Deprecated.label(), "Deprecated");
    }
}
