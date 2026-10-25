//! Named functions and the shared compilation of a function or closure body.

use php_parser::ast::{ClosureUse, Function, FunctionBody, Param};

use super::{Emit, Emitter, Scope};
use crate::bytecode::{FunctionDecl, Opcode, Operand, Parameter};

impl Emitter<'_> {
    /// Compiles the top-level function `function` into the unit's declarations.
    pub(super) fn function_declaration(&mut self, function: &Function) -> Emit {
        self.builder.line = function.line;
        let mut inner = Emitter::child(
            self.ctx,
            &function.name,
            Scope::Function(function.name.as_slice().into()),
        );
        inner.compile_body(function, &[], true).map(drop)
    }

    /// Compiles `function` with `uses` as extra slots. When `exported` the declaration is callable
    /// by name; otherwise it is a closure.
    pub(super) fn compile_body(
        &mut self,
        function: &Function,
        uses: &[ClosureUse],
        exported: bool,
    ) -> Emit<u32> {
        let code = self.compile_code(function, uses)?;
        let index = u32::try_from(self.ctx.functions.len()).map_err(|_overflow| {
            super::Failure::TooLarge {
                line: self.builder.line,
            }
        })?;
        let name: Box<[u8]> = if exported {
            function.name.as_slice().into()
        } else {
            Box::from([])
        };
        self.ctx.functions.push(FunctionDecl {
            name,
            line: function.line,
            code,
        });
        Ok(index)
    }

    /// Compiles `function` to a code object without recording a named declaration.
    pub(super) fn compile_code(
        &mut self,
        function: &Function,
        uses: &[ClosureUse],
    ) -> Emit<crate::bytecode::Code> {
        self.builder.line = function.line;
        self.reject_signature(function)?;
        if matches!(self.scope, Scope::Method { .. }) {
            self.builder.slot(b"this")?;
        }
        self.bind_params(&function.params)?;
        for use_var in uses {
            self.builder.slot(&use_var.name)?;
        }
        if function.is_generator {
            return self.unsupported("generators");
        }
        self.emit_body(function)?;
        let builder = std::mem::replace(&mut self.builder, super::builder::Builder::new(&[]));
        Ok(builder.finish())
    }

    fn reject_signature(&self, function: &Function) -> Emit {
        if function.by_ref {
            return self.unsupported("functions returning by reference");
        }
        if function.return_type.is_some() {
            return self.unsupported("return types");
        }
        if !function.attributes.is_empty() {
            return self.unsupported("attributes");
        }
        Ok(())
    }

    fn bind_params(&mut self, params: &[Param]) -> Emit {
        for param in params {
            self.builder.line = param.line;
            if param.declared_type.is_some() {
                return self.unsupported("parameter types");
            }
            if param.default.is_some() {
                return self.unsupported("parameter defaults");
            }
            if !param.attributes.is_empty() {
                return self.unsupported("attributes");
            }
            if param.by_ref && param.variadic {
                return self.unsupported("by-reference variadic parameters");
            }
            let slot = self.builder.slot(&param.name)?.index();
            self.builder.code_mut().params.push(Parameter {
                slot,
                by_ref: param.by_ref,
                variadic: param.variadic,
                line: param.line,
            });
        }
        Ok(())
    }

    fn emit_body(&mut self, function: &Function) -> Emit {
        match &function.body {
            FunctionBody::Block(stmts) => {
                self.stmts(stmts)?;
                self.builder
                    .op(
                        Opcode::Return,
                        Operand::UNUSED,
                        Operand::UNUSED,
                        Operand::UNUSED,
                    )
                    .map(drop)
            }
            FunctionBody::Expr(expr) => {
                let value = self.expr(expr)?;
                self.builder
                    .op(Opcode::Return, value, Operand::UNUSED, Operand::UNUSED)
                    .map(drop)
            }
            FunctionBody::Absent => self.unsupported("functions without a block body"),
        }
    }
}
