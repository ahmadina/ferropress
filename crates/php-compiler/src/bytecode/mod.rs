//! The bytecode the compiler produces and the VM runs (ADR-0007).
//!
//! A [`Unit`] holds a file's main [`Code`] and its declarations. Each code object is a flat list of
//! register-style [`Instruction`]s over local slots, temporaries and a literal pool, with lines kept
//! in a parallel table. Its `Display` form is the disassembly tests compare against.

mod code;
mod instruction;
mod literal;
mod operand;

pub use code::{Code, FunctionDecl, Parameter, Unit};
pub use instruction::{Instruction, Opcode};
pub use literal::Literal;
pub use operand::{Operand, OperandKind};
