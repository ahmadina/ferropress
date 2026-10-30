//! Calls between PHP functions: preparing arguments, binding parameters and returning.

use php_compiler::bytecode::{Instruction, Operand, OperandKind};
use php_value::{Object, PhpArray, PhpString, Slot, Value};

use crate::machine::{CallTarget, Exec, Flow, Machine};
use crate::{Output, VmError};

/// An argument prepared for a call.
#[derive(Debug)]
pub(crate) struct Argument {
    pub(crate) slot: Slot,
    /// Whether the value is a call's result, which a by-reference parameter accepts with a notice.
    pub(crate) from_call: bool,
}

/// A function, closure or method being prepared for [`Machine::call`].
pub(crate) struct Pending {
    pub(crate) code: usize,
    pub(crate) env: Option<Object>,
    pub(crate) this: Option<Object>,
    /// The class name when this call is the implicit constructor of a class that declares none.
    pub(crate) implicit_ctor: Option<Box<[u8]>>,
    /// A native `Generator` method, when this call is one.
    pub(crate) generator_method: Option<(Object, crate::generators::GenMethod)>,
    /// A bound built-in, when this call is one.
    pub(crate) native: Option<crate::builtins::Native>,
    /// A host function name, already lowercased.
    pub(crate) host: Option<Box<[u8]>>,
    /// A native `Fiber` method, when this call is one.
    pub(crate) fiber_method: Option<crate::fibers::FiberCall>,
    /// A native method of a Rust-owned class.
    pub(crate) native_method: Option<(Object, crate::classes::NativeMethod)>,
    pub(crate) args: Vec<Argument>,
}

impl<O: Output> Machine<'_, O> {
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
                generator_method: None,
                native: None,
                host: None,
                fiber_method: None,
                native_method: None,
                args: Vec::new(),
            });
            return Ok(());
        }
        if let Some(native) = crate::builtins::lookup(key.as_bytes()) {
            self.pending.push(Pending {
                code: 0,
                env: None,
                this: None,
                implicit_ctor: None,
                generator_method: None,
                native: Some(native),
                host: None,
                fiber_method: None,
                native_method: None,
                args: Vec::new(),
            });
            return Ok(());
        }
        if self.host_exists(key.as_bytes()) {
            self.pending.push(Pending {
                code: 0,
                env: None,
                this: None,
                implicit_ctor: None,
                generator_method: None,
                native: None,
                host: Some(key.as_bytes().into()),
                fiber_method: None,
                native_method: None,
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

    /// Calls the named function in one instruction: `a` is the name as written, `flags` is the
    /// argument count (0..=2), `b` and `c` are the arguments.
    pub(crate) fn invoke(&mut self, ins: &Instruction) -> Exec {
        let count = ins.flags & 3;
        if count > 2 {
            return Err(VmError::Malformed("invoke argc").into());
        }
        let pc = self.frame()?.pc.saturating_sub(1);
        let code = self.frame()?.code;
        let target = self.resolve_invoke(code, pc, ins.a)?;
        match target {
            CallTarget::Unknown => Err(VmError::Malformed("unresolved invoke").into()),
            CallTarget::Native(native) => {
                let mut prepared = [
                    Argument {
                        slot: Slot::Value(Value::Null),
                        from_call: false,
                    },
                    Argument {
                        slot: Slot::Value(Value::Null),
                        from_call: false,
                    },
                ];
                self.fill_invoke_args(ins, count, target, &mut prepared)?;
                self.dispatch_native(native, &prepared[..count as usize], ins.result)
            }
            CallTarget::Host => {
                let mut prepared = [
                    Argument {
                        slot: Slot::Value(Value::Null),
                        from_call: false,
                    },
                    Argument {
                        slot: Slot::Value(Value::Null),
                        from_call: false,
                    },
                ];
                self.fill_invoke_args(ins, count, target, &mut prepared)?;
                let written = self.read(ins.a)?;
                let Value::String(written) = written else {
                    return Err(VmError::Malformed("function name is not a string").into());
                };
                let key = written.as_bytes().to_ascii_lowercase();
                self.dispatch_host(&key, &prepared[..count as usize], ins.result)
            }
            CallTarget::User(user) => {
                let prepared = self.invoke_args(ins, count, target)?;
                self.pending.push(Pending {
                    code: user,
                    env: None,
                    this: None,
                    implicit_ctor: None,
                    generator_method: None,
                    native: None,
                    host: None,
                    fiber_method: None,
                    native_method: None,
                    args: prepared,
                });
                self.call(ins.result)
            }
        }
    }

    fn resolve_invoke(&mut self, code: usize, pc: usize, name: Operand) -> Exec<CallTarget> {
        if let Some(hit) = self
            .codes
            .get(code)
            .and_then(|loaded| loaded.calls.get(pc))
            .copied()
            && !matches!(hit, CallTarget::Unknown)
        {
            return Ok(hit);
        }
        let written = self.read(name)?;
        let Value::String(written) = written else {
            return Err(VmError::Malformed("function name is not a string").into());
        };
        let key = written.as_bytes().to_ascii_lowercase();
        let target = if let Some(&index) = self.functions.get(key.as_slice()) {
            CallTarget::User(index)
        } else if let Some(native) = crate::builtins::lookup(&key) {
            CallTarget::Native(native)
        } else if self.host_exists(&key) {
            CallTarget::Host
        } else {
            let mut message = b"Call to undefined function ".to_vec();
            message.extend_from_slice(written.as_bytes());
            message.extend_from_slice(b"()");
            return Err(self.throw("Error", message));
        };
        if let Some(slot) = self
            .codes
            .get_mut(code)
            .and_then(|loaded| loaded.calls.get_mut(pc))
        {
            *slot = target;
        }
        Ok(target)
    }

    fn fill_invoke_args(
        &mut self,
        ins: &Instruction,
        count: u8,
        target: CallTarget,
        dest: &mut [Argument],
    ) -> Exec {
        for (index, operand) in [ins.b, ins.c].into_iter().take(count as usize).enumerate() {
            let by_ref = match target {
                CallTarget::User(code) => self.codes[code]
                    .code
                    .params
                    .get(index)
                    .is_some_and(|param| param.by_ref && !param.variadic),
                _ => false,
            };
            let from_call = ins.flags & (1 << (2 + index)) != 0;
            dest[index] = self.invoke_arg(operand, by_ref, from_call)?;
        }
        Ok(())
    }

    fn invoke_args(
        &mut self,
        ins: &Instruction,
        count: u8,
        target: CallTarget,
    ) -> Exec<Vec<Argument>> {
        let mut prepared = Vec::with_capacity(count as usize);
        prepared.resize_with(count as usize, || Argument {
            slot: Slot::Value(Value::Null),
            from_call: false,
        });
        self.fill_invoke_args(ins, count, target, &mut prepared)?;
        Ok(prepared)
    }

    fn invoke_arg(&mut self, operand: Operand, by_ref: bool, from_call: bool) -> Exec<Argument> {
        if !by_ref {
            return Ok(Argument {
                slot: Slot::Value(self.read(operand)?),
                from_call,
            });
        }
        if operand.kind() != OperandKind::Slot {
            return Ok(Argument {
                slot: Slot::Value(self.read(operand)?),
                from_call,
            });
        }
        let slot = self
            .frame_mut()?
            .slots
            .get_mut(operand.index() as usize)
            .ok_or(VmError::Malformed("no such slot"))?
            .get_or_insert_with(|| Slot::Value(Value::Null));
        Ok(Argument {
            slot: Slot::Reference(slot.make_reference()),
            from_call: false,
        })
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
        let by_ref = match pending.native {
            Some(native) => native.takes_ref(pending.args.len()),
            None => self.codes[pending.code]
                .code
                .params
                .get(pending.args.len())
                .is_some_and(|param| param.by_ref && !param.variadic),
        };
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
        let pending = self
            .pending
            .pop()
            .ok_or(VmError::Malformed("call without a prepared function"))?;
        if pending.generator_method.is_some()
            || pending.fiber_method.is_some()
            || pending.native_method.is_some()
            || pending.native.is_some()
            || pending.host.is_some()
            || pending.implicit_ctor.is_some()
        {
            return self.dispatch_special_call(pending, result);
        }
        let Pending {
            code,
            env,
            this,
            args,
            ..
        } = pending;
        let mut frame = self.take_frame(code);
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
        let missing = self.bind_params(code, &mut frame, args)?;
        let required = self.codes[code]
            .code
            .params
            .iter()
            .filter(|param| !param.variadic)
            .count();
        let call_line = self.line;
        if self.codes[code].code.is_generator {
            return self.start_generator(frame, result);
        }
        self.frames.push(frame);
        let Some(line) = missing else {
            return Ok(());
        };
        let name = self.codes[code].code.name.clone();
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

    fn bind_params(
        &mut self,
        code: usize,
        frame: &mut crate::machine::Frame,
        args: Vec<Argument>,
    ) -> Exec<Option<u32>> {
        let mut args = args.into_iter();
        let mut missing = None;
        let param_count = self.codes[code].code.params.len();
        for index in 0..param_count {
            let param = self.codes[code].code.params[index].clone();
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
                        let param_name = self.codes[code]
                            .code
                            .slots
                            .get(param.slot as usize)
                            .cloned()
                            .unwrap_or_default();
                        let name = self.codes[code].code.name.clone();
                        return match self.not_by_reference(&name, index, &param_name) {
                            Ok(()) => Ok(missing),
                            Err(stop) => Err(stop),
                        };
                    }
                }
                *slot = Some(arg.slot);
            } else if missing.is_none() {
                missing = Some(param.line);
            }
        }
        frame.extra.extend(args.map(|arg| arg.slot.value()));
        Ok(missing)
    }

    fn dispatch_special_call(&mut self, pending: Pending, result: Operand) -> Exec {
        let Pending {
            this,
            implicit_ctor,
            generator_method,
            native,
            host,
            fiber_method,
            native_method,
            args,
            ..
        } = pending;
        if let Some((object, method)) = generator_method {
            return self.dispatch_generator(&object, method, &args, result);
        }
        if let Some(call) = fiber_method {
            return self.dispatch_fiber(call, &args, result);
        }
        if let Some((object, method)) = native_method {
            return self.dispatch_core_method(&object, method, &args, result);
        }
        if let Some(native) = native {
            return self.dispatch_native(native, &args, result);
        }
        if let Some(name) = host {
            return self.dispatch_host(&name, &args, result);
        }
        if let Some(class) = implicit_ctor {
            return self.finish_implicit(&class, this, &args);
        }
        Err(VmError::Malformed("call without a prepared function").into())
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
        self.finish_return(value)
    }

    /// Leaves the current frame with `value`, after any `finally` of this frame has run.
    pub(crate) fn finish_return(&mut self, value: Value) -> Exec<Flow> {
        if let Some(finally) = self.matching_finally() {
            self.finally_leave = Some(crate::exceptions::FinallyLeave::Return(value));
            self.frame_mut()?.pc = finally.target as usize;
            return Ok(Flow::Continue);
        }
        if let Some(id) = self.frame()?.generator {
            return self.finish_generator(id, value);
        }
        if let Some(id) = self.current_fiber
            && self
                .fibers
                .get(&id)
                .is_some_and(|fiber| self.frames.len() == fiber.root_depth + 1)
        {
            return self.finish_fiber(id, value);
        }
        let frame = self
            .frames
            .pop()
            .ok_or(VmError::Malformed("return without a frame"))?;
        let result = frame.result;
        self.recycle_frame(frame);
        self.return_value = value.clone();
        if self.frames.is_empty() {
            return Ok(Flow::Finished);
        }
        self.write(result, value)?;
        Ok(Flow::Continue)
    }

    fn finish_implicit(&mut self, class: &[u8], this: Option<Object>, args: &[Argument]) -> Exec {
        if class.eq_ignore_ascii_case(b"FiberError") {
            return Err(self.throw_from(
                "Error",
                b"The \"FiberError\" class is reserved for internal use and cannot be manually instantiated"
                    .to_vec(),
                b"FiberError->__construct",
                args.iter().map(|arg| arg.slot.value()).collect(),
            ));
        }
        if crate::exceptions::is_throwable(class)
            && let Some(this) = this
        {
            let message = match args.first() {
                Some(arg) => self.stringify(&arg.slot.value())?,
                None => PhpString::new(&[]),
            };
            this.with_properties_mut(|properties| {
                properties.assign(PhpString::new(b"message"), Value::String(message));
            });
        }
        Ok(())
    }
}

fn bind_this(this: &Object, slots: &mut [Option<Slot>], names: &[Box<[u8]>]) {
    if let Some(index) = names.iter().position(|name| &**name == b"this")
        && let Some(slot) = slots.get_mut(index)
    {
        *slot = Some(Slot::Value(Value::Object(this.clone())));
    }
}
