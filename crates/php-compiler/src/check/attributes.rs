//! Attributes: their arguments, and the checks made for the attribute classes the reference
//! builds in.

use php_parser::ast::{Arg, Args, Attribute, AttributeGroup, ClassKind, Modifiers};

use super::{Checker, Flow};

/// What an attribute list is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Class,
    Function,
    Method,
    Property,
    ClassConstant,
    Parameter,
    Constant,
}

/// Every target, in the order messages list them.
const TARGETS: [(Target, &str); 7] = [
    (Target::Class, "class"),
    (Target::Function, "function"),
    (Target::Method, "method"),
    (Target::Property, "property"),
    (Target::ClassConstant, "class constant"),
    (Target::Parameter, "parameter"),
    (Target::Constant, "constant"),
];

impl Target {
    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// The extra check a built-in attribute makes where it is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Validator {
    Attribute,
    AllowDynamicProperties,
    Deprecated,
    NoDiscard,
}

/// An attribute class the reference builds in. None of them may be repeated.
#[derive(Debug)]
struct Builtin {
    /// The class name in lowercase.
    name: &'static [u8],
    /// The targets allowed, as [`Target::bit`] flags.
    targets: u8,
    validator: Option<Validator>,
}

const DELAYED_TARGET_VALIDATION: &[u8] = b"delayedtargetvalidation";

const BUILTINS: [Builtin; 8] = [
    Builtin {
        name: b"attribute",
        targets: Target::Class.bit(),
        validator: Some(Validator::Attribute),
    },
    Builtin {
        name: b"returntypewillchange",
        targets: Target::Method.bit(),
        validator: None,
    },
    Builtin {
        name: b"allowdynamicproperties",
        targets: Target::Class.bit(),
        validator: Some(Validator::AllowDynamicProperties),
    },
    Builtin {
        name: b"sensitiveparameter",
        targets: Target::Parameter.bit(),
        validator: None,
    },
    Builtin {
        name: b"override",
        targets: Target::Method.bit() | Target::Property.bit(),
        validator: None,
    },
    Builtin {
        name: b"deprecated",
        targets: Target::Method.bit()
            | Target::Function.bit()
            | Target::ClassConstant.bit()
            | Target::Constant.bit()
            | Target::Class.bit(),
        validator: Some(Validator::Deprecated),
    },
    Builtin {
        name: b"nodiscard",
        targets: Target::Method.bit() | Target::Function.bit(),
        validator: Some(Validator::NoDiscard),
    },
    Builtin {
        name: DELAYED_TARGET_VALIDATION,
        targets: u8::MAX,
        validator: None,
    },
];

/// The built-in attribute class named `name`, compared case-insensitively.
fn builtin(name: &[u8]) -> Option<&'static Builtin> {
    BUILTINS
        .iter()
        .find(|builtin| name.eq_ignore_ascii_case(builtin.name))
}

/// The targets in `bits`, as messages list them.
fn target_names(bits: u8) -> String {
    TARGETS
        .iter()
        .filter(|(target, _)| bits & target.bit() != 0)
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// What applying an attribute list changed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Applied {
    /// Whether `#[\NoDiscard]` was applied to the function.
    pub(super) no_discard: bool,
}

impl<'a> Checker<'a> {
    /// Compiles the attribute lists `groups` attached to `target`.
    pub(super) fn attributes(
        &mut self,
        groups: &'a [AttributeGroup],
        target: Target,
    ) -> Flow<Applied> {
        self.attributes_on(groups, target, None)
    }

    /// Compiles `groups` attached to `target`. A promoted parameter's attributes compile once for
    /// the parameter and once for the property, `promoted` naming the other target; a built-in
    /// attribute allowed only there is left out.
    pub(super) fn attributes_on(
        &mut self,
        groups: &'a [AttributeGroup],
        target: Target,
        promoted: Option<Target>,
    ) -> Flow<Applied> {
        let mut compiled: Vec<(&Attribute, Option<&'static Builtin>, Vec<u8>)> = Vec::new();
        for attribute in groups.iter().flatten() {
            if matches!(attribute.args, Some(Args::CallableConvert)) {
                return self.fatal("Cannot create Closure as attribute argument");
            }
            let name = self.resolve_class(&attribute.name);
            let builtin = builtin(&name);
            if let (Some(builtin), Some(promoted)) = (builtin, promoted)
                && builtin.targets & target.bit() == 0
                && builtin.targets & promoted.bit() != 0
            {
                continue;
            }
            self.attribute_args(attribute)?;
            compiled.push((attribute, builtin, name));
        }
        let delayed = compiled.iter().any(|(_, builtin, _)| {
            builtin.is_some_and(|builtin| builtin.name == DELAYED_TARGET_VALIDATION)
        });
        let mut applied = Applied::default();
        for (index, (_, builtin, name)) in compiled.iter().enumerate() {
            let Some(builtin) = builtin else {
                continue;
            };
            let allowed = builtin.targets & target.bit() != 0;
            if !allowed && !delayed {
                let mut message = b"Attribute \"".to_vec();
                message.extend_from_slice(name);
                message.extend_from_slice(
                    format!(
                        "\" cannot target {} (allowed targets: {})",
                        target_names(target.bit()),
                        target_names(builtin.targets)
                    )
                    .as_bytes(),
                );
                return self.fatal(message);
            }
            let repeated = compiled
                .iter()
                .enumerate()
                .any(|(other, (_, other_builtin, _))| {
                    other != index
                        && other_builtin.is_some_and(|other| std::ptr::eq(other, *builtin))
                });
            if repeated {
                let mut message = b"Attribute \"".to_vec();
                message.extend_from_slice(name);
                message.extend_from_slice(b"\" must not be repeated");
                return self.fatal(message);
            }
            let Some(validator) = builtin.validator.filter(|_| allowed) else {
                continue;
            };
            match self.validation_error(validator, target) {
                Some(error) if !delayed => return self.fatal(error),
                Some(_) => {}
                None => applied.no_discard |= validator == Validator::NoDiscard,
            }
        }
        Ok(applied)
    }

    /// Checks the arguments of `attribute`, each a constant expression.
    fn attribute_args(&mut self, attribute: &'a Attribute) -> Flow {
        let Some(Args::List(args)) = &attribute.args else {
            return Ok(());
        };
        let mut named = false;
        for (index, arg) in args.iter().enumerate() {
            let value = match arg {
                Arg::Unpack(_) => {
                    return self.fatal("Cannot use unpacking in attribute argument list");
                }
                Arg::Named { name, value } => {
                    let repeated = args[..index].iter().any(
                        |earlier| matches!(earlier, Arg::Named { name: other, .. } if other == name),
                    );
                    if repeated {
                        let mut message = b"Duplicate named parameter $".to_vec();
                        message.extend_from_slice(name);
                        return self.fatal(message);
                    }
                    named = true;
                    value
                }
                Arg::Positional(value) => {
                    if named {
                        return self.fatal("Cannot use positional argument after named argument");
                    }
                    value
                }
            };
            self.constant(value, true)?;
        }
        Ok(())
    }

    /// The error `validator` reports for an attribute on `target`, if any.
    fn validation_error(&self, validator: Validator, target: Target) -> Option<Vec<u8>> {
        let class = self.class.as_ref();
        let kind = class.map(|class| class.kind);
        let problem = match validator {
            Validator::NoDiscard => {
                return self
                    .frame
                    .property
                    .is_some()
                    .then(|| b"#[\\NoDiscard] is not supported for property hooks".to_vec());
            }
            Validator::Deprecated if target != Target::Class => return None,
            Validator::Deprecated => match kind {
                Some(ClassKind::Trait) | None => return None,
                Some(ClassKind::Interface) => "#[\\Deprecated] to interface",
                Some(ClassKind::Enum) => "#[\\Deprecated] to enum",
                Some(ClassKind::Class) => "#[\\Deprecated] to class",
            },
            Validator::Attribute => match kind {
                Some(ClassKind::Trait) => "#[\\Attribute] to trait",
                Some(ClassKind::Interface) => "#[\\Attribute] to interface",
                Some(ClassKind::Enum) => "#[\\Attribute] to enum",
                _ if class.is_some_and(|class| class.modifiers.contains(Modifiers::ABSTRACT)) => {
                    "#[\\Attribute] to abstract class"
                }
                _ => return None,
            },
            Validator::AllowDynamicProperties => match kind {
                Some(ClassKind::Trait) => "#[\\AllowDynamicProperties] to trait",
                Some(ClassKind::Interface) => "#[\\AllowDynamicProperties] to interface",
                _ if class.is_some_and(|class| class.modifiers.contains(Modifiers::READONLY)) => {
                    "#[\\AllowDynamicProperties] to readonly class"
                }
                Some(ClassKind::Enum) => "#[\\AllowDynamicProperties] to enum",
                _ => return None,
            },
        };
        let mut message = format!("Cannot apply {problem} ").into_bytes();
        message.extend_from_slice(&self.class_label());
        Some(message)
    }
}

#[cfg(test)]
mod tests {
    use super::{BUILTINS, Target, builtin, target_names};
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

    fn fatal(source: &str, message: &str) {
        assert_eq!(
            diagnostics(source),
            [format!("Fatal error: {message} @1")],
            "{source}"
        );
    }

    fn clean(source: &str) {
        assert_eq!(diagnostics(source), Vec::<String>::new(), "{source}");
    }

    #[test]
    fn targets_are_named_in_flag_order() {
        assert_eq!(target_names(Target::Parameter.bit()), "parameter");
        assert_eq!(
            target_names(Target::Constant.bit() | Target::Class.bit() | Target::Method.bit()),
            "class, method, constant"
        );
        assert!(builtin(b"NoDiscard").is_some());
        assert!(builtin(b"N\\NoDiscard").is_none());
        assert!(
            BUILTINS
                .iter()
                .all(|builtin| builtin.name.to_ascii_lowercase() == builtin.name)
        );
    }

    #[test]
    fn builtin_attributes_check_their_target() {
        fatal(
            "#[Attribute] function f() {}",
            "Attribute \"Attribute\" cannot target function (allowed targets: class)",
        );
        fatal(
            "#[OVERRIDE] class A {}",
            "Attribute \"OVERRIDE\" cannot target class (allowed targets: method, property)",
        );
        fatal(
            "class A { #[\\Deprecated] public $a; }",
            "Attribute \"Deprecated\" cannot target property (allowed targets: class, function, method, class constant, constant)",
        );
        fatal(
            "enum E { #[\\Override] case A; }",
            "Attribute \"Override\" cannot target class constant (allowed targets: method, property)",
        );
        fatal(
            "#[\\Override] const A = 1;",
            "Attribute \"Override\" cannot target constant (allowed targets: method, property)",
        );
        fatal(
            "function f(#[\\Override] $a) {}",
            "Attribute \"Override\" cannot target parameter (allowed targets: method, property)",
        );
        fatal(
            "$f = #[\\Override] fn() => 1;",
            "Attribute \"Override\" cannot target function (allowed targets: method, property)",
        );
        fatal(
            "namespace N; use Override; #[Override] function f() {}",
            "Attribute \"Override\" cannot target function (allowed targets: method, property)",
        );
        clean("namespace N; #[Override] function f() {}");
        clean("class A { #[\\ReturnTypeWillChange] function f() {} }");
    }

    #[test]
    fn builtin_attributes_must_not_be_repeated() {
        fatal(
            "#[\\Attribute(\\Attribute::TARGET_ALL)] #[\\Attribute] class A {}",
            "Attribute \"Attribute\" must not be repeated",
        );
        fatal(
            "#[\\Override, A, \\Override] class B {}",
            "Attribute \"Override\" cannot target class (allowed targets: method, property)",
        );
        fatal(
            "function f(#[\\SensitiveParameter] #[\\SensitiveParameter] $a) {}",
            "Attribute \"SensitiveParameter\" must not be repeated",
        );
        clean("#[A] #[A] class B {}");
        clean("function f(#[\\SensitiveParameter] $a, #[\\SensitiveParameter] $b) {}");
    }

    #[test]
    fn delayed_target_validation_suppresses_target_and_validator_errors() {
        clean("#[\\DelayedTargetValidation] #[\\Override] function f() {}");
        clean("#[\\Override] #[\\DelayedTargetValidation] function f() {}");
        clean("#[\\DelayedTargetValidation] #[\\Attribute] trait T {}");
        fatal(
            "#[\\DelayedTargetValidation] #[\\NoDiscard] function f(): void {}",
            "A void function does not return a value, but #[\\NoDiscard] requires a return value",
        );
        clean("class A { public $a { #[\\DelayedTargetValidation] #[\\NoDiscard] get => 1; } }");
        fatal(
            "#[\\DelayedTargetValidation] #[\\Override] #[\\Override] function f() {}",
            "Attribute \"Override\" must not be repeated",
        );
        fatal(
            "#[\\DelayedTargetValidation] #[\\DelayedTargetValidation] class A {}",
            "Attribute \"DelayedTargetValidation\" must not be repeated",
        );
    }

    #[test]
    fn promoted_parameters_skip_attributes_of_the_other_target() {
        clean("class A { function __construct(#[\\SensitiveParameter] public $a) {} }");
        fatal(
            "class A { function __construct(#[\\Deprecated] public $a) {} }",
            "Attribute \"Deprecated\" cannot target parameter (allowed targets: class, function, method, class constant, constant)",
        );
        fatal(
            "class A { function __construct(#[\\Override] #[\\Override] public $a) {} }",
            "Attribute \"Override\" must not be repeated",
        );
    }

    #[test]
    fn class_validators_check_the_kind_of_class() {
        for (source, message) in [
            (
                "#[\\Attribute] abstract class A {}",
                "Cannot apply #[\\Attribute] to abstract class A",
            ),
            (
                "#[\\Attribute] enum E {}",
                "Cannot apply #[\\Attribute] to enum E",
            ),
            (
                "#[\\Attribute] interface I {}",
                "Cannot apply #[\\Attribute] to interface I",
            ),
            (
                "#[\\Attribute] trait T {}",
                "Cannot apply #[\\Attribute] to trait T",
            ),
            (
                "#[\\AllowDynamicProperties] trait T {}",
                "Cannot apply #[\\AllowDynamicProperties] to trait T",
            ),
            (
                "#[\\AllowDynamicProperties] interface I {}",
                "Cannot apply #[\\AllowDynamicProperties] to interface I",
            ),
            (
                "new #[\\AllowDynamicProperties] readonly class {};",
                "Cannot apply #[\\AllowDynamicProperties] to readonly class class@anonymous",
            ),
            (
                "#[\\AllowDynamicProperties] enum E {}",
                "Cannot apply #[\\AllowDynamicProperties] to enum E",
            ),
            (
                "#[\\Deprecated] class A {}",
                "Cannot apply #[\\Deprecated] to class A",
            ),
            (
                "#[\\Deprecated] interface I {}",
                "Cannot apply #[\\Deprecated] to interface I",
            ),
            (
                "#[\\Deprecated] enum E {}",
                "Cannot apply #[\\Deprecated] to enum E",
            ),
        ] {
            fatal(source, message);
        }
        clean("#[\\Attribute] final class A {}");
        clean("#[\\AllowDynamicProperties] abstract class A {}");
        clean("#[\\Deprecated] trait T {}");
        clean("#[\\Deprecated] function f() {}");
    }

    #[test]
    fn no_discard_needs_a_returned_value() {
        fatal(
            "#[\\NoDiscard] function f(): void {}",
            "A void function does not return a value, but #[\\NoDiscard] requires a return value",
        );
        fatal(
            "class A { #[\\NoDiscard] function f(): never {} }",
            "A never returning method does not return a value, but #[\\NoDiscard] requires a return value",
        );
        fatal(
            "class A { function f() { return #[\\NoDiscard] function (): void {}; } }",
            "A void method does not return a value, but #[\\NoDiscard] requires a return value",
        );
        fatal(
            "#[\\NoDiscard] fn(): void => 1;",
            "A void function does not return a value, but #[\\NoDiscard] requires a return value",
        );
        fatal(
            "$f = #[\\NoDiscard] function ($a) use ($a): void {};",
            "Cannot use lexical variable $a as a parameter name",
        );
        fatal(
            "class A { public $a { #[\\NoDiscard] get => 1; } }",
            "#[\\NoDiscard] is not supported for property hooks",
        );
        clean("#[\\NoDiscard] function f(): ?int {}");
        fatal(
            "#[\\NoDiscard] function f(): int|never {}",
            "never can only be used as a standalone type",
        );
    }

    #[test]
    fn no_discard_is_rejected_on_magic_methods_returning_nothing() {
        for (method, name) in [
            ("__construct()", "__construct"),
            ("__destruct()", "__destruct"),
            ("__clone()", "__clone"),
            ("__set($a, $b)", "__set"),
            ("__unserialize(array $a)", "__unserialize"),
        ] {
            fatal(
                &format!("class A {{ #[\\NoDiscard] function {method} {{}} }}"),
                &format!("Method A::{name} cannot be #[\\NoDiscard]"),
            );
        }
        clean("class A { #[\\NoDiscard] function __get($a) {} }");
        clean("class A { #[\\NoDiscard] function __toString() {} }");
    }
}
