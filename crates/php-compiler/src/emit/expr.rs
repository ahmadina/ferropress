//! Expressions: each compiles to the operand holding its value.
//!
//! A local variable compiles to its slot, read by the instruction that uses it, so an operand
//! changed by a later operand of the same instruction is read after the change.

use php_parser::ast::{
    Arg, Args, AssignOp, BinaryOp, Expr, ExprKind, IncDecOp, IncludeKind, Name, NameKind,
    StringPart, UnaryOp, VarName,
};

use super::{Emit, Emitter};
use crate::bytecode::{Instruction, Literal, Opcode, Operand};

/// The variables every scope shares, which the compiler does not give slots.
const SUPERGLOBALS: [&[u8]; 9] = [
    b"GLOBALS",
    b"_GET",
    b"_POST",
    b"_COOKIE",
    b"_SERVER",
    b"_ENV",
    b"_REQUEST",
    b"_FILES",
    b"_SESSION",
];

/// One part of an interpolated string after adjacent literal parts merge.
enum Element<'a> {
    Text(Vec<u8>),
    Expr(&'a Expr),
}

/// The opcode computing `left op right`, with whether the operands go in swapped.
fn binary_opcode(op: BinaryOp) -> Option<(Opcode, bool)> {
    let opcode = match op {
        BinaryOp::Add => Opcode::Add,
        BinaryOp::Sub => Opcode::Sub,
        BinaryOp::Mul => Opcode::Mul,
        BinaryOp::Div => Opcode::Div,
        BinaryOp::Mod => Opcode::Mod,
        BinaryOp::Pow => Opcode::Pow,
        BinaryOp::Concat => Opcode::Concat,
        BinaryOp::ShiftLeft => Opcode::ShiftLeft,
        BinaryOp::ShiftRight => Opcode::ShiftRight,
        BinaryOp::BitOr => Opcode::BitOr,
        BinaryOp::BitAnd => Opcode::BitAnd,
        BinaryOp::BitXor => Opcode::BitXor,
        BinaryOp::Xor => Opcode::BoolXor,
        BinaryOp::Equal => Opcode::Equal,
        BinaryOp::NotEqual => Opcode::NotEqual,
        BinaryOp::Identical => Opcode::Identical,
        BinaryOp::NotIdentical => Opcode::NotIdentical,
        BinaryOp::Less => Opcode::Less,
        BinaryOp::LessOrEqual => Opcode::LessOrEqual,
        BinaryOp::Greater => return Some((Opcode::Less, true)),
        BinaryOp::GreaterOrEqual => return Some((Opcode::LessOrEqual, true)),
        BinaryOp::Spaceship => Opcode::Spaceship,
        BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce | BinaryOp::Pipe => return None,
    };
    Some((opcode, false))
}

/// The opcode of a compound assignment, `None` for `??=`.
fn assign_opcode(op: AssignOp) -> Option<Opcode> {
    Some(match op {
        AssignOp::Add => Opcode::Add,
        AssignOp::Sub => Opcode::Sub,
        AssignOp::Mul => Opcode::Mul,
        AssignOp::Div => Opcode::Div,
        AssignOp::Mod => Opcode::Mod,
        AssignOp::Pow => Opcode::Pow,
        AssignOp::Concat => Opcode::Concat,
        AssignOp::ShiftLeft => Opcode::ShiftLeft,
        AssignOp::ShiftRight => Opcode::ShiftRight,
        AssignOp::BitOr => Opcode::BitOr,
        AssignOp::BitAnd => Opcode::BitAnd,
        AssignOp::BitXor => Opcode::BitXor,
        AssignOp::Coalesce => return None,
    })
}

/// `true`, `false` or `null` named by a constant, in any case.
pub(crate) fn keyword_constant(name: &Name) -> Option<Literal> {
    if name.kind == NameKind::Relative {
        return None;
    }
    let bytes = name.bytes.as_slice();
    if bytes.eq_ignore_ascii_case(b"true") {
        Some(Literal::Bool(true))
    } else if bytes.eq_ignore_ascii_case(b"false") {
        Some(Literal::Bool(false))
    } else if bytes.eq_ignore_ascii_case(b"null") {
        Some(Literal::Null)
    } else {
        None
    }
}

impl Emitter<'_> {
    /// Compiles `expr` for its value.
    pub(super) fn expr(&mut self, expr: &Expr) -> Emit<Operand> {
        let line = self.builder.line;
        self.builder.line = expr.line;
        let operand = self.value(expr, true);
        self.builder.line = line;
        operand
    }

    /// Compiles `expr` for its side effects only.
    pub(super) fn discard(&mut self, expr: &Expr) -> Emit {
        let line = self.builder.line;
        self.builder.line = expr.line;
        let done = match &expr.kind {
            ExprKind::Variable(name) => self.variable(name).map(drop),
            _ => self.value(expr, false).map(drop),
        };
        self.builder.line = line;
        done
    }

    /// Compiles `expr`; `used` tells whether its value is needed.
    fn value(&mut self, expr: &Expr, used: bool) -> Emit<Operand> {
        match &expr.kind {
            ExprKind::Int(value) => self.builder.literal(Literal::Int(*value)),
            ExprKind::Float(value) => self.builder.literal(Literal::Float(*value)),
            ExprKind::String(bytes) => self.builder.string(bytes),
            ExprKind::Constant(name) => match keyword_constant(name) {
                Some(value) => self.builder.literal(value),
                None => self.unsupported("constants"),
            },
            ExprKind::Variable(name) => self.variable(name),
            ExprKind::Interpolated(parts) => self.interpolated(parts),
            ExprKind::Assign { target, value } => self.assign(target, value, used),
            ExprKind::CompoundAssign { op, target, value } => {
                let Some(opcode) = assign_opcode(*op) else {
                    return self.unsupported("??=");
                };
                let slot = self.target(target)?;
                let value = self.expr(value)?;
                let result = self.builder.temp()?;
                self.builder.op(opcode, slot, value, result)?;
                self.builder
                    .op(Opcode::Assign, slot, result, Operand::UNUSED)?;
                Ok(result)
            }
            ExprKind::IncDec { op, operand } => self.inc_dec(*op, operand, used),
            ExprKind::Binary { op, left, right } => self.binary(*op, left, right),
            ExprKind::Unary { op, operand } => self.unary(*op, operand),
            ExprKind::Ternary {
                condition,
                then,
                otherwise,
                ..
            } => self.ternary(condition, then.as_deref(), otherwise),
            ExprKind::Print(value) => {
                let value = self.expr(value)?;
                self.builder
                    .op(Opcode::Echo, value, Operand::UNUSED, Operand::UNUSED)?;
                self.builder.literal(Literal::Int(1))
            }
            ExprKind::Call { callee, args } => self.call(callee, args, used),
            ExprKind::Closure(function) => self.closure(function, used),
            ExprKind::ArrowFunction { function, .. } => self.arrow(function, used),
            ExprKind::New { class, args } => self.new_object(class, args, used),
            ExprKind::NewAnonymous { .. } => self.unsupported("anonymous classes"),
            ExprKind::Instanceof { expr, class } => self.instanceof(expr, class),
            ExprKind::ClassConstant { class, name } => self.class_const(class, name),
            ExprKind::Throw(value) => self.throw_value(value),
            ExprKind::Yield { key, value } => {
                self.yield_expr(key.as_deref(), value.as_deref(), used)
            }
            ExprKind::YieldFrom(_) => self.unsupported("yield from"),
            ExprKind::Include { kind, expr } => self.include_file(*kind, expr, used),
            ExprKind::Property {
                object,
                name,
                nullsafe,
            } => {
                if *nullsafe {
                    return self.unsupported("nullsafe operators");
                }
                self.property(object, name, used)
            }
            ExprKind::MethodCall {
                object,
                name,
                args,
                nullsafe,
                ..
            } => {
                if *nullsafe {
                    return self.unsupported("nullsafe operators");
                }
                self.method_call(object, name, args, used)
            }
            ExprKind::StaticCall { class, name, args } => self.static_call(class, name, args, used),
            _ => self.unsupported("this expression"),
        }
    }

    fn assign(&mut self, target: &Expr, value: &Expr, used: bool) -> Emit<Operand> {
        if let ExprKind::Property {
            object,
            name,
            nullsafe,
        } = &target.kind
        {
            if *nullsafe {
                return self.unsupported("nullsafe operators");
            }
            let value = self.expr(value)?;
            return self.store_property(object, name, value, used);
        }
        let slot = self.target(target)?;
        let value = self.expr(value)?;
        let result = self.result(used)?;
        self.builder.op(Opcode::Assign, slot, value, result)?;
        Ok(result)
    }

    /// A temporary for the result when `used`, otherwise no operand.
    pub(crate) fn result(&mut self, used: bool) -> Emit<Operand> {
        if used {
            self.builder.temp()
        } else {
            Ok(Operand::UNUSED)
        }
    }

    /// The slot of the local variable `name`.
    fn variable(&mut self, name: &VarName) -> Emit<Operand> {
        match name {
            VarName::Name(name) if name.as_slice() == b"this" => {
                if matches!(self.scope, super::Scope::Method { .. }) {
                    self.builder.slot(name)
                } else {
                    self.unsupported("$this")
                }
            }
            VarName::Name(name) if SUPERGLOBALS.contains(&name.as_slice()) => {
                self.unsupported("superglobals")
            }
            VarName::Name(name) => self.builder.slot(name),
            VarName::Expr(_) => self.unsupported("variable variables"),
        }
    }

    /// The slot an assignment writes.
    fn target(&mut self, target: &Expr) -> Emit<Operand> {
        match &target.kind {
            ExprKind::Variable(name) => self.variable(name),
            _ => self.unsupported("assignments to this target"),
        }
    }

    fn inc_dec(&mut self, op: IncDecOp, operand: &Expr, used: bool) -> Emit<Operand> {
        let slot = self.target(operand)?;
        let opcode = match (op, used) {
            (IncDecOp::PreInc, _) | (IncDecOp::PostInc, false) => Opcode::PreIncrement,
            (IncDecOp::PreDec, _) | (IncDecOp::PostDec, false) => Opcode::PreDecrement,
            (IncDecOp::PostInc, true) => Opcode::PostIncrement,
            (IncDecOp::PostDec, true) => Opcode::PostDecrement,
        };
        let result = self.result(used)?;
        self.builder.op(opcode, slot, Operand::UNUSED, result)?;
        Ok(result)
    }

    /// A string with embedded expressions. Adjacent literal parts merge and empty ones drop out.
    /// One remaining part converts to a string; two concatenate, both read when they join; from
    /// three on, each part converts right after it is evaluated, before the next part is.
    fn interpolated(&mut self, parts: &[StringPart]) -> Emit<Operand> {
        let mut elements: Vec<Element<'_>> = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                StringPart::Literal(bytes) if bytes.is_empty() => {}
                StringPart::Literal(bytes) => match elements.last_mut() {
                    Some(Element::Text(text)) => text.extend_from_slice(bytes),
                    _ => elements.push(Element::Text(bytes.clone())),
                },
                StringPart::Expr(expr)
                | StringPart::BracedName(expr)
                | StringPart::BracedExpr(expr) => elements.push(Element::Expr(expr)),
            }
        }
        match elements.as_slice() {
            [] => self.builder.string(b""),
            [Element::Text(text)] => self.builder.string(text),
            [Element::Expr(expr)] => {
                let value = self.expr(expr)?;
                let result = self.builder.temp()?;
                self.builder
                    .op(Opcode::ToString, value, Operand::UNUSED, result)?;
                Ok(result)
            }
            [first, second] => {
                let left = self.element(first)?;
                let right = self.element(second)?;
                let result = self.builder.temp()?;
                self.builder.op(Opcode::Concat, left, right, result)?;
                Ok(result)
            }
            [first, rest @ ..] => {
                let value = self.element(first)?;
                let result = self.builder.temp()?;
                self.builder
                    .op(Opcode::ToString, value, Operand::UNUSED, result)?;
                for element in rest {
                    let value = self.element(element)?;
                    self.builder.op(Opcode::Concat, result, value, result)?;
                }
                Ok(result)
            }
        }
    }

    fn element(&mut self, element: &Element<'_>) -> Emit<Operand> {
        match element {
            Element::Text(text) => self.builder.string(text),
            Element::Expr(expr) => self.expr(expr),
        }
    }

    fn binary(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> Emit<Operand> {
        if let Some((opcode, swapped)) = binary_opcode(op) {
            let left = self.expr(left)?;
            let right = self.expr(right)?;
            let result = self.builder.temp()?;
            let (a, b) = if swapped {
                (right, left)
            } else {
                (left, right)
            };
            self.builder.op(opcode, a, b, result)?;
            return Ok(result);
        }
        let jump = match op {
            BinaryOp::And => Opcode::JumpIfFalse,
            BinaryOp::Or => Opcode::JumpIfTrue,
            BinaryOp::Coalesce => return self.unsupported("??"),
            _ => return self.unsupported("|>"),
        };
        let result = self.builder.temp()?;
        let value = self.expr(left)?;
        self.builder
            .op(Opcode::ToBool, value, Operand::UNUSED, result)?;
        let skip = self
            .builder
            .op(jump, result, Operand::UNUSED, Operand::UNUSED)?;
        let value = self.expr(right)?;
        self.builder
            .op(Opcode::ToBool, value, Operand::UNUSED, result)?;
        self.builder.patch_here(skip, super::builder::Field::B)?;
        Ok(result)
    }

    fn unary(&mut self, op: UnaryOp, operand: &Expr) -> Emit<Operand> {
        let value = self.expr(operand)?;
        let result = self.builder.temp()?;
        match op {
            UnaryOp::Plus | UnaryOp::Minus => {
                let sign =
                    self.builder
                        .literal(Literal::Int(if op == UnaryOp::Plus { 1 } else { -1 }))?;
                self.builder.op(Opcode::Mul, value, sign, result)?;
            }
            UnaryOp::Not => {
                self.builder
                    .op(Opcode::BoolNot, value, Operand::UNUSED, result)?;
            }
            UnaryOp::BitNot => {
                self.builder
                    .op(Opcode::BitNot, value, Operand::UNUSED, result)?;
            }
            UnaryOp::Silence => return self.unsupported("@"),
        }
        Ok(result)
    }

    /// `condition ? then : otherwise`, or `condition ?: otherwise` without `then`.
    fn ternary(
        &mut self,
        condition: &Expr,
        then: Option<&Expr>,
        otherwise: &Expr,
    ) -> Emit<Operand> {
        use super::builder::Field;
        let result = self.builder.temp()?;
        let value = self.expr(condition)?;
        let end = if let Some(then) = then {
            let skip =
                self.builder
                    .op(Opcode::JumpIfFalse, value, Operand::UNUSED, Operand::UNUSED)?;
            let then = self.expr(then)?;
            self.builder
                .op(Opcode::Copy, then, Operand::UNUSED, result)?;
            let end = self.builder.op(
                Opcode::Jump,
                Operand::UNUSED,
                Operand::UNUSED,
                Operand::UNUSED,
            )?;
            self.builder.patch_here(skip, Field::B)?;
            (end, Field::A)
        } else {
            self.builder
                .op(Opcode::Copy, value, Operand::UNUSED, result)?;
            let end =
                self.builder
                    .op(Opcode::JumpIfTrue, result, Operand::UNUSED, Operand::UNUSED)?;
            (end, Field::B)
        };
        let otherwise = self.expr(otherwise)?;
        self.builder
            .op(Opcode::Copy, otherwise, Operand::UNUSED, result)?;
        self.builder.patch_here(end.0, end.1)?;
        Ok(result)
    }

    /// A call of a named function or of a computed callee, with positional arguments.
    fn call(&mut self, callee: &Expr, args: &Args, used: bool) -> Emit<Operand> {
        let Args::List(args) = args else {
            return self.unsupported("first-class callables");
        };
        if let ExprKind::Name(name) = &callee.kind {
            if name.kind == NameKind::Relative {
                return self.unsupported("namespace-relative names");
            }
            let function = self.builder.string(&name.bytes.to_ascii_lowercase())?;
            let written = self.builder.string(&name.bytes)?;
            let mut init = Instruction::new(Opcode::InitCall);
            init.a = function;
            init.c = written;
            self.builder.emit(init)?;
        } else {
            let value = self.expr(callee)?;
            self.builder.op(
                Opcode::InitCallValue,
                value,
                Operand::UNUSED,
                Operand::UNUSED,
            )?;
        }
        self.send_args(args)?;
        let result = self.result(used)?;
        self.builder
            .op(Opcode::Call, Operand::UNUSED, Operand::UNUSED, result)?;
        Ok(result)
    }

    fn include_file(&mut self, kind: IncludeKind, path: &Expr, used: bool) -> Emit<Operand> {
        if kind == IncludeKind::Eval {
            return self.unsupported("eval");
        }
        let path = self.expr(path)?;
        let dest = self.result(used)?;
        let mut ins = Instruction::new(Opcode::Include);
        ins.a = path;
        ins.result = dest;
        ins.flags = match kind {
            IncludeKind::Include | IncludeKind::Eval => 0,
            IncludeKind::IncludeOnce => 1,
            IncludeKind::Require => 2,
            IncludeKind::RequireOnce => 3,
        };
        self.builder.emit(ins)?;
        Ok(dest)
    }

    fn throw_value(&mut self, value: &Expr) -> Emit<Operand> {
        let value = self.expr(value)?;
        self.builder
            .op(Opcode::Throw, value, Operand::UNUSED, Operand::UNUSED)?;
        Ok(Operand::UNUSED)
    }

    fn yield_expr(
        &mut self,
        key: Option<&Expr>,
        value: Option<&Expr>,
        used: bool,
    ) -> Emit<Operand> {
        let value = match value {
            Some(expr) => self.expr(expr)?,
            None => self.builder.literal(Literal::Null)?,
        };
        let key = match key {
            Some(expr) => self.expr(expr)?,
            None => Operand::UNUSED,
        };
        let dest = self.result(used)?;
        self.builder.op(Opcode::Yield, value, key, dest)?;
        Ok(dest)
    }

    pub(crate) fn send_args(&mut self, args: &[Arg]) -> Emit {
        for arg in args {
            let Arg::Positional(value) = arg else {
                return self.unsupported("named and unpacked arguments");
            };
            if let ExprKind::Variable(name) = &value.kind {
                let slot = self.variable(name)?;
                self.builder
                    .op(Opcode::SendVar, slot, Operand::UNUSED, Operand::UNUSED)?;
            } else {
                let from_call = matches!(
                    value.kind,
                    ExprKind::Call { .. }
                        | ExprKind::Binary {
                            op: BinaryOp::Pipe,
                            ..
                        }
                );
                let operand = self.expr(value)?;
                let mut send = Instruction::new(Opcode::Send);
                send.a = operand;
                send.flags = u8::from(from_call);
                self.builder.emit(send)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use php_parser::ast::{Expr, ExprKind, StringPart, VarName};

    use super::Emitter;
    use crate::bytecode::Literal;

    fn interpolated(parts: Vec<StringPart>) -> Expr {
        Expr {
            kind: ExprKind::Interpolated(parts),
            line: 1,
        }
    }

    #[test]
    fn interpolations_without_expressions_are_literals() {
        let mut ctx = super::super::EmitCtx::for_test();
        let mut emitter = Emitter::new(&mut ctx, b"", super::super::Scope::File);
        let empty = emitter.expr(&interpolated(Vec::new())).unwrap();
        let text = emitter
            .expr(&interpolated(vec![
                StringPart::Literal(b"a".to_vec()),
                StringPart::Literal(Vec::new()),
                StringPart::Literal(b"b".to_vec()),
            ]))
            .unwrap();
        let code = emitter.builder.finish();
        assert!(code.instructions.is_empty());
        assert_eq!(
            code.literals[empty.index() as usize],
            Literal::from(&b""[..])
        );
        assert_eq!(
            code.literals[text.index() as usize],
            Literal::from(&b"ab"[..])
        );
    }

    #[test]
    fn merged_literals_count_as_one_part() {
        let variable = Expr {
            kind: ExprKind::Variable(VarName::Name(b"a".to_vec())),
            line: 1,
        };
        let mut ctx = super::super::EmitCtx::for_test();
        let mut emitter = Emitter::new(&mut ctx, b"", super::super::Scope::File);
        emitter
            .expr(&interpolated(vec![
                StringPart::Literal(b"x".to_vec()),
                StringPart::Literal(b"y".to_vec()),
                StringPart::Expr(variable),
            ]))
            .unwrap();
        assert_eq!(
            emitter.builder.finish().to_string(),
            "code {main} temporaries=1\n   0    1  Concat T0 = \"xy\", $a\n"
        );
    }
}
