//! `throw` and `try`/`catch`: matching a thrown object to a handler and binding it.

use php_compiler::bytecode::{CatchHandler, FinallyHandler, Instruction};
use php_value::{Object, Slot, Value};

use crate::machine::{Exec, Flow, Machine};
use crate::thrown::Thrown;
use crate::{Output, VmError};

/// What a `finally` body must do after it finishes.
#[derive(Debug, Clone)]
pub(crate) enum FinallyLeave {
    /// The `try`/`catch` threw and was not caught there.
    Rethrow(Box<Thrown>),
    /// The `try`/`catch` returned this value.
    Return(Value),
}

/// Classes the VM itself throws, plus `Exception`.
const THROWABLE: &[&[u8]] = &[
    b"exception",
    b"error",
    b"typeerror",
    b"argumentcounterror",
    b"arithmeticerror",
    b"divisionbyzeroerror",
    b"parseerror",
    b"fibererror",
];

impl<O: Output> Machine<'_, '_, O> {
    /// Throws the object `a`.
    pub(crate) fn throw_value(&mut self, ins: &Instruction) -> Exec<Flow> {
        let value = self.read(ins.a)?;
        let Value::Object(object) = value else {
            return Err(self.throw("Error", b"Can only throw objects".to_vec()));
        };
        let class = object.class_name().as_bytes().to_vec();
        if !is_throwable(&class) {
            return Err(self.throw(
                "Error",
                b"Cannot throw objects that do not implement Throwable".to_vec(),
            ));
        }
        let message = object_message(&object);
        Err(self.thrown(&class, message, object))
    }

    /// Lands in a matching `catch` when one is active, otherwise leaves the error uncaught.
    pub(crate) fn catch_thrown(&mut self, thrown: &Thrown) -> Exec<bool> {
        while !self.frames.is_empty() {
            if let Some(handler) = self.matching_handler(thrown.class.as_ref()) {
                self.pending.clear();
                self.finally_leave = None;
                if let Some(index) = handler.slot {
                    let slot = self
                        .frame_mut()?
                        .slots
                        .get_mut(index as usize)
                        .ok_or(VmError::Malformed("catch slot out of range"))?;
                    *slot = Some(Slot::Value(Value::Object(thrown.object.clone())));
                }
                self.frame_mut()?.pc = handler.target as usize;
                return Ok(true);
            }
            if let Some(finally) = self.matching_finally() {
                self.pending.clear();
                self.finally_leave = Some(FinallyLeave::Rethrow(Box::new(thrown.clone())));
                self.frame_mut()?.pc = finally.target as usize;
                return Ok(true);
            }
            if self.frames.len() == 1 {
                self.leave_fiber();
                return Ok(false);
            }
            self.leave_fiber();
            self.frames.pop();
        }
        Ok(false)
    }

    /// Runs the pending leave after a `finally` body.
    pub(crate) fn end_finally(&mut self) -> Exec<Flow> {
        match self.finally_leave.take() {
            None => Ok(Flow::Continue),
            Some(FinallyLeave::Rethrow(thrown)) => Err(crate::machine::Stop::Thrown(thrown)),
            Some(FinallyLeave::Return(value)) => self.finish_return(value),
        }
    }
}

/// Whether `class` is a built-in throwable, compared case-insensitively.
pub(crate) fn is_throwable(class: &[u8]) -> bool {
    let key = class.to_ascii_lowercase();
    THROWABLE.contains(&key.as_slice())
}

/// Whether a `catch (handler)` arm receives an error of class `thrown`.
pub(crate) fn catch_matches(handler: &[u8], thrown: &[u8]) -> bool {
    let handler = handler.to_ascii_lowercase();
    let thrown = thrown.to_ascii_lowercase();
    if handler == thrown {
        return true;
    }
    match handler.as_slice() {
        b"throwable" => is_throwable(&thrown),
        b"error" => matches!(
            thrown.as_slice(),
            b"error"
                | b"typeerror"
                | b"argumentcounterror"
                | b"arithmeticerror"
                | b"divisionbyzeroerror"
                | b"parseerror"
                | b"fibererror"
        ),
        b"exception" => thrown == b"exception",
        b"arithmeticerror" => matches!(
            thrown.as_slice(),
            b"arithmeticerror" | b"divisionbyzeroerror"
        ),
        _ => false,
    }
}

fn object_message(object: &Object) -> Vec<u8> {
    object.with_properties(|properties| match properties.get(b"message") {
        Some(slot) => match slot.value() {
            Value::String(text) => text.as_bytes().to_vec(),
            other => other.type_name().as_bytes().to_vec(),
        },
        None => Vec::new(),
    })
}

impl<O: Output> Machine<'_, '_, O> {
    pub(crate) fn matching_finally(&self) -> Option<FinallyHandler> {
        let frame = self.frames.last()?;
        let pc = frame.pc.saturating_sub(1);
        self.codes[frame.code]
            .code
            .finallys
            .iter()
            .find(|handler| pc >= handler.start as usize && pc < handler.end as usize)
            .cloned()
    }

    fn matching_handler(&self, class: &[u8]) -> Option<CatchHandler> {
        let frame = self.frames.last()?;
        let pc = frame.pc.saturating_sub(1);
        self.codes[frame.code]
            .code
            .handlers
            .iter()
            .find(|handler| {
                pc >= handler.start as usize
                    && pc < handler.end as usize
                    && catch_matches(&handler.class, class)
            })
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::{catch_matches, is_throwable};

    #[test]
    fn throwable_classes_are_known() {
        assert!(is_throwable(b"Exception"));
        assert!(is_throwable(b"DivisionByZeroError"));
        assert!(is_throwable(b"FiberError"));
        assert!(!is_throwable(b"C"));
    }

    #[test]
    fn catch_arms_follow_the_built_in_hierarchy() {
        assert!(catch_matches(b"Exception", b"Exception"));
        assert!(catch_matches(b"Error", b"DivisionByZeroError"));
        assert!(catch_matches(b"Error", b"FiberError"));
        assert!(catch_matches(b"Throwable", b"TypeError"));
        assert!(catch_matches(b"ArithmeticError", b"DivisionByZeroError"));
        assert!(!catch_matches(b"Exception", b"Error"));
        assert!(!catch_matches(b"C", b"Exception"));
    }
}
