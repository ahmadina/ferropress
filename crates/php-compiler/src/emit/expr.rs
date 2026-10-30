//! Expressions: each compiles to the operand holding its value.
//!
//! A local variable compiles to its slot, read by the instruction that uses it, so an operand
//! changed by a later operand of the same instruction is read after the change.

use php_parser::ast::{
    Arg, Args, ArrayItem, ArraySyntax, AssignOp, BinaryOp, Expr, ExprKind, IncDecOp, IncludeKind,
    Name, NameKind, StringPart, UnaryOp, VarName,
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

/// A built-in whose call compiles to a dedicated opcode when the argument count matches.
fn builtin_call(name: &Name, argc: usize) -> Option<Opcode> {
    let written = name.bytes.as_slice();
    match argc {
        1 if written.eq_ignore_ascii_case(b"strlen") => Some(Opcode::Strlen),
        1 if written.eq_ignore_ascii_case(b"count") => Some(Opcode::Count),
        2 if written.eq_ignore_ascii_case(b"strpos") => Some(Opcode::Strpos),
        2 if written.eq_ignore_ascii_case(b"str_contains") => Some(Opcode::StrContains),
        _ => None,
    }
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
                if *op == AssignOp::Add && is_int_one(value) {
                    return self.inc_dec(IncDecOp::PreInc, target, used);
                }
                if let Some(opcode) = compound_assign(*op) {
                    let slot = self.target(target)?;
                    let value = self.expr(value)?;
                    let result = self.result(used)?;
                    self.builder.op(opcode, slot, value, result)?;
                    return Ok(if result.is_unused() { slot } else { result });
                }
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
            ExprKind::Array { items, syntax } => self.array_literal(items, *syntax, used),
            ExprKind::Dim { base, index } => self.fetch_dim(base, index.as_deref(), used),
            ExprKind::Isset(operand) => self.isset(operand, used),
            ExprKind::Empty(operand) => self.empty(operand, used),
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
        if let ExprKind::Dim { base, index } = &target.kind {
            return self.store_dim(base, index.as_deref(), value, used);
        }
        if is_increment_by_one(target, value) {
            return self.inc_dec(IncDecOp::PreInc, target, used);
        }
        if let Some(steps) = assign_ops(target, value) {
            return self.assign_steps(target, &steps, used);
        }
        let slot = self.target(target)?;
        let value = self.expr(value)?;
        let result = self.result(used)?;
        self.builder.op(Opcode::Assign, slot, value, result)?;
        Ok(result)
    }

    fn assign_steps(
        &mut self,
        target: &Expr,
        steps: &[(Opcode, &Expr)],
        used: bool,
    ) -> Emit<Operand> {
        let slot = self.target(target)?;
        let last = steps.len().saturating_sub(1);
        let mut value = slot;
        for (index, (opcode, rhs)) in steps.iter().enumerate() {
            let rhs = self.expr(rhs)?;
            let dest = if used && index == last {
                self.builder.temp()?
            } else {
                Operand::UNUSED
            };
            self.builder.op(*opcode, slot, rhs, dest)?;
            if !dest.is_unused() {
                value = dest;
            }
        }
        Ok(value)
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
    pub(super) fn variable(&mut self, name: &VarName) -> Emit<Operand> {
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
            if args.len() <= 2 && args.iter().all(|arg| matches!(arg, Arg::Positional(_))) {
                return self.invoke(name, args, used);
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

    fn invoke(&mut self, name: &Name, args: &[Arg], used: bool) -> Emit<Operand> {
        let mut operands = [Operand::UNUSED; 2];
        let mut from_call = [false; 2];
        for (index, arg) in args.iter().enumerate() {
            let (operand, call) = self.arg_operand(arg)?;
            operands[index] = operand;
            from_call[index] = call;
        }
        if let Some(opcode) = builtin_call(name, args.len()) {
            let result = self.result(used)?;
            let mut ins = Instruction::new(opcode);
            ins.a = operands[0];
            ins.b = operands[1];
            ins.result = result;
            self.builder.emit(ins)?;
            return Ok(result);
        }
        let written = self.builder.string(&name.bytes)?;
        let result = self.result(used)?;
        let mut ins = Instruction::new(Opcode::Invoke);
        ins.flags = match args.len() {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        if from_call[0] {
            ins.flags |= 4;
        }
        if from_call[1] {
            ins.flags |= 8;
        }
        ins.a = written;
        ins.b = operands[0];
        ins.c = operands[1];
        ins.result = result;
        self.builder.emit(ins)?;
        Ok(result)
    }

    fn arg_operand(&mut self, arg: &Arg) -> Emit<(Operand, bool)> {
        let Arg::Positional(value) = arg else {
            return self.unsupported("named and unpacked arguments");
        };
        if let ExprKind::Variable(name) = &value.kind {
            return Ok((self.variable(name)?, false));
        }
        let from_call = matches!(
            value.kind,
            ExprKind::Call { .. }
                | ExprKind::Binary {
                    op: BinaryOp::Pipe,
                    ..
                }
        );
        Ok((self.expr(value)?, from_call))
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

    fn array_literal(
        &mut self,
        items: &[Option<ArrayItem>],
        syntax: ArraySyntax,
        used: bool,
    ) -> Emit<Operand> {
        if syntax == ArraySyntax::List {
            return self.unsupported("list()");
        }
        let dest = self.builder.temp()?;
        self.builder
            .op(Opcode::NewArray, Operand::UNUSED, Operand::UNUSED, dest)?;
        for item in items {
            let Some(item) = item else {
                return self.unsupported("empty array elements");
            };
            match item {
                ArrayItem::Spread(_) => return self.unsupported("unpacked array items"),
                ArrayItem::Value { key, value, by_ref } => {
                    if *by_ref {
                        return self.unsupported("array items by reference");
                    }
                    let value = self.expr(value)?;
                    let key = match key {
                        Some(key) => self.expr(key)?,
                        None => Operand::UNUSED,
                    };
                    let mut item = Instruction::new(Opcode::ArrayItem);
                    item.a = dest;
                    item.b = value;
                    item.c = key;
                    self.builder.emit(item)?;
                }
            }
        }
        if used { Ok(dest) } else { Ok(Operand::UNUSED) }
    }

    fn fetch_dim(&mut self, base: &Expr, index: Option<&Expr>, used: bool) -> Emit<Operand> {
        let Some(index) = index else {
            return self.unsupported("[] in read context");
        };
        let base = self.expr(base)?;
        let index = self.expr(index)?;
        let dest = self.result(used)?;
        self.builder.op(Opcode::FetchDim, base, index, dest)?;
        Ok(dest)
    }

    fn store_dim(
        &mut self,
        base: &Expr,
        index: Option<&Expr>,
        value: &Expr,
        used: bool,
    ) -> Emit<Operand> {
        let ExprKind::Variable(name) = &base.kind else {
            return self.unsupported("assignments to this target");
        };
        let slot = self.variable(name)?;
        let value = self.expr(value)?;
        let index = match index {
            Some(index) => self.expr(index)?,
            None => Operand::UNUSED,
        };
        let mut store = Instruction::new(Opcode::StoreDim);
        store.a = slot;
        store.b = index;
        store.c = value;
        self.builder.emit(store)?;
        if used { Ok(value) } else { Ok(Operand::UNUSED) }
    }

    fn isset(&mut self, operand: &Expr, used: bool) -> Emit<Operand> {
        match &operand.kind {
            ExprKind::Variable(name) => {
                let slot = self.variable(name)?;
                self.isset_op(Opcode::Isset, slot, Operand::UNUSED, false, used)
            }
            ExprKind::Dim { base, index } => self.isset_dim(base, index.as_deref(), false, used),
            _ => self.unsupported("isset of this target"),
        }
    }

    fn empty(&mut self, operand: &Expr, used: bool) -> Emit<Operand> {
        match &operand.kind {
            ExprKind::Variable(name) => {
                let slot = self.variable(name)?;
                self.isset_op(Opcode::Isset, slot, Operand::UNUSED, true, used)
            }
            ExprKind::Dim { base, index } => self.isset_dim(base, index.as_deref(), true, used),
            ExprKind::Property { .. } | ExprKind::StaticProperty { .. } => {
                self.unsupported("empty of this target")
            }
            _ => {
                let value = self.expr(operand)?;
                self.isset_op(Opcode::Isset, value, Operand::UNUSED, true, used)
            }
        }
    }

    fn isset_dim(
        &mut self,
        base: &Expr,
        index: Option<&Expr>,
        empty: bool,
        used: bool,
    ) -> Emit<Operand> {
        let Some(index) = index else {
            return self.unsupported("[] in read context");
        };
        let ExprKind::Variable(name) = &base.kind else {
            return self.unsupported(if empty {
                "empty of this target"
            } else {
                "isset of this target"
            });
        };
        let base = self.variable(name)?;
        let index = self.expr(index)?;
        self.isset_op(Opcode::IssetDim, base, index, empty, used)
    }

    fn isset_op(
        &mut self,
        opcode: Opcode,
        a: Operand,
        b: Operand,
        empty: bool,
        used: bool,
    ) -> Emit<Operand> {
        let dest = self.result(used)?;
        let mut check = Instruction::new(opcode);
        check.flags = u8::from(empty);
        check.a = a;
        check.b = b;
        check.result = dest;
        self.builder.emit(check)?;
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

fn is_int_one(expr: &Expr) -> bool {
    matches!(expr.kind, ExprKind::Int(1))
}

fn compound_assign(op: AssignOp) -> Option<Opcode> {
    Some(match op {
        AssignOp::Add => Opcode::AddAssign,
        AssignOp::Sub => Opcode::SubAssign,
        AssignOp::Mul => Opcode::MulAssign,
        AssignOp::Mod => Opcode::ModAssign,
        _ => return None,
    })
}

fn bin_assign(op: BinaryOp) -> Option<Opcode> {
    Some(match op {
        BinaryOp::Add => Opcode::AddAssign,
        BinaryOp::Sub => Opcode::SubAssign,
        BinaryOp::Mul => Opcode::MulAssign,
        BinaryOp::Mod => Opcode::ModAssign,
        _ => return None,
    })
}

fn assign_ops<'a>(target: &Expr, value: &'a Expr) -> Option<Vec<(Opcode, &'a Expr)>> {
    let mut steps = Vec::new();
    let mut current = value;
    loop {
        let ExprKind::Binary { op, left, right } = &current.kind else {
            return None;
        };
        let opcode = bin_assign(*op)?;
        let commutative = matches!(*op, BinaryOp::Add | BinaryOp::Mul);
        if same_variable(target, left) {
            steps.push((opcode, right.as_ref()));
            steps.reverse();
            return Some(steps);
        }
        if commutative && same_variable(target, right) {
            steps.push((opcode, left.as_ref()));
            steps.reverse();
            return Some(steps);
        }
        steps.push((opcode, right.as_ref()));
        current = left;
    }
}

fn is_increment_by_one(target: &Expr, value: &Expr) -> bool {
    let ExprKind::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = &value.kind
    else {
        return false;
    };
    (same_variable(target, left) && is_int_one(right))
        || (same_variable(target, right) && is_int_one(left))
}

fn same_variable(left: &Expr, right: &Expr) -> bool {
    match (&left.kind, &right.kind) {
        (ExprKind::Variable(VarName::Name(a)), ExprKind::Variable(VarName::Name(b))) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use php_parser::ast::{BinaryOp, Expr, ExprKind, Name, NameKind, StringPart, VarName};

    use super::Emitter;
    use crate::bytecode::Literal;

    fn interpolated(parts: Vec<StringPart>) -> Expr {
        Expr {
            kind: ExprKind::Interpolated(parts),
            line: 1,
        }
    }

    #[test]
    fn increment_helpers_match_only_the_same_integer_one() {
        let i = Expr {
            kind: ExprKind::Variable(VarName::Name(b"i".to_vec())),
            line: 1,
        };
        let j = Expr {
            kind: ExprKind::Variable(VarName::Name(b"j".to_vec())),
            line: 1,
        };
        let one = Expr {
            kind: ExprKind::Int(1),
            line: 1,
        };
        let two = Expr {
            kind: ExprKind::Int(2),
            line: 1,
        };
        assert!(super::same_variable(&i, &i));
        assert!(!super::same_variable(&i, &j));
        assert!(!super::same_variable(&i, &one));
        assert!(super::is_int_one(&one));
        assert!(!super::is_int_one(&two));
        assert!(!super::is_int_one(&i));
        let plus_one = Expr {
            kind: ExprKind::Binary {
                op: BinaryOp::Add,
                left: Box::new(i.clone()),
                right: Box::new(one.clone()),
            },
            line: 1,
        };
        let plus_two = Expr {
            kind: ExprKind::Binary {
                op: BinaryOp::Add,
                left: Box::new(i.clone()),
                right: Box::new(two),
            },
            line: 1,
        };
        assert!(super::is_increment_by_one(&i, &plus_one));
        assert!(!super::is_increment_by_one(&i, &plus_two));
        assert!(!super::is_increment_by_one(&i, &one));
        let add_j = Expr {
            kind: ExprKind::Binary {
                op: BinaryOp::Add,
                left: Box::new(i.clone()),
                right: Box::new(j.clone()),
            },
            line: 1,
        };
        let chain = super::assign_ops(&i, &add_j).expect("add assign");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].0, crate::bytecode::Opcode::AddAssign);
        assert!(super::assign_ops(&i, &one).is_none());
        assert!(super::assign_ops(&i, &plus_two).is_some());
        let strlen = Name {
            kind: NameKind::Unqualified,
            bytes: b"STRLEN".to_vec(),
        };
        assert_eq!(
            super::builtin_call(&strlen, 1),
            Some(crate::bytecode::Opcode::Strlen)
        );
        assert!(super::builtin_call(&strlen, 0).is_none());
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
