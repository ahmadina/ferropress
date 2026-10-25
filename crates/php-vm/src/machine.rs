//! The interpreter loop: frames, operands and the instructions that compute values.

use std::collections::HashMap;

use php_compiler::bytecode::{Code, Instruction, Literal, Opcode, Operand, OperandKind, Unit};
use php_value::ops::{self, Collect, OpError};
use php_value::{ObjectStore, PhpString, Slot, Value};

use crate::thrown::{Thrown, TraceEntry};
use crate::{Output, VmError};

/// A code object with its literals as runtime values, built once per run.
pub(crate) struct Loaded<'u> {
    pub(crate) code: &'u Code,
    pub(crate) literals: Vec<Value>,
}

/// One active call.
pub(crate) struct Frame {
    /// Index of the code object in [`Machine::codes`].
    pub(crate) code: usize,
    /// The next instruction.
    pub(crate) pc: usize,
    /// Local variables; `None` is undefined.
    pub(crate) slots: Vec<Option<Slot>>,
    pub(crate) temps: Vec<Value>,
    /// Where the caller wants the return value.
    pub(crate) result: Operand,
    /// The line the call was made on.
    pub(crate) call_line: u32,
    /// How many arguments were passed.
    pub(crate) passed: usize,
    /// Arguments past the declared parameters, which the stack trace shows.
    pub(crate) extra: Vec<Value>,
}

impl Frame {
    pub(crate) fn new(code: usize, loaded: &Loaded<'_>) -> Self {
        Self {
            code,
            pc: 0,
            slots: std::iter::repeat_with(|| None)
                .take(loaded.code.slots.len())
                .collect(),
            temps: vec![Value::Null; loaded.code.temporaries as usize],
            result: Operand::UNUSED,
            call_line: 0,
            passed: 0,
            extra: Vec::new(),
        }
    }
}

/// Why execution left the instruction loop.
pub(crate) enum Stop {
    /// An error was thrown and not caught.
    Thrown(Box<Thrown>),
    /// The VM itself failed.
    Vm(VmError),
}

impl From<VmError> for Stop {
    fn from(error: VmError) -> Self {
        Self::Vm(error)
    }
}

pub(crate) type Exec<T = ()> = Result<T, Stop>;

/// What the loop does after an instruction.
pub(crate) enum Flow {
    Continue,
    Finished,
}

pub(crate) struct Machine<'u, 'o, O: Output> {
    pub(crate) codes: Vec<Loaded<'u>>,
    /// User functions by lowercase name: indices into `codes`.
    pub(crate) functions: HashMap<Box<[u8]>, usize>,
    pub(crate) file: Box<[u8]>,
    output: &'o mut O,
    pub(crate) frames: Vec<Frame>,
    /// Calls being prepared: the function and the arguments sent so far.
    pub(crate) pending: Vec<crate::calls::Pending>,
    pub(crate) objects: ObjectStore,
    pub(crate) cx: Collect,
    /// The line of the instruction running.
    pub(crate) line: u32,
}

/// `literal` as a runtime value.
fn value_of(literal: &Literal) -> Value {
    match literal {
        Literal::Null => Value::Null,
        Literal::Bool(value) => Value::Bool(*value),
        Literal::Int(value) => Value::Int(*value),
        Literal::Float(value) => Value::Float(*value),
        Literal::String(bytes) => Value::String(PhpString::new(bytes)),
    }
}

fn load(code: &Code) -> Loaded<'_> {
    Loaded {
        code,
        literals: code.literals.iter().map(value_of).collect(),
    }
}

/// Maps an operator failure to a thrown error or a VM failure.
fn op_error(error: OpError) -> Result<ops::Thrown, VmError> {
    match error {
        OpError::Throw(thrown) => Ok(thrown),
        OpError::Object => Err(VmError::Unsupported("objects")),
    }
}

impl<'u, 'o, O: Output> Machine<'u, 'o, O> {
    pub(crate) fn new(unit: &'u Unit, file: &[u8], output: &'o mut O) -> Self {
        let mut codes = vec![load(&unit.main)];
        let mut functions = HashMap::with_capacity(unit.functions.len());
        for function in &unit.functions {
            if !function.name.is_empty() {
                functions.insert(function.name.to_ascii_lowercase().into(), codes.len());
            }
            codes.push(load(&function.code));
        }
        Self {
            codes,
            functions,
            file: file.into(),
            output,
            frames: Vec::new(),
            pending: Vec::new(),
            objects: ObjectStore::new(),
            cx: Collect::default(),
            line: 0,
        }
    }

    pub(crate) fn run(mut self) -> Result<u8, VmError> {
        let main = Frame::new(0, &self.codes[0]);
        self.frames.push(main);
        loop {
            match self.step() {
                Ok(Flow::Continue) => {}
                Ok(Flow::Finished) => return Ok(0),
                Err(Stop::Thrown(thrown)) => {
                    self.flush();
                    let text = thrown.uncaught(self.cx.precision);
                    self.output.write(&text);
                    return Ok(255);
                }
                Err(Stop::Vm(error)) => return Err(error),
            }
        }
    }

    pub(crate) fn frame(&self) -> Result<&Frame, VmError> {
        self.frames
            .last()
            .ok_or(VmError::Malformed("no active frame"))
    }

    pub(crate) fn frame_mut(&mut self) -> Result<&mut Frame, VmError> {
        self.frames
            .last_mut()
            .ok_or(VmError::Malformed("no active frame"))
    }

    fn step(&mut self) -> Exec<Flow> {
        let frame = self.frame()?;
        let code = self.codes[frame.code].code;
        let pc = frame.pc;
        let instruction = *code
            .instructions
            .get(pc)
            .ok_or(VmError::Malformed("ran past the last instruction"))?;
        self.line = code.lines.get(pc).copied().unwrap_or(0);
        self.frame_mut()?.pc = pc + 1;
        let flow = self.execute(&instruction);
        self.flush();
        flow
    }

    fn execute(&mut self, ins: &Instruction) -> Exec<Flow> {
        let done = match ins.opcode {
            Opcode::Nop => Ok(()),
            Opcode::Copy => self
                .read(ins.a)
                .and_then(|value| self.write(ins.result, value)),
            Opcode::Assign => self.assign(ins),
            Opcode::Jump => self.jump(ins.a),
            Opcode::JumpIfFalse | Opcode::JumpIfTrue => self.branch(ins),
            Opcode::PreIncrement | Opcode::PostIncrement => self.step_variable(ins, ops::increment),
            Opcode::PreDecrement | Opcode::PostDecrement => self.step_variable(ins, ops::decrement),
            Opcode::Echo => self.echo(ins.a),
            Opcode::InitCall => self.init_call(ins),
            Opcode::InitCallValue => self.init_call_value(ins.a),
            Opcode::MakeClosure => self.make_closure(ins),
            Opcode::Capture => self.capture(ins),
            Opcode::Send => self
                .read(ins.a)
                .and_then(|value| self.send(Slot::Value(value), ins.flags & 1 != 0)),
            Opcode::SendVar => self.send_variable(ins.a),
            Opcode::Call => self.call(ins.result),
            Opcode::Return => return self.return_from(ins.a),
            _ => self.operator(ins),
        };
        done.map(|()| Flow::Continue)
    }

    /// The instructions computing a value from their operands.
    fn operator(&mut self, ins: &Instruction) -> Exec {
        match ins.opcode {
            Opcode::Add => self.arithmetic(ins, ops::add),
            Opcode::Sub => self.arithmetic(ins, ops::sub),
            Opcode::Mul => self.arithmetic(ins, ops::mul),
            Opcode::Div => self.arithmetic(ins, ops::div),
            Opcode::Mod => self.arithmetic(ins, ops::modulo),
            Opcode::Pow => self.arithmetic(ins, ops::pow),
            Opcode::Concat => self.arithmetic(ins, ops::concat),
            Opcode::ShiftLeft => self.arithmetic(ins, ops::shift_left),
            Opcode::ShiftRight => self.arithmetic(ins, ops::shift_right),
            Opcode::BitAnd => self.arithmetic(ins, ops::bit_and),
            Opcode::BitOr => self.arithmetic(ins, ops::bit_or),
            Opcode::BitXor => self.arithmetic(ins, ops::bit_xor),
            Opcode::BoolXor => self.arithmetic(ins, |left, right, cx| {
                Ok(Value::Bool(left.to_bool(cx)? ^ right.to_bool(cx)?))
            }),
            Opcode::Equal => self.arithmetic(ins, |left, right, cx| {
                ops::loose_equals(left, right, cx).map(Value::Bool)
            }),
            Opcode::NotEqual => self.arithmetic(ins, |left, right, cx| {
                ops::loose_equals(left, right, cx).map(|equal| Value::Bool(!equal))
            }),
            Opcode::Identical => self.arithmetic(ins, |left, right, _| {
                ops::identical(left, right).map(Value::Bool)
            }),
            Opcode::NotIdentical => self.arithmetic(ins, |left, right, _| {
                ops::identical(left, right).map(|same| Value::Bool(!same))
            }),
            Opcode::Less => self.arithmetic(ins, |left, right, cx| {
                ops::less(left, right, cx).map(Value::Bool)
            }),
            Opcode::LessOrEqual => self.arithmetic(ins, |left, right, cx| {
                ops::less_or_equal(left, right, cx).map(Value::Bool)
            }),
            Opcode::Spaceship => self.arithmetic(ins, |left, right, cx| {
                ops::compare(left, right, cx).map(|order| Value::Int(order as i64))
            }),
            Opcode::BitNot => self.unary(ins, ops::bit_not),
            Opcode::BoolNot => self.unary(ins, |value, cx| Ok(Value::Bool(!value.to_bool(cx)?))),
            Opcode::ToBool => self.unary(ins, |value, cx| Ok(Value::Bool(value.to_bool(cx)?))),
            Opcode::ToString => {
                let operand = self.read(ins.a)?;
                let text = self.stringify(&operand)?;
                self.write(ins.result, Value::String(text))
            }
            _ => Err(VmError::Unsupported("this instruction").into()),
        }
    }

    fn assign(&mut self, ins: &Instruction) -> Exec {
        let value = self.read(ins.b)?;
        self.write(ins.a, value.clone())?;
        self.write(ins.result, value)
    }

    /// Jumps to `b` when `a` converts to the boolean the opcode tests for.
    fn branch(&mut self, ins: &Instruction) -> Exec {
        let value = self.read(ins.a)?;
        let truth = self.compute(|cx| value.to_bool(cx))?;
        if truth == (ins.opcode == Opcode::JumpIfTrue) {
            self.jump(ins.b)?;
        }
        Ok(())
    }

    fn echo(&mut self, operand: Operand) -> Exec {
        let value = self.read(operand)?;
        let text = self.stringify(&value)?;
        self.flush();
        self.output.write(text.as_bytes());
        Ok(())
    }

    /// Runs an operation, turning what it throws into a thrown error.
    pub(crate) fn compute<T>(
        &mut self,
        op: impl FnOnce(&mut Collect) -> Result<T, OpError>,
    ) -> Exec<T> {
        match op(&mut self.cx) {
            Ok(value) => Ok(value),
            Err(error) => {
                let thrown = op_error(error)?;
                Err(self.throw(thrown.class(), thrown.to_string().into_bytes()))
            }
        }
    }

    fn arithmetic(
        &mut self,
        ins: &Instruction,
        op: impl FnOnce(&Value, &Value, &mut Collect) -> Result<Value, OpError>,
    ) -> Exec {
        let left = self.read(ins.a)?;
        let right = self.read(ins.b)?;
        let value = self.compute(|cx| op(&left, &right, cx))?;
        self.write(ins.result, value)
    }

    fn unary(
        &mut self,
        ins: &Instruction,
        op: impl FnOnce(&Value, &mut Collect) -> Result<Value, OpError>,
    ) -> Exec {
        let operand = self.read(ins.a)?;
        let value = self.compute(|cx| op(&operand, cx))?;
        self.write(ins.result, value)
    }

    /// `++` or `--` on the variable `a`, giving the new value, or the old one for the post forms.
    fn step_variable(
        &mut self,
        ins: &Instruction,
        op: impl FnOnce(&Value, &mut Collect) -> Result<Value, OpError>,
    ) -> Exec {
        let old = self.read(ins.a)?;
        let new = self.compute(|cx| op(&old, cx))?;
        self.write(ins.a, new.clone())?;
        let post = matches!(ins.opcode, Opcode::PostIncrement | Opcode::PostDecrement);
        self.write(ins.result, if post { old } else { new })
    }

    fn jump(&mut self, target: Operand) -> Exec {
        if target.kind() != OperandKind::Target {
            return Err(VmError::Malformed("jump without a target").into());
        }
        self.frame_mut()?.pc = target.index() as usize;
        Ok(())
    }

    /// The value of `operand`; reading an undefined variable warns and gives `null`.
    pub(crate) fn read(&mut self, operand: Operand) -> Exec<Value> {
        let index = operand.index() as usize;
        let frame = self.frame()?;
        match operand.kind() {
            OperandKind::Slot => match frame.slots.get(index) {
                Some(Some(slot)) => Ok(slot.value()),
                Some(None) => {
                    self.undefined_variable(index)?;
                    Ok(Value::Null)
                }
                None => Err(VmError::Malformed("no such slot").into()),
            },
            OperandKind::Temp => frame
                .temps
                .get(index)
                .cloned()
                .ok_or_else(|| VmError::Malformed("no such temporary").into()),
            OperandKind::Literal => self.codes[frame.code]
                .literals
                .get(index)
                .cloned()
                .ok_or_else(|| VmError::Malformed("no such literal").into()),
            OperandKind::Unused | OperandKind::Target => {
                Err(VmError::Malformed("operand holds no value").into())
            }
        }
    }

    /// Stores `value` into `operand`: a temporary, or a variable assigned through its reference.
    pub(crate) fn write(&mut self, operand: Operand, value: Value) -> Exec {
        let index = operand.index() as usize;
        let frame = self.frame_mut()?;
        match operand.kind() {
            OperandKind::Unused => Ok(()),
            OperandKind::Temp => {
                let temp = frame
                    .temps
                    .get_mut(index)
                    .ok_or(VmError::Malformed("no such temporary"))?;
                *temp = value;
                Ok(())
            }
            OperandKind::Slot => {
                let slot = frame
                    .slots
                    .get_mut(index)
                    .ok_or(VmError::Malformed("no such slot"))?;
                match slot {
                    Some(slot) => {
                        slot.assign(value);
                    }
                    None => *slot = Some(Slot::Value(value)),
                }
                Ok(())
            }
            OperandKind::Literal | OperandKind::Target => {
                Err(VmError::Malformed("operand cannot be written").into())
            }
        }
    }

    fn undefined_variable(&mut self, index: usize) -> Exec {
        let frame = self.frame()?;
        let name = self.codes[frame.code]
            .code
            .slots
            .get(index)
            .ok_or(VmError::Malformed("no such slot"))?;
        let mut message = b"Undefined variable $".to_vec();
        message.extend_from_slice(name);
        self.display(b"Warning", &message);
        Ok(())
    }

    /// Displays the diagnostics operations raised.
    fn flush(&mut self) {
        let raised = std::mem::take(&mut self.cx.diagnostics);
        for diagnostic in raised {
            self.display(
                diagnostic.severity().label().as_bytes(),
                &diagnostic.message(),
            );
        }
    }

    /// Displays a diagnostic on the current line.
    pub(crate) fn display(&mut self, label: &[u8], message: &[u8]) {
        let mut text = Vec::with_capacity(message.len() + self.file.len() + 32);
        text.push(b'\n');
        text.extend_from_slice(label);
        text.extend_from_slice(b": ");
        text.extend_from_slice(message);
        text.extend_from_slice(b" in ");
        text.extend_from_slice(&self.file);
        text.extend_from_slice(b" on line ");
        text.extend_from_slice(self.line.to_string().as_bytes());
        text.push(b'\n');
        self.output.write(&text);
    }

    /// A thrown error of `class` with `message`, raised on the current line.
    pub(crate) fn throw(&self, class: &'static str, message: Vec<u8>) -> Stop {
        let trace = self
            .frames
            .iter()
            .skip(1)
            .rev()
            .map(|frame| TraceEntry {
                file: self.file.clone(),
                line: frame.call_line,
                function: self.codes[frame.code].code.name.clone(),
                args: self.trace_args(frame),
            })
            .collect();
        Stop::Thrown(Box::new(Thrown {
            class,
            message,
            file: self.file.clone(),
            line: self.line,
            trace,
        }))
    }

    /// The arguments a stack trace shows for `frame`: the current values of the parameters that
    /// were passed, then the arguments past the parameters.
    fn trace_args(&self, frame: &Frame) -> Vec<Value> {
        let code = self.codes[frame.code].code;
        let declared = code.params.iter().filter(|param| !param.variadic);
        let mut args: Vec<Value> = declared
            .take(frame.passed)
            .map(|param| {
                frame
                    .slots
                    .get(param.slot as usize)
                    .and_then(Option::as_ref)
                    .map_or(Value::Null, Slot::value)
            })
            .collect();
        args.extend(frame.extra.iter().cloned());
        args
    }
}
