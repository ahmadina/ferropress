//! Emits the bytecode for a program that passed the compile-time checks.
//!
//! Constructs not compiled yet stop with [`Failure::Unsupported`] naming the construct, never with
//! code that behaves differently.

mod builder;
mod class;
mod closure;
mod expr;
mod free;
mod function;
mod stmt;

use php_parser::ast::{Program, TopStatement};

use crate::bytecode::{Literal, Opcode, Operand, Unit};
use builder::Builder;

/// Why emitting stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Failure {
    /// The program uses a construct the compiler does not compile yet.
    #[error("compiling {construct} is not supported yet (line {line})")]
    Unsupported {
        /// What the construct is.
        construct: &'static str,
        /// Its line.
        line: u32,
    },
    /// A code object needs more slots, literals, temporaries or instructions than operands hold.
    #[error("the code at line {line} exceeds the bytecode's operand limit")]
    TooLarge {
        /// The line being compiled.
        line: u32,
    },
}

pub(crate) type Emit<T = ()> = Result<T, Failure>;

/// The loop being compiled, with the jumps its `break` and `continue` statements left to patch.
#[derive(Debug, Default)]
struct Loop {
    breaks: Vec<u32>,
    continues: Vec<u32>,
}

/// Shared declarations and the file path used in generated closure names.
struct EmitCtx {
    functions: Vec<crate::bytecode::FunctionDecl>,
    classes: Vec<crate::bytecode::ClassDecl>,
    file: Box<[u8]>,
    /// Methods of the class currently being compiled, so `new` sees `__construct`.
    current_methods: Vec<Box<[u8]>>,
}

/// The code object being compiled, which names nested closures.
enum Scope {
    File,
    Function(Box<[u8]>),
    Closure(Box<[u8]>),
    Method { class: Box<[u8]>, name: Box<[u8]> },
}

/// Emits one code object at a time; nested functions and closures share [`EmitCtx`].
pub(crate) struct Emitter<'a> {
    builder: Builder,
    loops: Vec<Loop>,
    /// How many conditionals and loops enclose the statement being compiled.
    conditional: u32,
    ctx: &'a mut EmitCtx,
    scope: Scope,
}

impl<'a> Emitter<'a> {
    fn new(ctx: &'a mut EmitCtx, name: &[u8], scope: Scope) -> Self {
        Self {
            builder: Builder::new(name),
            loops: Vec::new(),
            conditional: 0,
            ctx,
            scope,
        }
    }

    /// A nested function or closure, whose declarations are conditional.
    fn child(ctx: &'a mut EmitCtx, name: &[u8], scope: Scope) -> Self {
        let mut emitter = Self::new(ctx, name, scope);
        emitter.conditional = 1;
        emitter
    }
}

/// Emits the unit for `program`, compiled from the file at path `file`.
pub(crate) fn run(program: &Program, file: &[u8]) -> Emit<Unit> {
    let mut ctx = EmitCtx {
        functions: Vec::new(),
        classes: Vec::new(),
        file: file.into(),
        current_methods: Vec::new(),
    };
    let main = emit_main(&mut ctx, program)?;
    Ok(Unit {
        main,
        functions: ctx.functions,
        classes: ctx.classes,
    })
}

fn emit_main(ctx: &mut EmitCtx, program: &Program) -> Emit<crate::bytecode::Code> {
    let mut emitter = Emitter::new(ctx, b"", Scope::File);
    emitter.builder.line = 1;
    for statement in &program.statements {
        emitter.top_statement(statement)?;
    }
    let one = emitter.builder.literal(Literal::Int(1))?;
    emitter
        .builder
        .op(Opcode::Return, one, Operand::UNUSED, Operand::UNUSED)?;
    Ok(emitter.builder.finish())
}

impl Emitter<'_> {
    fn unsupported<T>(&self, construct: &'static str) -> Emit<T> {
        Err(Failure::Unsupported {
            construct,
            line: self.builder.line,
        })
    }

    fn top_statement(&mut self, statement: &TopStatement) -> Emit {
        match statement {
            TopStatement::Statement(stmt) => self.stmt(stmt),
            TopStatement::HaltCompiler { .. } => Ok(()),
            TopStatement::Namespace { line, .. } => {
                self.builder.line = *line;
                self.unsupported("namespaces")
            }
            TopStatement::Use { line, .. } | TopStatement::GroupUse { line, .. } => {
                self.builder.line = *line;
                self.unsupported("use imports")
            }
            TopStatement::Const { line, .. } => {
                self.builder.line = *line;
                self.unsupported("constant declarations")
            }
        }
    }
}

#[cfg(test)]
impl EmitCtx {
    fn for_test() -> Self {
        Self {
            functions: Vec::new(),
            file: Box::from(&b"/t.php"[..]),
            classes: Vec::new(),
            current_methods: Vec::new(),
        }
    }
}
