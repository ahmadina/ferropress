//! The compile-time checks, run over the tree in the order the reference compiler visits it.
//!
//! The reference reports a diagnostic on its *current line*: the line of the statement or
//! expression it last started compiling, never restored when a child finishes. [`Checker::line`]
//! follows the same rule, so a diagnostic raised between two children reports the line of the
//! last node visited.

mod attributes;
mod classes;
mod constant;
mod expr;
mod hooks;
mod jumps;
mod members;
mod methods;
mod names;
mod types;

use std::collections::{HashMap, HashSet};

use php_parser::ast::{
    Case, ClassKind, Expr, ExprKind, Function, FunctionBody, Modifiers, Param, Program, Stmt,
    StmtKind, Try,
};
use php_value::Value;

use crate::diagnostic::{Diagnostic, Diagnostics, Severity};
use attributes::Target;
use constant::Folded;
use jumps::{Jumpable, Jumps};
use types::{ANONYMOUS_CLASS, DeclaredType, Mask};

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

/// The modifiers that make a constructor parameter a promoted property.
const PROMOTION: Modifiers = Modifiers::VISIBILITY
    .union(Modifiers::SET_VISIBILITY)
    .union(Modifiers::READONLY)
    .union(Modifiers::FINAL);

pub(crate) fn is_auto_global(name: &[u8]) -> bool {
    AUTO_GLOBALS.contains(&name)
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
    /// A closure or arrow function, by its generated name.
    Closure(ClosureName),
}

/// The generated name of a closure. Inside a method of an anonymous class the name embeds the
/// class's generated name, which continues after a NUL byte with a per-process counter; only the
/// text before that byte, which is all that messages show, is known then.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ClosureName {
    /// The name, or its text up to the NUL byte when incomplete.
    text: Vec<u8>,
    /// Whether `text` is the whole name.
    complete: bool,
}

/// The state of the function being compiled; the file's top level is a function too.
#[derive(Debug, Default)]
struct Frame {
    /// What is being compiled.
    code: Code,
    /// The property whose default, attributes or hooks are being compiled.
    property: Option<Vec<u8>>,
    /// The kind of the property hook being compiled.
    hook: Option<hooks::HookKind>,
    /// Whether `#[\NoDiscard]` applies to the function.
    no_discard: bool,
    /// The enclosing loops, switches and `finally` blocks, the labels and the pending jumps.
    jumps: Jumps,
    /// Names of the `static` variables declared so far.
    statics: Vec<Vec<u8>>,
    /// The declared return type, or the one implied by the function's name.
    returns: Option<DeclaredType>,
    /// Whether the function is a generator, whose `return` statements are not type-checked.
    is_generator: bool,
    /// Whether the function returns, or for a generator yields, by reference.
    by_ref: bool,
    /// The compiled types of the parameters so far, `None` for untyped ones.
    params: Vec<Option<DeclaredType>>,
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
    kind: ClassKind,
    /// `abstract`, `final` and `readonly` as declared.
    modifiers: Modifiers,
    /// The resolved name of the class a class extends.
    parent: Option<Vec<u8>>,
    /// Whether the class is an enum with a valid backing type.
    is_backed: bool,
    /// The properties declared so far, including promoted ones.
    properties: HashSet<Vec<u8>>,
    /// The constants and enum cases declared so far.
    constants: HashSet<Vec<u8>>,
}

impl ClassScope {
    fn is_trait(&self) -> bool {
        self.kind == ClassKind::Trait
    }

    /// The name the reference prints in messages about the class.
    fn label(&self) -> &[u8] {
        self.name.as_deref().unwrap_or(ANONYMOUS_CLASS)
    }
}

pub(crate) struct Checker<'a> {
    /// The path of the file being compiled.
    file: &'a [u8],
    line: u32,
    warnings: Vec<Diagnostic>,
    frame: Frame,
    namespace: Option<Vec<u8>>,
    names: names::FileNames,
    class: Option<ClassScope>,
    /// What constant folding made of the nodes it changed.
    folded: HashMap<*const Expr, Folded<'a>>,
}

impl<'a> Checker<'a> {
    fn new(file: &'a [u8]) -> Self {
        Self {
            file,
            line: 1,
            warnings: Vec::new(),
            frame: Frame::default(),
            namespace: None,
            names: names::FileNames::default(),
            class: None,
            folded: HashMap::new(),
        }
    }

    pub(crate) fn run(program: &'a Program, file: &'a [u8]) -> Diagnostics {
        let mut checker = Self::new(file);
        let fatal = checker
            .top_statements(&program.statements, true)
            .and_then(|()| {
                let jumps = std::mem::take(&mut checker.frame.jumps);
                checker.resolve_jumps(&jumps)
            })
            .err();
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

    fn stmts(&mut self, stmts: &'a [Stmt]) -> Flow {
        stmts.iter().try_for_each(|stmt| self.stmt(stmt))
    }

    fn stmt(&mut self, stmt: &'a Stmt) -> Flow {
        self.line = stmt.line;
        match &stmt.kind {
            StmtKind::Nop => Ok(()),
            StmtKind::Label(name) => {
                if self.frame.jumps.define(name) {
                    return Ok(());
                }
                let mut message = b"Label '".to_vec();
                message.extend_from_slice(name);
                message.extend_from_slice(b"' already defined");
                self.fatal(message)
            }
            StmtKind::Goto(label) => {
                self.frame.jumps.goto(label, self.line);
                Ok(())
            }
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
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.return_value(value)?;
                }
                self.returned(value.as_ref())
            }
            StmtKind::Global(variable) => self.global(variable),
            StmtKind::Static { name, default } => self.static_variable(name, default.as_ref()),
            StmtKind::Unset(variable) => self.unset(variable),
            StmtKind::Try(try_stmt) => self.try_stmt(try_stmt),
            StmtKind::Declare { directives, body } => {
                self.declare(directives, body.as_deref(), None)
            }
            StmtKind::Function(function) => self.function_declaration(function, false),
            StmtKind::Class(class) => self.class(class),
        }
    }

    fn exprs(&mut self, exprs: &'a [Expr]) -> Flow {
        exprs.iter().try_for_each(|expr| self.expr(expr))
    }

    fn in_jumpable(&mut self, kind: Jumpable, body: impl FnOnce(&mut Self) -> Flow) -> Flow {
        self.frame.jumps.enter(kind);
        let result = body(self);
        self.frame.jumps.leave();
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
        let available = self.frame.jumps.depth();
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
        self.frame.jumps.break_continue(target, self.line);
        if !is_break && self.frame.jumps.kind(target) == Some(Jumpable::Switch) {
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
        let finally_id = try_stmt
            .finally
            .is_some()
            .then(|| self.frame.jumps.try_with_finally());
        self.stmts(&try_stmt.body)?;
        for catch in &try_stmt.catches {
            self.line = catch.line;
            self.caught_classes(&catch.types, catch.variable.as_deref())?;
            self.stmts(&catch.body)?;
        }
        let (Some(finally), Some(id)) = (&try_stmt.finally, finally_id) else {
            return Ok(());
        };
        self.frame.jumps.enter_finally(id);
        let result = self.stmts(finally);
        self.frame.jumps.leave_finally();
        result
    }

    /// Compiles `function`, which is `code`, in a frame of its own, then resolves its jumps.
    fn function(&mut self, function: &'a Function, code: Code) -> Flow {
        let frame = self.function_frame(function, code)?;
        self.resolve_jumps(&frame.jumps)
    }

    /// Compiles `function` like [`Self::function`] up to its jumps, and returns its frame.
    fn function_frame(&mut self, function: &'a Function, code: Code) -> Flow<Frame> {
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
        let target = if matches!(self.frame.code, Code::Method { .. }) {
            Target::Method
        } else {
            Target::Function
        };
        let result = self
            .attributes(&function.attributes, target)
            .and_then(|applied| {
                self.frame.no_discard = applied.no_discard;
                if !is_free {
                    return self.function_inner(function);
                }
                let outer_class = self.class.take();
                let result = self.function_inner(function);
                self.class = outer_class;
                result
            });
        let frame = std::mem::replace(&mut self.frame, outer);
        result.map(|()| frame)
    }

    fn function_inner(&mut self, function: &'a Function) -> Flow {
        self.signature(function)?;
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
        let uses = function.uses.iter().map(|use_var| use_var.name.clone());
        self.frame.statics.extend(uses);
        self.no_discard_return()?;
        match &function.body {
            FunctionBody::Block(stmts) => self.stmts(stmts),
            FunctionBody::Expr(expr) => {
                let line = self.line;
                let returns_never = self
                    .frame
                    .returns
                    .as_ref()
                    .is_some_and(|returns| returns.mask().intersects(Mask::NEVER));
                if returns_never {
                    return self.expr(expr);
                }
                self.return_value(expr)?;
                self.line = line;
                self.returned(Some(expr))
            }
            FunctionBody::Absent => Ok(()),
        }
    }

    /// Compiles the return type and the parameters, which the reference does before the body.
    fn signature(&mut self, function: &'a Function) -> Flow {
        let returns = match &function.return_type {
            Some(declared) => Some(self.declared_type(declared, false)?.0),
            None => match &self.frame.code {
                Code::Method { name, .. } if name.eq_ignore_ascii_case(b"__tostring") => {
                    Some(DeclaredType::of(Mask::STRING))
                }
                _ => None,
            },
        };
        let returns_void = returns
            .as_ref()
            .is_some_and(|returns| returns.mask().intersects(Mask::VOID));
        if function.by_ref && returns_void {
            self.deprecated_in_function(
                b"Returning by reference from a void function is deprecated",
            );
        }
        self.params(
            &function.params,
            function.modifiers.contains(Modifiers::ABSTRACT),
        )?;
        if let (true, Some(returns)) = (function.is_generator, &returns)
            && !returns.admits_generators()
        {
            let mut message = b"Generator return type must be a supertype of Generator, ".to_vec();
            message.extend(returns.display());
            message.extend_from_slice(b" given");
            return self.fatal(message);
        }
        self.frame.returns = returns;
        self.frame.is_generator = function.is_generator;
        self.frame.by_ref = function.by_ref;
        Ok(())
    }

    /// Compiles the parameters of a function, which is abstract when `is_abstract`.
    fn params(&mut self, params: &'a [Param], is_abstract: bool) -> Flow {
        let last_required = params
            .iter()
            .rposition(|param| param.default.is_none() && !param.variadic);
        for (index, param) in params.iter().enumerate() {
            let required = last_required
                .filter(|&last| index < last)
                .map(|last| params[last].name.as_slice());
            self.param(&params[..index], param, required)?;
            if param.hooks.is_some() || param.modifiers.intersects(PROMOTION) {
                self.promoted_property(param, is_abstract)?;
            }
        }
        Ok(())
    }

    /// Compiles `param`, which follows `earlier`; `required` names the last required parameter
    /// when one follows it.
    fn param(&mut self, earlier: &'a [Param], param: &'a Param, required: Option<&[u8]>) -> Flow {
        self.line = param.line;
        if is_auto_global(&param.name) {
            let mut message = b"Cannot re-assign auto-global variable ".to_vec();
            message.extend_from_slice(&param.name);
            return self.fatal(message);
        }
        if earlier.iter().any(|other| other.name == param.name) {
            let mut message = b"Redefinition of parameter $".to_vec();
            message.extend_from_slice(&param.name);
            return self.fatal(message);
        }
        if param.name == b"this" {
            return self.fatal("Cannot use $this as parameter");
        }
        if earlier.iter().any(|other| other.variadic) {
            return self.fatal("Only the last parameter can be variadic");
        }
        if param.variadic && param.default.is_some() {
            return self.fatal("Variadic parameter cannot have a default value");
        }
        let default = match &param.default {
            Some(default) => Some(self.constant(default, true)?),
            None => None,
        };
        let promoted = param.hooks.is_some() || param.modifiers.intersects(PROMOTION);
        self.attributes_on(
            &param.attributes,
            Target::Parameter,
            promoted.then_some(Target::Property),
        )?;
        let mut forced = false;
        let mut compiled = None;
        if let Some(declared) = &param.declared_type {
            let force_null = !promoted && matches!(default, Some(Some(Value::Null)));
            let (declared, was_forced) = self.declared_type(declared, force_null)?;
            forced = was_forced;
            if forced {
                let mut message = b"Implicitly marking parameter $".to_vec();
                message.extend_from_slice(&param.name);
                message.extend_from_slice(
                    b" as nullable is deprecated, the explicit nullable type must be used instead",
                );
                self.deprecated_in_function(&message);
            }
            if declared.mask().intersects(Mask::VOID) {
                return self.fatal("void cannot be used as a parameter type");
            }
            if declared.mask().intersects(Mask::NEVER) {
                return self.fatal("never cannot be used as a parameter type");
            }
            if let Some(Some(value)) = &default
                && !force_null
                && !declared.accepts(value)
            {
                let mut message = format!(
                    "Cannot use {} as default value for parameter $",
                    types::type_name(value)
                )
                .into_bytes();
                message.extend_from_slice(&param.name);
                message.extend_from_slice(b" of type ");
                message.extend(declared.display());
                return self.fatal(message);
            }
            compiled = Some(declared);
        }
        self.frame.params.push(compiled);
        if let (Some(required), Some(_), false) = (required, &default, forced) {
            let mut message = b"Optional parameter $".to_vec();
            message.extend_from_slice(&param.name);
            message.extend_from_slice(b" declared before required parameter $");
            message.extend_from_slice(required);
            message.extend_from_slice(b" is implicitly treated as a required parameter");
            self.deprecated_in_function(&message);
        }
        Ok(())
    }

    /// Reports `message` as a deprecation about the function being compiled, which the
    /// reference prefixes with the function's name.
    fn deprecated_in_function(&mut self, message: &[u8]) {
        let mut text = match &self.frame.code {
            Code::Function(name) => name.clone(),
            Code::Method {
                class: Some(class),
                name,
            } => {
                let mut text = class.clone();
                text.extend_from_slice(b"::");
                text.extend_from_slice(name);
                text
            }
            Code::Method { class: None, .. } => ANONYMOUS_CLASS.to_vec(),
            Code::Closure(name) => name.text.clone(),
            Code::File => return,
        };
        text.extend_from_slice(b"(): ");
        text.extend_from_slice(message);
        self.report(Severity::Deprecated, text);
    }

    /// Checks that a function with `#[\NoDiscard]` can return a value.
    fn no_discard_return(&self) -> Flow {
        let (true, Some(returns)) = (self.frame.no_discard, &self.frame.returns) else {
            return Ok(());
        };
        let kind = if self.class.is_some() {
            "method"
        } else {
            "function"
        };
        let mask = returns.mask();
        let returned = if mask.intersects(Mask::VOID) {
            "void"
        } else if mask.intersects(Mask::NEVER) {
            "never returning"
        } else {
            return Ok(());
        };
        self.fatal(format!(
            "A {returned} {kind} does not return a value, but #[\\NoDiscard] requires a return value"
        ))
    }

    /// Checks a `return` of `value` against the declared return type.
    fn returned(&self, value: Option<&'a Expr>) -> Flow {
        let (false, Some(returns)) = (self.frame.is_generator, &self.frame.returns) else {
            return Ok(());
        };
        let kind = if self.class.is_some() {
            "method"
        } else {
            "function"
        };
        let mask = returns.mask();
        if mask.intersects(Mask::VOID) {
            return match value {
                None => Ok(()),
                Some(value) if self.is_null_constant(value) => self.fatal(format!(
                    "A void {kind} must not return a value (did you mean \"return;\" instead of \"return null;\"?)"
                )),
                Some(_) => self.fatal(format!("A void {kind} must not return a value")),
            };
        }
        if mask.intersects(Mask::NEVER) {
            return self.fatal(format!("A never-returning {kind} must not return"));
        }
        match value {
            Some(_) => Ok(()),
            None if mask.intersects(Mask::NULL) => self.fatal(format!(
                "A {kind} with return type must return a value (did you mean \"return null;\" instead of \"return;\"?)"
            )),
            None => self.fatal(format!("A {kind} with return type must return a value")),
        }
    }

    /// Whether `value` compiles to the constant `null`.
    fn is_null_constant(&self, value: &'a Expr) -> bool {
        match &self.resolve(value).kind {
            ExprKind::Constant(name) => matches!(self.special_constant(name), Some(Value::Null)),
            _ => false,
        }
    }
}
