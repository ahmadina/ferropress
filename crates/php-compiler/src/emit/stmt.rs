//! Statements: control flow, `echo` and `return`.

use php_parser::ast::{Expr, ExprKind, NameKind, Stmt, StmtKind, Try};

use super::builder::Field;
use super::{Emit, Emitter, Loop};
use crate::bytecode::{CatchHandler, Opcode, Operand};

impl Emitter<'_> {
    pub(super) fn stmts(&mut self, stmts: &[Stmt]) -> Emit {
        stmts.iter().try_for_each(|stmt| self.stmt(stmt))
    }

    pub(super) fn stmt(&mut self, stmt: &Stmt) -> Emit {
        self.builder.line = stmt.line;
        match &stmt.kind {
            StmtKind::Nop => Ok(()),
            StmtKind::Block(stmts) => self.stmts(stmts),
            StmtKind::Expr(expr) => self.discard(expr),
            StmtKind::Echo(expr) => {
                let value = self.expr(expr)?;
                self.builder
                    .op(Opcode::Echo, value, Operand::UNUSED, Operand::UNUSED)
                    .map(drop)
            }
            StmtKind::If(branches) => {
                let mut ends = Vec::new();
                for (condition, body) in &branches.branches {
                    let value = self.expr(condition)?;
                    let skip = self.builder.op(
                        Opcode::JumpIfFalse,
                        value,
                        Operand::UNUSED,
                        Operand::UNUSED,
                    )?;
                    self.nested(|emitter| emitter.stmt(body))?;
                    ends.push(self.jump()?);
                    self.builder.patch_here(skip, Field::B)?;
                }
                if let Some(otherwise) = &branches.otherwise {
                    self.nested(|emitter| emitter.stmt(otherwise))?;
                }
                ends.into_iter()
                    .try_for_each(|end| self.builder.patch_here(end, Field::A))
            }
            StmtKind::While { condition, body } => {
                let start = self.builder.here()?;
                let value = self.expr(condition)?;
                let exit = self.builder.op(
                    Opcode::JumpIfFalse,
                    value,
                    Operand::UNUSED,
                    Operand::UNUSED,
                )?;
                self.loop_body(body, start, Some(exit))
            }
            StmtKind::DoWhile { body, condition } => self.do_while(body, condition),
            StmtKind::For(for_loop) => {
                self.discard_all(&for_loop.init)?;
                let start = self.builder.here()?;
                let exit = match for_loop.condition.split_last() {
                    Some((last, rest)) => {
                        self.discard_all(rest)?;
                        let value = self.expr(last)?;
                        Some(self.builder.op(
                            Opcode::JumpIfFalse,
                            value,
                            Operand::UNUSED,
                            Operand::UNUSED,
                        )?)
                    }
                    None => None,
                };
                self.loops.push(Loop::default());
                self.nested(|emitter| emitter.stmt(&for_loop.body))?;
                let step = self.builder.here()?;
                self.discard_all(&for_loop.step)?;
                self.close_loop(start, step, exit)
            }
            StmtKind::Break(depth) => self.break_continue(true, depth.as_ref()),
            StmtKind::Continue(depth) => self.break_continue(false, depth.as_ref()),
            StmtKind::Return(value) => {
                let value = match value {
                    Some(value) => self.expr(value)?,
                    None => Operand::UNUSED,
                };
                self.builder
                    .op(Opcode::Return, value, Operand::UNUSED, Operand::UNUSED)
                    .map(drop)
            }
            StmtKind::Function(function) if self.conditional == 0 => {
                self.function_declaration(function)
            }
            StmtKind::Function(_) => self.unsupported("conditional function declarations"),
            StmtKind::Foreach(_) => self.unsupported("foreach"),
            StmtKind::Switch { .. } => self.unsupported("switch"),
            StmtKind::Global(_) => self.unsupported("global"),
            StmtKind::Static { .. } => self.unsupported("static variables"),
            StmtKind::Unset(_) => self.unsupported("unset"),
            StmtKind::Label(_) | StmtKind::Goto(_) => self.unsupported("goto"),
            StmtKind::Try(parts) => self.try_statement(parts),
            StmtKind::Declare { .. } => self.unsupported("declare"),
            StmtKind::Class(class) if self.conditional == 0 => self.class_declaration(class),
            StmtKind::Class(_) => self.unsupported("conditional class declarations"),
        }
    }

    /// Compiles `body` as the inside of a conditional or loop, where declarations are conditional.
    fn nested(&mut self, body: impl FnOnce(&mut Self) -> Emit) -> Emit {
        self.conditional += 1;
        let result = body(self);
        self.conditional -= 1;
        result
    }

    fn try_statement(&mut self, parts: &Try) -> Emit {
        if parts.finally.is_some() {
            return self.unsupported("finally");
        }
        if parts.catches.is_empty() {
            return self.unsupported("finally");
        }
        let start = self.builder.here()?;
        self.stmts(&parts.body)?;
        let end = self.builder.here()?;
        let mut exits = vec![self.jump()?];
        for catch in &parts.catches {
            self.builder.line = catch.line;
            let target = self.builder.here()?;
            let slot = match &catch.variable {
                Some(name) => Some(self.builder.slot(name)?.index()),
                None => None,
            };
            for name in &catch.types {
                if name.kind == NameKind::Relative {
                    return self.unsupported("namespace-relative names");
                }
                self.builder.code_mut().handlers.push(CatchHandler {
                    start,
                    end,
                    target,
                    class: name.bytes.to_ascii_lowercase().into(),
                    slot,
                });
            }
            self.stmts(&catch.body)?;
            exits.push(self.jump()?);
        }
        exits
            .into_iter()
            .try_for_each(|exit| self.builder.patch_here(exit, Field::A))
    }

    /// Emits a jump to be patched.
    fn jump(&mut self) -> Emit<u32> {
        self.builder.op(
            Opcode::Jump,
            Operand::UNUSED,
            Operand::UNUSED,
            Operand::UNUSED,
        )
    }

    fn discard_all(&mut self, exprs: &[Expr]) -> Emit {
        exprs.iter().try_for_each(|expr| self.discard(expr))
    }

    /// Compiles a `while` body that loops back to `start`, leaving through `exit` when the
    /// condition fails.
    fn loop_body(&mut self, body: &Stmt, start: u32, exit: Option<u32>) -> Emit {
        self.loops.push(Loop::default());
        self.nested(|emitter| emitter.stmt(body))?;
        self.close_loop(start, start, exit)
    }

    /// Ends the innermost loop: jumps back to `start`, and patches its `continue` statements to
    /// `step`, and its `break` statements and `exit` past the loop.
    fn close_loop(&mut self, start: u32, step: u32, exit: Option<u32>) -> Emit {
        let back = self.jump()?;
        self.builder.patch(back, Field::A, start)?;
        let finished = self.loops.pop().unwrap_or_default();
        for jump in finished.continues {
            self.builder.patch(jump, Field::A, step)?;
        }
        for jump in finished.breaks {
            self.builder.patch_here(jump, Field::A)?;
        }
        exit.map_or(Ok(()), |exit| self.builder.patch_here(exit, Field::B))
    }

    fn do_while(&mut self, body: &Stmt, condition: &Expr) -> Emit {
        let start = self.builder.here()?;
        self.loops.push(Loop::default());
        self.nested(|emitter| emitter.stmt(body))?;
        let step = self.builder.here()?;
        let value = self.expr(condition)?;
        let back = self
            .builder
            .op(Opcode::JumpIfTrue, value, Operand::UNUSED, Operand::UNUSED)?;
        self.builder.patch(back, Field::B, start)?;
        let finished = self.loops.pop().unwrap_or_default();
        for jump in finished.continues {
            self.builder.patch(jump, Field::A, step)?;
        }
        finished
            .breaks
            .into_iter()
            .try_for_each(|jump| self.builder.patch_here(jump, Field::A))
    }

    /// `break` or `continue`, leaving `depth` loops (one by default; the checks proved it a
    /// positive literal within the enclosing loops).
    fn break_continue(&mut self, is_break: bool, depth: Option<&Expr>) -> Emit {
        let depth = match depth.map(|depth| &depth.kind) {
            None => 1,
            Some(ExprKind::Int(depth)) => usize::try_from(*depth).unwrap_or(usize::MAX),
            Some(_) => return self.unsupported("computed break depths"),
        };
        let jump = self.jump()?;
        let Some(index) = self.loops.len().checked_sub(depth) else {
            return self.unsupported("break outside the compiled loops");
        };
        if let Some(target) = self.loops.get_mut(index) {
            if is_break {
                target.breaks.push(jump);
            } else {
                target.continues.push(jump);
            }
        }
        Ok(())
    }
}
