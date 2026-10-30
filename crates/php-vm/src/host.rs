//! Host-provided PHP functions (ADR-0003).
//!
//! The engine calls [`Host`] for names that are not user functions or language built-ins.
//! Bindings implement the trait; the VM does not own CMS behavior.

use std::rc::Rc;

use php_compiler::bytecode::Operand;
use php_value::{Invoke, Slot, Value};

use crate::calls::Argument;
use crate::machine::{Exec, Machine};
use crate::session::SessionError;
use crate::{Output, VmError};

/// A host that answers PHP function names the engine does not implement.
pub trait Host {
    /// Whether `name` (already lowercased) is a host function.
    fn exists(&self, name: &[u8]) -> bool;

    /// Runs the host function `name` with `args`.
    ///
    /// `invoke` is this request's engine, so a host function may call back into PHP.
    ///
    /// # Errors
    ///
    /// [`SessionError`] when the host cannot finish or a nested PHP call throws.
    fn call(
        &self,
        name: &[u8],
        args: &mut [Slot],
        invoke: &mut dyn Invoke<Error = SessionError>,
    ) -> Result<Value, SessionError>;
}

/// A reference-counted host table.
pub type SharedHost = Rc<dyn Host>;

impl<O: Output> Machine<'_, O> {
    pub(crate) fn host_exists(&self, key: &[u8]) -> bool {
        self.host.as_ref().is_some_and(|host| host.exists(key))
    }

    pub(crate) fn dispatch_host(
        &mut self,
        name: &[u8],
        args: &[Argument],
        result: Operand,
    ) -> Exec {
        let host = self
            .host
            .clone()
            .ok_or(VmError::Malformed("host call without a host"))?;
        let mut slots: Vec<Slot> = args
            .iter()
            .map(|arg| crate::closures::copy_slot(&arg.slot))
            .collect();
        match host.call(name, &mut slots, self) {
            Ok(value) => self.write(result, value),
            Err(SessionError::Vm(error)) => Err(error.into()),
            Err(SessionError::Thrown {
                class,
                message,
                object,
            }) => Err(self.thrown(class.as_bytes(), message, object)),
        }
    }
}

impl<O: Output> Invoke for Machine<'_, O> {
    type Error = SessionError;

    fn invoke(&mut self, callee: &Value, args: &mut [Slot]) -> Result<Value, Self::Error> {
        self.invoke_callable(callee, args)
            .map_err(SessionError::from_stop)
    }
}
