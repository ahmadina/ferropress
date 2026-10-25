//! Free variables of an arrow function, in the order they first appear.

use std::collections::HashSet;

use php_parser::ast::{
    Arg, Args, ArrayItem, Expr, ExprKind, Function, FunctionBody, Member, StringPart, VarName,
};

/// Names captured by an arrow, in source order, without the arrow's own parameters.
#[derive(Default)]
pub(super) struct FreeVars {
    pub(super) names: Vec<Vec<u8>>,
    seen: HashSet<Vec<u8>>,
}

impl FreeVars {
    fn add(&mut self, name: &[u8], bound: &HashSet<Vec<u8>>) {
        if bound.contains(name) || self.seen.contains(name) {
            return;
        }
        self.seen.insert(name.to_vec());
        self.names.push(name.to_vec());
    }
}

/// Walks `expr` and records variables that are not in `bound`.
pub(super) fn walk_expr(expr: &Expr, bound: &HashSet<Vec<u8>>, free: &mut FreeVars) {
    match &expr.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::String(_)
        | ExprKind::Constant(_)
        | ExprKind::Magic(_)
        | ExprKind::Name(_) => {}
        ExprKind::Variable(name) => walk_var(name, bound, free),
        ExprKind::Interpolated(parts) | ExprKind::ShellExec(parts) => {
            for part in parts {
                if let StringPart::Expr(expr)
                | StringPart::BracedName(expr)
                | StringPart::BracedExpr(expr) = part
                {
                    walk_expr(expr, bound, free);
                }
            }
        }
        ExprKind::Array { items, .. } => {
            for item in items.iter().flatten() {
                walk_array_item(item, bound, free);
            }
        }
        ExprKind::Closure(function) => walk_nested_closure(function, bound, free),
        ExprKind::ArrowFunction { function, .. } => walk_nested_arrow(function, bound, free),
        _ => walk_expr_parts(expr, bound, free),
    }
}

fn walk_var(name: &VarName, bound: &HashSet<Vec<u8>>, free: &mut FreeVars) {
    match name {
        VarName::Name(name) => free.add(name, bound),
        VarName::Expr(expr) => walk_expr(expr, bound, free),
    }
}

fn walk_array_item(item: &ArrayItem, bound: &HashSet<Vec<u8>>, free: &mut FreeVars) {
    match item {
        ArrayItem::Value { key, value, .. } => {
            if let Some(key) = key {
                walk_expr(key, bound, free);
            }
            walk_expr(value, bound, free);
        }
        ArrayItem::Spread(expr) => walk_expr(expr, bound, free),
    }
}

fn walk_nested_closure(function: &Function, bound: &HashSet<Vec<u8>>, free: &mut FreeVars) {
    for use_var in &function.uses {
        free.add(&use_var.name, bound);
    }
    for param in &function.params {
        if let Some(default) = &param.default {
            walk_expr(default, bound, free);
        }
    }
}

fn walk_nested_arrow(function: &Function, bound: &HashSet<Vec<u8>>, free: &mut FreeVars) {
    let mut inner = bound.clone();
    for param in &function.params {
        inner.insert(param.name.clone());
    }
    for param in &function.params {
        if let Some(default) = &param.default {
            walk_expr(default, &inner, free);
        }
    }
    if let FunctionBody::Expr(body) = &function.body {
        walk_expr(body, &inner, free);
    }
}

fn walk_member(member: &Member, bound: &HashSet<Vec<u8>>, free: &mut FreeVars) {
    if let Member::Expr(expr) = member {
        walk_expr(expr, bound, free);
    }
}

fn walk_args(args: &Args, bound: &HashSet<Vec<u8>>, free: &mut FreeVars) {
    let Args::List(args) = args else {
        return;
    };
    for arg in args {
        match arg {
            Arg::Positional(expr) | Arg::Unpack(expr) | Arg::Named { value: expr, .. } => {
                walk_expr(expr, bound, free);
            }
        }
    }
}

fn walk_opt(expr: Option<&Expr>, bound: &HashSet<Vec<u8>>, free: &mut FreeVars) {
    if let Some(expr) = expr {
        walk_expr(expr, bound, free);
    }
}

/// Remaining expression forms: operators, calls and assignments.
fn walk_expr_parts(expr: &Expr, bound: &HashSet<Vec<u8>>, free: &mut FreeVars) {
    match &expr.kind {
        ExprKind::Dim { base, index } => {
            walk_expr(base, bound, free);
            walk_opt(index.as_deref(), bound, free);
        }
        ExprKind::Property { object, name, .. } => {
            walk_expr(object, bound, free);
            walk_member(name, bound, free);
        }
        ExprKind::StaticProperty { class, name, .. } => {
            walk_expr(class, bound, free);
            walk_var(name, bound, free);
        }
        ExprKind::Call { callee, args } => {
            walk_expr(callee, bound, free);
            walk_args(args, bound, free);
        }
        ExprKind::MethodCall {
            object, name, args, ..
        } => {
            walk_expr(object, bound, free);
            walk_member(name, bound, free);
            walk_args(args, bound, free);
        }
        ExprKind::StaticCall {
            class, name, args, ..
        } => {
            walk_expr(class, bound, free);
            walk_member(name, bound, free);
            walk_args(args, bound, free);
        }
        ExprKind::New { class, args } => {
            walk_expr(class, bound, free);
            walk_args(args, bound, free);
        }
        ExprKind::ClassConstant { class, name } => {
            walk_expr(class, bound, free);
            walk_member(name, bound, free);
        }
        ExprKind::Unary { operand, .. }
        | ExprKind::Cast { operand, .. }
        | ExprKind::VoidCast(operand)
        | ExprKind::IncDec { operand, .. }
        | ExprKind::Isset(operand)
        | ExprKind::Empty(operand)
        | ExprKind::Include { expr: operand, .. }
        | ExprKind::Print(operand)
        | ExprKind::YieldFrom(operand)
        | ExprKind::Throw(operand)
        | ExprKind::Reference(operand)
        | ExprKind::ClassName(operand) => walk_expr(operand, bound, free),
        ExprKind::Binary { left, right, .. }
        | ExprKind::Assign {
            target: left,
            value: right,
        }
        | ExprKind::AssignRef {
            target: left,
            value: right,
        }
        | ExprKind::CompoundAssign {
            target: left,
            value: right,
            ..
        }
        | ExprKind::Instanceof {
            expr: left,
            class: right,
        } => {
            walk_expr(left, bound, free);
            walk_expr(right, bound, free);
        }
        ExprKind::Ternary {
            condition,
            then,
            otherwise,
            ..
        } => {
            walk_expr(condition, bound, free);
            walk_opt(then.as_deref(), bound, free);
            walk_expr(otherwise, bound, free);
        }
        ExprKind::Yield { key, value } => {
            walk_opt(key.as_deref(), bound, free);
            walk_opt(value.as_deref(), bound, free);
        }
        ExprKind::Match { subject, arms } => {
            walk_expr(subject, bound, free);
            for arm in arms {
                if let Some(conditions) = &arm.conditions {
                    for condition in conditions {
                        walk_expr(condition, bound, free);
                    }
                }
                walk_expr(&arm.body, bound, free);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use php_parser::{ParseOptions, parse};

    use super::{FreeVars, walk_expr};
    use php_parser::ast::{ExprKind, FunctionBody, StmtKind, TopStatement};
    use std::collections::HashSet;

    fn arrow_body(source: &str) -> php_parser::ast::Expr {
        let program = parse(source.as_bytes(), ParseOptions::default())
            .unwrap()
            .result
            .unwrap();
        let TopStatement::Statement(stmt) = &program.statements[0] else {
            panic!("expected a statement");
        };
        let StmtKind::Expr(expr) = &stmt.kind else {
            panic!("expected an expression statement");
        };
        let ExprKind::Assign { value, .. } = &expr.kind else {
            panic!("expected an assignment");
        };
        let ExprKind::ArrowFunction { function, .. } = &value.kind else {
            panic!("expected an arrow function");
        };
        let FunctionBody::Expr(body) = &function.body else {
            panic!("expected an arrow body");
        };
        body.as_ref().clone()
    }

    #[test]
    fn free_vars_follow_appearance_and_skip_parameters() {
        let body = arrow_body("<?php $f = fn($x) => $x + $a + $b + $a;");
        let mut bound = HashSet::new();
        bound.insert(b"x".to_vec());
        bound.insert(b"this".to_vec());
        let mut free = FreeVars::default();
        walk_expr(&body, &bound, &mut free);
        assert_eq!(free.names, [b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn nested_arrows_need_the_outer_variable() {
        let body = arrow_body("<?php $f = fn() => fn() => $a;");
        let bound = HashSet::new();
        let mut free = FreeVars::default();
        walk_expr(&body, &bound, &mut free);
        assert_eq!(free.names, [b"a".to_vec()]);
    }

    fn free_names(source: &str) -> Vec<Vec<u8>> {
        let body = arrow_body(source);
        let mut free = FreeVars::default();
        walk_expr(&body, &HashSet::new(), &mut free);
        free.names
    }

    #[test]
    fn free_vars_walk_calls_indexes_and_nested_uses() {
        assert_eq!(
            free_names("<?php $f = fn() => $a[$b] + $c($d) + (function() use ($e) {});"),
            [b"a", b"b", b"c", b"d", b"e"].map(Vec::from)
        );
        assert_eq!(
            free_names("<?php $f = fn() => $p ? $q : $r;"),
            [b"p", b"q", b"r"].map(Vec::from)
        );
        assert_eq!(
            free_names("<?php $f = fn() => \"$a{$b}\";"),
            [b"a", b"b"].map(Vec::from)
        );
    }
}
