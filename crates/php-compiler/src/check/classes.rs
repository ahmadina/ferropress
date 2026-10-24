//! Class references: the class operand of ordinary expressions, `X::class`, the classes a
//! declaration extends, implements or uses, and the classes a `catch` names.

use php_parser::ast::{
    ClassKind, Expr, ExprKind, MethodReference, Modifiers, Name, NameKind, TraitAdaptation,
    TraitUse,
};
use php_value::Value;

use super::constant::Fetch;
use super::types::value_name;
use super::{Checker, Flow};

impl<'a> Checker<'a> {
    /// Compiles the class operand of `X::m()`, `X::$p`, `X::C`, `new X` and `instanceof X`.
    pub(super) fn class_ref(&mut self, class: &'a Expr) -> Flow {
        match &class.kind {
            ExprKind::Name(name) => match Fetch::named(&name.bytes) {
                Some(fetch) if name.kind != NameKind::FullyQualified => {
                    self.ensure_valid_fetch(fetch)
                }
                _ => self.class_name(name).map(drop),
            },
            ExprKind::Int(_) | ExprKind::Float(_) => self.fatal("Illegal class name"),
            ExprKind::String(bytes) => self.string_class(bytes).map(drop),
            _ => {
                self.expr(class)?;
                match self.compiled_constant(class)? {
                    None => Ok(()),
                    Some(Value::String(name)) => match Fetch::named(name.as_bytes()) {
                        Some(fetch) => self.ensure_valid_fetch(fetch),
                        None => self.string_class(name.as_bytes()).map(drop),
                    },
                    Some(_) => self.fatal("Illegal class name"),
                }
            }
        }
    }

    /// Compiles `expr instanceof class`, whose class is not compiled when `expr` is constant.
    pub(super) fn instanceof(&mut self, expr: &'a Expr, class: &'a Expr) -> Flow {
        self.expr(expr)?;
        if self.compiled_constant(expr)?.is_some() {
            return Ok(());
        }
        self.class_ref(class)
    }

    /// Compiles `class::class` in ordinary code.
    pub(super) fn class_name_fetch(&mut self, class: &'a Expr) -> Flow {
        let special = match &class.kind {
            ExprKind::Int(_) | ExprKind::Float(_) => return self.fatal("Illegal class name"),
            ExprKind::Name(name) => match Fetch::named(&name.bytes) {
                None => return self.class_name(name).map(drop),
                fetch => fetch,
            },
            ExprKind::String(bytes) => match Fetch::named(bytes) {
                None => return self.string_class(bytes).map(drop),
                fetch => fetch,
            },
            _ => None,
        };
        if let Some(fetch) = special {
            return self.ensure_valid_fetch(fetch);
        }
        self.expr(class)?;
        match self.compiled_constant(class)? {
            Some(value) => self.fatal(format!("Cannot use \"::class\" on {}", value_name(&value))),
            None => Ok(()),
        }
    }

    /// The class a string names, which is fully qualified with or without its leading `\`.
    fn string_class(&self, bytes: &[u8]) -> Flow<Vec<u8>> {
        let name = bytes.strip_prefix(b"\\").unwrap_or(bytes);
        if Fetch::named(name).is_some() {
            let mut message = b"'\\".to_vec();
            message.extend_from_slice(name);
            message.extend_from_slice(b"' is an invalid class name");
            return self.fatal(message);
        }
        Ok(name.to_vec())
    }

    /// The value `expr` compiles to when the reference computes it while compiling: literals,
    /// `true`, `false` and `null`, `X::class` for a class known by name, and operators and casts
    /// over those.
    pub(super) fn compiled_constant(&mut self, expr: &'a Expr) -> Flow<Option<Value>> {
        match &expr.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::String(_) => self.fold(expr),
            ExprKind::Constant(name) => Ok(self.special_constant(name)),
            ExprKind::ClassName(class) => Ok(self.known_class_name(class)),
            ExprKind::Binary { left, right, .. } => {
                if self.compiled_constant(left)?.is_none()
                    || self.compiled_constant(right)?.is_none()
                {
                    return Ok(None);
                }
                self.fold(expr)
            }
            ExprKind::Unary { operand, .. } | ExprKind::Cast { operand, .. } => {
                if self.compiled_constant(operand)?.is_none() {
                    return Ok(None);
                }
                self.fold(expr)
            }
            _ => Ok(None),
        }
    }

    /// The name `class::class` resolves to while compiling, when it does.
    fn known_class_name(&self, class: &Expr) -> Option<Value> {
        let (written, name) = match &class.kind {
            ExprKind::Name(name) => (&name.bytes, Some(name)),
            ExprKind::String(bytes) => (bytes, None),
            _ => return None,
        };
        let scope = self.class.as_ref().filter(|_| self.is_scope_known());
        let resolved = match Fetch::named(written) {
            Some(Fetch::SelfClass) => scope?.name.clone()?,
            Some(Fetch::Parent) => scope?.parent.clone()?,
            Some(Fetch::Static) => return None,
            None => match name {
                Some(name) => self.resolve_class(name),
                None => written.strip_prefix(b"\\").unwrap_or(written).to_vec(),
            },
        };
        Some(Value::String(resolved.into()))
    }

    /// Resolves a class a declaration extends, implements or uses; `what` names its role.
    pub(super) fn declared_class(&mut self, name: &Name, what: &str) -> Flow<Vec<u8>> {
        if name.kind != NameKind::FullyQualified && Fetch::named(&name.bytes).is_some() {
            let mut message = b"Cannot use \"".to_vec();
            message.extend_from_slice(&name.bytes);
            message.extend_from_slice(format!("\" as {what}, as it is reserved").as_bytes());
            return self.fatal(message);
        }
        self.class_name(name)
    }

    /// Compiles the trait list of a `use` in the body of a `kind`.
    pub(super) fn trait_use(&mut self, kind: ClassKind, trait_use: &TraitUse) -> Flow {
        for name in &trait_use.traits {
            if kind == ClassKind::Interface {
                let mut message = b"Cannot use traits inside of interfaces. ".to_vec();
                message.extend_from_slice(&name.bytes);
                message.extend_from_slice(b" is used in ");
                message.extend_from_slice(&self.class_label());
                return self.fatal(message);
            }
            self.declared_class(name, "trait name")?;
        }
        for adaptation in trait_use.adaptations.iter().flatten() {
            match adaptation {
                TraitAdaptation::Precedence { method, instead_of } => {
                    self.method_reference(method)?;
                    for name in instead_of {
                        self.declared_class(name, "trait name")?;
                    }
                }
                TraitAdaptation::Alias {
                    method, modifier, ..
                } => {
                    if modifier.contains(Modifiers::STATIC) {
                        return self
                            .fatal("Cannot use \"static\" as method modifier in trait alias");
                    }
                    if modifier.contains(Modifiers::ABSTRACT) {
                        return self
                            .fatal("Cannot use \"abstract\" as method modifier in trait alias");
                    }
                    self.method_reference(method)?;
                }
            }
        }
        Ok(())
    }

    /// Resolves the trait of `[Trait::]method` in a trait adaptation.
    fn method_reference(&mut self, method: &MethodReference) -> Flow {
        method.class.as_ref().map_or(Ok(()), |class| {
            self.declared_class(class, "trait name").map(drop)
        })
    }

    /// Checks the classes a `catch` names, which must be class names rather than `self`,
    /// `parent` or `static`, and the variable it stores the exception in.
    pub(super) fn caught_classes(&mut self, classes: &[Name], variable: Option<&[u8]>) -> Flow {
        for class in classes {
            if class.kind != NameKind::FullyQualified && Fetch::named(&class.bytes).is_some() {
                return self.fatal("Bad class name in the catch statement");
            }
            self.class_name(class)?;
            if variable == Some(b"this") {
                return self.fatal("Cannot re-assign $this");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::check::Checker;
    use php_parser::{ParseOptions, parse};

    /// Every diagnostic compiling `source` reports, as `message @line`.
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

    #[test]
    fn class_operands_follow_the_reference_rules() {
        let no_scope = "Cannot use \"self\" when no class scope is active @1";
        assert_cases(&[
            ("function f() { self::f(); }", &[no_scope]),
            ("function f() { namespace\\self::$a; }", &[no_scope]),
            ("namespace\\self::f();", &[]),
            ("$f = function () { new self; };", &[]),
            (
                "function f() { \\self::C; }",
                &["'\\self' is an invalid class name @1"],
            ),
            (
                "function f() { new (\"\\\\parent\"); }",
                &["'\\parent' is an invalid class name @1"],
            ),
            ("function f() { \"\\\\A\"::f(); }", &[]),
            ("(1)::f();", &["Illegal class name @1"]),
            ("new (1.5);", &["Illegal class name @1"]),
            ("$a::f(); new ($a . 'b');", &[]),
            ("function f() { (1 + 1)::f(); }", &["Illegal class name @1"]),
            (
                "function f() { new ('se' . 'lf'); }",
                &["'\\self' is an invalid class name @1"],
            ),
            ("function f() { ('\\\\A' . 'b')::$a; }", &[]),
            ("1 instanceof self;", &[]),
            ("function f() { $a instanceof self; }", &[no_scope]),
            ("$a instanceof (2);", &["Illegal class name @1"]),
        ]);
    }

    #[test]
    fn class_name_fetches_reject_constant_operands() {
        assert_cases(&[
            ("(1)::class;", &["Illegal class name @1"]),
            (
                "function f() { \"self\"::class; }",
                &["Cannot use \"self\" when no class scope is active @1"],
            ),
            ("class A { function f() { \\self::class; } }", &[]),
            ("'\\\\A'::class; A::class;", &[]),
            (
                "'\\\\self'::class;",
                &["'\\self' is an invalid class name @1"],
            ),
            ("(true)::class;", &["Cannot use \"::class\" on true @1"]),
            ("(null)::class;", &["Cannot use \"::class\" on null @1"]),
            ("(-1)::class;", &["Cannot use \"::class\" on int @1"]),
            ("((int) 1.5)::class;", &["Cannot use \"::class\" on int @1"]),
            ("A::class::class;", &["Cannot use \"::class\" on string @1"]),
            (
                "'A'::class::class;",
                &["Cannot use \"::class\" on string @1"],
            ),
            (
                "class A { function f() { self::class::class; } }",
                &["Cannot use \"::class\" on string @1"],
            ),
            (
                "class A extends B { function f() { parent::class::class; } }",
                &["Cannot use \"::class\" on string @1"],
            ),
            (
                "class A { function f() { parent::class::class; } }",
                &["Cannot use \"parent\" when current class scope has no parent @1"],
            ),
            ("trait T { function f() { self::class::class; } }", &[]),
            ("class A { function f() { static::class::class; } }", &[]),
            ("$a::class; ($a . 'b')::class; (A . 'b')::class;", &[]),
        ]);
    }

    #[test]
    fn declarations_reject_reserved_parents_interfaces_and_traits() {
        assert_cases(&[
            (
                "class A extends Self {}",
                &["Cannot use \"Self\" as class name, as it is reserved @1"],
            ),
            (
                "class A extends \\parent {}",
                &["'\\parent' is an invalid class name @1"],
            ),
            (
                "interface I extends A, static {}",
                &["Cannot use \"static\" as interface name, as it is reserved @1"],
            ),
            (
                "new class implements self {};",
                &["Cannot use \"self\" as interface name, as it is reserved @1"],
            ),
            (
                "class A {\n use T,\n parent; }",
                &["Cannot use \"parent\" as trait name, as it is reserved @2"],
            ),
            (
                "interface I { use T; }",
                &["Cannot use traits inside of interfaces. T is used in I @1"],
            ),
            (
                "namespace N; interface I { use \\T; }",
                &["Cannot use traits inside of interfaces. T is used in N\\I @1"],
            ),
            (
                "#[A(self::B)] class A extends self {}",
                &["Cannot use \"self\" as class name, as it is reserved @1"],
            ),
            ("class A extends B implements C { use D; }", &[]),
        ]);
    }

    #[test]
    fn catches_name_classes() {
        assert_cases(&[
            (
                "try {} catch (A|self $e) {}",
                &["Bad class name in the catch statement @1"],
            ),
            (
                "try {\n} catch (\nnamespace\\static) {}",
                &["Bad class name in the catch statement @3"],
            ),
            (
                "try {} catch (\\Parent) {}",
                &["'\\Parent' is an invalid class name @1"],
            ),
            ("try {} catch (A|\\B $e) {} catch (C) {}", &[]),
            (
                "try {} catch (A | static $this) {}",
                &["Cannot re-assign $this @1"],
            ),
            (
                "try {} catch (static $this) {}",
                &["Bad class name in the catch statement @1"],
            ),
            ("try {} catch (A $e) { $this->a; }", &[]),
        ]);
    }

    #[test]
    fn trait_adaptations_check_modifiers_and_trait_names() {
        assert_cases(&[
            (
                "class A { use T { f as static g; } }",
                &["Cannot use \"static\" as method modifier in trait alias @1"],
            ),
            (
                "class A { use T { f as abstract; } }",
                &["Cannot use \"abstract\" as method modifier in trait alias @1"],
            ),
            (
                "class A { use T { self::f as g; } }",
                &["Cannot use \"self\" as trait name, as it is reserved @1"],
            ),
            (
                "class A { use T { T::f insteadof U, parent; } }",
                &["Cannot use \"parent\" as trait name, as it is reserved @1"],
            ),
            (
                "class A { use T { static::f insteadof U; } }",
                &["Cannot use \"static\" as trait name, as it is reserved @1"],
            ),
            (
                "class A { use T, U { T::f insteadof U; f as protected g; U::f as final; } }",
                &[],
            ),
        ]);
    }
}
