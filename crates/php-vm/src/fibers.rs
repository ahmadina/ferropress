//! Fibers: `new Fiber($callable)`, `start`/`resume` and `Fiber::suspend`.

use std::collections::HashMap;

use php_compiler::bytecode::{Instruction, Opcode, Operand, OperandKind};
use php_value::{Object, ObjectId, Slot, Value};

use crate::calls::{Argument, Pending};
use crate::machine::{Exec, Flow, Frame, Machine};
use crate::{Output, VmError};

/// A native Fiber method, instance or static.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FiberMethod {
    Construct,
    Start,
    Resume,
    Suspend,
    GetReturn,
    IsStarted,
    IsSuspended,
    IsRunning,
    IsTerminated,
    Throw,
}

impl FiberMethod {
    pub(crate) fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"__construct" => Self::Construct,
            b"start" => Self::Start,
            b"resume" => Self::Resume,
            b"suspend" => Self::Suspend,
            b"getreturn" => Self::GetReturn,
            b"isstarted" => Self::IsStarted,
            b"issuspended" => Self::IsSuspended,
            b"isrunning" => Self::IsRunning,
            b"isterminated" => Self::IsTerminated,
            b"throw" => Self::Throw,
            _ => return None,
        })
    }
}

/// A prepared Fiber call.
pub(crate) enum FiberCall {
    /// `$fiber->method()`.
    Instance(Object, FiberMethod),
    /// `Fiber::method()`.
    Static(FiberMethod),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FiberState {
    Created,
    Running,
    Suspended,
    Terminated,
}

/// Saved state of one fiber object.
pub(crate) struct Fiber {
    callback: Option<Value>,
    state: FiberState,
    return_value: Value,
    saved: Vec<Frame>,
    pending: Option<Value>,
    caller_result: Operand,
    pub(crate) root_depth: usize,
}

impl<O: Output> Machine<'_, O> {
    /// Starts a static call on the class named by `a`.
    pub(crate) fn init_static_call(&mut self, ins: &Instruction) -> Exec {
        let Value::String(class) = self.read(ins.a)? else {
            return Err(VmError::Malformed("class name is not a string").into());
        };
        let Value::String(key) = self.read(ins.b)? else {
            return Err(VmError::Malformed("method name is not a string").into());
        };
        let Value::String(written) = self.read(ins.c)? else {
            return Err(VmError::Malformed("method name is not a string").into());
        };
        if class.as_bytes().eq_ignore_ascii_case(b"Fiber")
            && let Some(method) = FiberMethod::from_name(key.as_bytes())
        {
            self.pending.push(Pending {
                code: 0,
                env: None,
                this: None,
                implicit_ctor: None,
                generator_method: None,
                native: None,
                fiber_method: Some(FiberCall::Static(method)),
                native_method: None,
                args: Vec::new(),
            });
            return Ok(());
        }
        self.init_class_static_call(class.as_bytes(), key.as_bytes(), written.as_bytes())
    }

    /// Runs a native Fiber method.
    pub(crate) fn dispatch_fiber(
        &mut self,
        call: FiberCall,
        args: &[Argument],
        result: Operand,
    ) -> Exec {
        match call {
            FiberCall::Static(FiberMethod::Suspend) => self.fiber_suspend(args, result),
            FiberCall::Static(method) => Err(self.throw(
                "Error",
                format!(
                    "Non-static method Fiber::{}() cannot be called statically",
                    method_name(method)
                )
                .into_bytes(),
            )),
            FiberCall::Instance(object, FiberMethod::Construct) => {
                self.fiber_construct(&object, args)
            }
            FiberCall::Instance(object, method) => {
                self.dispatch_fiber_instance(&object, method, args, result)
            }
        }
    }

    fn dispatch_fiber_instance(
        &mut self,
        object: &Object,
        method: FiberMethod,
        args: &[Argument],
        result: Operand,
    ) -> Exec {
        match method {
            FiberMethod::Start => self.fiber_start(object, args, result),
            FiberMethod::Resume => self.fiber_resume(object, args, result),
            FiberMethod::GetReturn => self.fiber_get_return(object, result),
            FiberMethod::IsStarted => {
                self.fiber_flag(object, result, |fiber| fiber.state != FiberState::Created)
            }
            FiberMethod::IsSuspended => {
                self.fiber_flag(object, result, |fiber| fiber.state == FiberState::Suspended)
            }
            FiberMethod::IsRunning => {
                self.fiber_flag(object, result, |fiber| fiber.state == FiberState::Running)
            }
            FiberMethod::IsTerminated => self.fiber_flag(object, result, |fiber| {
                fiber.state == FiberState::Terminated
            }),
            FiberMethod::Throw => self.fiber_throw(object, args, result),
            FiberMethod::Suspend => self.fiber_suspend(args, result),
            FiberMethod::Construct => Ok(()),
        }
    }

    /// The fiber callback returned; start/resume yield `null`.
    pub(crate) fn finish_fiber(&mut self, id: ObjectId, value: Value) -> Exec<Flow> {
        let dest = {
            let Some(fiber) = self.fibers.get_mut(&id) else {
                return Err(VmError::Malformed("unknown fiber").into());
            };
            fiber.state = FiberState::Terminated;
            fiber.return_value = value;
            fiber.caller_result
        };
        self.frames
            .pop()
            .ok_or(VmError::Malformed("return without a frame"))?;
        self.current_fiber = None;
        self.write(dest, Value::Null)?;
        Ok(Flow::Continue)
    }

    /// Marks the current fiber finished when an uncaught throw leaves its last frame.
    pub(crate) fn leave_fiber(&mut self) {
        let Some(id) = self.current_fiber else {
            return;
        };
        let Some(fiber) = self.fibers.get(&id) else {
            return;
        };
        if self.frames.len() != fiber.root_depth + 1 {
            return;
        }
        if let Some(fiber) = self.fibers.get_mut(&id) {
            fiber.state = FiberState::Terminated;
        }
        self.current_fiber = None;
    }
}

impl<O: Output> Machine<'_, O> {
    fn fiber_construct(&mut self, object: &Object, args: &[Argument]) -> Exec {
        if args.len() != 1 {
            let mut message = b"Fiber::__construct() expects exactly 1 argument, ".to_vec();
            message.extend_from_slice(args.len().to_string().as_bytes());
            message.extend_from_slice(b" given");
            return Err(self.throw_from(
                "ArgumentCountError",
                message,
                b"Fiber->__construct",
                Vec::new(),
            ));
        }
        let value = args[0].slot.value();
        let callback = match value {
            Value::Object(ref callback)
                if callback
                    .class_name()
                    .as_bytes()
                    .eq_ignore_ascii_case(b"Closure") =>
            {
                value
            }
            Value::String(ref name) if self.callable_function(name.as_bytes()) => value,
            other => {
                return Err(self.throw_from(
                    "TypeError",
                    invalid_callback(&other),
                    b"Fiber->__construct",
                    vec![other],
                ));
            }
        };
        self.fibers.insert(
            object.id(),
            Fiber {
                callback: Some(callback),
                state: FiberState::Created,
                return_value: Value::Null,
                saved: Vec::new(),
                pending: None,
                caller_result: Operand::UNUSED,
                root_depth: 0,
            },
        );
        Ok(())
    }

    fn callable_function(&self, written: &[u8]) -> bool {
        let stripped = written.strip_prefix(b"\\").unwrap_or(written);
        let key = stripped.to_ascii_lowercase();
        self.functions.contains_key(key.as_slice()) || crate::builtins::lookup(&key).is_some()
    }

    fn fiber_start(&mut self, object: &Object, args: &[Argument], result: Operand) -> Exec {
        let id = object.id();
        let Some(fiber) = self.fibers.get_mut(&id) else {
            return Err(VmError::Malformed("unknown fiber").into());
        };
        if fiber.state != FiberState::Created {
            return Err(self.fiber_error(
                b"Cannot start a fiber that has already been started",
                b"Fiber->start",
                args,
            ));
        }
        let Some(callback) = fiber.callback.clone() else {
            return Err(VmError::Malformed("fiber has no callback").into());
        };
        fiber.state = FiberState::Running;
        fiber.caller_result = result;
        fiber.root_depth = self.frames.len();
        self.current_fiber = Some(id);
        match callback {
            Value::Object(closure) => self.init_closure_call(closure)?,
            Value::String(name) => self.init_string_call(name.as_bytes())?,
            _ => return Err(VmError::Malformed("fiber callback is not callable").into()),
        }
        if let Some(pending) = self.pending.last_mut() {
            pending.args = args
                .iter()
                .map(|arg| Argument {
                    slot: Slot::Value(arg.slot.value()),
                    from_call: false,
                })
                .collect();
        }
        self.call(Operand::UNUSED)
    }

    fn fiber_resume(&mut self, object: &Object, args: &[Argument], result: Operand) -> Exec {
        let id = object.id();
        let sent = args.first().map_or(Value::Null, |arg| arg.slot.value());
        let Some(fiber) = self.fibers.get_mut(&id) else {
            return Err(VmError::Malformed("unknown fiber").into());
        };
        if fiber.state != FiberState::Suspended {
            return Err(self.fiber_error(
                b"Cannot resume a fiber that is not suspended",
                b"Fiber->resume",
                args,
            ));
        }
        fiber.state = FiberState::Running;
        fiber.caller_result = result;
        fiber.pending = Some(sent);
        fiber.root_depth = self.frames.len();
        let mut saved = std::mem::take(&mut fiber.saved);
        if let Some(frame) = saved.last_mut()
            && let Some(sent) = fiber.pending.take()
        {
            inject_call_result(&self.codes[frame.code], frame, sent)?;
        }
        self.current_fiber = Some(id);
        self.frames.append(&mut saved);
        Ok(())
    }

    fn fiber_suspend(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = args.first().map_or(Value::Null, |arg| arg.slot.value());
        let Some(id) = self.current_fiber else {
            return Err(self.fiber_error(
                b"Cannot suspend outside of a fiber",
                b"Fiber::suspend",
                args,
            ));
        };
        let dest = {
            let Some(fiber) = self.fibers.get_mut(&id) else {
                return Err(VmError::Malformed("unknown fiber").into());
            };
            if fiber.state != FiberState::Running {
                return Err(self.fiber_error(
                    b"Cannot suspend outside of a fiber",
                    b"Fiber::suspend",
                    args,
                ));
            }
            let dest = fiber.caller_result;
            let base = fiber.root_depth;
            fiber.saved = self.frames.split_off(base);
            fiber.state = FiberState::Suspended;
            dest
        };
        let _ = result;
        self.current_fiber = None;
        self.write(dest, value)
    }

    fn fiber_get_return(&mut self, object: &Object, result: Operand) -> Exec {
        let Some(fiber) = self.fibers.get(&object.id()) else {
            return Err(VmError::Malformed("unknown fiber").into());
        };
        if fiber.state != FiberState::Terminated {
            return Err(self.fiber_error(
                b"Cannot get fiber return value: The fiber has not returned",
                b"Fiber->getReturn",
                &[],
            ));
        }
        self.write(result, fiber.return_value.clone())
    }

    fn fiber_error(
        &self,
        message: &[u8],
        function: &[u8],
        args: &[Argument],
    ) -> crate::machine::Stop {
        self.throw_from(
            "FiberError",
            message.to_vec(),
            function,
            args.iter().map(|arg| arg.slot.value()).collect(),
        )
    }

    fn fiber_flag(&mut self, object: &Object, result: Operand, test: fn(&Fiber) -> bool) -> Exec {
        let Some(fiber) = self.fibers.get(&object.id()) else {
            return Err(VmError::Malformed("unknown fiber").into());
        };
        self.write(result, Value::Bool(test(fiber)))
    }

    fn fiber_throw(&mut self, object: &Object, args: &[Argument], result: Operand) -> Exec {
        let value = args.first().map_or(Value::Null, |arg| arg.slot.value());
        let Value::Object(thrown) = value else {
            return Err(self.throw("Error", b"Can only throw objects".to_vec()));
        };
        let class = thrown.class_name().as_bytes().to_vec();
        if !crate::exceptions::is_throwable(&class) {
            return Err(self.throw(
                "Error",
                b"Cannot throw objects that do not implement Throwable".to_vec(),
            ));
        }
        let message = thrown.with_properties(|properties| match properties.get(b"message") {
            Some(slot) => match slot.value() {
                Value::String(text) => text.as_bytes().to_vec(),
                other => other.type_name().as_bytes().to_vec(),
            },
            None => Vec::new(),
        });
        self.fiber_resume(object, &[], result)?;
        Err(self.thrown(&class, message, thrown))
    }
}

fn method_name(method: FiberMethod) -> &'static str {
    match method {
        FiberMethod::Construct => "__construct",
        FiberMethod::Start => "start",
        FiberMethod::Resume => "resume",
        FiberMethod::Suspend => "suspend",
        FiberMethod::GetReturn => "getReturn",
        FiberMethod::IsStarted => "isStarted",
        FiberMethod::IsSuspended => "isSuspended",
        FiberMethod::IsRunning => "isRunning",
        FiberMethod::IsTerminated => "isTerminated",
        FiberMethod::Throw => "throw",
    }
}

fn inject_call_result(loaded: &crate::machine::Loaded, frame: &mut Frame, sent: Value) -> Exec {
    if frame.pc == 0 {
        return Ok(());
    }
    let Some(ins) = loaded.code.instructions.get(frame.pc - 1) else {
        return Ok(());
    };
    if ins.opcode != Opcode::Call || ins.result.is_unused() {
        return Ok(());
    }
    match ins.result.kind() {
        OperandKind::Temp => {
            let slot = frame
                .temps
                .get_mut(ins.result.index() as usize)
                .ok_or(VmError::Malformed("no such temporary"))?;
            *slot = sent;
            Ok(())
        }
        OperandKind::Slot => {
            let slot = frame
                .slots
                .get_mut(ins.result.index() as usize)
                .ok_or(VmError::Malformed("no such slot"))?;
            *slot = Some(Slot::Value(sent));
            Ok(())
        }
        _ => Err(VmError::Malformed("operand cannot be written").into()),
    }
}

fn invalid_callback(value: &Value) -> Vec<u8> {
    let mut message =
        b"Fiber::__construct(): Argument #1 ($callback) must be a valid callback, ".to_vec();
    match value {
        Value::String(name) => {
            message.extend_from_slice(b"function \"");
            message.extend_from_slice(name.as_bytes());
            message.extend_from_slice(b"\" not found or invalid function name");
        }
        _ => message.extend_from_slice(b"no array or string given"),
    }
    message
}

/// Registers the built-in `Fiber` class.
pub(crate) fn register_fiber(table: &mut HashMap<Box<[u8]>, crate::classes::Class>) {
    table
        .entry(b"fiber".to_vec().into())
        .or_insert_with(|| crate::classes::Class {
            name: b"Fiber".to_vec().into(),
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
