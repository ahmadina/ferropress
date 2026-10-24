//! Method declarations: their modifiers and bodies, redeclarations, and the signatures the
//! reference requires of magic methods.

use std::collections::HashSet;

use php_parser::ast::{ClassKind, ClassLike, Function, FunctionBody, Modifiers};

use super::types::{DeclaredType, Mask};
use super::{Checker, Code, Flow, Frame};
use crate::diagnostic::Severity;

/// Whether a magic method must be static.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Binding {
    Instance,
    Static,
}

/// What the reference requires of a magic method.
struct Magic {
    /// The lowercase name.
    name: &'static [u8],
    /// The exact number of non-variadic parameters, when fixed.
    args: Option<u32>,
    binding: Binding,
    /// Whether a non-public declaration warns.
    public: bool,
    /// Whether a return type is forbidden.
    no_return: bool,
    /// The kinds each declared parameter type must admit.
    params: &'static [Mask],
    /// The kinds a declared return type may hold.
    returns: Option<Mask>,
}

impl Magic {
    const fn instance(name: &'static [u8], args: Option<u32>) -> Self {
        Self {
            name,
            args,
            binding: Binding::Instance,
            public: true,
            no_return: false,
            params: &[],
            returns: None,
        }
    }

    const fn returning(self, returns: Mask) -> Self {
        Self {
            returns: Some(returns),
            ..self
        }
    }

    const fn taking(self, params: &'static [Mask]) -> Self {
        Self { params, ..self }
    }

    const fn static_method(self) -> Self {
        Self {
            binding: Binding::Static,
            ..self
        }
    }
}

const MAGIC: [Magic; 17] = [
    Magic {
        public: false,
        no_return: true,
        ..Magic::instance(b"__construct", None)
    },
    Magic {
        public: false,
        no_return: true,
        ..Magic::instance(b"__destruct", Some(0))
    },
    Magic {
        public: false,
        ..Magic::instance(b"__clone", Some(0)).returning(Mask::VOID)
    },
    Magic::instance(b"__get", Some(1)).taking(&[Mask::STRING]),
    Magic::instance(b"__set", Some(2))
        .taking(&[Mask::STRING])
        .returning(Mask::VOID),
    Magic::instance(b"__unset", Some(1))
        .taking(&[Mask::STRING])
        .returning(Mask::VOID),
    Magic::instance(b"__isset", Some(1))
        .taking(&[Mask::STRING])
        .returning(Mask::BOOL),
    Magic::instance(b"__call", Some(2)).taking(&[Mask::STRING, Mask::ARRAY]),
    Magic::instance(b"__callstatic", Some(2))
        .taking(&[Mask::STRING, Mask::ARRAY])
        .static_method(),
    Magic::instance(b"__tostring", Some(0)).returning(Mask::STRING),
    Magic::instance(b"__debuginfo", Some(0)).returning(Mask::ARRAY.union(Mask::NULL)),
    Magic::instance(b"__serialize", Some(0)).returning(Mask::ARRAY),
    Magic::instance(b"__unserialize", Some(1))
        .taking(&[Mask::ARRAY])
        .returning(Mask::VOID),
    Magic::instance(b"__set_state", Some(1))
        .taking(&[Mask::ARRAY])
        .returning(Mask::OBJECT)
        .static_method(),
    Magic::instance(b"__invoke", None),
    Magic::instance(b"__sleep", Some(0)).returning(Mask::ARRAY),
    Magic::instance(b"__wakeup", Some(0)).returning(Mask::VOID),
];

impl<'a> Checker<'a> {
    /// Compiles `method` of `class`; `declared` holds the lowercase names of the methods
    /// declared before it.
    pub(super) fn method(
        &mut self,
        class: &'a ClassLike,
        method: &'a Function,
        declared: &mut HashSet<Vec<u8>>,
    ) -> Flow {
        self.line = method.line;
        self.method_declaration(class, method, declared)?;
        let code = Code::Method {
            class: self.class.as_ref().and_then(|scope| scope.name.clone()),
            name: method.name.clone(),
        };
        let frame = self.function_frame(method, code)?;
        self.line = method.line;
        self.magic_method(method, &frame)?;
        self.resolve_jumps(&frame.jumps)
    }

    /// The checks the reference makes before compiling a method.
    fn method_declaration(
        &mut self,
        class: &ClassLike,
        method: &Function,
        declared: &mut HashSet<Vec<u8>>,
    ) -> Flow {
        let modifiers = method.modifiers;
        let subject = self.method_subject(method);
        if modifiers.contains(Modifiers::PRIVATE.union(Modifiers::FINAL))
            && !method.name.eq_ignore_ascii_case(b"__construct")
        {
            self.report(
                Severity::Warning,
                "Private methods cannot be final as they are never overridden by other classes",
            );
        }
        let may_be_abstract =
            class.modifiers.contains(Modifiers::ABSTRACT) || class.kind == ClassKind::Trait;
        if modifiers.contains(Modifiers::ABSTRACT) && !may_be_abstract {
            let mut message = match (class.kind, &class.name) {
                (_, None) => b"Anonymous class method ".to_vec(),
                (ClassKind::Enum, _) => b"Enum method ".to_vec(),
                (ClassKind::Interface, _) => b"Interface method ".to_vec(),
                _ => {
                    let mut message = b"Class ".to_vec();
                    message.extend_from_slice(&self.class_label());
                    message.extend_from_slice(b" declares abstract method ");
                    message.extend_from_slice(&method.name);
                    message.extend_from_slice(b"() and must therefore be declared abstract");
                    return self.fatal(message);
                }
            };
            message.extend_from_slice(if class.name.is_some() {
                &subject
            } else {
                &method.name
            });
            message.extend_from_slice(b"() must not be abstract");
            return self.fatal(message);
        }
        let in_interface = class.kind == ClassKind::Interface;
        if in_interface {
            if modifiers.intersects(Modifiers::PRIVATE.union(Modifiers::PROTECTED)) {
                let mut message = b"Access type for interface method ".to_vec();
                message.extend_from_slice(&subject);
                message.extend_from_slice(b"() must be public");
                return self.fatal(message);
            }
            if modifiers.contains(Modifiers::FINAL) {
                let mut message = b"Interface method ".to_vec();
                message.extend_from_slice(&subject);
                message.extend_from_slice(b"() must not be final");
                return self.fatal(message);
            }
        }
        let has_body = !matches!(method.body, FunctionBody::Absent);
        if in_interface || modifiers.contains(Modifiers::ABSTRACT) {
            let kind = if in_interface {
                "Interface"
            } else {
                "Abstract"
            };
            let problem =
                if modifiers.contains(Modifiers::PRIVATE) && class.kind != ClassKind::Trait {
                    "cannot be declared private"
                } else if has_body {
                    "cannot contain body"
                } else {
                    ""
                };
            if !problem.is_empty() {
                let mut message = format!("{kind} function ").into_bytes();
                message.extend_from_slice(&subject);
                message.extend_from_slice(format!("() {problem}").as_bytes());
                return self.fatal(message);
            }
        } else if !has_body {
            let mut message = b"Non-abstract method ".to_vec();
            message.extend_from_slice(&subject);
            message.extend_from_slice(b"() must contain body");
            return self.fatal(message);
        }
        if !declared.insert(method.name.to_ascii_lowercase()) {
            let mut message = b"Cannot redeclare ".to_vec();
            message.extend_from_slice(&subject);
            message.extend_from_slice(b"()");
            return self.fatal(message);
        }
        Ok(())
    }

    /// `Class::method`, as messages about `method` name it.
    fn method_subject(&self, method: &Function) -> Vec<u8> {
        let mut subject = self.class_label();
        subject.extend_from_slice(b"::");
        subject.extend_from_slice(&method.name);
        subject
    }

    /// Checks the signature of a magic method once its body has compiled into `frame`.
    fn magic_method(&mut self, method: &Function, frame: &Frame) -> Flow {
        let lower = method.name.to_ascii_lowercase();
        let Some(magic) = MAGIC.iter().find(|magic| magic.name == lower) else {
            return Ok(());
        };
        let subject = self.method_subject(method);
        let fail = |problem: &str| {
            let mut message = b"Method ".to_vec();
            message.extend_from_slice(&subject);
            message.extend_from_slice(format!("() {problem}").as_bytes());
            message
        };
        let modifiers = method.modifiers;
        if let Some(args) = magic.args {
            let declared = method.params.iter().filter(|param| !param.variadic).count();
            if u32::try_from(declared).ok() != Some(args) {
                let problem = match args {
                    0 => "cannot take arguments".to_owned(),
                    1 => "must take exactly 1 argument".to_owned(),
                    _ => format!("must take exactly {args} arguments"),
                };
                return self.fatal(fail(&problem));
            }
            if method
                .params
                .iter()
                .take(declared)
                .any(|param| param.by_ref)
            {
                return self.fatal(fail("cannot take arguments by reference"));
            }
        }
        let is_static = modifiers.contains(Modifiers::STATIC);
        match (magic.binding, is_static) {
            (Binding::Instance, true) => return self.fatal(fail("cannot be static")),
            (Binding::Static, false) => return self.fatal(fail("must be static")),
            _ => {}
        }
        let no_discard = || {
            let mut message = b"Method ".to_vec();
            message.extend_from_slice(&subject);
            message.extend_from_slice(b" cannot be #[\\NoDiscard]");
            message
        };
        if magic.no_return && method.return_type.is_some() {
            return self.fatal(fail("cannot declare a return type"));
        }
        if magic.no_return && frame.no_discard {
            return self.fatal(no_discard());
        }
        if magic.public && modifiers.intersects(Modifiers::PRIVATE.union(Modifiers::PROTECTED)) {
            let mut message = b"The magic method ".to_vec();
            message.extend_from_slice(&subject);
            message.extend_from_slice(b"() must have public visibility");
            self.report(Severity::Warning, message);
        }
        for (index, (&expected, param)) in magic.params.iter().zip(&method.params).enumerate() {
            let declared = frame.params.get(index).and_then(Option::as_ref);
            if declared.is_some_and(|declared| !declared.mask().intersects(expected)) {
                let mut message = subject.clone();
                message.extend_from_slice(format!("(): Parameter #{} ($", index + 1).as_bytes());
                message.extend_from_slice(&param.name);
                message.extend_from_slice(b") must be of type ");
                message.extend(DeclaredType::of(expected).display());
                message.extend_from_slice(b" when declared");
                return self.fatal(message);
            }
        }
        if magic.returns == Some(Mask::VOID) && frame.no_discard {
            return self.fatal(no_discard());
        }
        match (magic.returns, &frame.returns) {
            (Some(expected), Some(returns))
                if method.return_type.is_some() && !returns_within(returns, expected) =>
            {
                let mut message = subject;
                message.extend_from_slice(b"(): Return type must be ");
                message.extend(DeclaredType::of(expected).display());
                message.extend_from_slice(b" when declared");
                self.fatal(message)
            }
            _ => Ok(()),
        }
    }
}

/// Whether a declared return type fits the kinds a magic method may return: `never` always
/// does, and classes and `static` only where any object may be returned.
fn returns_within(returns: &DeclaredType, expected: Mask) -> bool {
    let mask = returns.mask();
    if mask.intersects(Mask::NEVER) {
        return true;
    }
    let extra = mask.without(expected);
    let has_classes = returns.has_classes() || extra.intersects(Mask::STATIC);
    extra.without(Mask::STATIC) == Mask::NONE && (!has_classes || expected == Mask::OBJECT)
}

#[cfg(test)]
mod tests {
    use crate::check::Checker;
    use php_parser::{ParseOptions, parse};

    /// Every diagnostic compiling `source` reports, as `severity: message @line`.
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
                    "{}: {} @{}",
                    diagnostic.severity.label(),
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

    #[test]
    fn method_declarations_check_modifiers_and_bodies() {
        assert_cases(&[
            (
                "class A {\n abstract function f(); }",
                &[
                    "Fatal error: Class A declares abstract method f() and must therefore be declared abstract @2",
                ],
            ),
            (
                "new class { abstract function f(); };",
                &["Fatal error: Anonymous class method f() must not be abstract @1"],
            ),
            (
                "enum E { abstract function f(); }",
                &["Fatal error: Enum method E::f() must not be abstract @1"],
            ),
            (
                "interface I { abstract function f(); }",
                &["Fatal error: Interface method I::f() must not be abstract @1"],
            ),
            (
                "interface I { protected function f(); }",
                &["Fatal error: Access type for interface method I::f() must be public @1"],
            ),
            (
                "interface I { final function f(); }",
                &["Fatal error: Interface method I::f() must not be final @1"],
            ),
            (
                "interface I { function f() {} }",
                &["Fatal error: Interface function I::f() cannot contain body @1"],
            ),
            (
                "abstract class A { abstract private function f(); }",
                &["Fatal error: Abstract function A::f() cannot be declared private @1"],
            ),
            ("trait T { abstract private function f(); }", &[]),
            (
                "trait T { abstract private function f() {} }",
                &["Fatal error: Abstract function T::f() cannot contain body @1"],
            ),
            (
                "class A { function f(); }",
                &["Fatal error: Non-abstract method A::f() must contain body @1"],
            ),
            (
                "namespace N; class A { function f() {} function F() {} }",
                &["Fatal error: Cannot redeclare N\\A::F() @1"],
            ),
            (
                "class A { final private function f() {} final private function __Construct() {} }",
                &[
                    "Warning: Private methods cannot be final as they are never overridden by other classes @1",
                ],
            ),
            (
                "abstract class A { abstract function f(); } interface I { static function g(); }",
                &[],
            ),
        ]);
    }

    #[test]
    fn magic_methods_check_their_parameters() {
        assert_cases(&[
            (
                "class A { function __destruct($a) {} }",
                &["Fatal error: Method A::__destruct() cannot take arguments @1"],
            ),
            (
                "class A { function __get(&...$a) {} }",
                &["Fatal error: Method A::__get() must take exactly 1 argument @1"],
            ),
            (
                "class A { function __call($a) {} }",
                &["Fatal error: Method A::__call() must take exactly 2 arguments @1"],
            ),
            (
                "class A { function __set($a, &$b) {} }",
                &["Fatal error: Method A::__set() cannot take arguments by reference @1"],
            ),
            (
                "class A { function __get($a, &...$b) {} function __invoke(&$a) {} }",
                &[],
            ),
            (
                "class A { function __call(string $a, ?A $b) {} }",
                &[
                    "Fatal error: A::__call(): Parameter #2 ($b) must be of type array when declared @1",
                ],
            ),
            (
                "class A { static function __set_state(int $a) {} }",
                &[
                    "Fatal error: A::__set_state(): Parameter #1 ($a) must be of type array when declared @1",
                ],
            ),
            (
                "class A { function __get(mixed $a) {} function __call(string|int $a, iterable $b) {} }",
                &[],
            ),
        ]);
    }

    #[test]
    fn magic_methods_check_binding_visibility_and_returns() {
        assert_cases(&[
            (
                "class A { static function __construct() {} }",
                &["Fatal error: Method A::__construct() cannot be static @1"],
            ),
            (
                "class A { function __callStatic($a, $b) {} }",
                &["Fatal error: Method A::__callStatic() must be static @1"],
            ),
            (
                "class A { function __destruct(): void {} }",
                &["Fatal error: Method A::__destruct() cannot declare a return type @1"],
            ),
            (
                "class A { private function __set(int $a, $b) {} }",
                &[
                    "Warning: The magic method A::__set() must have public visibility @1",
                    "Fatal error: A::__set(): Parameter #1 ($a) must be of type string when declared @1",
                ],
            ),
            ("class A { protected function __construct() {} }", &[]),
            (
                "class A { function __toString(): static {} }",
                &["Fatal error: A::__toString(): Return type must be string when declared @1"],
            ),
            (
                "class A { function __debugInfo(): iterable {} }",
                &["Fatal error: A::__debugInfo(): Return type must be ?array when declared @1"],
            ),
            (
                "class A { function __isset($a): int {} }",
                &["Fatal error: A::__isset(): Return type must be bool when declared @1"],
            ),
            (
                "class A { static function __set_state(array $a): int {} }",
                &["Fatal error: A::__set_state(): Return type must be object when declared @1"],
            ),
            (
                "class A { static function __set_state(array $a): static {} function __isset($a): true {} function __get($a): never {} function __toString() { return ''; } }",
                &[],
            ),
            (
                "class A {\n function __get(\n) {} }",
                &["Fatal error: Method A::__get() must take exactly 1 argument @2"],
            ),
        ]);
    }
}
