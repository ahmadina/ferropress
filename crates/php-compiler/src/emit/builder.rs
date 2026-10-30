//! Builds one code object: instructions with their lines, interned literals and slots, and
//! temporaries.

use std::collections::HashMap;

use super::{Emit, Failure};
use crate::bytecode::{Code, Instruction, Literal, Opcode, Operand};

/// A literal as interned in the pool: equal keys share one entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum LiteralKey {
    Null,
    Bool(bool),
    Int(i64),
    /// A float by its bits, so `0.0` and `-0.0` stay apart and `NAN` equals itself.
    Float(u64),
    String(Box<[u8]>),
}

/// Which operand of an instruction a jump target goes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    A,
    B,
    C,
}

#[derive(Debug, Default)]
pub(super) struct Builder {
    code: Code,
    slots: HashMap<Box<[u8]>, u32>,
    literals: HashMap<LiteralKey, u32>,
    /// The line given to the instructions emitted next.
    pub(super) line: u32,
}

/// `index` as an operand index, or the failure of exceeding the limit.
fn checked(operand: Option<Operand>, line: u32) -> Emit<Operand> {
    operand.ok_or(Failure::TooLarge { line })
}

/// `count` as a `u32`, or the failure of exceeding the limit.
fn count(count: usize, line: u32) -> Emit<u32> {
    u32::try_from(count).map_err(|_overflow| Failure::TooLarge { line })
}

impl Builder {
    pub(super) fn new(name: &[u8]) -> Self {
        Self {
            code: Code {
                name: name.into(),
                ..Code::default()
            },
            ..Self::default()
        }
    }

    pub(super) fn finish(self) -> Code {
        self.code
    }

    /// The code being built, for setting its parameters and flags.
    pub(super) fn code_mut(&mut self) -> &mut Code {
        &mut self.code
    }

    /// The slot of the local variable `name`, allocated on first use.
    pub(super) fn slot(&mut self, name: &[u8]) -> Emit<Operand> {
        let index = if let Some(&index) = self.slots.get(name) {
            index
        } else {
            let index = count(self.code.slots.len(), self.line)?;
            self.code.slots.push(name.into());
            self.slots.insert(name.into(), index);
            index
        };
        checked(Operand::slot(index), self.line)
    }

    /// The pool entry holding `value`, shared with equal literals.
    pub(super) fn literal(&mut self, value: Literal) -> Emit<Operand> {
        let key = match &value {
            Literal::Null => LiteralKey::Null,
            Literal::Bool(value) => LiteralKey::Bool(*value),
            Literal::Int(value) => LiteralKey::Int(*value),
            Literal::Float(value) => LiteralKey::Float(value.to_bits()),
            Literal::String(text) => LiteralKey::String(text.clone()),
        };
        if let Some(&index) = self.literals.get(&key) {
            return checked(Operand::literal(index), self.line);
        }
        let index = count(self.code.literals.len(), self.line)?;
        self.code.literals.push(value);
        self.literals.insert(key, index);
        checked(Operand::literal(index), self.line)
    }

    /// The literal string `bytes`.
    pub(super) fn string(&mut self, bytes: &[u8]) -> Emit<Operand> {
        self.literal(Literal::from(bytes))
    }

    /// A new temporary.
    pub(super) fn temp(&mut self) -> Emit<Operand> {
        let index = self.code.temporaries;
        let operand = checked(Operand::temp(index), self.line)?;
        self.code.temporaries += 1;
        Ok(operand)
    }

    /// Appends `instruction` on the current line and returns its index.
    pub(super) fn emit(&mut self, instruction: Instruction) -> Emit<u32> {
        let index = self.here()?;
        self.code.instructions.push(instruction);
        self.code.lines.push(self.line);
        Ok(index)
    }

    /// Appends `opcode` with operands `a` and `b` and a result.
    pub(super) fn op(
        &mut self,
        opcode: Opcode,
        a: Operand,
        b: Operand,
        result: Operand,
    ) -> Emit<u32> {
        let mut instruction = Instruction::new(opcode);
        instruction.a = a;
        instruction.b = b;
        instruction.result = result;
        self.emit(instruction)
    }

    /// The index the next instruction gets.
    pub(super) fn here(&self) -> Emit<u32> {
        count(self.code.instructions.len(), self.line)
    }

    /// Points the jump at `at` to `target`, in `field`.
    pub(super) fn patch(&mut self, at: u32, field: Field, target: u32) -> Emit {
        let operand = checked(Operand::target(target), self.line)?;
        if let Some(instruction) = self.code.instructions.get_mut(at as usize) {
            match field {
                Field::A => instruction.a = operand,
                Field::B => instruction.b = operand,
                Field::C => instruction.c = operand,
            }
        }
        Ok(())
    }

    /// Points the jump at `at` to the next instruction.
    pub(super) fn patch_here(&mut self, at: u32, field: Field) -> Emit {
        let target = self.here()?;
        self.patch(at, field, target)
    }

    /// Turns a trailing unused `PreIncrement` into `IncJump` to `start`.
    pub(super) fn fuse_inc_jump(&mut self, start: u32) -> bool {
        let Ok(target) = checked(Operand::target(start), self.line) else {
            return false;
        };
        let Some(last) = self.code.instructions.last_mut() else {
            return false;
        };
        if last.opcode != Opcode::PreIncrement || !last.result.is_unused() {
            return false;
        }
        last.opcode = Opcode::IncJump;
        last.b = target;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{Builder, Field, LiteralKey};
    use crate::bytecode::{Literal, Opcode, Operand};
    use crate::emit::Failure;

    #[test]
    fn literals_and_slots_are_interned() {
        let mut builder = Builder::new(b"f");
        let one = builder.literal(Literal::Int(1)).unwrap();
        assert_eq!(builder.literal(Literal::Int(1)).unwrap(), one);
        let zero = builder.literal(Literal::Float(0.0)).unwrap();
        assert_ne!(builder.literal(Literal::Float(-0.0)).unwrap(), zero);
        let nan = builder.literal(Literal::Float(f64::NAN)).unwrap();
        assert_eq!(builder.literal(Literal::Float(f64::NAN)).unwrap(), nan);
        assert_eq!(builder.string(b"a").unwrap(), builder.string(b"a").unwrap());
        assert_eq!(builder.slot(b"a").unwrap(), builder.slot(b"a").unwrap());
        assert_ne!(builder.slot(b"b").unwrap(), builder.slot(b"a").unwrap());
        assert_eq!(LiteralKey::Null, LiteralKey::Null);
        let code = builder.finish();
        assert_eq!(&*code.name, b"f");
        assert_eq!(code.slots.len(), 2);
        assert_eq!(code.literals.len(), 5);
    }

    #[test]
    fn jumps_are_patched_and_lines_recorded() {
        let mut builder = Builder::new(b"");
        builder.line = 3;
        let jump = builder
            .op(
                Opcode::Jump,
                Operand::UNUSED,
                Operand::UNUSED,
                Operand::UNUSED,
            )
            .unwrap();
        let condition = builder.temp().unwrap();
        let branch = builder
            .op(
                Opcode::JumpIfFalse,
                condition,
                Operand::UNUSED,
                Operand::UNUSED,
            )
            .unwrap();
        builder.patch_here(jump, Field::A).unwrap();
        builder.patch(branch, Field::B, 0).unwrap();
        builder.patch(9, Field::A, 0).unwrap();
        let code = builder.finish();
        assert_eq!(code.lines, [3, 3]);
        assert_eq!(code.instructions[0].a, Operand::target(2).unwrap());
        assert_eq!(code.instructions[1].b, Operand::target(0).unwrap());
        assert_eq!(code.temporaries, 1);
    }

    #[test]
    fn unused_increment_fuses_into_inc_jump() {
        let mut builder = Builder::new(b"");
        builder
            .op(
                Opcode::PreIncrement,
                Operand::slot(0).unwrap(),
                Operand::UNUSED,
                Operand::UNUSED,
            )
            .unwrap();
        assert!(builder.fuse_inc_jump(0));
        assert!(!builder.fuse_inc_jump(0));
        let empty = Builder::new(b"");
        assert!(!{
            let mut builder = empty;
            builder.fuse_inc_jump(0)
        });
        let code = {
            let mut builder = Builder::new(b"");
            builder
                .op(
                    Opcode::PreIncrement,
                    Operand::slot(0).unwrap(),
                    Operand::UNUSED,
                    Operand::UNUSED,
                )
                .unwrap();
            builder.fuse_inc_jump(3);
            builder.finish()
        };
        assert_eq!(code.instructions[0].opcode, Opcode::IncJump);
        assert_eq!(code.instructions[0].b, Operand::target(3).unwrap());
    }

    #[test]
    fn operand_limits_are_failures() {
        let mut builder = Builder::new(b"");
        builder.line = 7;
        builder.code_mut().temporaries = Operand::MAX_INDEX + 1;
        assert_eq!(builder.temp(), Err(Failure::TooLarge { line: 7 }));
        assert_eq!(
            builder.patch(0, Field::A, Operand::MAX_INDEX + 1),
            Err(Failure::TooLarge { line: 7 })
        );
    }
}
