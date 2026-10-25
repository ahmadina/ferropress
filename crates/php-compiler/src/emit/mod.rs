//! Emits the bytecode for a program that passed the compile-time checks.
//!
//! Constructs not compiled yet stop with [`Failure::Unsupported`] naming the construct, never with
//! code that behaves differently.

mod builder;
mod expr;
mod stmt;

use php_parser::ast::{Function, FunctionBody, Program, TopStatement};

use crate::bytecode::{FunctionDecl, Literal, Opcode, Operand, Parameter, Unit};
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

/// Emits one code object at a time; functions declared inside compile with their own emitter.
pub(crate) struct Emitter {
    builder: Builder,
    loops: Vec<Loop>,
    /// How many conditionals and loops enclose the statement being compiled.
    conditional: u32,
    functions: Vec<FunctionDecl>,
}

impl Emitter {
    fn new(name: &[u8]) -> Self {
        Self {
            builder: Builder::new(name),
            loops: Vec::new(),
            conditional: 0,
            functions: Vec::new(),
        }
    }

    /// Emits the unit for `program`, which passed the compile-time checks.
    pub(crate) fn run(program: &Program) -> Emit<Unit> {
        let mut emitter = Self::new(b"");
        emitter.builder.line = 1;
        for statement in &program.statements {
            emitter.top_statement(statement)?;
        }
        let one = emitter.builder.literal(Literal::Int(1))?;
        emitter
            .builder
            .op(Opcode::Return, one, Operand::UNUSED, Operand::UNUSED)?;
        Ok(Unit {
            main: emitter.builder.finish(),
            functions: emitter.functions,
        })
    }

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

    /// Compiles the top-level function `function` into the unit's declarations.
    fn function_declaration(&mut self, function: &Function) -> Emit {
        self.builder.line = function.line;
        if function.return_type.is_some() {
            return self.unsupported("return types");
        }
        if !function.attributes.is_empty() {
            return self.unsupported("attributes");
        }
        let mut inner = Self::new(&function.name);
        inner.builder.line = function.line;
        inner.conditional = 1;
        for param in &function.params {
            inner.builder.line = param.line;
            if param.declared_type.is_some() {
                return inner.unsupported("parameter types");
            }
            if param.default.is_some() {
                return inner.unsupported("parameter defaults");
            }
            if !param.attributes.is_empty() {
                return inner.unsupported("attributes");
            }
            let slot = inner.builder.slot(&param.name)?.index();
            inner.builder.code_mut().params.push(Parameter {
                slot,
                by_ref: param.by_ref,
                variadic: param.variadic,
            });
        }
        if function.is_generator {
            return inner.unsupported("generators");
        }
        inner.builder.code_mut().by_ref = function.by_ref;
        match &function.body {
            FunctionBody::Block(stmts) => inner.stmts(stmts)?,
            FunctionBody::Expr(_) | FunctionBody::Absent => {
                return inner.unsupported("functions without a block body");
            }
        }
        inner.builder.op(
            Opcode::Return,
            Operand::UNUSED,
            Operand::UNUSED,
            Operand::UNUSED,
        )?;
        self.functions.append(&mut inner.functions);
        self.functions.push(FunctionDecl {
            name: function.name.as_slice().into(),
            line: function.line,
            code: inner.builder.finish(),
        });
        Ok(())
    }
}
