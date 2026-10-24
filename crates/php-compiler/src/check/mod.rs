//! The compile-time checks, run over the tree in the order the reference compiler visits it.
//!
//! The reference reports a diagnostic on its *current line*: the line of the statement or
//! expression it last started compiling, never restored when a child finishes. [`Checker::line`]
//! follows the same rule, so a diagnostic raised between two children reports the line of the
//! last node visited.

mod constant;
mod expr;

use std::collections::HashMap;

use php_parser::ast::{
    Case, Catch, ClassKind, ClassLike, ClassMember, Expr, ExprKind, Function, FunctionBody,
    HookBody, Program, Stmt, StmtKind, TopStatement, Try,
};

use crate::diagnostic::{Diagnostic, Diagnostics, Severity};
use constant::Folded;

/// The result of a check; `Err` carries the fatal error that stops compilation.
pub(crate) type Flow<T = ()> = Result<T, Box<Diagnostic>>;

/// Names PHP treats as superglobals.
const AUTO_GLOBALS: [&[u8]; 8] = [
    b"GLOBALS",
    b"_GET",
    b"_POST",
    b"_COOKIE",
    b"_SERVER",
    b"_ENV",
    b"_REQUEST",
    b"_FILES",
];

pub(crate) fn is_auto_global(name: &[u8]) -> bool {
    AUTO_GLOBALS.contains(&name)
}

/// A loop or `switch` that `break` and `continue` can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Jumpable {
    Loop,
    Switch,
}

/// The code a frame compiles, which names `__FUNCTION__` and `__METHOD__`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
enum Code {
    /// The top level of the file.
    #[default]
    File,
    /// A function, by its namespaced name.
    Function(Vec<u8>),
    /// A method or property hook of `class` (`None` when anonymous).
    Method {
        class: Option<Vec<u8>>,
        name: Vec<u8>,
    },
    /// A closure or arrow function, whose generated name holds the file path.
    Closure,
}

/// The state of the function being compiled; the file's top level is a function too.
#[derive(Debug, Default)]
struct Frame {
    /// What is being compiled.
    code: Code,
    /// The property whose default, attributes or hooks are being compiled.
    property: Option<Vec<u8>>,
    /// The enclosing loops and switches, innermost last.
    jumpables: Vec<Jumpable>,
    /// Names of the `static` variables declared so far.
    statics: Vec<Vec<u8>>,
}

impl Frame {
    fn new(code: Code) -> Self {
        Self {
            code,
            ..Self::default()
        }
    }
}

/// The class-like declaration being compiled.
#[derive(Debug, Clone)]
struct ClassScope {
    /// The namespaced name; `None` for an anonymous class, whose name holds the file path.
    name: Option<Vec<u8>>,
    is_trait: bool,
    has_parent: bool,
}

pub(crate) struct Checker<'a> {
    line: u32,
    warnings: Vec<Diagnostic>,
    frame: Frame,
    namespace: Option<Vec<u8>>,
    class: Option<ClassScope>,
    /// What constant folding made of the nodes it changed.
    folded: HashMap<*const Expr, Folded<'a>>,
}

impl<'a> Checker<'a> {
    fn new() -> Self {
        Self {
            line: 1,
            warnings: Vec::new(),
            frame: Frame::default(),
            namespace: None,
            class: None,
            folded: HashMap::new(),
        }
    }

    pub(crate) fn run(program: &'a Program) -> Diagnostics {
        let mut checker = Self::new();
        let fatal = checker.top_statements(&program.statements).err();
        Diagnostics {
            warnings: checker.warnings,
            fatal: fatal.map(|fatal| *fatal),
        }
    }

    /// Stops compilation with `message` on the current line.
    fn fatal<T>(&self, message: impl Into<Vec<u8>>) -> Flow<T> {
        Err(Box::new(Diagnostic {
            severity: Severity::Fatal,
            message: message.into(),
            line: self.line,
        }))
    }

    /// Reports `message` on the current line and continues.
    fn report(&mut self, severity: Severity, message: impl Into<Vec<u8>>) {
        self.warnings.push(Diagnostic {
            severity,
            message: message.into(),
            line: self.line,
        });
    }

    /// `name` in the current namespace.
    fn qualify(&self, name: &[u8]) -> Vec<u8> {
        match &self.namespace {
            Some(namespace) => {
                let mut full = namespace.clone();
                full.push(b'\\');
                full.extend_from_slice(name);
                full
            }
            None => name.to_vec(),
        }
    }

    fn top_statements(&mut self, statements: &'a [TopStatement]) -> Flow {
        for statement in statements {
            match statement {
                TopStatement::Statement(stmt) => self.stmt(stmt)?,
                TopStatement::Namespace {
                    name,
                    body: Some(body),
                    line,
                } => {
                    self.line = *line;
                    self.namespace.clone_from(name);
                    self.top_statements(body)?;
                    self.namespace = None;
                }
                TopStatement::Namespace {
                    name,
                    body: None,
                    line,
                } => {
                    self.line = *line;
                    self.namespace.clone_from(name);
                }
                TopStatement::Const {
                    items,
                    attributes,
                    line,
                } => {
                    self.line = *line;
                    for item in items {
                        self.constant(&item.value, true)?;
                        if [b"true".as_slice(), b"false", b"null"]
                            .iter()
                            .any(|special| item.name.eq_ignore_ascii_case(special))
                        {
                            let mut message = b"Cannot redeclare constant '".to_vec();
                            message.extend_from_slice(&item.name);
                            message.push(b'\'');
                            return self.fatal(message);
                        }
                    }
                    if !attributes.is_empty() {
                        if items.len() > 1 {
                            return self
                                .fatal("Cannot apply attributes to multiple constants at once");
                        }
                        self.attributes(attributes)?;
                    }
                }
                TopStatement::HaltCompiler { line }
                | TopStatement::Use { line, .. }
                | TopStatement::GroupUse { line, .. } => self.line = *line,
            }
        }
        Ok(())
    }

    fn stmts(&mut self, stmts: &'a [Stmt]) -> Flow {
        stmts.iter().try_for_each(|stmt| self.stmt(stmt))
    }

    fn stmt(&mut self, stmt: &'a Stmt) -> Flow {
        self.line = stmt.line;
        match &stmt.kind {
            StmtKind::Nop | StmtKind::Label(_) | StmtKind::Goto(_) => Ok(()),
            StmtKind::Block(stmts) => self.stmts(stmts),
            StmtKind::Expr(expr) | StmtKind::Echo(expr) => self.expr(expr),
            StmtKind::If(branches) => {
                for (condition, body) in &branches.branches {
                    self.expr(condition)?;
                    self.stmt(body)?;
                }
                branches
                    .otherwise
                    .as_ref()
                    .map_or(Ok(()), |body| self.stmt(body))
            }
            StmtKind::While { condition, body } => {
                self.in_jumpable(Jumpable::Loop, |checker| checker.stmt(body))?;
                self.expr(condition)
            }
            StmtKind::DoWhile { body, condition } => {
                self.in_jumpable(Jumpable::Loop, |checker| checker.stmt(body))?;
                self.expr(condition)
            }
            StmtKind::For(for_loop) => {
                self.exprs(&for_loop.init)?;
                self.in_jumpable(Jumpable::Loop, |checker| checker.stmt(&for_loop.body))?;
                self.exprs(&for_loop.step)?;
                self.exprs(&for_loop.condition)
            }
            StmtKind::Foreach(foreach) => self.foreach(foreach),
            StmtKind::Switch { subject, cases } => self.switch(subject, cases),
            StmtKind::Break(depth) => self.break_continue(true, depth.as_ref()),
            StmtKind::Continue(depth) => self.break_continue(false, depth.as_ref()),
            StmtKind::Return(value) => value.as_ref().map_or(Ok(()), |value| self.expr(value)),
            StmtKind::Global(variable) => self.global(variable),
            StmtKind::Static { name, default } => self.static_variable(name, default.as_ref()),
            StmtKind::Unset(variable) => self.unset(variable),
            StmtKind::Try(try_stmt) => self.try_stmt(try_stmt),
            StmtKind::Declare { directives, body } => {
                for directive in directives {
                    self.expr(&directive.value)?;
                }
                body.as_ref().map_or(Ok(()), |body| self.stmt(body))
            }
            StmtKind::Function(function) => {
                let name = self.qualify(&function.name);
                self.function(function, Code::Function(name))
            }
            StmtKind::Class(class) => self.class(class),
        }
    }

    fn exprs(&mut self, exprs: &'a [Expr]) -> Flow {
        exprs.iter().try_for_each(|expr| self.expr(expr))
    }

    fn in_jumpable(&mut self, kind: Jumpable, body: impl FnOnce(&mut Self) -> Flow) -> Flow {
        self.frame.jumpables.push(kind);
        let result = body(self);
        self.frame.jumpables.pop();
        result
    }

    fn foreach(&mut self, foreach: &'a php_parser::ast::Foreach) -> Flow {
        if let Some(key) = &foreach.key {
            match key.kind {
                ExprKind::Reference(_) => return self.fatal("Key element cannot be a reference"),
                ExprKind::Array { .. } => return self.fatal("Cannot use list as key element"),
                _ => {}
            }
        }
        self.expr(&foreach.subject)?;
        let value = match &foreach.value.kind {
            ExprKind::Reference(inner) => inner,
            _ => &foreach.value,
        };
        if expr::is_this(value) {
            return self.fatal("Cannot re-assign $this");
        }
        self.assign_target(value)?;
        if let Some(key) = &foreach.key {
            self.assign_target(key)?;
        }
        self.in_jumpable(Jumpable::Loop, |checker| checker.stmt(&foreach.body))
    }

    fn switch(&mut self, subject: &'a Expr, cases: &'a [Case]) -> Flow {
        self.expr(subject)?;
        self.in_jumpable(Jumpable::Switch, |checker| {
            let mut has_default = false;
            for case in cases {
                if case.semicolon {
                    checker.line = case.line;
                    checker.report(
                        Severity::Deprecated,
                        "Case statements followed by a semicolon (;) are deprecated, use a colon (:) instead",
                    );
                }
                match &case.condition {
                    Some(condition) => checker.expr(condition)?,
                    None if has_default => {
                        checker.line = case.line;
                        return checker
                            .fatal("Switch statements may only contain one default clause");
                    }
                    None => has_default = true,
                }
            }
            cases.iter().try_for_each(|case| checker.stmts(&case.body))
        })
    }

    fn break_continue(&mut self, is_break: bool, depth: Option<&Expr>) -> Flow {
        let keyword = if is_break { "break" } else { "continue" };
        let depth = match depth.map(|depth| &depth.kind) {
            None => 1,
            Some(ExprKind::Int(depth)) if *depth >= 1 => *depth,
            Some(ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::String(_)) => {
                return self.fatal(format!(
                    "'{keyword}' operator accepts only positive integers"
                ));
            }
            Some(_) => {
                return self.fatal(format!(
                    "'{keyword}' operator with non-integer operand is no longer supported"
                ));
            }
        };
        let available = self.frame.jumpables.len();
        if available == 0 {
            return self.fatal(format!("'{keyword}' not in the 'loop' or 'switch' context"));
        }
        let Some(target) = usize::try_from(depth)
            .ok()
            .and_then(|depth| available.checked_sub(depth))
        else {
            let plural = if depth == 1 { "" } else { "s" };
            return self.fatal(format!("Cannot '{keyword}' {depth} level{plural}"));
        };
        if !is_break && self.frame.jumpables[target] == Jumpable::Switch {
            let suggestion = if target == 0 {
                String::new()
            } else {
                format!(". Did you mean to use \"continue {}\"?", depth + 1)
            };
            let message = if depth == 1 {
                format!("\"continue\" targeting switch is equivalent to \"break\"{suggestion}")
            } else {
                format!(
                    "\"continue {depth}\" targeting switch is equivalent to \"break {depth}\"{suggestion}"
                )
            };
            self.report(Severity::Warning, message);
        }
        Ok(())
    }

    fn global(&mut self, variable: &'a Expr) -> Flow {
        if let ExprKind::Variable(php_parser::ast::VarName::Expr(name)) = &variable.kind {
            self.expr(name)?;
        }
        if expr::is_this(variable) {
            return self.fatal("Cannot use $this as global variable");
        }
        Ok(())
    }

    fn static_variable(&mut self, name: &[u8], default: Option<&'a Expr>) -> Flow {
        if name == b"this" {
            return self.fatal("Cannot use $this as static variable");
        }
        if self.frame.statics.iter().any(|known| known == name) {
            let mut message = b"Duplicate declaration of static variable $".to_vec();
            message.extend_from_slice(name);
            return self.fatal(message);
        }
        self.frame.statics.push(name.to_vec());
        default.map_or(Ok(()), |default| self.expr(default))
    }

    fn try_stmt(&mut self, try_stmt: &'a Try) -> Flow {
        if try_stmt.catches.is_empty() && try_stmt.finally.is_none() {
            return self.fatal("Cannot use try without catch or finally");
        }
        self.stmts(&try_stmt.body)?;
        for Catch { body, .. } in &try_stmt.catches {
            self.stmts(body)?;
        }
        try_stmt
            .finally
            .as_ref()
            .map_or(Ok(()), |finally| self.stmts(finally))
    }

    /// Compiles `function`, which is `code`, in a frame of its own.
    fn function(&mut self, function: &'a Function, code: Code) -> Flow {
        self.line = function.line;
        for use_var in &function.uses {
            if use_var.name == b"this" {
                return self.fatal("Cannot use $this as lexical variable");
            }
            if is_auto_global(&use_var.name) {
                return self.fatal("Cannot use auto-global as lexical variable");
            }
            if function
                .uses
                .iter()
                .take_while(|other| !std::ptr::eq(*other, use_var))
                .any(|other| other.name == use_var.name)
            {
                let mut message = b"Cannot use variable $".to_vec();
                message.extend_from_slice(&use_var.name);
                message.extend_from_slice(b" twice");
                return self.fatal(message);
            }
        }
        let is_free = matches!(code, Code::Function(_));
        let outer = std::mem::replace(&mut self.frame, Frame::new(code));
        let outer_class = self.class.clone();
        let result = self.attributes(&function.attributes).and_then(|()| {
            if is_free {
                self.class = None;
            }
            self.function_inner(function)
        });
        self.frame = outer;
        self.class = outer_class;
        result
    }

    fn function_inner(&mut self, function: &'a Function) -> Flow {
        for (index, param) in function.params.iter().enumerate() {
            self.line = param.line;
            if is_auto_global(&param.name) {
                let mut message = b"Cannot re-assign auto-global variable ".to_vec();
                message.extend_from_slice(&param.name);
                return self.fatal(message);
            }
            if function.params[..index]
                .iter()
                .any(|earlier| earlier.name == param.name)
            {
                let mut message = b"Redefinition of parameter $".to_vec();
                message.extend_from_slice(&param.name);
                return self.fatal(message);
            }
            if param.name == b"this" {
                return self.fatal("Cannot use $this as parameter");
            }
            if let Some(default) = &param.default {
                self.constant(default, true)?;
            }
            self.attributes(&param.attributes)?;
        }
        for use_var in &function.uses {
            if function
                .params
                .iter()
                .any(|param| param.name == use_var.name)
            {
                let mut message = b"Cannot use lexical variable $".to_vec();
                message.extend_from_slice(&use_var.name);
                message.extend_from_slice(b" as a parameter name");
                return self.fatal(message);
            }
        }
        match &function.body {
            FunctionBody::Block(stmts) => self.stmts(stmts),
            FunctionBody::Expr(expr) => self.expr(expr),
            FunctionBody::Absent => Ok(()),
        }
    }

    fn class(&mut self, class: &'a ClassLike) -> Flow {
        self.line = class.line;
        let scope = ClassScope {
            name: class.name.as_deref().map(|name| self.qualify(name)),
            is_trait: class.kind == ClassKind::Trait,
            has_parent: class.kind == ClassKind::Class && class.extends.is_some(),
        };
        let outer = self.class.replace(scope);
        let result = self.class_body(class);
        self.class = outer;
        result
    }

    fn class_body(&mut self, class: &'a ClassLike) -> Flow {
        self.attributes(&class.attributes)?;
        for member in &class.members {
            match member {
                ClassMember::Method(method) => {
                    let code = Code::Method {
                        class: self.class.as_ref().and_then(|scope| scope.name.clone()),
                        name: method.name.clone(),
                    };
                    self.function(method, code)?;
                }
                ClassMember::Property(group) => {
                    self.line = group.line;
                    for property in &group.properties {
                        let outer = self.frame.property.replace(property.name.clone());
                        let result = self.property(property, &group.attributes);
                        self.frame.property = outer;
                        result?;
                    }
                }
                ClassMember::Constant(group) => {
                    self.line = group.line;
                    for item in &group.items {
                        self.constant(&item.value, false)?;
                        self.attributes(&group.attributes)?;
                    }
                }
                ClassMember::EnumCase(case) => {
                    self.line = case.line;
                    if let Some(value) = &case.value {
                        self.constant(value, false)?;
                    }
                    self.attributes(&case.attributes)?;
                }
                ClassMember::TraitUse(_) => {}
            }
        }
        Ok(())
    }

    fn property(
        &mut self,
        property: &'a php_parser::ast::Property,
        attributes: &'a [php_parser::ast::AttributeGroup],
    ) -> Flow {
        if let Some(default) = &property.default {
            self.constant(default, false)?;
        }
        for hook in property.hooks.iter().flatten() {
            let mut name = b"$".to_vec();
            name.extend_from_slice(&property.name);
            name.extend_from_slice(b"::");
            name.extend_from_slice(&hook.name);
            let code = Code::Method {
                class: self.class.as_ref().and_then(|scope| scope.name.clone()),
                name,
            };
            self.hook_body(&hook.body, code)?;
        }
        self.attributes(attributes)
    }

    fn hook_body(&mut self, body: &'a HookBody, code: Code) -> Flow {
        let property = self.frame.property.clone();
        let outer = std::mem::replace(
            &mut self.frame,
            Frame {
                property,
                ..Frame::new(code)
            },
        );
        let result = match body {
            HookBody::Absent => Ok(()),
            HookBody::Expr(expr) => self.expr(expr),
            HookBody::Block(stmts) => self.stmts(stmts),
        };
        self.frame = outer;
        result
    }
}
