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
    /// Jumps to `c` when `a < b` is false.
    JumpIfNotLess,
    /// Jumps to `c` when `a <= b` is false.
    JumpIfNotLessEqual,
    /// Increments the variable `a` and jumps to `b`.
    IncJump,
    /// `a = a + b`; `result`, when used, receives the sum.
    AddAssign,
    /// `a = a - b`; `result`, when used, receives the difference.
    SubAssign,
    /// `a = a * b`; `result`, when used, receives the product.
    MulAssign,
    /// `a = a % b`; `result`, when used, receives the remainder.
    ModAssign,
    /// Converts `a` to a string.
    ToString,
    /// Starts a call of the function named by the literal `a` (lowercase); `b`, when used, is the
    /// global name tried when no function `a` exists; `c` is the name as written, for messages.
    InitCall,
    /// Starts a call of the value `a`: a `Closure` object, or a string naming a function.
    InitCallValue,
    /// `result` is a new `Closure` of the function declaration `a` (its index in the unit).
    MakeClosure,
    /// Binds the variable `b` into the closure `a` as that closure's slot `c`; flag bit 0 is set
    /// when the binding is by reference.
    Capture,
    /// Passes `a` by value as the next argument of the call being prepared. Flag bit 0 is set when
    /// `a` is a call's result, which a by-reference parameter accepts with a notice.
    Send,
    /// Passes the variable `a` as the next argument: by reference when the parameter takes one,
    /// by value otherwise.
    SendVar,
    /// Calls the function being prepared; `result`, when used, receives its return value.
    Call,
    /// Calls the named function `a` (as written). Bits 0–1 of `flags` are the argument
    /// count (0..=2) in `b` and `c`; bits 2–3 mark those arguments as a call's result.
    Invoke,
    /// `result = strlen(a)`.
    Strlen,
    /// `result = strpos(a, b)` with no offset.
    Strpos,
    /// `result = str_contains(a, b)`.
    StrContains,
    /// `result = count(a)`.
    Count,
    /// Adds one to the variable `a`; `result`, when used, receives the new value.
    PreIncrement,
    /// Subtracts one from the variable `a`; `result`, when used, receives the new value.
    PreDecrement,
    /// Adds one to the variable `a`; `result` receives the value before.
    PostIncrement,
    /// Subtracts one from the variable `a`; `result` receives the value before.
    PostDecrement,
    /// Prints `a`, converted to a string.
    Echo,
    /// Returns `a` (or `null` when unused) from the code object.
    Return,
    /// `result` is a new object of the class named by the literal `a`.
    New,
    /// `result = a->b`, where `b` is the property name.
    LoadProp,
    /// `a->b = c`.
    StoreProp,
    /// Starts a call of method `b` (lowercase) on the object `a`; `c` is the name as written.
    /// Flag bit 0 is set for the implicit constructor of a class that declares none.
    InitMethodCall,
    /// Throws the object `a`.
    Throw,
    /// Includes the file named by `a`. Flags: 0 `include`, 1 `include_once`, 2 `require`,
    /// 3 `require_once`. `result` receives the included file's return value.
    Include,
    /// Ends a `finally` body: rethrows or returns what the `try`/`catch` left pending.
    EndFinally,
    /// `result = a instanceof b`, where `b` is the class name.
    InstanceOf,
    /// `result = a::b`, an enum case named `b` of the class named by the literal `a`.
    ClassConst,
    /// Suspends a generator: `a` is the yielded value, `b` the key when used; `result` is the
    /// value later sent into the `yield`.
    Yield,
    /// Starts a static call of method `b` (lowercase) on the class named by `a`; `c` is the name
    /// as written.
    InitStaticCall,
    /// Registers the class at unit index `a` that was compiled as deferred.
    BindClass,
    /// `result` is a new empty array.
    NewArray,
    /// Stores `b` in the array `a`: appended when `c` is unused, otherwise under the key `c`.
    ArrayItem,
    /// `result = a[b]`.
    FetchDim,
    /// `a[b] = c`; `b` unused appends. `a` is the variable written back.
    StoreDim,
    /// `result` is whether `a` has an element at or after cursor `b`; `b` becomes that position.
    IterReady,
    /// `result = a[b]` as a foreach pick; flag bit 0 yields the key instead of the value.
    IterPick,
    /// `result` is whether `a` is set and not `null`. Flag bit 0 asks whether `a` is empty instead.
    Isset,
    /// `result` is whether `a[b]` is set and not `null`. Flag bit 0 asks whether `a[b]` is empty.
    IssetDim,
    /// `result` is the global constant named by the string `a`.
    FetchConst,
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
            Self::JumpIfNotLess => "JumpIfNotLess",
            Self::JumpIfNotLessEqual => "JumpIfNotLessEqual",
            Self::IncJump => "IncJump",
            Self::AddAssign => "AddAssign",
            Self::SubAssign => "SubAssign",
            Self::MulAssign => "MulAssign",
            Self::ModAssign => "ModAssign",
            Self::ToString => "ToString",
            Self::InitCall => "InitCall",
            Self::InitCallValue => "InitCallValue",
            Self::MakeClosure => "MakeClosure",
            Self::Capture => "Capture",
            Self::Send => "Send",
            Self::SendVar => "SendVar",
            Self::Call => "Call",
            Self::Invoke => "Invoke",
            Self::Strlen => "Strlen",
            Self::Strpos => "Strpos",
            Self::StrContains => "StrContains",
            Self::Count => "Count",
            Self::PreIncrement => "PreIncrement",
            Self::PreDecrement => "PreDecrement",
            Self::PostIncrement => "PostIncrement",
            Self::PostDecrement => "PostDecrement",
            Self::Echo => "Echo",
            Self::Return => "Return",
            Self::New => "New",
            Self::LoadProp => "LoadProp",
            Self::StoreProp => "StoreProp",
            Self::InitMethodCall => "InitMethodCall",
            Self::Throw => "Throw",
            Self::Include => "Include",
            Self::EndFinally => "EndFinally",
            Self::InstanceOf => "InstanceOf",
            Self::ClassConst => "ClassConst",
            Self::Yield => "Yield",
            Self::InitStaticCall => "InitStaticCall",
            Self::BindClass => "BindClass",
            Self::NewArray => "NewArray",
            Self::ArrayItem => "ArrayItem",
            Self::FetchDim => "FetchDim",
            Self::StoreDim => "StoreDim",
            Self::IterReady => "IterReady",
            Self::IterPick => "IterPick",
            Self::Isset => "Isset",
            Self::IssetDim => "IssetDim",
            Self::FetchConst => "FetchConst",
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
            Opcode::JumpIfNotLess,
            Opcode::JumpIfNotLessEqual,
            Opcode::IncJump,
            Opcode::AddAssign,
            Opcode::SubAssign,
            Opcode::MulAssign,
            Opcode::ModAssign,
            Opcode::ToString,
            Opcode::InitCall,
            Opcode::InitCallValue,
            Opcode::MakeClosure,
            Opcode::Capture,
            Opcode::Send,
            Opcode::SendVar,
            Opcode::Call,
            Opcode::Invoke,
            Opcode::Strlen,
            Opcode::Strpos,
            Opcode::StrContains,
            Opcode::Count,
            Opcode::PreIncrement,
            Opcode::PreDecrement,
            Opcode::PostIncrement,
            Opcode::PostDecrement,
            Opcode::Echo,
            Opcode::Return,
            Opcode::New,
            Opcode::LoadProp,
            Opcode::StoreProp,
            Opcode::InitMethodCall,
            Opcode::Throw,
            Opcode::Include,
            Opcode::EndFinally,
            Opcode::InstanceOf,
            Opcode::ClassConst,
            Opcode::Yield,
            Opcode::InitStaticCall,
            Opcode::BindClass,
            Opcode::NewArray,
            Opcode::ArrayItem,
            Opcode::FetchDim,
            Opcode::StoreDim,
            Opcode::IterReady,
            Opcode::IterPick,
            Opcode::Isset,
            Opcode::IssetDim,
            Opcode::FetchConst,
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
