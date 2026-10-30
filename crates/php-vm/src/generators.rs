//! Generators: calling a yielding function returns a `Generator` that `yield` suspends.

use std::collections::HashMap;

use php_compiler::bytecode::{Instruction, Operand};
use php_value::{Object, ObjectId, PhpString, Slot, Value};

use crate::calls::Argument;
use crate::machine::{Exec, Flow, Frame, Machine};
use crate::{Output, VmError};

/// A native method of `Generator`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum GenMethod {
    Current,
    Key,
    Next,
    Valid,
    Send,
    Rewind,
    GetReturn,
    Throw,
}

impl GenMethod {
    pub(crate) fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"current" => Self::Current,
            b"key" => Self::Key,
            b"next" => Self::Next,
            b"valid" => Self::Valid,
            b"send" => Self::Send,
            b"rewind" => Self::Rewind,
            b"getreturn" => Self::GetReturn,
            b"throw" => Self::Throw,
            _ => return None,
        })
    }
}

/// What the PHP call that resumed the generator wants written when it suspends or finishes.
#[derive(Debug, Clone, Copy)]
enum GenWant {
    Current,
    Key,
    Valid,
    Void,
    Send,
}

/// Saved state of one generator object.
pub(crate) struct Generator {
    code: usize,
    pc: usize,
    slots: Vec<Option<Slot>>,
    temps: Vec<Value>,
    extra: Vec<Value>,
    passed: usize,
    current: Value,
    key: Value,
    next_key: i64,
    started: bool,
    /// Whether `next`/`send` has moved past the first yield.
    advanced: bool,
    finished: bool,
    return_value: Value,
    pending_send: Option<Value>,
    want: GenWant,
    result: Operand,
}

impl<O: Output> Machine<'_, O> {
    /// Calling a generator function creates the object instead of running the body.
    pub(crate) fn start_generator(&mut self, frame: Frame, result: Operand) -> Exec {
        let object = self
            .objects
            .create(PhpString::new(b"Generator"))
            .map_err(|_full| VmError::Unsupported("this many objects"))?;
        let id = object.id();
        self.generators.insert(
            id,
            Generator {
                code: frame.code,
                pc: 0,
                slots: frame.slots,
                temps: frame.temps,
                extra: frame.extra,
                passed: frame.passed,
                current: Value::Null,
                key: Value::Null,
                next_key: 0,
                started: false,
                advanced: false,
                finished: false,
                return_value: Value::Null,
                pending_send: None,
                want: GenWant::Void,
                result: Operand::UNUSED,
            },
        );
        self.write(result, Value::Object(object))
    }

    /// Runs a native `Generator` method.
    pub(crate) fn dispatch_generator(
        &mut self,
        object: &Object,
        method: GenMethod,
        args: &[Argument],
        result: Operand,
    ) -> Exec {
        let id = object.id();
        match method {
            GenMethod::Current => self.gen_field(id, GenWant::Current, result),
            GenMethod::Key => self.gen_field(id, GenWant::Key, result),
            GenMethod::Valid => self.gen_field(id, GenWant::Valid, result),
            GenMethod::Next => self.gen_advance(id, Value::Null, GenWant::Void, result),
            GenMethod::Send => {
                let value = args.first().map_or(Value::Null, |arg| arg.slot.value());
                self.gen_advance(id, value, GenWant::Send, result)
            }
            GenMethod::Rewind => self.gen_rewind(id, result),
            GenMethod::GetReturn => self.gen_return_value(id, result),
            GenMethod::Throw => self.gen_throw(id, args, result),
        }
    }

    /// `yield` inside a running generator: suspends, or injects a pending send without leaving.
    pub(crate) fn yield_value(&mut self, ins: &Instruction) -> Exec<Flow> {
        let value = self.read(ins.a)?;
        let key = if ins.b.is_unused() {
            None
        } else {
            Some(self.read(ins.b)?)
        };
        let Some(id) = self.frame()?.generator else {
            return Err(VmError::Malformed("yield outside a generator").into());
        };
        let Some(state) = self.generators.get_mut(&id) else {
            return Err(VmError::Malformed("unknown generator").into());
        };
        state.current = value;
        if let Some(key) = key {
            state.key = key;
        } else {
            state.key = Value::Int(state.next_key);
            state.next_key += 1;
        }
        state.started = true;
        if let Some(sent) = state.pending_send.take() {
            self.write(ins.result, sent)?;
            return Ok(Flow::Continue);
        }
        let want = state.want;
        let dest = state.result;
        let current = state.current.clone();
        let yielded_key = state.key.clone();
        self.suspend_generator(id)?;
        let written = match want {
            GenWant::Current => current,
            GenWant::Key => yielded_key,
            GenWant::Valid => Value::Bool(true),
            GenWant::Void | GenWant::Send => {
                if matches!(want, GenWant::Send) {
                    current
                } else {
                    Value::Null
                }
            }
        };
        self.write(dest, written)?;
        Ok(Flow::Continue)
    }

    /// A `return` from a generator body closes it.
    pub(crate) fn finish_generator(&mut self, id: ObjectId, value: Value) -> Exec<Flow> {
        let Some(state) = self.generators.get_mut(&id) else {
            return Err(VmError::Malformed("unknown generator").into());
        };
        state.finished = true;
        state.started = true;
        state.return_value = value;
        state.current = Value::Null;
        let want = state.want;
        let dest = state.result;
        self.frames
            .pop()
            .ok_or(VmError::Malformed("return without a frame"))?;
        let written = match want {
            GenWant::Valid => Value::Bool(false),
            _ => Value::Null,
        };
        self.write(dest, written)?;
        Ok(Flow::Continue)
    }
}

impl<O: Output> Machine<'_, O> {
    fn gen_field(&mut self, id: ObjectId, want: GenWant, result: Operand) -> Exec {
        let Some(state) = self.generators.get(&id) else {
            return Err(VmError::Malformed("unknown generator").into());
        };
        if !state.started && !state.finished {
            return self.resume_generator(id, want, result);
        }
        let value = match want {
            GenWant::Current => state.current.clone(),
            GenWant::Key => state.key.clone(),
            GenWant::Valid => Value::Bool(!state.finished),
            GenWant::Void | GenWant::Send => Value::Null,
        };
        self.write(result, value)
    }

    fn gen_advance(&mut self, id: ObjectId, sent: Value, want: GenWant, result: Operand) -> Exec {
        let Some(state) = self.generators.get_mut(&id) else {
            return Err(VmError::Malformed("unknown generator").into());
        };
        if state.finished {
            return self.write(result, Value::Null);
        }
        state.advanced = true;
        state.pending_send = Some(sent);
        self.resume_generator(id, want, result)
    }

    fn gen_rewind(&mut self, id: ObjectId, result: Operand) -> Exec {
        let Some(state) = self.generators.get(&id) else {
            return Err(VmError::Malformed("unknown generator").into());
        };
        if state.advanced {
            return Err(self.throw_from(
                "Exception",
                b"Cannot rewind a generator that was already run".to_vec(),
                b"Generator->rewind",
                Vec::new(),
            ));
        }
        if state.started {
            return self.write(result, Value::Null);
        }
        self.resume_generator(id, GenWant::Void, result)
    }

    fn gen_return_value(&mut self, id: ObjectId, result: Operand) -> Exec {
        let Some(state) = self.generators.get(&id) else {
            return Err(VmError::Malformed("unknown generator").into());
        };
        if !state.finished {
            return Err(self.throw_from(
                "Exception",
                b"Cannot get return value of a generator that hasn't returned".to_vec(),
                b"Generator->getReturn",
                Vec::new(),
            ));
        }
        self.write(result, state.return_value.clone())
    }

    fn gen_throw(&mut self, id: ObjectId, args: &[Argument], result: Operand) -> Exec {
        let value = args.first().map_or(Value::Null, |arg| arg.slot.value());
        let Value::Object(object) = value else {
            return Err(self.throw("Error", b"Can only throw objects".to_vec()));
        };
        let class = object.class_name().as_bytes().to_vec();
        if !crate::exceptions::is_throwable(&class) {
            return Err(self.throw(
                "Error",
                b"Cannot throw objects that do not implement Throwable".to_vec(),
            ));
        }
        let message = object.with_properties(|properties| match properties.get(b"message") {
            Some(slot) => match slot.value() {
                Value::String(text) => text.as_bytes().to_vec(),
                other => other.type_name().as_bytes().to_vec(),
            },
            None => Vec::new(),
        });
        let Some(state) = self.generators.get(&id) else {
            return Err(VmError::Malformed("unknown generator").into());
        };
        if state.finished {
            return Err(self.thrown(&class, message, object));
        }
        self.resume_generator(id, GenWant::Void, result)?;
        if let Some(frame) = self.frames.last_mut()
            && frame.pc > 0
        {
            frame.pc -= 1;
        }
        Err(self.thrown(&class, message, object))
    }

    fn resume_generator(&mut self, id: ObjectId, want: GenWant, result: Operand) -> Exec {
        let Some(state) = self.generators.get_mut(&id) else {
            return Err(VmError::Malformed("unknown generator").into());
        };
        state.want = want;
        state.result = result;
        let started = state.started;
        let mut frame = Frame::new(state.code, &self.codes[state.code]);
        frame.slots = std::mem::take(&mut state.slots);
        frame.temps = std::mem::take(&mut state.temps);
        frame.extra = std::mem::take(&mut state.extra);
        frame.passed = state.passed;
        frame.pc = state.pc;
        frame.generator = Some(id);
        if started && let Some(sent) = state.pending_send.take() {
            inject_sent(&self.codes[frame.code], &mut frame, sent)?;
        }
        state.started = true;
        self.frames.push(frame);
        Ok(())
    }

    fn suspend_generator(&mut self, id: ObjectId) -> Exec {
        let frame = self
            .frames
            .pop()
            .ok_or(VmError::Malformed("yield without a frame"))?;
        let Some(state) = self.generators.get_mut(&id) else {
            return Err(VmError::Malformed("unknown generator").into());
        };
        state.pc = frame.pc;
        state.slots = frame.slots;
        state.temps = frame.temps;
        state.extra = frame.extra;
        Ok(())
    }
}

fn inject_sent(loaded: &crate::machine::Loaded, frame: &mut Frame, sent: Value) -> Exec {
    if frame.pc == 0 {
        return Ok(());
    }
    let Some(ins) = loaded.code.instructions.get(frame.pc - 1) else {
        return Ok(());
    };
    if ins.opcode != php_compiler::bytecode::Opcode::Yield || ins.result.is_unused() {
        return Ok(());
    }
    write_frame(frame, ins.result, sent)
}

fn write_frame(frame: &mut Frame, operand: Operand, value: Value) -> Exec {
    match operand.kind() {
        php_compiler::bytecode::OperandKind::Temp => {
            let slot = frame
                .temps
                .get_mut(operand.index() as usize)
                .ok_or(VmError::Malformed("no such temporary"))?;
            *slot = value;
            Ok(())
        }
        php_compiler::bytecode::OperandKind::Slot => {
            let slot = frame
                .slots
                .get_mut(operand.index() as usize)
                .ok_or(VmError::Malformed("no such slot"))?;
            *slot = Some(Slot::Value(value));
            Ok(())
        }
        _ => Err(VmError::Malformed("operand cannot be written").into()),
    }
}

/// Registers the built-in `Generator` class.
pub(crate) fn register_generator(table: &mut HashMap<Box<[u8]>, crate::classes::Class>) {
    table
        .entry(b"generator".to_vec().into())
        .or_insert_with(|| crate::classes::Class {
            name: b"Generator".to_vec().into(),
            kind: php_compiler::bytecode::ClassKind::Class,
            parent: None,
            properties: Vec::new(),
            methods: HashMap::new(),
            native_methods: HashMap::new(),
            implements: Vec::new(),
            uses: Vec::new(),
            case_names: Vec::new(),
            case_objects: HashMap::new(),
        });
}
