//! A request that stays alive after its main code so Rust can invoke PHP callables.

use std::fmt;

use std::rc::Rc;

use php_compiler::bytecode::{Operand, Unit};
use php_value::{Invoke, Object, Slot, Value};

use crate::host::Host;

use crate::machine::{Exec, Machine, Stop};
use crate::{Output, VmError};

/// Why [`Session::invoke`] did not return a PHP value.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// The VM itself failed.
    #[error(transparent)]
    Vm(#[from] VmError),
    /// PHP threw and nothing in the callable caught it.
    #[error("uncaught {class}: {}", String::from_utf8_lossy(message))]
    Thrown {
        /// The thrown object's class name.
        class: String,
        /// The thrown object's message bytes.
        message: Vec<u8>,
        /// The live thrown object.
        object: Object,
    },
}

impl SessionError {
    pub(crate) fn from_stop(stop: Stop) -> Self {
        match stop {
            Stop::Vm(error) => Self::Vm(error),
            Stop::Thrown(thrown) => Self::Thrown {
                class: String::from_utf8_lossy(&thrown.class).into_owned(),
                message: thrown.message,
                object: thrown.object,
            },
        }
    }
}

/// A compiled unit whose main code has run and whose functions and objects are still live.
///
/// Handles are request-affine: the session is not [`Send`]. Contract: ADR-0003.
pub struct Session<'o, O: Output> {
    machine: Machine<'o, O>,
    status: u8,
}

impl<O: Output> fmt::Debug for Session<'_, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl<'o, O: Output> Session<'o, O> {
    /// Loads `unit` and runs its main code, writing printed output to `output`.
    ///
    /// # Errors
    ///
    /// [`VmError`] when the bytecode is malformed or needs what this VM cannot run.
    pub fn boot(unit: &Unit, file: &[u8], output: &'o mut O) -> Result<Self, VmError> {
        Self::boot_with_host(unit, file, output, None)
    }

    /// Loads `unit` with `host` already attached so main code can call host functions.
    ///
    /// # Errors
    ///
    /// [`VmError`] when the bytecode is malformed or needs what this VM cannot run.
    pub fn boot_with_host(
        unit: &Unit,
        file: &[u8],
        output: &'o mut O,
        host: Option<Rc<dyn Host>>,
    ) -> Result<Self, VmError> {
        let mut machine = Machine::new(unit, file, output);
        machine.host = host;
        let status = machine.run()?;
        Ok(Self { machine, status })
    }

    /// Attaches `host` so later calls (and `function_exists`) see those names.
    pub fn attach_host(&mut self, host: Rc<dyn Host>) {
        self.machine.host = Some(host);
    }

    /// The exit status of the main code: 0 when it finished, 255 after an uncaught error.
    #[must_use]
    pub fn status(&self) -> u8 {
        self.status
    }

    /// The value the main code returned, or `null` when it did not.
    #[must_use]
    pub fn returned(&self) -> &Value {
        &self.machine.return_value
    }

    /// Runs `callee` with `args` on this request and returns the callable's result.
    ///
    /// Nested invokes (a native that calls back into PHP) share this request and stack.
    ///
    /// # Errors
    ///
    /// [`SessionError::Vm`] when the engine fails; [`SessionError::Thrown`] when PHP throws and the
    /// callable does not catch it.
    pub fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, SessionError> {
        self.machine
            .invoke_callable(callee, args)
            .map_err(SessionError::from_stop)
    }
}

impl<O: Output> Invoke for Session<'_, O> {
    type Error = SessionError;

    fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
        Session::invoke(self, callee, args)
    }
}

impl<O: Output> Machine<'_, O> {
    pub(crate) fn invoke_callable(&mut self, callee: &Value, args: &mut [Slot]) -> Exec<Value> {
        let frames = self.frames.len();
        let pending = self.pending.len();
        match callee {
            Value::String(name) => self.init_string_call(name.as_bytes())?,
            Value::Object(object) => self.init_closure_call(object.clone())?,
            other => return Err(self.not_callable(other)),
        }
        for arg in args.iter() {
            self.send(crate::closures::copy_slot(arg), false)?;
        }
        if let Err(stop) = self.call(Operand::UNUSED) {
            self.abort_invoke(frames, pending);
            return Err(stop);
        }
        if self.frames.len() == frames {
            return Ok(std::mem::take(&mut self.return_value));
        }
        match self.drive_until(frames) {
            Ok(()) => Ok(std::mem::take(&mut self.return_value)),
            Err(stop) => {
                self.abort_invoke(frames, pending);
                Err(stop)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SessionError;
    use php_value::{ObjectStore, PhpString};

    #[test]
    fn thrown_errors_name_the_class_and_message() -> Result<(), String> {
        let object = ObjectStore::new()
            .create(PhpString::from("Error"))
            .map_err(|error| error.to_string())?;
        let error = SessionError::Thrown {
            class: "Error".into(),
            message: b"nope".to_vec(),
            object,
        };
        assert_eq!(error.to_string(), "uncaught Error: nope");
        Ok(())
    }
}
