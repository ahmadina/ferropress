//! Expressions: reads, writes, lists and calls.

use php_parser::ast::{
    Arg, Args, ArrayItem, ArraySyntax, BinaryOp, CastKind, Expr, ExprKind, MatchArm, Member,
    StringPart, VarName,
};

use super::{Checker, Code, Flow};
use crate::diagnostic::Severity;

/// How a variable is fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fetch {
    Read,
    Isset,
    Write,
    ReadWrite,
    Unset,
    Argument,
}

impl Fetch {
    const fn writes(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite | Self::Unset)
    }
}

fn is_named(expr: &Expr, name: &[u8]) -> bool {
    matches!(&expr.kind, ExprKind::Variable(VarName::Name(found)) if found == name)
}

/// `$this`.
pub(super) fn is_this(expr: &Expr) -> bool {
    is_named(expr, b"this")
}

/// `$GLOBALS`.
fn is_globals(expr: &Expr) -> bool {
    is_named(expr, b"GLOBALS")
}

/// `$GLOBALS[…]`.
fn is_global_var(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Dim { base, .. } if is_globals(base))
}

fn is_variable(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::Variable(_)
            | ExprKind::Dim { .. }
            | ExprKind::Property { .. }
            | ExprKind::StaticProperty { .. }
    )
}

fn is_call(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::Call { .. } | ExprKind::MethodCall { .. } | ExprKind::StaticCall { .. }
    )
}

/// Whether a `?->` earlier in the chain may skip `expr`.
fn is_short_circuited(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Property { nullsafe: true, .. } | ExprKind::MethodCall { nullsafe: true, .. } => {
            true
        }
        ExprKind::Dim { base: inner, .. }
        | ExprKind::Property { object: inner, .. }
        | ExprKind::MethodCall { object: inner, .. }
        | ExprKind::StaticProperty { class: inner, .. }
        | ExprKind::StaticCall { class: inner, .. } => is_short_circuited(inner),
        _ => false,
    }
}

/// Whether a list item may be assigned to.
fn is_writable(expr: &Expr) -> bool {
    let mut base = expr;
    while let ExprKind::Dim { base: inner, .. } | ExprKind::Property { object: inner, .. } =
        &base.kind
    {
        base = inner;
    }
    (is_variable(base) || is_call(base)) && !is_short_circuited(base)
}

impl<'a> Checker<'a> {
    /// Compiles `expr`, or what folding replaced it with; a folded value needs nothing more.
    pub(super) fn expr(&mut self, expr: &'a Expr) -> Flow {
        let expr = self.resolve(expr);
        if self.folded.contains_key(&std::ptr::from_ref(expr)) {
            return Ok(());
        }
        self.unfolded(expr)
    }

    fn unfolded(&mut self, expr: &'a Expr) -> Flow {
        self.line = expr.line;
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::String(_)
            | ExprKind::Name(_)
            | ExprKind::Constant(_)
            | ExprKind::Magic(_)
            | ExprKind::Variable(VarName::Name(_)) => Ok(()),
            ExprKind::Interpolated(parts) => self.interpolated(parts),
            ExprKind::ShellExec(parts) => {
                self.report(
                    Severity::Deprecated,
                    "The backtick (`) operator is deprecated, use shell_exec() instead",
                );
                self.interpolated(parts)
            }
            ExprKind::Variable(VarName::Expr(name)) => self.expr(name),
            ExprKind::Array { items, .. } => self.array(expr, items),
            ExprKind::Dim { .. }
            | ExprKind::Property { .. }
            | ExprKind::StaticProperty { .. }
            | ExprKind::Call { .. }
            | ExprKind::MethodCall { .. }
            | ExprKind::StaticCall { .. } => self.var(expr, Fetch::Read),
            ExprKind::New { class, args } => {
                self.class_ref(class)?;
                self.args(args)
            }
            ExprKind::NewAnonymous { class, args } => {
                self.args(args)?;
                self.class(class)
            }
            ExprKind::ClassConstant { class, name } => {
                self.class_ref(class)?;
                self.member(name)
            }
            ExprKind::ClassName(class) => self.class_name_fetch(class),
            ExprKind::Cast {
                kind: CastKind::Unset,
                ..
            } => self.fatal("The (unset) cast is no longer supported"),
            ExprKind::Unary { operand, .. }
            | ExprKind::Cast { operand, .. }
            | ExprKind::VoidCast(operand)
            | ExprKind::Print(operand)
            | ExprKind::Throw(operand)
            | ExprKind::Include { expr: operand, .. } => self.expr(operand),
            ExprKind::IncDec { operand, .. } => {
                self.ensure_writable(operand)?;
                self.var(operand, Fetch::ReadWrite)
            }
            ExprKind::Binary { op, left, right } => self.binary(*op, left, right),
            ExprKind::Instanceof { expr, class } => self.instanceof(expr, class),
            ExprKind::Assign { target, value } => self.assign(target, value),
            ExprKind::AssignRef { target, value } => self.assign_ref(target, value),
            ExprKind::CompoundAssign { target, value, .. } => {
                self.ensure_writable(target)?;
                self.var(target, Fetch::ReadWrite)?;
                self.expr(value)
            }
            ExprKind::Ternary {
                condition,
                then,
                otherwise,
                ..
            } => self.ternary(condition, then.as_deref(), otherwise),
            ExprKind::Isset(operand) => self.isset(operand),
            ExprKind::Empty(operand) => {
                if is_variable(operand) {
                    self.var(operand, Fetch::Isset)
                } else {
                    self.expr(operand)
                }
            }
            ExprKind::Yield { key, value } => {
                self.mark_generator()?;
                if let Some(value) = value {
                    self.expr(value)?;
                }
                key.as_ref().map_or(Ok(()), |key| self.expr(key))
            }
            ExprKind::YieldFrom(operand) => {
                self.mark_generator()?;
                if self.frame.by_ref {
                    return self.fatal("Cannot use \"yield from\" inside a by-reference generator");
                }
                self.expr(operand)
            }
            ExprKind::Closure(function) | ExprKind::ArrowFunction { function, .. } => {
                self.function(function, Code::Closure(self.closure_name(function.line)))
            }
            ExprKind::Match { subject, arms } => self.match_expr(subject, arms),
            ExprKind::Reference(operand) => self.var(operand, Fetch::Write),
        }
    }

    fn binary(&mut self, op: BinaryOp, left: &'a Expr, right: &'a Expr) -> Flow {
        let unparenthesized_arrow = matches!(
            right.kind,
            ExprKind::ArrowFunction {
                parenthesized: false,
                ..
            }
        );
        if op == BinaryOp::Pipe && unparenthesized_arrow {
            return self
                .fatal("Arrow functions on the right hand side of |> must be parenthesized");
        }
        self.expr(left)?;
        self.expr(right)
    }

    fn ternary(
        &mut self,
        condition: &'a Expr,
        then: Option<&'a Expr>,
        otherwise: &'a Expr,
    ) -> Flow {
        self.unparenthesized_ternary(condition, then.is_some())?;
        self.expr(condition)?;
        if let Some(then) = then {
            self.expr(then)?;
        }
        self.expr(otherwise)
    }

    fn mark_generator(&self) -> Flow {
        if self.frame.code == Code::File {
            self.fatal("The \"yield\" expression can only be used inside a function")
        } else {
            Ok(())
        }
    }

    fn interpolated(&mut self, parts: &'a [StringPart]) -> Flow {
        for part in parts {
            match part {
                StringPart::Literal(_) => continue,
                StringPart::BracedName(expr)
                    if matches!(expr.kind, ExprKind::Variable(_) | ExprKind::Dim { .. }) =>
                {
                    self.report(
                        Severity::Deprecated,
                        "Using ${var} in strings is deprecated, use {$var} instead",
                    );
                }
                StringPart::BracedExpr(expr) if matches!(expr.kind, ExprKind::Variable(_)) => {
                    self.report(
                        Severity::Deprecated,
                        "Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead",
                    );
                }
                StringPart::Expr(_) | StringPart::BracedName(_) | StringPart::BracedExpr(_) => {}
            }
            if let Some(expr) = part.expr() {
                self.expr(expr)?;
            }
        }
        Ok(())
    }

    /// An array literal: folded first, like a constant expression, then compiled if it stays.
    fn array(&mut self, expr: &'a Expr, items: &'a [Option<ArrayItem>]) -> Flow {
        if self.fold(expr)?.is_some() {
            return Ok(());
        }
        for item in items.iter().flatten() {
            match item {
                ArrayItem::Value { key, value, by_ref } => {
                    if let Some(key) = key {
                        self.expr(key)?;
                    }
                    if *by_ref {
                        self.var(value, Fetch::Write)?;
                    } else {
                        self.expr(value)?;
                    }
                }
                ArrayItem::Spread(value) => self.expr(value)?,
            }
        }
        Ok(())
    }

    /// Compiles `expr` as a variable fetched for `fetch`.
    pub(super) fn var(&mut self, expr: &'a Expr, fetch: Fetch) -> Flow {
        self.line = expr.line;
        match &expr.kind {
            ExprKind::Variable(VarName::Name(_)) => Ok(()),
            ExprKind::Variable(VarName::Expr(name)) => self.expr(name),
            ExprKind::Dim { base, index } => {
                if is_globals(base) {
                    return match index {
                        None => self.fatal("Cannot append to $GLOBALS"),
                        Some(index) => self.expr(index),
                    };
                }
                self.var(base, fetch)?;
                match index {
                    None if matches!(fetch, Fetch::Read | Fetch::Isset) => {
                        self.fatal("Cannot use [] for reading")
                    }
                    None if fetch == Fetch::Unset => self.fatal("Cannot use [] for unsetting"),
                    None => Ok(()),
                    Some(index) => self.expr(index),
                }
            }
            ExprKind::Property { object, name, .. } => {
                if !is_this(object) {
                    self.var(object, fetch)?;
                }
                self.member(name)
            }
            ExprKind::StaticProperty { class, name, .. } => {
                self.class_ref(class)?;
                match name {
                    VarName::Name(_) => Ok(()),
                    VarName::Expr(name) => self.expr(name),
                }
            }
            ExprKind::Call { callee, args } => {
                if !matches!(callee.kind, ExprKind::Name(_)) {
                    self.expr(callee)?;
                }
                self.args(args)
            }
            ExprKind::MethodCall {
                object, name, args, ..
            } => {
                if !is_this(object) {
                    self.expr(object)?;
                }
                self.member(name)?;
                self.args(args)
            }
            ExprKind::StaticCall { class, name, args } => {
                self.class_ref(class)?;
                self.member(name)?;
                self.args(args)
            }
            _ if fetch.writes() => self.fatal("Cannot use temporary expression in write context"),
            _ => self.expr(expr),
        }
    }

    fn member(&mut self, member: &'a Member) -> Flow {
        match member {
            Member::Name(_) => Ok(()),
            Member::Expr(name) => self.expr(name),
        }
    }

    fn args(&mut self, args: &'a Args) -> Flow {
        let Args::List(args) = args else {
            return Ok(());
        };
        let mut named = false;
        let mut unpacked = false;
        for arg in args {
            let value = match arg {
                Arg::Unpack(value) => {
                    if named {
                        return self.fatal("Cannot use argument unpacking after named arguments");
                    }
                    unpacked = true;
                    self.expr(value)?;
                    continue;
                }
                Arg::Named { value, .. } => {
                    named = true;
                    value
                }
                Arg::Positional(value) => {
                    if unpacked {
                        return self
                            .fatal("Cannot use positional argument after argument unpacking");
                    }
                    if named {
                        return self.fatal("Cannot use positional argument after named argument");
                    }
                    value
                }
            };
            if is_variable(value) {
                self.var(value, Fetch::Argument)?;
            } else {
                self.expr(value)?;
            }
        }
        Ok(())
    }

    fn ensure_writable(&self, target: &Expr) -> Flow {
        match &target.kind {
            ExprKind::Call { .. }
            | ExprKind::Binary {
                op: BinaryOp::Pipe, ..
            } => self.fatal("Can't use function return value in write context"),
            ExprKind::MethodCall { .. } | ExprKind::StaticCall { .. } => {
                self.fatal("Can't use method return value in write context")
            }
            _ if is_short_circuited(target) => {
                self.fatal("Can't use nullsafe operator in write context")
            }
            _ if is_globals(target) => self
                .fatal("$GLOBALS can only be modified using the $GLOBALS[$name] = $value syntax"),
            _ => Ok(()),
        }
    }

    /// Compiles `target` as the destination of an assignment whose value is already computed.
    pub(super) fn assign_target(&mut self, target: &'a Expr) -> Flow {
        if let ExprKind::Array { items, syntax } = &target.kind {
            return self.list_assign(items, *syntax);
        }
        if is_this(target) {
            return self.fatal("Cannot re-assign $this");
        }
        self.ensure_writable(target)?;
        self.var(target, Fetch::Write)
    }

    fn assign(&mut self, target: &'a Expr, value: &'a Expr) -> Flow {
        if is_this(target) {
            return self.fatal("Cannot re-assign $this");
        }
        self.ensure_writable(target)?;
        if let ExprKind::Array { items, syntax } = &target.kind {
            if list_has_refs(items) {
                self.var(value, Fetch::Write)?;
            } else {
                self.expr(value)?;
            }
            return self.list_assign(items, *syntax);
        }
        self.var(target, Fetch::Write)?;
        self.expr(value)
    }

    fn assign_ref(&mut self, target: &'a Expr, value: &'a Expr) -> Flow {
        if is_this(target) {
            return self.fatal("Cannot re-assign $this");
        }
        self.ensure_writable(target)?;
        if is_short_circuited(value) {
            return self.fatal("Cannot take reference of a nullsafe chain");
        }
        if is_globals(value) {
            return self.fatal("Cannot acquire reference to $GLOBALS");
        }
        self.var(target, Fetch::Write)?;
        self.var(value, Fetch::Write)
    }

    fn list_assign(&mut self, items: &'a [Option<ArrayItem>], style: ArraySyntax) -> Flow {
        let keyed = matches!(
            items.iter().flatten().next(),
            Some(ArrayItem::Value { key: Some(_), .. })
        );
        let mut has_items = false;
        for item in items {
            let (key, value) = match item {
                None if keyed => {
                    return self.fatal("Cannot use empty array entries in keyed array assignment");
                }
                None => continue,
                Some(ArrayItem::Spread(_)) => {
                    return self.fatal("Spread operator is not supported in assignments");
                }
                Some(ArrayItem::Value { key, value, .. }) => (key, value),
            };
            has_items = true;
            if keyed != key.is_some() {
                return self.fatal("Cannot mix keyed and unkeyed array entries in assignments");
            }
            if let Some(key) = key {
                self.expr(key)?;
            }
            match &value.kind {
                ExprKind::Array {
                    syntax: ArraySyntax::Long,
                    ..
                } => return self.fatal("Cannot assign to array(), use [] instead"),
                ExprKind::Array { syntax, .. } if *syntax != style => {
                    return self.fatal("Cannot mix [] and list()");
                }
                ExprKind::Array { items, syntax } => {
                    self.list_assign(items, *syntax)?;
                    continue;
                }
                _ if !is_writable(value) => {
                    return self.fatal("Assignments can only happen to writable values");
                }
                _ => {}
            }
            if is_this(value) {
                return self.fatal("Cannot re-assign $this");
            }
            self.ensure_writable(value)?;
            self.var(value, Fetch::Write)?;
        }
        if has_items {
            Ok(())
        } else {
            self.fatal("Cannot use empty list")
        }
    }

    fn isset(&mut self, operand: &'a Expr) -> Flow {
        if !is_variable(operand) {
            return self.fatal(
                "Cannot use isset() on the result of an expression (you can use \"null !== expression\" instead)",
            );
        }
        if is_globals(operand) {
            return Ok(());
        }
        if let ExprKind::Dim { base, index } = &operand.kind
            && is_globals(base)
        {
            return match index {
                None => self.fatal("Cannot use [] for reading"),
                Some(index) => self.expr(index),
            };
        }
        self.var(operand, Fetch::Isset)
    }

    pub(super) fn unset(&mut self, operand: &'a Expr) -> Flow {
        self.ensure_writable(operand)?;
        if is_global_var(operand) {
            return match &operand.kind {
                ExprKind::Dim {
                    index: Some(index), ..
                } => self.expr(index),
                _ => self.fatal("Cannot use [] for unsetting"),
            };
        }
        if is_this(operand) {
            return self.fatal("Cannot unset $this");
        }
        self.var(operand, Fetch::Unset)
    }

    fn unparenthesized_ternary(&self, condition: &Expr, has_then: bool) -> Flow {
        let ExprKind::Ternary {
            then: inner_then,
            parenthesized: false,
            ..
        } = &condition.kind
        else {
            return Ok(());
        };
        match (inner_then.is_some(), has_then) {
            (true, true) => self.fatal(
                "Unparenthesized `a ? b : c ? d : e` is not supported. Use either `(a ? b : c) ? d : e` or `a ? b : (c ? d : e)`",
            ),
            (true, false) => self.fatal(
                "Unparenthesized `a ? b : c ?: d` is not supported. Use either `(a ? b : c) ?: d` or `a ? b : (c ?: d)`",
            ),
            (false, true) => self.fatal(
                "Unparenthesized `a ?: b ? c : d` is not supported. Use either `(a ?: b) ? c : d` or `a ?: (b ? c : d)`",
            ),
            (false, false) => Ok(()),
        }
    }

    fn match_expr(&mut self, subject: &'a Expr, arms: &'a [MatchArm]) -> Flow {
        self.expr(subject)?;
        let mut has_default = false;
        for arm in arms {
            match &arm.conditions {
                None if has_default => {
                    self.line = arm.body.line;
                    return self.fatal("Match expressions may only contain one default arm");
                }
                None => has_default = true,
                Some(conditions) => self.exprs(conditions)?,
            }
        }
        arms.iter().try_for_each(|arm| self.expr(&arm.body))
    }
}

/// Whether any item of a destructuring list, at any depth, is taken by reference.
fn list_has_refs(items: &[Option<ArrayItem>]) -> bool {
    items.iter().flatten().any(|item| match item {
        ArrayItem::Value { by_ref: true, .. } => true,
        ArrayItem::Value {
            value:
                Expr {
                    kind: ExprKind::Array { items, .. },
                    ..
                },
            ..
        } => list_has_refs(items),
        _ => false,
    })
}
