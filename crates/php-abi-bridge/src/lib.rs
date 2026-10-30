//! Invokes live PHP callables from the native core (ADR-0003).
//!
//! Domain crates hold [`php_value::Value`] handles and call PHP through [`Invoke`]. This crate
//! implements that port on a [`php_vm::Session`]: identity, by-reference arguments and one request
//! stack. Compiling a program is out of scope of the invoke itself; [`Bridge::boot`] takes a
//! compiled unit. PHP classes extending Rust-owned classes are not bound yet.
//!
//! ```
//! use php_abi_bridge::Bridge;
//! use php_compiler::compile;
//! use php_parser::{ParseOptions, parse};
//! use php_value::{Invoke, Slot, Value};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let source = b"<?php function inc($x) { return $x + 1; }";
//! let program = parse(source, ParseOptions::default())?.result?;
//! let compiled = compile(&program, b"/t.php")?;
//! let unit = compiled.unit.ok_or("no unit")?;
//! let mut output = Vec::new();
//! let mut bridge = Bridge::boot(&unit, b"/t.php", &mut output)?;
//! let got = bridge.invoke(&Value::from("inc"), &mut [Slot::from(Value::from(1_i64))])?;
//! assert!(matches!(got, Value::Int(2)));
//! # Ok(())
//! # }
//! ```

mod error;

pub use error::BridgeError;

use php_compiler::bytecode::Unit;
use php_value::{Invoke, Slot, Value};
use php_vm::{Output, Session};

/// One request: compiled PHP that has run its main code and can still be called.
pub struct Bridge<'o, O: Output> {
    session: Session<'o, O>,
}

impl<O: Output> std::fmt::Debug for Bridge<'_, O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bridge")
            .field("status", &self.session.status())
            .finish_non_exhaustive()
    }
}

impl<'o, O: Output> Bridge<'o, O> {
    /// Runs `unit`'s main code and keeps the request alive for later [`Invoke`] calls.
    ///
    /// # Errors
    ///
    /// [`BridgeError::Vm`] when the bytecode is malformed or needs what the VM cannot run.
    pub fn boot(unit: &Unit, file: &[u8], output: &'o mut O) -> Result<Self, BridgeError> {
        Ok(Self {
            session: Session::boot(unit, file, output)?,
        })
    }

    /// The exit status of the main code: 0 when it finished, 255 after an uncaught error.
    #[must_use]
    pub fn status(&self) -> u8 {
        self.session.status()
    }

    /// The value the main code returned, or `null` when it did not.
    #[must_use]
    pub fn returned(&self) -> &Value {
        self.session.returned()
    }

    /// Runs `callee` with `args` and returns the callable's result.
    ///
    /// # Errors
    ///
    /// [`BridgeError::Session`] when PHP throws or the engine fails.
    pub fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, BridgeError> {
        Ok(self.session.invoke(callee, args)?)
    }
}

impl<O: Output> Invoke for Bridge<'_, O> {
    type Error = BridgeError;

    fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
        Bridge::invoke(self, callee, args)
    }
}

#[cfg(test)]
mod tests {
    use super::BridgeError;
    use php_value::{ObjectStore, PhpString};
    use php_vm::{SessionError, VmError};

    #[test]
    fn errors_describe_themselves() -> Result<(), String> {
        let vm = BridgeError::from(VmError::Malformed("no such slot"));
        assert_eq!(vm.to_string(), "malformed bytecode: no such slot");
        let object = ObjectStore::new()
            .create(PhpString::from("Error"))
            .map_err(|error| error.to_string())?;
        let session = BridgeError::from(SessionError::Thrown {
            class: "Error".into(),
            message: b"nope".to_vec(),
            object,
        });
        assert_eq!(session.to_string(), "uncaught Error: nope");
        Ok(())
    }
}
