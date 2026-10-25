//! Closures and arrow functions: a code object plus the captures that fill it.

use std::collections::HashSet;

use php_parser::ast::{ClosureUse, Function, FunctionBody};

use super::free::{FreeVars, walk_expr};
use super::{Emit, Emitter, Scope};
use crate::bytecode::{Instruction, Literal, Opcode, Operand};

impl Emitter<'_> {
    /// Compiles a `function (…) use (…) { … }` expression.
    pub(super) fn closure(&mut self, function: &Function, needed: bool) -> Emit<Operand> {
        self.emit_callable(function, &function.uses, needed)
    }

    /// Compiles `fn (…) => expr`, capturing the free variables of the body by value.
    pub(super) fn arrow(&mut self, function: &Function, needed: bool) -> Emit<Operand> {
        let captured = arrow_uses(function);
        self.emit_callable(function, &captured, needed)
    }

    fn emit_callable(
        &mut self,
        function: &Function,
        captured: &[ClosureUse],
        needed: bool,
    ) -> Emit<Operand> {
        self.builder.line = function.line;
        let name = self.closure_name(function.line);
        let index = {
            let mut inner = Emitter::child(self.ctx, &name, Scope::Closure(name.clone().into()));
            inner.compile_body(function, captured, false)?
        };
        let dest = if needed || !captured.is_empty() {
            self.builder.temp()?
        } else {
            Operand::UNUSED
        };
        let index = self.builder.literal(Literal::Int(i64::from(index)))?;
        self.builder
            .op(Opcode::MakeClosure, index, Operand::UNUSED, dest)?;
        self.bind_uses(dest, captured)?;
        Ok(if needed { dest } else { Operand::UNUSED })
    }

    fn bind_uses(&mut self, closure: Operand, captured: &[ClosureUse]) -> Emit {
        if captured.is_empty() {
            return Ok(());
        }
        let slots = self
            .ctx
            .functions
            .last()
            .map(|decl| decl.code.slots.clone())
            .unwrap_or_default();
        for use_var in captured {
            let Some(inner) = slots
                .iter()
                .position(|slot| &**slot == use_var.name.as_slice())
            else {
                return Err(super::Failure::TooLarge {
                    line: self.builder.line,
                });
            };
            let inner = u32::try_from(inner).map_err(|_overflow| super::Failure::TooLarge {
                line: self.builder.line,
            })?;
            let outer = self.builder.slot(&use_var.name)?;
            let slot = self.builder.literal(Literal::Int(i64::from(inner)))?;
            let mut capture = Instruction::new(Opcode::Capture);
            capture.a = closure;
            capture.b = outer;
            capture.c = slot;
            capture.flags = u8::from(use_var.by_ref);
            self.builder.emit(capture)?;
        }
        Ok(())
    }

    /// `{closure:PARENT:LINE}` as the reference names a closure in this scope.
    fn closure_name(&self, line: u32) -> Vec<u8> {
        let mut text = b"{closure:".to_vec();
        match &self.scope {
            Scope::File => text.extend_from_slice(&self.ctx.file),
            Scope::Function(name) => {
                text.extend_from_slice(name);
                text.extend_from_slice(b"()");
            }
            Scope::Method { class, name } => {
                text.extend_from_slice(class);
                text.extend_from_slice(b"::");
                text.extend_from_slice(name);
                text.extend_from_slice(b"()");
            }
            Scope::Closure(name) => text.extend_from_slice(name),
        }
        text.push(b':');
        text.extend_from_slice(line.to_string().as_bytes());
        text.push(b'}');
        text
    }
}

/// The `use` list an arrow function implies: free variables of the body, by value.
fn arrow_uses(function: &Function) -> Vec<ClosureUse> {
    let mut bound = HashSet::new();
    for param in &function.params {
        bound.insert(param.name.clone());
    }
    bound.insert(b"this".to_vec());
    let mut free = FreeVars::default();
    for param in &function.params {
        if let Some(default) = &param.default {
            walk_expr(default, &bound, &mut free);
        }
    }
    if let FunctionBody::Expr(body) = &function.body {
        walk_expr(body, &bound, &mut free);
    }
    free.names
        .into_iter()
        .map(|name| ClosureUse {
            name,
            by_ref: false,
        })
        .collect()
}
