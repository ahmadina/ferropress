//! Property hooks, and calls of a parent class's hook from inside one.

use php_parser::ast::{
    Args, ClassKind, Expr, ExprKind, Hook, HookBody, Member, Modifiers, Param, Type, VarName,
};

use super::attributes::Target;
use super::types::{DeclaredType, Mask};
use super::{Checker, Code, Flow, Frame};

/// Which of a property's two hooks a hook is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HookKind {
    Get,
    Set,
}

impl HookKind {
    /// The kind a hook written `name` declares, compared case-insensitively.
    fn named(name: &[u8]) -> Option<Self> {
        if name.eq_ignore_ascii_case(b"get") {
            Some(Self::Get)
        } else if name.eq_ignore_ascii_case(b"set") {
            Some(Self::Set)
        } else {
            None
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Set => "set",
        }
    }
}

/// A property declaration with hooks, as its hooks are compiled.
pub(super) struct HookedProperty<'a> {
    pub(super) name: &'a [u8],
    /// The property's modifiers, with `readonly` added in a readonly class.
    pub(super) modifiers: Modifiers,
    pub(super) declared_type: Option<&'a Type>,
    /// The compiled type.
    pub(super) declared: Option<DeclaredType>,
    /// Whether the property has an explicit default value, which promoted properties never have.
    pub(super) has_default: bool,
    pub(super) hooks: &'a [Hook],
}

impl HookedProperty<'_> {
    /// Whether the property stores no value: it is in an interface, or no hook body uses it.
    fn is_virtual(&self, in_interface: bool) -> bool {
        in_interface
            || !self.hooks.iter().any(|hook| match &hook.body {
                HookBody::Absent => false,
                HookBody::Expr(_) if hook.name.eq_ignore_ascii_case(b"set") => true,
                HookBody::Expr(_) | HookBody::Block(_) => hook.uses_property,
            })
    }
}

impl<'a> Checker<'a> {
    /// Compiles the hooks of `property`, which has just been declared.
    pub(super) fn property_hooks(&mut self, property: &HookedProperty<'a>) -> Flow {
        if property.modifiers.contains(Modifiers::READONLY) {
            return self.fatal("Hooked properties cannot be readonly");
        }
        if property.hooks.is_empty() {
            return self.fatal("Property hook list must not be empty");
        }
        let in_interface = self
            .class
            .as_ref()
            .is_some_and(|class| class.kind == ClassKind::Interface);
        let mut get: Option<&Hook> = None;
        let mut set: Option<&Hook> = None;
        let mut has_abstract_hook = false;
        for hook in property.hooks {
            let (kind, is_abstract) = self.hook_declaration(property, hook, in_interface)?;
            has_abstract_hook |= is_abstract;
            let params = self.hook_function(property, hook, kind)?;
            self.line = hook.end_line;
            let slot = match kind {
                HookKind::Get => &mut get,
                HookKind::Set => &mut set,
            };
            if slot.replace(hook).is_some() {
                let mut message = b"Cannot redeclare property hook \"".to_vec();
                message.extend_from_slice(&hook.name);
                message.push(b'"');
                return self.fatal(message);
            }
            if let (Some(property_type), Some(Some(param_type)), Some(params)) =
                (&property.declared, params.first(), &hook.params)
                && property_type.accepted_by(param_type) == Some(false)
            {
                return self.hook_variance_error(property.name, &params[0]);
            }
        }
        let has_parent = self
            .class
            .as_ref()
            .is_some_and(|class| class.parent.is_some());
        if has_parent {
            return Ok(());
        }
        let is_virtual = property.is_virtual(in_interface);
        let subject = self.member_subject(b"$", property.name);
        if is_virtual && property.has_default {
            let mut message = b"Cannot specify default value for virtual hooked property ".to_vec();
            message.extend_from_slice(&subject);
            return self.fatal(message);
        }
        if get.is_some_and(|get| get.by_ref) && !is_virtual && set.is_some() {
            let mut message = b"Get hook of backed property ".to_vec();
            message.extend_from_slice(&self.member_subject(b"", property.name));
            message.extend_from_slice(b" with set hook may not return by reference");
            return self.fatal(message);
        }
        if property.modifiers.contains(Modifiers::ABSTRACT) && !has_abstract_hook {
            let mut message = b"Abstract property ".to_vec();
            message.extend_from_slice(&subject);
            message.extend_from_slice(b" must specify at least one abstract hook");
            return self.fatal(message);
        }
        if is_virtual
            && property.modifiers.intersects(Modifiers::SET_VISIBILITY)
            && (get.is_none() || set.is_none())
        {
            let mut message = if get.is_none() {
                b"Write-only".to_vec()
            } else {
                b"Read-only".to_vec()
            };
            message.extend_from_slice(b" virtual property ");
            message.extend_from_slice(&subject);
            message.extend_from_slice(b" must not specify asymmetric visibility");
            return self.fatal(message);
        }
        Ok(())
    }

    /// The checks on a hook's modifiers, body and parameter list made before it compiles; returns
    /// its kind and whether it is abstract.
    fn hook_declaration(
        &mut self,
        property: &HookedProperty<'_>,
        hook: &Hook,
        in_interface: bool,
    ) -> Flow<(HookKind, bool)> {
        self.line = hook.line;
        if property.modifiers.contains(Modifiers::STATIC) {
            return self.fatal("Cannot declare hooks for static property");
        }
        let is_private = property.modifiers.contains(Modifiers::PRIVATE);
        let is_final = hook.modifiers.contains(Modifiers::FINAL);
        if is_final && is_private {
            return self.fatal("Property hook cannot be both final and private");
        }
        let has_body = !matches!(hook.body, HookBody::Absent);
        let is_abstract =
            in_interface || (property.modifiers.contains(Modifiers::ABSTRACT) && !has_body);
        if is_abstract {
            if has_body {
                return self.fatal("Abstract property hook cannot have body");
            }
            if is_private {
                return self.fatal("Property hook cannot be both abstract and private");
            }
            if is_final {
                return self.fatal("Property hook cannot be both abstract and final");
            }
        } else if !has_body {
            return self.fatal("Non-abstract property hook must have a body");
        }
        let subject = self.member_subject(b"$", property.name);
        let Some(kind) = HookKind::named(&hook.name) else {
            let mut message = b"Unknown hook \"".to_vec();
            message.extend_from_slice(&hook.name);
            message.extend_from_slice(b"\" for property ");
            message.extend_from_slice(&subject);
            message.extend_from_slice(b", expected \"get\" or \"set\"");
            return self.fatal(message);
        };
        match (kind, &hook.params) {
            (HookKind::Get, Some(_)) => {
                let mut message = b"get hook of property ".to_vec();
                message.extend_from_slice(&subject);
                message.extend_from_slice(b" must not have a parameter list");
                return self.fatal(message);
            }
            (HookKind::Set, Some(params)) => {
                let [param] = params.as_slice() else {
                    let mut message = hook.name.clone();
                    message.extend_from_slice(b" hook of property ");
                    message.extend_from_slice(&subject);
                    message.extend_from_slice(b" must accept exactly one parameters");
                    return self.fatal(message);
                };
                let problem = if param.by_ref {
                    "be pass-by-reference"
                } else if param.variadic {
                    "be variadic"
                } else if param.default.is_some() {
                    "have a default value"
                } else {
                    ""
                };
                if !problem.is_empty() {
                    let mut message = b"Parameter $".to_vec();
                    message.extend_from_slice(&param.name);
                    message.extend_from_slice(b" of ");
                    message.extend_from_slice(&hook.name);
                    message.extend_from_slice(b" hook ");
                    message.extend_from_slice(&subject);
                    message.extend_from_slice(format!(" must not {problem}").as_bytes());
                    return self.fatal(message);
                }
                if property.declared_type.is_some() != param.declared_type.is_some() {
                    return self.hook_variance_error(property.name, param);
                }
            }
            _ => {}
        }
        Ok((kind, is_abstract))
    }

    fn hook_variance_error<T>(&self, property: &[u8], param: &Param) -> Flow<T> {
        let mut message = b"Type of parameter $".to_vec();
        message.extend_from_slice(&param.name);
        message.extend_from_slice(b" of hook ");
        message.extend_from_slice(&self.member_subject(b"$", property));
        message.extend_from_slice(b"::set must be compatible with property type");
        self.fatal(message)
    }

    /// Compiles `hook` as a method: a get hook returns the property's type; a set hook takes one
    /// value, of the property's type unless declared, and returns `void`. Returns the compiled
    /// parameter types.
    fn hook_function(
        &mut self,
        property: &HookedProperty<'a>,
        hook: &'a Hook,
        kind: HookKind,
    ) -> Flow<Vec<Option<DeclaredType>>> {
        let mut name = b"$".to_vec();
        name.extend_from_slice(property.name);
        name.extend_from_slice(b"::");
        name.extend_from_slice(&hook.name);
        let code = Code::Method {
            class: self.class.as_ref().and_then(|scope| scope.name.clone()),
            name,
        };
        let outer = std::mem::replace(
            &mut self.frame,
            Frame {
                property: Some(property.name.to_vec()),
                hook: Some(kind),
                ..Frame::new(code)
            },
        );
        let result = self.hook_inner(property, hook, kind);
        let frame = std::mem::replace(&mut self.frame, outer);
        result.map(|()| frame.params)
    }

    fn hook_inner(
        &mut self,
        property: &HookedProperty<'a>,
        hook: &'a Hook,
        kind: HookKind,
    ) -> Flow {
        self.attributes(&hook.attributes, Target::Method)?;
        let returns = match (kind, property.declared_type) {
            (HookKind::Get, Some(declared)) => Some(self.declared_type(declared, false)?.0),
            (HookKind::Get, None) => None,
            (HookKind::Set, _) => Some(DeclaredType::of(Mask::VOID)),
        };
        if hook.by_ref && kind == HookKind::Set {
            self.deprecated_in_function(
                b"Returning by reference from a void function is deprecated",
            );
        }
        match (kind, &hook.params) {
            (HookKind::Set, Some(params)) => self.params(params, false)?,
            (HookKind::Set, None) => {
                let value = match property.declared_type {
                    Some(declared) => Some(self.declared_type(declared, false)?.0),
                    None => None,
                };
                self.frame.params.push(value);
            }
            (HookKind::Get, _) => {}
        }
        if let (true, Some(returns)) = (hook.is_generator, &returns)
            && !returns.admits_generators()
        {
            let mut message = b"Generator return type must be a supertype of Generator, ".to_vec();
            message.extend(returns.display());
            message.extend_from_slice(b" given");
            return self.fatal(message);
        }
        self.frame.returns = returns;
        self.frame.is_generator = hook.is_generator;
        self.frame.by_ref = hook.by_ref;
        match &hook.body {
            HookBody::Absent => Ok(()),
            HookBody::Expr(expr) => {
                let line = self.line;
                self.expr(expr)?;
                if kind == HookKind::Set {
                    return Ok(());
                }
                self.line = line;
                self.returned(Some(expr))
            }
            HookBody::Block(stmts) => self.stmts(stmts),
        }
    }

    /// Compiles `class::name(args)` when it is `parent::$property::get()` or `::set()`, a call of
    /// the parent class's hook; returns whether it was.
    pub(super) fn parent_hook_call(
        &mut self,
        class: &'a Expr,
        name: &Member,
        args: &'a Args,
    ) -> Flow<bool> {
        let ExprKind::StaticProperty {
            class: parent,
            name: VarName::Name(property),
            parenthesized: false,
        } = &class.kind
        else {
            return Ok(false);
        };
        let is_parent = match &parent.kind {
            ExprKind::Name(name) => name.bytes.eq_ignore_ascii_case(b"parent"),
            ExprKind::String(bytes) => bytes.eq_ignore_ascii_case(b"parent"),
            _ => false,
        };
        let (true, Member::Name(hook)) = (is_parent, name) else {
            return Ok(false);
        };
        let Some(kind) = HookKind::named(hook) else {
            return Ok(false);
        };
        if self.class.is_none() {
            return self.fatal("Cannot use \"parent\" when no class scope is active");
        }
        if matches!(args, Args::CallableConvert) {
            return self.fatal("Cannot create Closure for parent property hook call");
        }
        let mut message = b"Must not use parent::$".to_vec();
        message.extend_from_slice(property);
        message.extend_from_slice(b"::");
        message.extend_from_slice(hook);
        message.extend_from_slice(b"() ");
        match (&self.frame.property, self.frame.hook) {
            (Some(active), _) if active != property => {
                message.extend_from_slice(b"in a different property ($");
                message.extend_from_slice(active);
                message.push(b')');
            }
            (Some(_), Some(active)) if active != kind => {
                message.extend_from_slice(b"in a different property hook (");
                message.extend_from_slice(active.label().as_bytes());
                message.push(b')');
            }
            (Some(_), Some(_)) => {
                self.args(args)?;
                return Ok(true);
            }
            _ => message.extend_from_slice(b"outside a property hook"),
        }
        self.fatal(message)
    }
}

#[cfg(test)]
mod tests {
    use super::HookKind;
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

    fn fatal(source: &str, message: &str) {
        assert_eq!(
            diagnostics(source),
            [format!("Fatal error: {message} @1")],
            "{source}"
        );
    }

    #[test]
    fn hook_kinds_are_named_case_insensitively() {
        assert_eq!(HookKind::named(b"GET"), Some(HookKind::Get));
        assert_eq!(HookKind::named(b"sEt"), Some(HookKind::Set));
        assert_eq!(HookKind::named(b"foo"), None);
        assert_eq!(HookKind::Get.label(), "get");
        assert_eq!(HookKind::Set.label(), "set");
    }

    #[test]
    fn hook_lists_check_the_property() {
        fatal(
            "class A { public readonly int $a { get => 1; } }",
            "Hooked properties cannot be readonly",
        );
        fatal(
            "readonly class A { public int $a { get => 1; } }",
            "Hooked properties cannot be readonly",
        );
        fatal(
            "class A { public $a { } }",
            "Property hook list must not be empty",
        );
        fatal(
            "class A { public static $a { get {} } }",
            "Cannot declare hooks for static property",
        );
        fatal(
            "class A { function __construct(public $a { }) {} }",
            "Property hook list must not be empty",
        );
    }

    #[test]
    fn hooks_check_modifiers_and_bodies() {
        fatal(
            "class A { private $a { final get {} } }",
            "Property hook cannot be both final and private",
        );
        fatal(
            "abstract class A { abstract private $a { get; } }",
            "Property hook cannot be both abstract and private",
        );
        fatal(
            "interface I { public $a { final get; } }",
            "Property hook cannot be both abstract and final",
        );
        fatal(
            "interface I { public $a { get => 1; } }",
            "Abstract property hook cannot have body",
        );
        fatal(
            "class A { public $a { get; } }",
            "Non-abstract property hook must have a body",
        );
        fatal(
            "class A { public $a { final foo {} } }",
            "Unknown hook \"foo\" for property A::$a, expected \"get\" or \"set\"",
        );
        assert_cases(&[(
            "class A {\n public $a {\n get\n ;\n }\n}",
            &["Fatal error: Non-abstract property hook must have a body @3"],
        )]);
    }

    #[test]
    fn hook_parameter_lists_are_checked() {
        fatal(
            "class A { public $a { get() {} } }",
            "get hook of property A::$a must not have a parameter list",
        );
        fatal(
            "class A { public $a { SET($x, $y) {} } }",
            "SET hook of property A::$a must accept exactly one parameters",
        );
        fatal(
            "class A { public $a { set(&$x) {} } }",
            "Parameter $x of set hook A::$a must not be pass-by-reference",
        );
        fatal(
            "class A { public $a { set(...$x) {} } }",
            "Parameter $x of set hook A::$a must not be variadic",
        );
        fatal(
            "class A { public $a { set($x = 1) {} } }",
            "Parameter $x of set hook A::$a must not have a default value",
        );
        fatal(
            "class A { public $a { set(int $x) {} } }",
            "Type of parameter $x of hook A::$a::set must be compatible with property type",
        );
        fatal(
            "class A { public int $a { set($x) {} } }",
            "Type of parameter $x of hook A::$a::set must be compatible with property type",
        );
        fatal(
            "class A { public $a { set(public $x) {} } }",
            "Cannot declare promoted property outside a constructor",
        );
    }

    #[test]
    fn hooks_compile_as_methods() {
        fatal(
            "class A { public int $a { get { yield 1; } } }",
            "Generator return type must be a supertype of Generator, int given",
        );
        fatal(
            "class A { public $a { set { yield; } } }",
            "Generator return type must be a supertype of Generator, void given",
        );
        fatal(
            "class A { public int $a { get { return; } } }",
            "A method with return type must return a value",
        );
        fatal(
            "class A { public $a { set { return 1; } } }",
            "A void method must not return a value",
        );
        assert_cases(&[
            (
                "class A { public $a { &set {} } }",
                &[
                    "Deprecated: A::$a::set(): Returning by reference from a void function is deprecated @1",
                ],
            ),
            ("class A { public iterable $a { get { yield 1; } } }", &[]),
            ("class A { public $a { set { return; } } }", &[]),
            (
                "class A { public $a { get => $undefined; set => 1; } }",
                &[],
            ),
            (
                "class A { public integer $a { get => 1; set {} } }",
                &[
                    "Warning: \"integer\" will be interpreted as a class name. Did you mean \"int\"? Write \"\\integer\" to suppress this warning @1",
                    "Warning: \"integer\" will be interpreted as a class name. Did you mean \"int\"? Write \"\\integer\" to suppress this warning @1",
                    "Warning: \"integer\" will be interpreted as a class name. Did you mean \"int\"? Write \"\\integer\" to suppress this warning @1",
                ],
            ),
        ]);
    }

    #[test]
    fn hooks_are_declared_once_after_compiling() {
        fatal(
            "class A { public $a { GET {} get {} } }",
            "Cannot redeclare property hook \"get\"",
        );
        assert_cases(&[
            (
                "class A {\n public $a {\n final\n get\n {} \n get\n {\n }\n }\n}",
                &["Fatal error: Cannot redeclare property hook \"get\" @8"],
            ),
            (
                "class A { public int $a { get => 1; get { return; } } }",
                &["Fatal error: A method with return type must return a value @1"],
            ),
        ]);
    }

    #[test]
    fn set_parameter_types_must_admit_the_property_type() {
        let incompatible =
            "Type of parameter $v of hook A::$a::set must be compatible with property type";
        for source in [
            "class A { public ?A $a { set(A $v) {} } }",
            "class A { public A|int $a { set(int $v) {} } }",
            "class A { public iterable $a { set(array $v) {} } }",
            "class A { public int $a { set(?float $v) {} } }",
            "class A { function __construct(public int $a { set(string $v) {} }) {} }",
        ] {
            fatal(source, incompatible);
        }
        for source in [
            "class A { public int $a { set(mixed $v) {} } }",
            "class A { public A&B $a { set(A $v) {} } }",
            "class A { public A $a { set(A|B $v) {} } }",
            "class A { public self $a { set(A $v) {} } }",
            "class A { public B $a { set(object $v) {} } }",
            "class A { public B $a { set(C $v) {} } }",
            "class A { public false $a { set(bool $v) {} } }",
        ] {
            assert_cases(&[(source, &[])]);
        }
        assert_cases(&[(
            "class A {\n public int $a {\n set(string $x)\n {\n }\n }\n}",
            &[
                "Fatal error: Type of parameter $x of hook A::$a::set must be compatible with property type @5",
            ],
        )]);
    }

    #[test]
    fn hooked_properties_are_verified_without_a_parent() {
        fatal(
            "interface I { public $a = 1 { get; } }",
            "Cannot specify default value for virtual hooked property I::$a",
        );
        fatal(
            "class A { public $a = 1 { get => fn() => $this->a; } }",
            "Cannot specify default value for virtual hooked property A::$a",
        );
        fatal(
            "class A { public $a = 1 { get => $THIS->a; } }",
            "Cannot specify default value for virtual hooked property A::$a",
        );
        fatal(
            "class A { public $a { &get => $this->a; set => 1; } }",
            "Get hook of backed property A::a with set hook may not return by reference",
        );
        fatal(
            "abstract class A { abstract public $a { get => 1; } }",
            "Abstract property A::$a must specify at least one abstract hook",
        );
        fatal(
            "class A { public private(set) int $a { get => 1; } }",
            "Read-only virtual property A::$a must not specify asymmetric visibility",
        );
        fatal(
            "class A { public private(set) int $a { set {} } }",
            "Write-only virtual property A::$a must not specify asymmetric visibility",
        );
        assert_cases(&[
            ("class A { public $a = 1 { get => $this->a; } }", &[]),
            ("class A { public $a = 1 { set => 1; } }", &[]),
            ("class A { public $a = 1 { get => ${'this'}->a; } }", &[]),
            ("class A { public $a { &get => 1; set {} } }", &[]),
            (
                "abstract class A { abstract public $a { get => 1; set; } }",
                &[],
            ),
            (
                "class A { public private(set) int $a { get => 1; set {} } }",
                &[],
            ),
            (
                "class A { function __construct(public $a = 1 { get => 1; }) {} }",
                &[],
            ),
            (
                "class B {} class A extends B { public $a = 1 { get => 1; } }",
                &[],
            ),
            (
                "abstract class A {\n abstract public $a {\n get => 1;\n }\n}",
                &[
                    "Fatal error: Abstract property A::$a must specify at least one abstract hook @3",
                ],
            ),
        ]);
    }

    #[test]
    fn parent_hook_calls_must_name_the_current_hook() {
        fatal(
            "function f() { \"PARENT\"::$a::get(); }",
            "Cannot use \"parent\" when no class scope is active",
        );
        fatal(
            "class A { public $a { get => parent::$a::get(...); } }",
            "Cannot create Closure for parent property hook call",
        );
        fatal(
            "class A { function f() { return parent::$a::get(); } }",
            "Must not use parent::$a::get() outside a property hook",
        );
        fatal(
            "class A { public $a { get => fn() => parent::$a::set(); } }",
            "Must not use parent::$a::set() outside a property hook",
        );
        fatal(
            "class A { public $a { get => parent::$b::get(); } }",
            "Must not use parent::$b::get() in a different property ($a)",
        );
        fatal(
            "class A { public $a { set { parent::$a::Get(); } } }",
            "Must not use parent::$a::Get() in a different property hook (set)",
        );
        fatal(
            "class A { public $a { get => (parent::$a)::get(); } }",
            "Cannot use \"parent\" when current class scope has no parent",
        );
        fatal(
            "class A { public $a { get => parent::$a::foo(); } }",
            "Cannot use \"parent\" when current class scope has no parent",
        );
        assert_cases(&[
            ("class A { public $a { get => parent::$a::GET(); } }", &[]),
            ("class A { public $a { get => \\parent::$a::get(); } }", &[]),
            (
                "class A { public $a { get => parent::${'a'}::get(); } }",
                &[],
            ),
            (
                "class A { public $a { set { parent::$a::set($undefined); } } }",
                &[],
            ),
        ]);
    }
}
