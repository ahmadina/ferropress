//! Closures: creating the `Closure` object, binding `use` variables and calling a value.

use php_compiler::bytecode::{Instruction, Operand, OperandKind};
use php_value::{Object, PhpString, Slot, Value};

use crate::calls::Pending;
use crate::machine::{Exec, Machine};
use crate::{Output, VmError};

/// Property holding the code-object index of a compiled closure.
const CODE: &[u8] = b"\0code";

impl<O: Output> Machine<'_, '_, O> {
    /// `result` is a new `Closure` of the function declaration whose index is the integer `a`.
    pub(crate) fn make_closure(&mut self, ins: &Instruction) -> Exec {
        let index = int_operand(&self.read(ins.a)?)?;
        let code = index
            .checked_add(1)
            .ok_or(VmError::Malformed("closure index out of range"))?;
        if code >= self.codes.len() {
            return Err(VmError::Malformed("closure index out of range").into());
        }
        let object = self
            .objects
            .create(PhpString::from("Closure"))
            .map_err(|_full| VmError::Unsupported("this many objects"))?;
        let stored = i64::try_from(code)
            .map_err(|_overflow| VmError::Malformed("closure index out of range"))?;
        object.with_properties_mut(|properties| {
            properties.assign(PhpString::from(CODE), Value::Int(stored));
        });
        self.write(ins.result, Value::Object(object))
    }

    /// Binds the variable `b` into the closure `a` as slot `c`.
    pub(crate) fn capture(&mut self, ins: &Instruction) -> Exec {
        let Value::Object(object) = self.read(ins.a)? else {
            return Err(VmError::Malformed("capture target is not a closure").into());
        };
        let inner = int_operand(&self.read(ins.c)?)?;
        let code = closure_code(&object)?;
        let name = self
            .codes
            .get(code)
            .and_then(|loaded| loaded.code.slots.get(inner))
            .cloned()
            .ok_or(VmError::Malformed("capture slot out of range"))?;
        if ins.flags & 1 == 0 {
            let value = self.read(ins.b)?;
            object.with_properties_mut(|properties| {
                properties.assign(PhpString::new(&name), value);
            });
            return Ok(());
        }
        let reference = self.reference_of(ins.b)?;
        object.with_properties_mut(|properties| {
            properties.bind(PhpString::new(&name), reference);
        });
        Ok(())
    }

    /// Starts a call of the value `a`: a string names a function; a `Closure` is invoked.
    pub(crate) fn init_call_value(&mut self, operand: Operand) -> Exec {
        let value = self.read(operand)?;
        match value {
            Value::String(name) => self.init_string_call(name.as_bytes()),
            Value::Object(object) => self.init_closure_call(object),
            other => Err(self.not_callable(&other)),
        }
    }

    pub(crate) fn init_string_call(&mut self, written: &[u8]) -> Exec {
        let stripped = written.strip_prefix(b"\\").unwrap_or(written);
        let key = stripped.to_ascii_lowercase();
        if let Some(&code) = self.functions.get(key.as_slice()) {
            self.pending.push(Pending {
                code,
                env: None,
                this: None,
                implicit_ctor: None,
                generator_method: None,
                native: None,
                fiber_method: None,
                args: Vec::new(),
            });
            return Ok(());
        }
        if let Some(native) = crate::builtins::lookup(&key) {
            self.pending.push(Pending {
                code: 0,
                env: None,
                this: None,
                implicit_ctor: None,
                generator_method: None,
                native: Some(native),
                fiber_method: None,
                args: Vec::new(),
            });
            return Ok(());
        }
        let mut message = b"Call to undefined function ".to_vec();
        message.extend_from_slice(stripped);
        message.extend_from_slice(b"()");
        Err(self.throw("Error", message))
    }

    pub(crate) fn init_closure_call(&mut self, object: Object) -> Exec {
        if object.class_name().as_bytes() != b"Closure" {
            return Err(self.not_callable(&Value::Object(object)));
        }
        let code = closure_code(&object)?;
        self.pending.push(Pending {
            code,
            env: Some(object),
            this: None,
            implicit_ctor: None,
            generator_method: None,
            native: None,
            fiber_method: None,
            args: Vec::new(),
        });
        Ok(())
    }

    /// Converts `value` to a string, or throws when it is an object.
    pub(crate) fn stringify(&mut self, value: &Value) -> Exec<PhpString> {
        if let Value::Object(object) = value {
            let mut message = b"Object of class ".to_vec();
            message.extend_from_slice(object.class_name().as_bytes());
            message.extend_from_slice(b" could not be converted to string");
            return Err(self.throw("Error", message));
        }
        self.compute(|cx| value.to_php_string(cx))
    }

    fn reference_of(&mut self, operand: Operand) -> Exec<php_value::Reference> {
        if operand.kind() != OperandKind::Slot {
            return Err(VmError::Malformed("capture by reference is not a slot").into());
        }
        let slot = self
            .frame_mut()?
            .slots
            .get_mut(operand.index() as usize)
            .ok_or(VmError::Malformed("no such slot"))?
            .get_or_insert_with(|| Slot::Value(Value::Null));
        Ok(slot.make_reference())
    }

    fn not_callable(&self, value: &Value) -> crate::machine::Stop {
        let mut message = b"Value of type ".to_vec();
        match value {
            Value::Object(object) => message.extend_from_slice(object.class_name().as_bytes()),
            other => message.extend_from_slice(other.type_name().as_bytes()),
        }
        message.extend_from_slice(b" is not callable");
        self.throw("Error", message)
    }
}

fn copy_slot(slot: &Slot) -> Slot {
    match slot {
        Slot::Value(value) => Slot::Value(value.clone()),
        Slot::Reference(reference) => Slot::Reference(reference.clone()),
    }
}

/// Copies captured slots from `env` into `slots` by name.
pub(crate) fn apply_captured_slots(
    env: &Object,
    slots: &mut [Option<Slot>],
    names: &[Box<[u8]>],
) -> Exec {
    let captured = env.with_properties(|properties| {
        properties
            .iter()
            .filter_map(|(name, slot)| {
                if name.as_bytes() == CODE {
                    return None;
                }
                let index = names.iter().position(|slot| &**slot == name.as_bytes())?;
                Some((index, copy_slot(slot)))
            })
            .collect::<Vec<_>>()
    });
    for (index, slot) in captured {
        let dest = slots
            .get_mut(index)
            .ok_or(VmError::Malformed("capture slot out of range"))?;
        *dest = Some(slot);
    }
    Ok(())
}

fn int_operand(value: &Value) -> Result<usize, crate::machine::Stop> {
    let Value::Int(index) = value else {
        return Err(VmError::Malformed("closure index is not an integer").into());
    };
    usize::try_from(*index)
        .map_err(|_overflow| VmError::Malformed("closure index out of range").into())
}

fn closure_code(object: &Object) -> Result<usize, crate::machine::Stop> {
    let value = object.with_properties(|properties| properties.get(CODE).map(Slot::value));
    let Some(Value::Int(index)) = value else {
        return Err(VmError::Malformed("closure is missing its code").into());
    };
    usize::try_from(index)
        .map_err(|_overflow| VmError::Malformed("closure index out of range").into())
}
