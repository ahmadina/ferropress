//! Calls between PHP functions: preparing arguments, binding parameters and returning.

use php_compiler::bytecode::{Instruction, Operand, OperandKind};
use php_value::{Object, PhpArray, Slot, Value};

use crate::machine::{Exec, Flow, Frame, Machine};
use crate::{Output, VmError};

/// An argument prepared for a call.
#[derive(Debug)]
pub(crate) struct Argument {
    slot: Slot,
    /// Whether the value is a call's result, which a by-reference parameter accepts with a notice.
    from_call: bool,
}

/// A function, closure or method being prepared for [`Machine::call`].
pub(crate) struct Pending {
    pub(crate) code: usize,
    pub(crate) env: Option<Object>,
    pub(crate) this: Option<Object>,
    /// The class name when this call is the implicit constructor of a class that declares none.
    pub(crate) implicit_ctor: Option<Box<[u8]>>,
    pub(crate) args: Vec<Argument>,
}

impl<O: Output> Machine<'_, '_, O> {
    /// Starts a call of the function named by the literal `a`; an undefined function throws
    /// before any argument is evaluated.
    pub(crate) fn init_call(&mut self, ins: &Instruction) -> Exec {
        let key = self.read(ins.a)?;
        let Value::String(key) = key else {
            return Err(VmError::Malformed("function name is not a string").into());
        };
        if let Some(&code) = self.functions.get(key.as_bytes()) {
            self.pending.push(Pending {
                code,
                env: None,
                this: None,
                implicit_ctor: None,
                args: Vec::new(),
            });
            return Ok(());
        }
        let written = self.read(ins.c)?;
        let Value::String(written) = written else {
            return Err(VmError::Malformed("function name is not a string").into());
        };
        let mut message = b"Call to undefined function ".to_vec();
        message.extend_from_slice(written.as_bytes());
        message.extend_from_slice(b"()");
        Err(self.throw("Error", message))
    }

    pub(crate) fn send(&mut self, arg: Slot, from_call: bool) -> Exec {
        let pending = self
            .pending
            .last_mut()
            .ok_or(VmError::Malformed("argument sent outside a call"))?;
        pending.args.push(Argument {
            slot: arg,
            from_call,
        });
        Ok(())
    }

    /// Sends the variable `operand`: by reference when the parameter at this position takes one,
    /// creating the variable if needed; otherwise by value.
    pub(crate) fn send_variable(&mut self, operand: Operand) -> Exec {
        let pending = self
            .pending
            .last()
            .ok_or(VmError::Malformed("argument sent outside a call"))?;
        let by_ref = self.codes[pending.code]
            .code
            .params
            .get(pending.args.len())
            .is_some_and(|param| param.by_ref && !param.variadic);
        if !by_ref {
            let value = self.read(operand)?;
            return self.send(Slot::Value(value), false);
        }
        if operand.kind() != OperandKind::Slot {
            return Err(VmError::Malformed("variable argument is not a slot").into());
        }
        let slot = self
            .frame_mut()?
            .slots
            .get_mut(operand.index() as usize)
            .ok_or(VmError::Malformed("no such slot"))?
            .get_or_insert_with(|| Slot::Value(Value::Null));
        let reference = slot.make_reference();
        self.send(Slot::Reference(reference), false)
    }

    /// Calls the prepared function, binding its parameters; too few arguments throw
    /// `ArgumentCountError` from inside the called function.
    pub(crate) fn call(&mut self, result: Operand) -> Exec {
        let Pending {
            code,
            env,
            this,
            implicit_ctor,
            args,
        } = self
            .pending
            .pop()
            .ok_or(VmError::Malformed("call without a prepared function"))?;
        if implicit_ctor.is_some() {
            return Ok(());
        }
        let mut frame = Frame::new(code, &self.codes[code]);
        if let Some(env) = env {
            crate::closures::apply_captured_slots(
                &env,
                &mut frame.slots,
                &self.codes[code].code.slots,
            )?;
        }
        if let Some(this) = this {
            bind_this(&this, &mut frame.slots, &self.codes[code].code.slots);
        }
        frame.result = result;
        frame.call_line = self.line;
        frame.passed = args.len();
        let params = self.codes[code].code.params.clone();
        let slot_names = self.codes[code].code.slots.clone();
        let name = self.codes[code].code.name.clone();
        let mut args = args.into_iter();
        let mut missing = None;
        for (index, param) in params.iter().enumerate() {
            let slot = frame
                .slots
                .get_mut(param.slot as usize)
                .ok_or(VmError::Malformed("parameter slot out of range"))?;
            if param.variadic {
                let mut rest = PhpArray::new();
                for arg in args.by_ref() {
                    let value = arg.slot.value();
                    frame.extra.push(value.clone());
                    rest.append(value)
                        .map_err(|_full| VmError::Unsupported("arrays this large"))?;
                }
                *slot = Some(Slot::Value(Value::Array(rest)));
            } else if let Some(arg) = args.next() {
                if param.by_ref && !matches!(arg.slot, Slot::Reference(_)) {
                    if arg.from_call {
                        self.display(b"Notice", b"Only variables should be passed by reference");
                    } else {
                        let param_name = slot_names
                            .get(param.slot as usize)
                            .cloned()
                            .unwrap_or_default();
                        return self.not_by_reference(&name, index, &param_name);
                    }
                }
                *slot = Some(arg.slot);
            } else if missing.is_none() {
                missing = Some(param.line);
            }
        }
        frame.extra.extend(args.map(|arg| arg.slot.value()));
        let required = params.iter().filter(|param| !param.variadic).count();
        let call_line = self.line;
        self.frames.push(frame);
        let Some(line) = missing else {
            return Ok(());
        };
        self.line = line;
        let mut message = b"Too few arguments to function ".to_vec();
        message.extend_from_slice(&name);
        message.extend_from_slice(b"(), ");
        message.extend_from_slice(self.frame()?.passed.to_string().as_bytes());
        message.extend_from_slice(b" passed in ");
        message.extend_from_slice(&self.file);
        message.extend_from_slice(b" on line ");
        message.extend_from_slice(call_line.to_string().as_bytes());
        message.extend_from_slice(b" and exactly ");
        message.extend_from_slice(required.to_string().as_bytes());
        message.extend_from_slice(b" expected");
        Err(self.throw("ArgumentCountError", message))
    }

    /// A by-reference parameter received a value, not a variable.
    fn not_by_reference(&self, name: &[u8], index: usize, param: &[u8]) -> Exec {
        let mut message = name.to_vec();
        message.extend_from_slice(b"(): Argument #");
        message.extend_from_slice((index + 1).to_string().as_bytes());
        message.extend_from_slice(b" ($");
        message.extend_from_slice(param);
        message.extend_from_slice(b") could not be passed by reference");
        Err(self.throw("Error", message))
    }

    /// Returns `a` (or `null` when unused) to the caller; returning from the main code ends the
    /// script.
    pub(crate) fn return_from(&mut self, operand: Operand) -> Exec<Flow> {
        let value = if operand.is_unused() {
            Value::Null
        } else {
            self.read(operand)?
        };
        let frame = self
            .frames
            .pop()
            .ok_or(VmError::Malformed("return without a frame"))?;
        if self.frames.is_empty() {
            return Ok(Flow::Finished);
        }
        self.write(frame.result, value)?;
        Ok(Flow::Continue)
    }
}

fn bind_this(this: &Object, slots: &mut [Option<Slot>], names: &[Box<[u8]>]) {
    if let Some(index) = names.iter().position(|name| &**name == b"this")
        && let Some(slot) = slots.get_mut(index)
    {
        *slot = Some(Slot::Value(Value::Object(this.clone())));
    }
}
