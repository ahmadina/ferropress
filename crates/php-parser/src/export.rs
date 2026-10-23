//! Prints an expression back as source text the way the reference prints the message of a failed
//! `assert()`: a normalized layout with four-space indentation, parentheses only where operator
//! priority needs them, and literals in a canonical spelling.

use crate::ast::{
    Arg, Args, ArrayItem, AssignOp, AttributeGroup, BinaryOp, CastKind, ClassKind, ClassLike,
    ClassMember, Expr, ExprKind, Function, FunctionBody, Hook, HookBody, IncDecOp, IncludeKind,
    MagicConstant, MatchArm, Member, MethodReference, Modifiers, Name, NameKind, Param,
    PropertyGroup, SimpleType, Stmt, StmtKind, StringPart, TraitAdaptation, Type, UnaryOp, VarName,
};
use crate::float;

/// Renders `expr` as the reference renders an expression.
#[must_use]
pub fn export(expr: &Expr) -> Vec<u8> {
    let mut printer = Printer::default();
    printer.expr(expr, 0, 0);
    printer.out
}

/// Whether `byte` may appear in a variable name after its first character.
const fn is_name_byte(byte: u8) -> bool {
    byte == b'_' || byte >= 0x7f || byte.is_ascii_alphanumeric()
}

/// Whether `name` can be written after `$` without braces.
fn is_plain_name(name: &[u8]) -> bool {
    match name.split_first() {
        Some((&first, rest)) => {
            (first == b'_' || first >= 0x7f || first.is_ascii_alphabetic())
                && rest.iter().all(|&byte| is_name_byte(byte))
        }
        None => false,
    }
}

/// Whether text starting with `byte` would extend an interpolated `$name` written before it.
const fn extends_variable(byte: u8) -> bool {
    byte == b'[' || is_name_byte(byte)
}

/// How a binary operator prints: its text, its own priority, and the priorities its left and right
/// operands are printed at.
const fn binary_layout(op: BinaryOp) -> (&'static str, u16, u16, u16) {
    match op {
        BinaryOp::Add => (" + ", 200, 200, 201),
        BinaryOp::Sub => (" - ", 200, 200, 201),
        BinaryOp::Mul => (" * ", 210, 210, 211),
        BinaryOp::Div => (" / ", 210, 210, 211),
        BinaryOp::Mod => (" % ", 210, 210, 211),
        BinaryOp::ShiftLeft => (" << ", 190, 190, 191),
        BinaryOp::ShiftRight => (" >> ", 190, 190, 191),
        BinaryOp::Concat => (" . ", 185, 185, 186),
        BinaryOp::BitOr => (" | ", 140, 140, 141),
        BinaryOp::BitAnd => (" & ", 160, 160, 161),
        BinaryOp::BitXor => (" ^ ", 150, 150, 151),
        BinaryOp::Identical => (" === ", 170, 171, 171),
        BinaryOp::NotIdentical => (" !== ", 170, 171, 171),
        BinaryOp::Equal => (" == ", 170, 171, 171),
        BinaryOp::NotEqual => (" != ", 170, 171, 171),
        BinaryOp::Less => (" < ", 180, 181, 181),
        BinaryOp::LessOrEqual => (" <= ", 180, 181, 181),
        BinaryOp::Greater => (" > ", 180, 181, 181),
        BinaryOp::GreaterOrEqual => (" >= ", 180, 181, 181),
        BinaryOp::Spaceship => (" <=> ", 180, 181, 181),
        BinaryOp::Pow => (" ** ", 250, 251, 250),
        BinaryOp::Xor => (" xor ", 40, 40, 41),
        BinaryOp::And => (" && ", 130, 130, 131),
        BinaryOp::Or => (" || ", 120, 120, 121),
        BinaryOp::Coalesce => (" ?? ", 110, 111, 110),
        BinaryOp::Pipe => (" |> ", 183, 183, 184),
    }
}

const fn assign_text(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Add => " += ",
        AssignOp::Sub => " -= ",
        AssignOp::Mul => " *= ",
        AssignOp::Div => " /= ",
        AssignOp::Mod => " %= ",
        AssignOp::Pow => " **= ",
        AssignOp::Concat => " .= ",
        AssignOp::ShiftLeft => " <<= ",
        AssignOp::ShiftRight => " >>= ",
        AssignOp::BitOr => " |= ",
        AssignOp::BitAnd => " &= ",
        AssignOp::BitXor => " ^= ",
        AssignOp::Coalesce => " ??= ",
    }
}

const fn magic_text(constant: MagicConstant) -> &'static str {
    match constant {
        MagicConstant::Line => "__LINE__",
        MagicConstant::File => "__FILE__",
        MagicConstant::Dir => "__DIR__",
        MagicConstant::Class => "__CLASS__",
        MagicConstant::Trait => "__TRAIT__",
        MagicConstant::Method => "__METHOD__",
        MagicConstant::Function => "__FUNCTION__",
        MagicConstant::Property => "__PROPERTY__",
        MagicConstant::Namespace => "__NAMESPACE__",
    }
}

const fn cast_text(kind: CastKind) -> &'static str {
    match kind {
        CastKind::Int => "(int)",
        CastKind::Float => "(float)",
        CastKind::String => "(string)",
        CastKind::Array => "(array)",
        CastKind::Object => "(object)",
        CastKind::Bool => "(bool)",
        CastKind::Unset => "(unset)",
    }
}

const fn include_text(kind: IncludeKind) -> &'static str {
    match kind {
        IncludeKind::Include => "include",
        IncludeKind::IncludeOnce => "include_once",
        IncludeKind::Require => "require",
        IncludeKind::RequireOnce => "require_once",
        IncludeKind::Eval => "eval",
    }
}

/// Which kind of function is printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FunctionStyle {
    Declaration,
    Method,
    Closure,
    Arrow { parenthesized: bool },
}

#[derive(Debug, Default)]
struct Printer {
    out: Vec<u8>,
}

impl Printer {
    fn text(&mut self, text: &str) {
        self.out.extend_from_slice(text.as_bytes());
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
    }

    fn byte(&mut self, byte: u8) {
        self.out.push(byte);
    }

    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.text("    ");
        }
    }

    /// The escapes of a double-quoted (or backtick) string body.
    fn escaped(&mut self, text: &[u8], quote: u8) {
        for &byte in text {
            if byte < b' ' {
                match byte {
                    b'\n' => self.text("\\n"),
                    b'\r' => self.text("\\r"),
                    b'\t' => self.text("\\t"),
                    0x0c => self.text("\\f"),
                    0x0b => self.text("\\v"),
                    0x1b => self.text("\\e"),
                    _ => {
                        self.text("\\0");
                        self.byte(b'0' + byte / 8);
                        self.byte(b'0' + byte % 8);
                    }
                }
            } else {
                if byte == quote || byte == b'$' || byte == b'\\' {
                    self.byte(b'\\');
                }
                self.byte(byte);
            }
        }
    }

    /// A string literal: single-quoted unless it has control characters.
    fn string(&mut self, text: &[u8]) {
        if text.iter().any(|&byte| byte < b' ') {
            self.byte(b'"');
            self.escaped(text, b'"');
            self.byte(b'"');
            return;
        }
        self.byte(b'\'');
        for &byte in text {
            if byte == b'\'' || byte == b'\\' {
                self.byte(b'\\');
            }
            self.byte(byte);
        }
        self.byte(b'\'');
    }

    fn name(&mut self, name: &Name) {
        match name.kind {
            NameKind::Unqualified => {}
            NameKind::FullyQualified => self.byte(b'\\'),
            NameKind::Relative => self.text("namespace\\"),
        }
        self.bytes(&name.bytes);
    }

    /// An expression in a class or function name position.
    fn qualified(&mut self, expr: &Expr, indent: usize) {
        match &expr.kind {
            ExprKind::Name(name) => self.name(name),
            ExprKind::String(text) => {
                self.byte(b'\\');
                self.bytes(text);
            }
            _ => self.expr(expr, 0, indent),
        }
    }

    fn name_list(&mut self, names: &[Name], separator: &str) {
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                self.text(separator);
            }
            self.bytes(&name.bytes);
        }
    }

    /// A variable name after `$`.
    fn var_name(&mut self, name: &VarName, indent: usize) {
        match name {
            VarName::Name(bytes) => self.braced_name(bytes),
            VarName::Expr(expr) => self.computed_name(expr, indent),
        }
    }

    /// A property or method name after `->` or `::`.
    fn member_var(&mut self, name: &Member, indent: usize) {
        match name {
            Member::Name(bytes) => self.braced_name(bytes),
            Member::Expr(expr) => self.computed_name(expr, indent),
        }
    }

    fn braced_name(&mut self, bytes: &[u8]) {
        if is_plain_name(bytes) {
            self.bytes(bytes);
        } else {
            self.byte(b'{');
            self.bytes(bytes);
            self.byte(b'}');
        }
    }

    fn computed_name(&mut self, expr: &Expr, indent: usize) {
        if matches!(expr.kind, ExprKind::Variable(_)) {
            self.expr(expr, 0, indent);
        } else {
            self.byte(b'{');
            self.expr(expr, 0, indent);
            self.byte(b'}');
        }
    }

    /// A class constant name after `::`.
    fn member_name(&mut self, name: &Member, indent: usize) {
        match name {
            Member::Name(bytes) => self.bytes(bytes),
            Member::Expr(expr) => self.expr(expr, 0, indent),
        }
    }

    fn interpolated(&mut self, parts: &[StringPart], quote: u8, indent: usize) {
        for (index, part) in parts.iter().enumerate() {
            match part {
                StringPart::Literal(text) => self.escaped(text, quote),
                StringPart::Expr(expr) => {
                    let simple = matches!(&expr.kind, ExprKind::Variable(VarName::Name(_)))
                        || matches!(
                            &expr.kind,
                            ExprKind::Variable(VarName::Expr(inner))
                                if matches!(inner.kind, ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::String(_))
                        );
                    let next_extends = match parts.get(index + 1) {
                        Some(StringPart::Literal(text)) => {
                            extends_variable(text.first().copied().unwrap_or(0))
                        }
                        _ => false,
                    };
                    if simple && !next_extends {
                        self.expr(expr, 0, indent);
                    } else {
                        self.byte(b'{');
                        self.expr(expr, 0, indent);
                        self.byte(b'}');
                    }
                }
            }
        }
    }

    fn args(&mut self, args: &Args, indent: usize) {
        let list = match args {
            Args::CallableConvert => {
                self.text("...");
                return;
            }
            Args::List(list) => list,
        };
        for (index, arg) in list.iter().enumerate() {
            if index > 0 {
                self.text(", ");
            }
            match arg {
                Arg::Positional(value) => self.expr(value, 20, indent),
                Arg::Named { name, value } => {
                    self.bytes(name);
                    self.text(": ");
                    self.expr(value, 20, indent);
                }
                Arg::Unpack(value) => {
                    self.text("...");
                    self.expr(value, 20, indent);
                }
            }
        }
    }

    fn array(&mut self, items: &[Option<ArrayItem>], indent: usize) {
        self.byte(b'[');
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                self.text(", ");
            }
            match item {
                None => {}
                Some(ArrayItem::Value { key, value, by_ref }) => {
                    if let Some(key) = key {
                        self.expr(key, 80, indent);
                        self.text(" => ");
                    }
                    if *by_ref {
                        self.byte(b'&');
                    }
                    self.expr(value, 80, indent);
                }
                Some(ArrayItem::Spread(value)) => {
                    self.text("...");
                    self.expr(value, 20, indent);
                }
            }
        }
        self.byte(b']');
    }

    fn open(&mut self, priority: u16, own: u16) {
        if priority > own {
            self.byte(b'(');
        }
    }

    fn close(&mut self, priority: u16, own: u16) {
        if priority > own {
            self.byte(b')');
        }
    }

    fn binary(
        &mut self,
        layout: (&str, u16, u16, u16),
        operands: (&Expr, &Expr),
        priority: u16,
        indent: usize,
    ) {
        let (text, own, left, right) = layout;
        self.open(priority, own);
        self.expr(operands.0, left, indent);
        self.text(text);
        self.expr(operands.1, right, indent);
        self.close(priority, own);
    }

    fn prefix(&mut self, text: &str, operand: &Expr, priority: u16, indent: usize) {
        self.open(priority, 240);
        self.text(text);
        self.expr(operand, 241, indent);
        self.close(priority, 240);
    }

    fn function_call(&mut self, name: &str, operand: &Expr, indent: usize) {
        self.text(name);
        self.byte(b'(');
        self.expr(operand, 0, indent);
        self.byte(b')');
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one arm per expression kind keeps the layout rules in one place"
    )]
    fn expr(&mut self, expr: &Expr, priority: u16, indent: usize) {
        match &expr.kind {
            ExprKind::Int(value) => self.text(&value.to_string()),
            ExprKind::Float(value) => self.text(&float::export(*value)),
            ExprKind::String(text) => self.string(text),
            ExprKind::Interpolated(parts) => {
                self.byte(b'"');
                self.interpolated(parts, b'"', indent);
                self.byte(b'"');
            }
            ExprKind::ShellExec(parts) => {
                self.byte(b'`');
                self.interpolated(parts, b'`', indent);
                self.byte(b'`');
            }
            ExprKind::Name(name) | ExprKind::Constant(name) => self.name(name),
            ExprKind::Magic(constant) => self.text(magic_text(*constant)),
            ExprKind::Variable(name) => {
                self.byte(b'$');
                self.var_name(name, indent);
            }
            ExprKind::Array { items, .. } => self.array(items, indent),
            ExprKind::Dim { base, index } => {
                self.expr(base, 260, indent);
                self.byte(b'[');
                if let Some(index) = index {
                    self.expr(index, 0, indent);
                }
                self.byte(b']');
            }
            ExprKind::Property {
                object,
                name,
                nullsafe,
            } => {
                self.expr(object, 0, indent);
                self.text(if *nullsafe { "?->" } else { "->" });
                self.member_var(name, indent);
            }
            ExprKind::StaticProperty { class, name } => {
                self.qualified(class, indent);
                self.text("::$");
                self.var_name(name, indent);
            }
            ExprKind::Call { callee, args } => {
                if matches!(callee.kind, ExprKind::Closure(_)) {
                    self.byte(b'(');
                    self.expr(callee, 0, indent);
                    self.byte(b')');
                } else {
                    self.qualified(callee, indent);
                }
                self.byte(b'(');
                self.args(args, indent);
                self.byte(b')');
            }
            ExprKind::MethodCall {
                object,
                name,
                args,
                nullsafe,
            } => {
                self.expr(object, 0, indent);
                self.text(if *nullsafe { "?->" } else { "->" });
                self.member_var(name, indent);
                self.byte(b'(');
                self.args(args, indent);
                self.byte(b')');
            }
            ExprKind::StaticCall { class, name, args } => {
                self.qualified(class, indent);
                self.text("::");
                self.member_var(name, indent);
                self.byte(b'(');
                self.args(args, indent);
                self.byte(b')');
            }
            ExprKind::New { class, args } => {
                self.text("new ");
                self.qualified(class, indent);
                self.byte(b'(');
                self.args(args, indent);
                self.byte(b')');
            }
            ExprKind::NewAnonymous { class, args } => {
                self.text("new ");
                self.attributes(&class.attributes, indent, false);
                self.text("class");
                if !matches!(args, Args::List(list) if list.is_empty()) {
                    self.byte(b'(');
                    self.args(args, indent);
                    self.byte(b')');
                }
                self.class_body(class, indent);
            }
            ExprKind::ClassConstant { class, name } => {
                self.qualified(class, indent);
                self.text("::");
                self.member_name(name, indent);
            }
            ExprKind::ClassName(class) => {
                self.qualified(class, indent);
                self.text("::class");
            }
            ExprKind::Unary { op, operand } => {
                let text = match op {
                    UnaryOp::Plus => "+",
                    UnaryOp::Minus => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::BitNot => "~",
                    UnaryOp::Silence => "@",
                };
                self.prefix(text, operand, priority, indent);
            }
            ExprKind::Cast { kind, operand } => {
                self.prefix(cast_text(*kind), operand, priority, indent);
            }
            ExprKind::VoidCast(operand) => self.prefix("(void)", operand, priority, indent),
            ExprKind::IncDec { op, operand } => match op {
                IncDecOp::PreInc => self.prefix("++", operand, priority, indent),
                IncDecOp::PreDec => self.prefix("--", operand, priority, indent),
                IncDecOp::PostInc | IncDecOp::PostDec => {
                    self.open(priority, 240);
                    self.expr(operand, 241, indent);
                    self.text(if *op == IncDecOp::PostInc { "++" } else { "--" });
                    self.close(priority, 240);
                }
            },
            ExprKind::Binary { op, left, right } => {
                self.binary(binary_layout(*op), (left, right), priority, indent);
            }
            ExprKind::Instanceof { expr: value, class } => {
                self.expr(value, 0, indent);
                self.text(" instanceof ");
                self.qualified(class, indent);
            }
            ExprKind::Assign { target, value } => {
                self.binary((" = ", 90, 91, 90), (target, value), priority, indent);
            }
            ExprKind::AssignRef { target, value } => {
                self.binary((" =& ", 90, 91, 90), (target, value), priority, indent);
            }
            ExprKind::CompoundAssign { op, target, value } => {
                self.binary(
                    (assign_text(*op), 90, 91, 90),
                    (target, value),
                    priority,
                    indent,
                );
            }
            ExprKind::Ternary {
                condition,
                then,
                otherwise,
                ..
            } => {
                self.open(priority, 100);
                self.expr(condition, 100, indent);
                if let Some(then) = then {
                    self.text(" ? ");
                    self.expr(then, 101, indent);
                    self.text(" : ");
                } else {
                    self.text(" ?: ");
                }
                self.expr(otherwise, 101, indent);
                self.close(priority, 100);
            }
            ExprKind::Isset(operand) => self.function_call("isset", operand, indent),
            ExprKind::Empty(operand) => self.function_call("empty", operand, indent),
            ExprKind::Include {
                kind,
                expr: operand,
            } => {
                self.function_call(include_text(*kind), operand, indent);
            }
            ExprKind::Print(operand) => {
                self.open(priority, 60);
                self.text("print ");
                self.expr(operand, 61, indent);
                self.close(priority, 60);
            }
            ExprKind::Yield { key, value } => {
                self.open(priority, 70);
                self.text("yield ");
                if let Some(value) = value {
                    if let Some(key) = key {
                        self.expr(key, 70, indent);
                        self.text(" => ");
                    }
                    self.expr(value, 70, indent);
                }
                self.close(priority, 70);
            }
            ExprKind::YieldFrom(operand) => {
                self.open(priority, 85);
                self.text("yield from ");
                self.expr(operand, 86, indent);
                self.close(priority, 85);
            }
            ExprKind::Throw(operand) => {
                self.text("throw ");
                self.expr(operand, priority, indent);
            }
            ExprKind::Closure(function) => {
                self.function(function, FunctionStyle::Closure, indent);
            }
            ExprKind::ArrowFunction {
                function,
                parenthesized,
            } => self.function(
                function,
                FunctionStyle::Arrow {
                    parenthesized: *parenthesized,
                },
                indent,
            ),
            ExprKind::Match { subject, arms } => {
                self.text("match (");
                self.expr(subject, 0, indent);
                self.text(") {\n");
                for arm in arms {
                    self.match_arm(arm, indent + 1);
                }
                self.indent(indent);
                self.byte(b'}');
            }
            ExprKind::Reference(target) => {
                self.byte(b'&');
                self.expr(target, priority, indent);
            }
        }
    }

    fn match_arm(&mut self, arm: &MatchArm, indent: usize) {
        self.indent(indent);
        match &arm.conditions {
            Some(conditions) => {
                for (index, condition) in conditions.iter().enumerate() {
                    if index > 0 {
                        self.text(", ");
                    }
                    self.expr(condition, 0, indent);
                }
                self.text(" => ");
            }
            None => self.text("default => "),
        }
        self.expr(&arm.body, 0, 0);
        self.text(",\n");
    }

    fn attributes(&mut self, groups: &[AttributeGroup], indent: usize, newlines: bool) {
        for group in groups {
            self.text("#[");
            for (index, attribute) in group.iter().enumerate() {
                if index > 0 {
                    self.text(", ");
                }
                self.name(&attribute.name);
                if let Some(args) = &attribute.args {
                    self.byte(b'(');
                    self.args(args, indent);
                    self.byte(b')');
                }
            }
            self.byte(b']');
            if newlines {
                self.byte(b'\n');
                self.indent(indent);
            } else {
                self.byte(b' ');
            }
        }
    }

    fn visibility(&mut self, modifiers: Modifiers, with_set: bool) {
        if modifiers.contains(Modifiers::PUBLIC) {
            self.text("public ");
        } else if modifiers.contains(Modifiers::PROTECTED) {
            self.text("protected ");
        } else if modifiers.contains(Modifiers::PRIVATE) {
            self.text("private ");
        }
        if !with_set {
            return;
        }
        if modifiers.contains(Modifiers::PRIVATE_SET) {
            self.text("private(set) ");
        } else if modifiers.contains(Modifiers::PROTECTED_SET) {
            self.text("protected(set) ");
        } else if modifiers.contains(Modifiers::PUBLIC_SET) {
            self.text("public(set) ");
        }
    }

    fn type_decl(&mut self, declared: &Type) {
        match declared {
            Type::Union(types) | Type::Intersection(types) => {
                let separator = if matches!(declared, Type::Union(_)) {
                    b'|'
                } else {
                    b'&'
                };
                for (index, element) in types.iter().enumerate() {
                    if index > 0 {
                        self.byte(separator);
                    }
                    self.type_decl(element);
                }
            }
            Type::Simple { kind, nullable } => {
                if *nullable {
                    self.byte(b'?');
                }
                match kind {
                    SimpleType::Array => self.text("array"),
                    SimpleType::Callable => self.text("callable"),
                    SimpleType::Static => self.text("static"),
                    SimpleType::Name(name) if *nullable => self.bytes(&name.bytes),
                    SimpleType::Name(name) => self.name(name),
                }
            }
        }
    }

    fn params(&mut self, params: &[Param], indent: usize) {
        for (index, param) in params.iter().enumerate() {
            if index > 0 {
                self.text(", ");
            }
            self.attributes(&param.attributes, indent, false);
            self.visibility(param.modifiers, true);
            if param.modifiers.contains(Modifiers::FINAL) {
                self.text("final ");
            }
            if let Some(declared) = &param.declared_type {
                self.type_decl(declared);
                self.byte(b' ');
            }
            if param.by_ref {
                self.byte(b'&');
            }
            if param.variadic {
                self.text("...");
            }
            self.byte(b'$');
            self.bytes(&param.name);
            if let Some(default) = &param.default {
                self.text(" = ");
                self.expr(default, 0, indent);
            }
            if let Some(hooks) = &param.hooks {
                self.hooks(hooks, indent);
            }
        }
    }

    fn hooks(&mut self, hooks: &[Hook], indent: usize) {
        let inner = indent + 1;
        self.text(" {\n");
        self.indent(inner);
        for (index, hook) in hooks.iter().enumerate() {
            let mut flags = hook.modifiers;
            if hook.by_ref {
                flags = flags.union(Modifiers::PRIVATE_SET);
            }
            self.visibility(flags, true);
            if flags.contains(Modifiers::FINAL) {
                self.text("final ");
            }
            self.bytes(&hook.name);
            match &hook.body {
                HookBody::Absent => self.byte(b';'),
                HookBody::Expr(value) => {
                    self.text(" => ");
                    self.expr(value, 0, inner);
                    self.byte(b';');
                }
                HookBody::Block(body) => {
                    self.text(" {\n");
                    self.stmts(body, inner + 1);
                    self.indent(inner);
                    self.byte(b'}');
                }
            }
            if index + 1 < hooks.len() {
                self.byte(b'\n');
                self.indent(inner);
            }
        }
        self.byte(b'\n');
        self.indent(indent);
        self.byte(b'}');
    }

    fn function(&mut self, function: &Function, style: FunctionStyle, indent: usize) {
        let parenthesized = matches!(
            style,
            FunctionStyle::Arrow {
                parenthesized: true
            }
        );
        if parenthesized {
            self.byte(b'(');
        }
        let inline = matches!(style, FunctionStyle::Closure | FunctionStyle::Arrow { .. });
        self.attributes(&function.attributes, indent, !inline);
        self.visibility(function.modifiers, false);
        if function.modifiers.contains(Modifiers::STATIC) {
            self.text("static ");
        }
        if function.modifiers.contains(Modifiers::ABSTRACT) {
            self.text("abstract ");
        }
        if function.modifiers.contains(Modifiers::FINAL) {
            self.text("final ");
        }
        self.text(if matches!(style, FunctionStyle::Arrow { .. }) {
            "fn"
        } else {
            "function "
        });
        if function.by_ref {
            self.byte(b'&');
        }
        if !inline {
            self.bytes(&function.name);
        }
        self.byte(b'(');
        self.params(&function.params, indent);
        self.byte(b')');
        if !function.uses.is_empty() {
            self.text(" use(");
            for (index, captured) in function.uses.iter().enumerate() {
                if index > 0 {
                    self.text(", ");
                }
                if captured.by_ref {
                    self.byte(b'&');
                }
                self.byte(b'$');
                self.bytes(&captured.name);
            }
            self.byte(b')');
        }
        if let Some(declared) = &function.return_type {
            self.text(": ");
            self.type_decl(declared);
        }
        match &function.body {
            FunctionBody::Expr(body) => {
                self.text(" => ");
                self.expr(body, 0, indent);
                if parenthesized {
                    self.byte(b')');
                }
            }
            FunctionBody::Block(body) => {
                self.text(" {\n");
                self.stmts(body, indent + 1);
                self.indent(indent);
                self.byte(b'}');
                if style != FunctionStyle::Closure {
                    self.byte(b'\n');
                }
            }
            FunctionBody::Absent => self.text(";\n"),
        }
    }

    /// `extends`, `implements` and the member block of a class.
    fn class_body(&mut self, class: &ClassLike, indent: usize) {
        if let Some(parent) = &class.extends {
            self.text(" extends ");
            self.name(parent);
        }
        if !class.implements.is_empty() {
            self.text(" implements ");
            self.name_list(&class.implements, ", ");
        }
        self.text(" {\n");
        for member in &class.members {
            self.member(member, indent + 1);
        }
        self.indent(indent);
        self.byte(b'}');
    }

    fn class(&mut self, class: &ClassLike, indent: usize) {
        self.attributes(&class.attributes, indent, true);
        match class.kind {
            ClassKind::Interface => self.text("interface "),
            ClassKind::Trait => self.text("trait "),
            ClassKind::Enum => self.text("enum "),
            ClassKind::Class => {
                if class.modifiers.contains(Modifiers::ABSTRACT) {
                    self.text("abstract ");
                }
                if class.modifiers.contains(Modifiers::FINAL) {
                    self.text("final ");
                }
                if class.modifiers.contains(Modifiers::READONLY) {
                    self.text("readonly ");
                }
                self.text("class ");
            }
        }
        if let Some(name) = &class.name {
            self.bytes(name);
        }
        if let (ClassKind::Enum, Some(backing)) = (class.kind, &class.backing_type) {
            self.text(": ");
            self.type_decl(backing);
        }
        self.class_body(class, indent);
        self.byte(b'\n');
    }

    fn property_group(&mut self, group: &PropertyGroup, indent: usize) {
        self.attributes(&group.attributes, indent, true);
        self.visibility(group.modifiers, true);
        if group.modifiers.contains(Modifiers::STATIC) {
            self.text("static ");
        }
        if group.modifiers.contains(Modifiers::READONLY) {
            self.text("readonly ");
        }
        if let Some(declared) = &group.property_type {
            self.type_decl(declared);
            self.byte(b' ');
        }
        for (index, property) in group.properties.iter().enumerate() {
            if index > 0 {
                self.text(", ");
            }
            self.byte(b'$');
            self.bytes(&property.name);
            if let Some(default) = &property.default {
                self.text(" = ");
                self.expr(default, 0, indent + 1);
            }
            if let Some(hooks) = &property.hooks {
                self.hooks(hooks, indent);
            }
        }
        let hooked = group
            .properties
            .first()
            .is_some_and(|property| property.hooks.is_some());
        if !hooked {
            self.byte(b';');
        }
    }

    fn method_reference(&mut self, reference: &MethodReference) {
        if let Some(class) = &reference.class {
            self.bytes(&class.bytes);
            self.text("::");
        }
        self.bytes(&reference.method);
    }

    fn member(&mut self, member: &ClassMember, indent: usize) {
        self.indent(indent);
        match member {
            ClassMember::Property(group) => self.property_group(group, indent),
            ClassMember::Constant(group) => {
                self.attributes(&group.attributes, indent, true);
                self.visibility(group.modifiers, false);
                self.text("const ");
                if let Some(declared) = &group.constant_type {
                    self.type_decl(declared);
                    self.byte(b' ');
                }
                for (index, item) in group.items.iter().enumerate() {
                    if index > 0 {
                        self.text(", ");
                    }
                    self.bytes(&item.name);
                    self.text(" = ");
                    self.expr(&item.value, 20, indent);
                }
                self.byte(b';');
            }
            ClassMember::Method(function) => {
                self.function(function, FunctionStyle::Method, indent);
            }
            ClassMember::EnumCase(case) => {
                self.attributes(&case.attributes, indent, true);
                self.text("case ");
                self.bytes(&case.name);
                if let Some(value) = &case.value {
                    self.text(" = ");
                    self.expr(value, 0, indent);
                }
                self.byte(b';');
            }
            ClassMember::TraitUse(trait_use) => {
                self.text("use ");
                self.name_list(&trait_use.traits, ", ");
                match &trait_use.adaptations {
                    None => self.byte(b';'),
                    Some(adaptations) => {
                        self.text(" {\n");
                        for adaptation in adaptations {
                            self.indent(indent + 1);
                            self.adaptation(adaptation);
                            self.text(";\n");
                        }
                        self.indent(indent);
                        self.byte(b'}');
                    }
                }
            }
        }
        self.byte(b'\n');
    }

    fn adaptation(&mut self, adaptation: &TraitAdaptation) {
        match adaptation {
            TraitAdaptation::Precedence { method, instead_of } => {
                self.method_reference(method);
                self.text(" insteadof ");
                self.name_list(instead_of, ", ");
            }
            TraitAdaptation::Alias {
                method,
                modifier,
                alias,
            } => {
                self.method_reference(method);
                if modifier.contains(Modifiers::PUBLIC) {
                    self.text(" as public");
                } else if modifier.contains(Modifiers::PROTECTED) {
                    self.text(" as protected");
                } else if modifier.contains(Modifiers::PRIVATE) {
                    self.text(" as private");
                } else if alias.is_some() {
                    self.text(" as");
                }
                if let Some(alias) = alias {
                    self.byte(b' ');
                    self.bytes(alias);
                }
            }
        }
    }

    fn stmts(&mut self, list: &[Stmt], indent: usize) {
        for statement in list {
            self.stmt(statement, indent);
        }
    }

    /// A statement on its own line(s); a block prints its statements at the same depth.
    fn stmt(&mut self, statement: &Stmt, indent: usize) {
        let terminated = match &statement.kind {
            StmtKind::Nop => return,
            StmtKind::Block(list) => {
                self.stmts(list, indent);
                return;
            }
            StmtKind::Label(_)
            | StmtKind::If(_)
            | StmtKind::Switch { .. }
            | StmtKind::While { .. }
            | StmtKind::Try(_)
            | StmtKind::For(_)
            | StmtKind::Foreach(_)
            | StmtKind::Function(_)
            | StmtKind::Class(_)
            | StmtKind::Declare { .. } => false,
            _ => true,
        };
        self.indent(indent);
        self.stmt_body(statement, indent);
        if terminated {
            self.byte(b';');
        }
        self.byte(b'\n');
    }

    /// The body of a nested block: `{`, its statements one level deeper, and the closing `}`.
    fn nested(&mut self, body: &Stmt, indent: usize) {
        self.text(") {\n");
        self.stmt(body, indent + 1);
        self.indent(indent);
        self.byte(b'}');
    }

    fn keyword_operand(&mut self, keyword: &str, operand: Option<&Expr>, indent: usize) {
        self.text(keyword);
        if let Some(operand) = operand {
            self.byte(b' ');
            self.expr(operand, 0, indent);
        }
    }

    fn expr_list(&mut self, list: &[Expr], indent: usize) {
        for (index, item) in list.iter().enumerate() {
            if index > 0 {
                self.text(", ");
            }
            self.expr(item, 20, indent);
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one arm per statement kind keeps the layout rules in one place"
    )]
    fn stmt_body(&mut self, statement: &Stmt, indent: usize) {
        match &statement.kind {
            StmtKind::Nop | StmtKind::Block(_) => {}
            StmtKind::Expr(value) => self.expr(value, 0, indent),
            StmtKind::Echo(value) => self.keyword_operand("echo", Some(value), indent),
            StmtKind::If(branches) => {
                let mut current = &**branches;
                loop {
                    for (index, (condition, body)) in current.branches.iter().enumerate() {
                        if index == 0 {
                            self.text("if (");
                        } else {
                            self.indent(indent);
                            self.text("} elseif (");
                        }
                        self.expr(condition, 0, indent);
                        self.text(") {\n");
                        self.stmt(body, indent + 1);
                    }
                    match &current.otherwise {
                        Some(Stmt {
                            kind: StmtKind::If(next),
                            ..
                        }) => {
                            self.indent(indent);
                            self.text("} else ");
                            current = next;
                        }
                        Some(body) => {
                            self.indent(indent);
                            self.text("} else {\n");
                            self.stmt(body, indent + 1);
                            break;
                        }
                        None => break,
                    }
                }
                self.indent(indent);
                self.byte(b'}');
            }
            StmtKind::While { condition, body } => {
                self.text("while (");
                self.expr(condition, 0, indent);
                self.nested(body, indent);
            }
            StmtKind::DoWhile { body, condition } => {
                self.text("do {\n");
                self.stmt(body, indent + 1);
                self.indent(indent);
                self.text("} while (");
                self.expr(condition, 0, indent);
                self.byte(b')');
            }
            StmtKind::For(parts) => {
                self.text("for (");
                self.expr_list(&parts.init, indent);
                self.byte(b';');
                if !parts.condition.is_empty() {
                    self.byte(b' ');
                    self.expr_list(&parts.condition, indent);
                }
                self.byte(b';');
                if !parts.step.is_empty() {
                    self.byte(b' ');
                    self.expr_list(&parts.step, indent);
                }
                self.nested(&parts.body, indent);
            }
            StmtKind::Foreach(parts) => {
                self.text("foreach (");
                self.expr(&parts.subject, 0, indent);
                self.text(" as ");
                if let Some(key) = &parts.key {
                    self.expr(key, 0, indent);
                    self.text(" => ");
                }
                self.expr(&parts.value, 0, indent);
                self.nested(&parts.body, indent);
            }
            StmtKind::Switch { subject, cases } => {
                self.text("switch (");
                self.expr(subject, 0, indent);
                self.text(") {\n");
                for case in cases {
                    self.indent(indent + 1);
                    match &case.condition {
                        Some(condition) => {
                            self.text("case ");
                            self.expr(condition, 0, indent + 1);
                            self.text(":\n");
                        }
                        None => self.text("default:\n"),
                    }
                    self.stmts(&case.body, indent + 2);
                }
                self.indent(indent);
                self.byte(b'}');
            }
            StmtKind::Break(value) => self.keyword_operand("break", value.as_ref(), indent),
            StmtKind::Continue(value) => self.keyword_operand("continue", value.as_ref(), indent),
            StmtKind::Return(value) => self.keyword_operand("return", value.as_ref(), indent),
            StmtKind::Global(value) => self.keyword_operand("global", Some(value), indent),
            StmtKind::Static { name, default } => {
                self.text("static $");
                self.bytes(name);
                if let Some(default) = default {
                    self.text(" = ");
                    self.expr(default, 0, indent);
                }
            }
            StmtKind::Unset(value) => self.function_call("unset", value, indent),
            StmtKind::Label(name) => {
                self.bytes(name);
                self.byte(b':');
            }
            StmtKind::Goto(name) => {
                self.text("goto ");
                self.bytes(name);
            }
            StmtKind::Try(parts) => {
                self.text("try {\n");
                self.stmts(&parts.body, indent + 1);
                self.indent(indent);
                for catch in &parts.catches {
                    self.text("} catch (");
                    self.name_list(&catch.types, "|");
                    if let Some(variable) = &catch.variable {
                        self.text(" $");
                        self.braced_name(variable);
                    }
                    self.text(") {\n");
                    self.stmts(&catch.body, indent + 1);
                    self.indent(indent);
                }
                if let Some(finally) = &parts.finally {
                    self.text("} finally {\n");
                    self.stmts(finally, indent + 1);
                    self.indent(indent);
                }
                self.byte(b'}');
            }
            StmtKind::Declare { directives, body } => {
                self.text("declare(");
                for (index, directive) in directives.iter().enumerate() {
                    if index > 0 {
                        self.text(", ");
                    }
                    self.bytes(&directive.name);
                    self.text(" = ");
                    self.expr(&directive.value, 0, indent);
                }
                self.byte(b')');
                match body {
                    Some(body) => {
                        self.text(" {\n");
                        self.stmt(body, indent + 1);
                        self.indent(indent);
                        self.byte(b'}');
                    }
                    None => self.byte(b';'),
                }
            }
            StmtKind::Function(function) => {
                self.function(function, FunctionStyle::Declaration, indent);
            }
            StmtKind::Class(class) => self.class(class, indent),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{export, extends_variable, is_plain_name};
    use crate::ast::{Expr, ExprKind};

    fn literal(kind: ExprKind) -> Expr {
        Expr { kind, line: 1 }
    }

    #[test]
    fn literals_use_the_canonical_spelling() {
        assert_eq!(
            export(&literal(ExprKind::String(b"it's".to_vec()))),
            b"'it\\'s'"
        );
        assert_eq!(
            export(&literal(ExprKind::String(b"a\n$".to_vec()))),
            b"\"a\\n\\$\""
        );
        assert_eq!(export(&literal(ExprKind::String(vec![1]))), b"\"\\001\"");
        assert_eq!(export(&literal(ExprKind::Float(2.0))), b"2.0");
        assert_eq!(export(&literal(ExprKind::Int(-3))), b"-3");
    }

    #[test]
    fn variable_names_and_interpolation_boundaries() {
        assert!(is_plain_name(b"a1"));
        assert!(!is_plain_name(b"1a"));
        assert!(!is_plain_name(b""));
        assert!(extends_variable(b'['));
        assert!(extends_variable(b'x'));
        assert!(!extends_variable(0));
        assert!(!extends_variable(b' '));
    }
}
