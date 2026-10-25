//! Instructions: an opcode, flag bits and up to three operands plus a result.

use super::Operand;

/// What an instruction does. The set is internal to Ferropress (ADR-0007): it changes with the VM
/// as long as diagnostics and behavior stay the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Opcode {
    /// Does nothing.
    Nop,
    /// `result = a`: copies a value into a temporary.
    Copy,
    /// `a = b`, assigning to the local `a`; `result`, when used, receives the assigned value.
    Assign,
    /// `result = a + b`.
    Add,
    /// `result = a - b`.
    Sub,
    /// `result = a * b`.
    Mul,
    /// `result = a / b`.
    Div,
    /// `result = a % b`.
    Mod,
    /// `result = a ** b`.
    Pow,
    /// `result = a . b`.
    Concat,
    /// `result = a << b`.
    ShiftLeft,
    /// `result = a >> b`.
    ShiftRight,
    /// `result = a & b`.
    BitAnd,
    /// `result = a | b`.
    BitOr,
    /// `result = a ^ b`.
    BitXor,
    /// `result = a xor b`.
    BoolXor,
    /// `result = a == b`.
    Equal,
    /// `result = a != b`.
    NotEqual,
    /// `result = a === b`.
    Identical,
    /// `result = a !== b`.
    NotIdentical,
    /// `result = a < b`.
    Less,
    /// `result = a <= b`.
    LessOrEqual,
    /// `result = a <=> b`.
    Spaceship,
    /// `result = ~a`.
    BitNot,
    /// `result = !a`.
    BoolNot,
    /// `result = (bool) a`.
    ToBool,
    /// Jumps to `a`.
    Jump,
    /// Jumps to `b` when `a` is falsy.
    JumpIfFalse,
    /// Jumps to `b` when `a` is truthy.
    JumpIfTrue,
    /// Prints `a`, converted to a string.
    Echo,
    /// Returns `a` (or `null` when unused) from the code object.
    Return,
}

impl Opcode {
    /// The name the disassembler shows.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Nop => "Nop",
            Self::Copy => "Copy",
            Self::Assign => "Assign",
            Self::Add => "Add",
            Self::Sub => "Sub",
            Self::Mul => "Mul",
            Self::Div => "Div",
            Self::Mod => "Mod",
            Self::Pow => "Pow",
            Self::Concat => "Concat",
            Self::ShiftLeft => "ShiftLeft",
            Self::ShiftRight => "ShiftRight",
            Self::BitAnd => "BitAnd",
            Self::BitOr => "BitOr",
            Self::BitXor => "BitXor",
            Self::BoolXor => "BoolXor",
            Self::Equal => "Equal",
            Self::NotEqual => "NotEqual",
            Self::Identical => "Identical",
            Self::NotIdentical => "NotIdentical",
            Self::Less => "Less",
            Self::LessOrEqual => "LessOrEqual",
            Self::Spaceship => "Spaceship",
            Self::BitNot => "BitNot",
            Self::BoolNot => "BoolNot",
            Self::ToBool => "ToBool",
            Self::Jump => "Jump",
            Self::JumpIfFalse => "JumpIfFalse",
            Self::JumpIfTrue => "JumpIfTrue",
            Self::Echo => "Echo",
            Self::Return => "Return",
        }
    }
}

/// One instruction. It is a fixed-size `Copy` value; its line lives in the code object's line
/// table, out of the hot instruction list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Instruction {
    /// What the instruction does.
    pub opcode: Opcode,
    /// Opcode-specific flag bits.
    pub flags: u8,
    /// The first operand.
    pub a: Operand,
    /// The second operand.
    pub b: Operand,
    /// The third operand.
    pub c: Operand,
    /// Where the result goes.
    pub result: Operand,
}

impl Instruction {
    /// An instruction with no flags and every operand unused.
    #[must_use]
    pub const fn new(opcode: Opcode) -> Self {
        Self {
            opcode,
            flags: 0,
            a: Operand::UNUSED,
            b: Operand::UNUSED,
            c: Operand::UNUSED,
            result: Operand::UNUSED,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Instruction, Opcode};
    use crate::bytecode::Operand;

    #[test]
    fn opcodes_are_named_as_declared() {
        let opcodes = [
            Opcode::Nop,
            Opcode::Copy,
            Opcode::Assign,
            Opcode::Add,
            Opcode::Sub,
            Opcode::Mul,
            Opcode::Div,
            Opcode::Mod,
            Opcode::Pow,
            Opcode::Concat,
            Opcode::ShiftLeft,
            Opcode::ShiftRight,
            Opcode::BitAnd,
            Opcode::BitOr,
            Opcode::BitXor,
            Opcode::BoolXor,
            Opcode::Equal,
            Opcode::NotEqual,
            Opcode::Identical,
            Opcode::NotIdentical,
            Opcode::Less,
            Opcode::LessOrEqual,
            Opcode::Spaceship,
            Opcode::BitNot,
            Opcode::BoolNot,
            Opcode::ToBool,
            Opcode::Jump,
            Opcode::JumpIfFalse,
            Opcode::JumpIfTrue,
            Opcode::Echo,
            Opcode::Return,
        ];
        for opcode in opcodes {
            assert_eq!(opcode.name(), format!("{opcode:?}"));
        }
    }

    #[test]
    fn instructions_are_small_and_start_unused() {
        assert!(size_of::<Instruction>() <= 20);
        let instruction = Instruction::new(Opcode::Nop);
        assert_eq!(instruction.flags, 0);
        assert!(
            [
                instruction.a,
                instruction.b,
                instruction.c,
                instruction.result
            ]
            .iter()
            .all(|operand| *operand == Operand::UNUSED)
        );
    }
}
