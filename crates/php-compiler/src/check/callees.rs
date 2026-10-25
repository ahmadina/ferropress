//! The functions and methods a call is known to reach while compiling. A known callee decides how
//! each argument is fetched: a variable passed to a by-value parameter is read, one passed to a
//! by-reference parameter is written, and with an unknown callee the fetch waits for run time.

use std::borrow::Cow;
use std::collections::HashMap;

use php_parser::ast::{
    ClassKind, ClassLike, ClassMember, Expr, ExprKind, Function, Member, Modifiers, Name, NameKind,
    Param,
};
use php_value::Value;

use super::{Checker, ClassScope, Code};

/// Methods by lowercase name.
pub(super) type Methods<'a> = HashMap<Vec<u8>, &'a Function>;

/// The functions and classes bound while compiling the file, by lowercase name.
#[derive(Debug, Default)]
pub(super) struct Bound<'a> {
    functions: HashMap<Vec<u8>, &'a Function>,
    classes: HashMap<Vec<u8>, Methods<'a>>,
}

impl<'a> Bound<'a> {
    /// Records the top-level function `lower`, once its body has compiled.
    pub(super) fn function(&mut self, lower: Vec<u8>, function: &'a Function) {
        self.functions.entry(lower).or_insert(function);
    }

    /// Records the top-level class `class`, compiled into `scope`, when it is bound while
    /// compiling: it has no parent, no interfaces and no traits, it is not an enum (which always
    /// implements an interface), and no set hook's parameter type waits for run time to be
    /// checked. The first class of a name wins.
    pub(super) fn class(&mut self, class: &ClassLike, scope: ClassScope<'a>) {
        let has_traits = class
            .members
            .iter()
            .any(|member| matches!(member, ClassMember::TraitUse(_)));
        let Some(name) = scope.name else {
            return;
        };
        if class.extends.is_some()
            || !class.implements.is_empty()
            || class.kind == ClassKind::Enum
            || has_traits
            || scope.links_at_run_time
        {
            return;
        }
        self.classes
            .entry(name.to_ascii_lowercase())
            .or_insert(scope.methods);
    }
}

/// Whether the parameter at `position` of `params` takes its argument by reference. Arguments past
/// the declared parameters go to the variadic one, or are taken by value when there is none.
fn by_ref(params: &[Param], position: usize) -> bool {
    let variadic = params.last().filter(|param| param.variadic);
    let fixed = params.len() - usize::from(variadic.is_some());
    match params.get(position) {
        Some(param) if position < fixed => param.by_ref,
        _ => variadic.is_some_and(|param| param.by_ref),
    }
}

/// How an argument is fetched once the callee is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Sent {
    ByValue,
    ByRef,
}

/// The parameters of a known callee.
#[derive(Debug, Clone, Copy)]
pub(super) struct Callee<'a>(&'a [Param]);

impl Callee<'_> {
    /// How the positional argument at `position` is sent.
    pub(super) fn positional(self, position: usize) -> Sent {
        if by_ref(self.0, position) {
            Sent::ByRef
        } else {
            Sent::ByValue
        }
    }

    /// How the argument named `name` is sent, when it names a parameter other than the variadic
    /// one (compared case-sensitively); `None` leaves the fetch to run time.
    pub(super) fn named(self, name: &[u8]) -> Option<Sent> {
        self.0
            .iter()
            .take_while(|param| !param.variadic)
            .position(|param| param.name == name)
            .map(|position| self.positional(position))
    }
}

/// A class named as written, or by a string.
enum ClassName<'n> {
    SelfClass,
    Named(Cow<'n, [u8]>),
}

impl<'a> Checker<'a> {
    /// The function a call of `callee` reaches, when known while compiling: a function the file
    /// declared at the top level before, named so that the name resolves while compiling.
    pub(super) fn called_function(&self, callee: &Expr) -> Option<Callee<'a>> {
        let name = match &callee.kind {
            ExprKind::Name(name) => self.compile_time_function_name(name)?,
            ExprKind::String(text) => text.strip_prefix(b"\\").unwrap_or(text).to_vec(),
            _ => return None,
        };
        let function = self.bound.functions.get(&name.to_ascii_lowercase())?;
        Some(Callee(&function.params))
    }

    /// The name of a function call when it resolves while compiling: anything but an unqualified
    /// one-part name inside a namespace that no `use function` imports, which falls back to the
    /// global function at run time.
    fn compile_time_function_name(&self, name: &Name) -> Option<Vec<u8>> {
        let bytes = name.bytes.as_slice();
        match name.kind {
            NameKind::FullyQualified => Some(bytes.to_vec()),
            NameKind::Relative => Some(self.qualify(bytes)),
            NameKind::Unqualified if bytes.contains(&b'\\') => Some(self.resolve_class(name)),
            NameKind::Unqualified => match self.names.imports.function(bytes) {
                Some(imported) => Some(imported.to_vec()),
                None if self.namespace.is_some() => None,
                None => Some(bytes.to_vec()),
            },
        }
    }

    /// The method `$this->name()` reaches, when known while compiling: in a method of the class
    /// itself (not static, not a closure, not a trait), a method declared before that is private
    /// or final, so no child class can replace it.
    pub(super) fn called_method(&self, object: &Expr, name: &'a Member) -> Option<Callee<'a>> {
        if !super::expr::is_this(object)
            || !matches!(self.frame.code, Code::Method { .. })
            || self.frame.modifiers.contains(Modifiers::STATIC)
        {
            return None;
        }
        let class = self.class.as_ref().filter(|class| !class.is_trait())?;
        let method = class
            .methods
            .get(&self.member_text(name)?.to_ascii_lowercase())?;
        method
            .modifiers
            .intersects(Modifiers::PRIVATE.union(Modifiers::FINAL))
            .then_some(Callee(&method.params))
    }

    /// The method `class::name()` reaches, when known while compiling: `self` where the scope is
    /// known, or a class bound before or being compiled, and a method other than the constructor
    /// that is public or belongs to the class being compiled.
    pub(super) fn called_static_method(
        &self,
        class: &'a Expr,
        name: &'a Member,
    ) -> Option<Callee<'a>> {
        let method = self.member_text(name)?.to_ascii_lowercase();
        if method == b"__construct" {
            return None;
        }
        let active = self.class.as_ref();
        let (methods, own) = match self.called_class(class)? {
            ClassName::SelfClass => {
                let scope_known = !matches!(self.frame.code, Code::Closure(_));
                let class = active.filter(|class| scope_known && !class.is_trait())?;
                (&class.methods, true)
            }
            ClassName::Named(name) => {
                let lower = name.to_ascii_lowercase();
                if let Some(methods) = self.bound.classes.get(&lower) {
                    (methods, false)
                } else {
                    let class = active.filter(|class| {
                        class
                            .name
                            .as_deref()
                            .is_some_and(|own| own.eq_ignore_ascii_case(&name))
                    })?;
                    (&class.methods, true)
                }
            }
        };
        let function = methods.get(&method)?;
        let public = !function
            .modifiers
            .intersects(Modifiers::PRIVATE.union(Modifiers::PROTECTED));
        (public || own).then_some(Callee(&function.params))
    }

    /// The class a static call names while compiling: `self`, or a resolved name. A special name
    /// is special unless fully qualified; a string names a class fully qualified, with or without
    /// its leading `\` (a special one has already failed as an invalid class name).
    fn called_class(&self, class: &'a Expr) -> Option<ClassName<'a>> {
        let text = match &class.kind {
            ExprKind::Name(name) if name.kind != NameKind::FullyQualified => {
                if is_special(&name.bytes) {
                    return special(&name.bytes);
                }
                Cow::Owned(self.resolve_class(name))
            }
            ExprKind::Name(name) => Cow::Borrowed(name.bytes.as_slice()),
            _ => match self.compiled_value(class)? {
                Value::String(text) => {
                    let bytes = text.as_bytes();
                    Cow::Owned(bytes.strip_prefix(b"\\").unwrap_or(bytes).to_vec())
                }
                _ => return None,
            },
        };
        Some(ClassName::Named(text))
    }

    /// The text of a member name known while compiling.
    fn member_text(&self, member: &'a Member) -> Option<Cow<'a, [u8]>> {
        match member {
            Member::Name(name) => Some(Cow::Borrowed(name)),
            Member::Expr(expr) => match self.value_of(expr)? {
                Value::String(text) => Some(Cow::Owned(text.as_bytes().to_vec())),
                _ => None,
            },
        }
    }
}

fn is_special(name: &[u8]) -> bool {
    [b"self".as_slice(), b"static", b"parent"]
        .iter()
        .any(|special| name.eq_ignore_ascii_case(special))
}

/// The class a special name names while compiling: only `self` names one.
fn special(name: &[u8]) -> Option<ClassName<'static>> {
    name.eq_ignore_ascii_case(b"self")
        .then_some(ClassName::SelfClass)
}

#[cfg(test)]
mod tests {
    use super::{Callee, Sent, by_ref};
    use crate::check::Checker;
    use php_parser::ast::Param;
    use php_parser::{ParseOptions, parse};

    /// The diagnostics compiling `source` reports, as `message @line`.
    fn diagnostics(source: &str) -> Vec<String> {
        let source = format!("<?php {source}");
        let program = parse(source.as_bytes(), ParseOptions::default())
            .unwrap()
            .result
            .unwrap();
        let checked = Checker::run(&program, b"");
        checked
            .warnings
            .iter()
            .chain(&checked.fatal)
            .map(|diagnostic| {
                format!(
                    "{} @{}",
                    String::from_utf8_lossy(&diagnostic.message),
                    diagnostic.line
                )
            })
            .collect()
    }

    fn assert_cases(cases: &[(&str, &[&str])]) {
        for (source, expected) in cases {
            assert_eq!(diagnostics(source), *expected, "{source}");
        }
    }

    const READ: &str = "Cannot use [] for reading @1";
    const TEMPORARY: &str = "Cannot use temporary expression in write context @1";

    /// The parameters of the first function `source` declares.
    fn params(source: &str) -> Vec<Param> {
        let source = format!("<?php {source}");
        let program = parse(source.as_bytes(), ParseOptions::default())
            .unwrap()
            .result
            .unwrap();
        match &program.statements[0] {
            php_parser::ast::TopStatement::Statement(stmt) => match &stmt.kind {
                php_parser::ast::StmtKind::Function(function) => function.params.clone(),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn arguments_past_the_parameters_go_to_the_variadic_one() {
        let fixed = params("function t($a, &$b) {}");
        assert!(!by_ref(&fixed, 0));
        assert!(by_ref(&fixed, 1));
        assert!(!by_ref(&fixed, 2));
        let variadic = params("function t($a, &...$b) {}");
        assert!(!by_ref(&variadic, 0));
        assert!(by_ref(&variadic, 1));
        assert!(by_ref(&variadic, 5));
        assert!(!by_ref(&[], 0));
    }

    #[test]
    fn named_arguments_match_parameters_other_than_the_variadic_one() {
        let params = params("function t($a, &$b, ...$c) {}");
        let callee = Callee(&params);
        assert_eq!(callee.named(b"a"), Some(Sent::ByValue));
        assert_eq!(callee.named(b"b"), Some(Sent::ByRef));
        assert_eq!(callee.named(b"B"), None);
        assert_eq!(callee.named(b"c"), None);
        assert_eq!(callee.positional(1), Sent::ByRef);
    }

    #[test]
    fn functions_bound_before_the_call_decide_the_fetch() {
        assert_cases(&[
            ("function t($v) {} t($a[]);", &[READ]),
            ("function t($v) {} T(v: $a[]); \\t($a[]);", &[READ]),
            ("function t($v) {} \"\\\\t\"($a[]);", &[READ]),
            ("t($a[]); function t($v) {}", &[]),
            ("function t($v) { t($a[]); }", &[]),
            ("if (1) { function t($v) {} } t($a[]);", &[]),
            ("function t(&$v) {} t($a[]); t(1); t($a?->b);", &[]),
            ("function t(&$v) {} t(\"abc\"[0]);", &[TEMPORARY]),
            ("function t(&$v) {} t($GLOBALS); t($this);", &[]),
            ("function t($v) {} t(w: $a[]); t(...$b);", &[]),
            ("function t() {} t($a[]);", &[READ]),
            ("namespace A; function t($v) {} t($a[]);", &[]),
            (
                "namespace A; function t($v) {} namespace\\t($a[]);",
                &[READ],
            ),
            (
                "namespace A { function t($v) {} } namespace B { use function A\\t; t($a[]); }",
                &[READ],
            ),
            (
                "namespace A { function t($v) {} } namespace B { use A; A\\t($a[]); }",
                &[READ],
            ),
        ]);
    }

    #[test]
    fn this_calls_know_private_and_final_methods_declared_before() {
        assert_cases(&[
            (
                "class C { private function m($v) {} function n() { $this->m($a[]); } }",
                &[READ],
            ),
            (
                "class C { final function m($v) {} function n() { $this?->M($a[]); } }",
                &[READ],
            ),
            (
                "class C { private function m($v) { $this->{'m'}($a[]); } }",
                &[READ],
            ),
            (
                "class C { function m($v) {} function n() { $this->m($a[]); } }",
                &[],
            ),
            (
                "class C { function n() { $this->m($a[]); } private function m($v) {} }",
                &[],
            ),
            (
                "class C { private function m($v) {} static function n() { $this->m($a[]); } }",
                &[],
            ),
            (
                "class C { private function m($v) {} function n() { fn() => $this->m($a[]); } }",
                &[],
            ),
            (
                "trait T { private function m($v) {} function n() { $this->m($a[]); } }",
                &[],
            ),
            (
                "class C { private function m($v) {} public $p { get => $this->m($a[]); } }",
                &[READ],
            ),
        ]);
    }

    #[test]
    fn static_calls_know_methods_of_bound_and_current_classes() {
        assert_cases(&[
            ("class C { static function m($v) {} } C::m($a[]);", &[READ]),
            (
                "class C { static function m($v) {} } \"\\\\C\"::m($a[]);",
                &[READ],
            ),
            (
                "class C { static function m($v) {} } (C::class)::m($a[]);",
                &[READ],
            ),
            (
                "class C { static function m($v) {} } c::{'M'}($a[]);",
                &[READ],
            ),
            (
                "interface I { static function m($v); } I::m($a[]);",
                &[READ],
            ),
            (
                "class C { private static function m($v) { self::m($a[]); } }",
                &[READ],
            ),
            (
                "class C { private static function m($v) { C::m($a[]); } }",
                &[READ],
            ),
            (
                "class C { static function m($v) {} function n() { namespace\\self::m($a[]); } }",
                &[READ],
            ),
            (
                "class C { static function m($v) {} function n() { \"self\"::m($a[]); } }",
                &["'\\self' is an invalid class name @1"],
            ),
            (
                "class C { static function m($v) {} function n() { \\self::m($a[]); } }",
                &["'\\self' is an invalid class name @1"],
            ),
            (
                "class C { static function m($v) {} function n() { static::m($a[]); } }",
                &[],
            ),
            (
                "class C { static function m($v) {} function n() { fn() => self::m($a[]); } }",
                &[],
            ),
            (
                "class C { private static function m($v) {} } C::m($a[]);",
                &[],
            ),
            (
                "class C { protected static function m($v) {} } C::m($a[]);",
                &[],
            ),
            (
                "class C { function __construct($v) {} } C::__construct($a[]);",
                &[],
            ),
            (
                "if (1) { class C { static function m($v) {} } } C::m($a[]);",
                &[],
            ),
            (
                "class P {} class C extends P { static function m($v) {} } C::m($a[]);",
                &[],
            ),
            (
                "interface I {} class C implements I { static function m($v) {} } C::m($a[]);",
                &[],
            ),
            (
                "class C { use T; static function m($v) {} } C::m($a[]);",
                &[],
            ),
            (
                "enum E { case A; static function m($v) {} } E::m($a[]);",
                &[],
            ),
            (
                "class C { public A $p { set(B $v) {} } static function m($v) {} } C::m($a[]);",
                &[],
            ),
            (
                "class C { public int $p { set(int|string $v) {} } static function m($v) {} } C::m($a[]);",
                &[READ],
            ),
            (
                "class C { static function m(&$v) {} } C::m(\"abc\"[0]);",
                &[TEMPORARY],
            ),
            (
                "class C { static function m($v) {} } class C {} C::m($a[]);",
                &[READ],
            ),
            (
                "C::m($a[]); $o = new class { static function m($v) {} };",
                &[],
            ),
        ]);
    }
}
