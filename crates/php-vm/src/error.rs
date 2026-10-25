//! Why the VM stopped without finishing the script.

/// A failure of the VM itself, as opposed to an error in the script, which is reported in the
/// script's output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum VmError {
    /// The bytecode refers to something that does not exist, which the compiler never produces.
    #[error("malformed bytecode: {0}")]
    Malformed(&'static str),
    /// The script reached a value or operation this VM cannot handle yet.
    #[error("running {0} is not supported yet")]
    Unsupported(&'static str),
}

#[cfg(test)]
mod tests {
    use super::VmError;

    #[test]
    fn failures_describe_themselves() {
        assert_eq!(
            VmError::Malformed("no such slot").to_string(),
            "malformed bytecode: no such slot"
        );
        assert_eq!(
            VmError::Unsupported("objects").to_string(),
            "running objects is not supported yet"
        );
    }
}
