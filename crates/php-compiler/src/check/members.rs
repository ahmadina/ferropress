//! Class-like declarations and their members other than methods: properties, class constants
//! and enum cases.

use std::collections::HashSet;

use php_parser::ast::{
    ClassKind, ClassLike, ClassMember, ConstantGroup, EnumCase, Modifiers, Param, Property,
    PropertyGroup,
};
use php_value::Value;

use super::attributes::Target;
use super::hooks::HookedProperty;
use super::types::{ANONYMOUS_CLASS, DeclaredType, Mask};
use super::{Checker, ClassScope, Code, Flow, types};

/// How restrictive a visibility is, from public (the default) to private.
fn visibility_rank(modifiers: Modifiers, protected: Modifiers, private: Modifiers) -> u8 {
    if modifiers.intersects(private) {
        2
    } else {
        u8::from(modifiers.intersects(protected))
    }
}

impl<'a> Checker<'a> {
    pub(super) fn class(&mut self, class: &'a ClassLike) -> Flow {
        self.line = class.line;
        if let Some(name) = &class.name {
            self.class_declaration(class.kind, name)?;
        }
        let parent = match &class.extends {
            Some(parent) => Some(self.declared_class(parent, "class name")?),
            None => None,
        };
        let scope = ClassScope {
            name: class.name.as_deref().map(|name| self.qualify(name)),
            kind: class.kind,
            modifiers: class.modifiers,
            parent,
            is_backed: false,
            properties: HashSet::new(),
            constants: HashSet::new(),
        };
        let outer = self.class.replace(scope);
        let result = self.class_body(class);
        self.class = outer;
        result
    }

    fn class_body(&mut self, class: &'a ClassLike) -> Flow {
        self.attributes(&class.attributes, Target::Class)?;
        for interface in &class.implements {
            self.declared_class(interface, "interface name")?;
        }
        if let Some(backing) = &class.backing_type {
            self.backing_type(backing)?;
        }
        let mut methods = HashSet::new();
        for member in &class.members {
            match member {
                ClassMember::Method(method) => self.method(class, method, &mut methods)?,
                ClassMember::Property(group) => {
                    self.line = group.line;
                    self.property_group(group)?;
                    for property in &group.properties {
                        let outer = self.frame.property.replace(property.name.clone());
                        let result = self.property(group, property);
                        self.frame.property = outer;
                        result?;
                    }
                }
                ClassMember::Constant(group) => {
                    self.line = group.line;
                    self.class_constants(group)?;
                }
                ClassMember::EnumCase(case) => {
                    self.line = case.line;
                    self.enum_case(case)?;
                }
                ClassMember::TraitUse(trait_use) => {
                    self.line = trait_use.line;
                    self.trait_use(class.kind, trait_use)?;
                }
            }
        }
        Ok(())
    }

    /// The class being compiled, as messages name it.
    pub(super) fn class_label(&self) -> Vec<u8> {
        self.class
            .as_ref()
            .map_or(ANONYMOUS_CLASS, ClassScope::label)
            .to_vec()
    }

    fn class_kind(&self) -> Option<ClassKind> {
        self.class.as_ref().map(|class| class.kind)
    }

    /// `Class::member`, or `Class::$member` with `sigil`, as messages name a member.
    pub(super) fn member_subject(&self, sigil: &[u8], name: &[u8]) -> Vec<u8> {
        let mut subject = self.class_label();
        subject.extend_from_slice(b"::");
        subject.extend_from_slice(sigil);
        subject.extend_from_slice(name);
        subject
    }

    fn backing_type(&mut self, backing: &'a php_parser::ast::Type) -> Flow {
        let declared = self.declared_type(backing, false)?.0;
        let mask = declared.mask();
        if declared.has_classes() || (mask != Mask::INT && mask != Mask::STRING) {
            let mut message = b"Enum backing type must be int or string, ".to_vec();
            message.extend(declared.display());
            message.extend_from_slice(b" given");
            return self.fatal(message);
        }
        if let Some(class) = &mut self.class {
            class.is_backed = true;
        }
        Ok(())
    }

    /// Declares the class constant or enum case `name`, which `modifiers` qualify.
    fn declare_constant(&mut self, name: &[u8], modifiers: Modifiers) -> Flow {
        let subject = self.member_subject(b"", name);
        if self.class_kind() == Some(ClassKind::Interface)
            && modifiers.intersects(Modifiers::PRIVATE.union(Modifiers::PROTECTED))
        {
            let mut message = b"Access type for interface constant ".to_vec();
            message.extend_from_slice(&subject);
            message.extend_from_slice(b" must be public");
            return self.fatal(message);
        }
        if name.eq_ignore_ascii_case(b"class") {
            return self.fatal(
                "A class constant must not be called 'class'; it is reserved for class name fetching",
            );
        }
        let inserted = self
            .class
            .as_mut()
            .is_some_and(|class| class.constants.insert(name.to_vec()));
        if !inserted {
            let mut message = b"Cannot redefine class constant ".to_vec();
            message.extend_from_slice(&subject);
            return self.fatal(message);
        }
        Ok(())
    }

    fn class_constants(&mut self, group: &'a ConstantGroup) -> Flow {
        for item in &group.items {
            let declared = match &group.constant_type {
                Some(declared) => Some(self.declared_type(declared, false)?.0),
                None => None,
            };
            let subject = self.member_subject(b"", &item.name);
            if let Some(declared) = &declared {
                let mask = declared.mask();
                let forbidden = Mask::CALLABLE.union(Mask::VOID).union(Mask::NEVER);
                if mask != Mask::ANY && mask.intersects(forbidden) {
                    let mut message = b"Class constant ".to_vec();
                    message.extend_from_slice(&subject);
                    message.extend_from_slice(b" cannot have type ");
                    message.extend(declared.display());
                    return self.fatal(message);
                }
            }
            if group
                .modifiers
                .contains(Modifiers::PRIVATE.union(Modifiers::FINAL))
            {
                let mut message = b"Private constant ".to_vec();
                message.extend_from_slice(&subject);
                message
                    .extend_from_slice(b" cannot be final as it is not visible to other classes");
                return self.fatal(message);
            }
            let value = self.constant(&item.value, false)?;
            if let (Some(declared), Some(value)) = (&declared, &value)
                && !declared.accepts(value)
            {
                let mut message = format!(
                    "Cannot use {} as value for class constant ",
                    types::type_name(value)
                )
                .into_bytes();
                message.extend_from_slice(&subject);
                message.extend_from_slice(b" of type ");
                message.extend(declared.display());
                return self.fatal(message);
            }
            self.declare_constant(&item.name, group.modifiers)?;
            self.attributes(&group.attributes, Target::ClassConstant)?;
        }
        Ok(())
    }

    fn enum_case(&mut self, case: &'a EnumCase) -> Flow {
        let Some(class) = self
            .class
            .as_ref()
            .filter(|class| class.kind == ClassKind::Enum)
        else {
            return self.fatal("Case can only be used in enums");
        };
        let problem = match (class.is_backed, &case.value) {
            (true, None) => "backed enum",
            (false, Some(_)) => "non-backed enum",
            _ => "",
        };
        if !problem.is_empty() {
            let mut message = b"Case ".to_vec();
            message.extend_from_slice(&case.name);
            message.extend_from_slice(format!(" of {problem} ").as_bytes());
            message.extend_from_slice(class.label());
            message.extend_from_slice(if class.is_backed {
                b" must have a value"
            } else {
                b" must not have a value"
            });
            return self.fatal(message);
        }
        if let Some(value) = &case.value {
            self.constant(value, false)?;
        }
        self.declare_constant(&case.name, Modifiers::PUBLIC)?;
        self.attributes(&case.attributes, Target::ClassConstant)
            .map(drop)
    }

    /// The checks the reference makes once for a property declaration statement.
    fn property_group(&self, group: &PropertyGroup) -> Flow {
        let modifiers = group.modifiers;
        if self.class_kind() == Some(ClassKind::Enum) {
            let mut message = b"Enum ".to_vec();
            message.extend_from_slice(&self.class_label());
            message.extend_from_slice(b" cannot include properties");
            return self.fatal(message);
        }
        if modifiers.contains(Modifiers::FINAL.union(Modifiers::PRIVATE)) {
            return self.fatal("Property cannot be both final and private");
        }
        if self.class_kind() == Some(ClassKind::Interface) {
            if modifiers.contains(Modifiers::FINAL) {
                return self.fatal("Property in interface cannot be final");
            }
            if modifiers.intersects(Modifiers::PROTECTED.union(Modifiers::PRIVATE)) {
                return self.fatal("Property in interface cannot be protected or private");
            }
            if modifiers.contains(Modifiers::ABSTRACT) {
                return self.fatal(
                    "Property in interface cannot be explicitly abstract. All interface members are implicitly abstract",
                );
            }
        }
        Ok(())
    }

    fn property(&mut self, group: &'a PropertyGroup, property: &'a Property) -> Flow {
        if property.hooks.is_none() {
            if self.class_kind() == Some(ClassKind::Interface) {
                return self.fatal("Interfaces may only include hooked properties");
            }
            if group.modifiers.contains(Modifiers::ABSTRACT) {
                return self.fatal("Only hooked properties may be declared abstract");
            }
        }
        let declared = match &group.property_type {
            Some(declared) => Some(self.declared_type(declared, false)?.0),
            None => None,
        };
        let subject = self.member_subject(b"$", &property.name);
        if let Some(declared) = &declared {
            let forbidden = Mask::VOID.union(Mask::NEVER).union(Mask::CALLABLE);
            if declared.mask().intersects(forbidden) {
                let mut message = b"Property ".to_vec();
                message.extend_from_slice(&subject);
                message.extend_from_slice(b" cannot have type ");
                message.extend(declared.display());
                return self.fatal(message);
            }
        }
        self.ensure_new_property(&property.name)?;
        if let Some(default) = &property.default {
            self.property_default(&subject, declared.as_ref(), default)?;
        }
        let modifiers = self.property_modifiers(group.modifiers);
        if modifiers.contains(Modifiers::READONLY) {
            let problem = if declared.is_none() {
                "must have type"
            } else if property.default.is_some() {
                "cannot have default value"
            } else {
                ""
            };
            if !problem.is_empty() {
                let mut message = b"Readonly property ".to_vec();
                message.extend_from_slice(&subject);
                message.extend_from_slice(format!(" {problem}").as_bytes());
                return self.fatal(message);
            }
            if modifiers.contains(Modifiers::STATIC) {
                let mut message = b"Static property ".to_vec();
                message.extend_from_slice(&subject);
                message.extend_from_slice(b" cannot be readonly");
                return self.fatal(message);
            }
        }
        self.declare_property(&property.name, modifiers, declared.is_some())?;
        if let Some(hooks) = &property.hooks {
            self.property_hooks(&HookedProperty {
                name: &property.name,
                modifiers,
                declared_type: group.property_type.as_ref(),
                declared,
                has_default: property.default.is_some(),
                hooks,
            })?;
        }
        self.attributes(&group.attributes, Target::Property)
            .map(drop)
    }

    fn property_default(
        &mut self,
        subject: &[u8],
        declared: Option<&DeclaredType>,
        default: &'a php_parser::ast::Expr,
    ) -> Flow {
        let value = self.constant(default, false)?;
        let (Some(declared), Some(value)) = (declared, &value) else {
            return Ok(());
        };
        if declared.accepts(value) {
            return Ok(());
        }
        let message = if matches!(value, Value::Null) && !declared.is_intersection() {
            let mut message = b"Default value for property of type ".to_vec();
            message.extend(declared.display());
            message.extend_from_slice(b" may not be null. Use the nullable type ");
            message.extend(declared.nullable().display());
            message.extend_from_slice(b" to allow null default value");
            message
        } else {
            let mut message = format!(
                "Cannot use {} as default value for property ",
                types::value_name(value)
            )
            .into_bytes();
            message.extend_from_slice(subject);
            message.extend_from_slice(b" of type ");
            message.extend(declared.display());
            message
        };
        self.fatal(message)
    }

    /// A property's modifiers, with `readonly` added in a readonly class.
    fn property_modifiers(&self, modifiers: Modifiers) -> Modifiers {
        let readonly_class = self
            .class
            .as_ref()
            .is_some_and(|class| class.modifiers.contains(Modifiers::READONLY));
        if readonly_class {
            modifiers.union(Modifiers::READONLY)
        } else {
            modifiers
        }
    }

    fn ensure_new_property(&self, name: &[u8]) -> Flow {
        let declared = self
            .class
            .as_ref()
            .is_some_and(|class| class.properties.contains(name));
        if declared {
            let mut message = b"Cannot redeclare ".to_vec();
            message.extend_from_slice(&self.member_subject(b"$", name));
            return self.fatal(message);
        }
        Ok(())
    }

    /// Declares the property `name` with `modifiers`, checking its set visibility.
    fn declare_property(&mut self, name: &[u8], modifiers: Modifiers, typed: bool) -> Flow {
        if modifiers.intersects(Modifiers::SET_VISIBILITY) {
            let subject = self.member_subject(b"$", name);
            if !typed {
                let mut message = b"Property with asymmetric visibility ".to_vec();
                message.extend_from_slice(&subject);
                message.extend_from_slice(b" must have type");
                return self.fatal(message);
            }
            let get = visibility_rank(modifiers, Modifiers::PROTECTED, Modifiers::PRIVATE);
            let set = visibility_rank(modifiers, Modifiers::PROTECTED_SET, Modifiers::PRIVATE_SET);
            if get > set {
                let mut message = b"Visibility of property ".to_vec();
                message.extend_from_slice(&subject);
                message.extend_from_slice(b" must not be weaker than set visibility");
                return self.fatal(message);
            }
        }
        if let Some(class) = &mut self.class {
            class.properties.insert(name.to_vec());
        }
        Ok(())
    }

    /// Checks and declares the property that `param` of `function` promotes.
    pub(super) fn promoted_property(&mut self, param: &'a Param, is_abstract: bool) -> Flow {
        let is_constructor = self.class.is_some()
            && matches!(&self.frame.code, Code::Method { name, .. }
                if name.eq_ignore_ascii_case(b"__construct"));
        if !is_constructor {
            return self.fatal("Cannot declare promoted property outside a constructor");
        }
        if is_abstract || self.class_kind() == Some(ClassKind::Interface) {
            return self.fatal("Cannot declare promoted property in an abstract constructor");
        }
        if param.variadic {
            return self.fatal("Cannot declare variadic promoted property");
        }
        self.ensure_new_property(&param.name)?;
        let declared = self.frame.params.last().cloned().flatten();
        if let Some(declared) = &declared
            && declared.mask().intersects(Mask::CALLABLE)
        {
            let mut message = b"Property ".to_vec();
            message.extend_from_slice(&self.member_subject(b"$", &param.name));
            message.extend_from_slice(b" cannot have type ");
            message.extend(declared.display());
            return self.fatal(message);
        }
        let modifiers = self.property_modifiers(param.modifiers);
        if declared.is_none() && param.hooks.is_none() && modifiers.contains(Modifiers::READONLY) {
            let mut message = b"Readonly property ".to_vec();
            message.extend_from_slice(&self.member_subject(b"$", &param.name));
            message.extend_from_slice(b" must have type");
            return self.fatal(message);
        }
        self.declare_property(&param.name, modifiers, declared.is_some())?;
        if let Some(hooks) = &param.hooks {
            self.property_hooks(&HookedProperty {
                name: &param.name,
                modifiers,
                declared_type: param.declared_type.as_ref(),
                declared,
                has_default: false,
                hooks,
            })?;
        }
        self.attributes_on(&param.attributes, Target::Property, Some(Target::Parameter))
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::visibility_rank;
    use crate::check::Checker;
    use php_parser::ast::Modifiers;
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
    fn visibility_ranks_order_public_protected_private() {
        let protected = Modifiers::PROTECTED;
        let private = Modifiers::PRIVATE;
        assert_eq!(visibility_rank(Modifiers::PUBLIC, protected, private), 0);
        assert_eq!(visibility_rank(Modifiers::NONE, protected, private), 0);
        assert_eq!(visibility_rank(Modifiers::PROTECTED, protected, private), 1);
        assert_eq!(visibility_rank(Modifiers::PRIVATE, protected, private), 2);
    }

    #[test]
    fn properties_are_declared_once_per_class() {
        assert_cases(&[
            (
                "class A { public $a, $a; }",
                &["Fatal error: Cannot redeclare A::$a @1"],
            ),
            ("class A { public $a; public $A; }", &[]),
            (
                "interface I { public $a { get; } public $a { get; } }",
                &["Fatal error: Cannot redeclare I::$a @1"],
            ),
            ("class A { public $a; } class B { public $a; }", &[]),
            (
                "class A { public $a; function f() { new class { public $a; }; } }",
                &[],
            ),
            (
                "class A {\n  public $a;\n  public\n  $a;\n}",
                &["Fatal error: Cannot redeclare A::$a @4"],
            ),
            (
                "class A { public int $a = \"x\"; public $a; }",
                &[
                    "Fatal error: Cannot use string as default value for property A::$a of type int @1",
                ],
            ),
        ]);
    }

    #[test]
    fn property_groups_check_their_container_and_modifiers() {
        assert_cases(&[
            (
                "enum E: int { case A = 1; public $a; }",
                &["Fatal error: Enum E cannot include properties @1"],
            ),
            (
                "final class A { final private int $a; }",
                &["Fatal error: Property cannot be both final and private @1"],
            ),
            (
                "interface I { final protected $a { get; } }",
                &["Fatal error: Property in interface cannot be final @1"],
            ),
            (
                "interface I { protected $a { get; } }",
                &["Fatal error: Property in interface cannot be protected or private @1"],
            ),
            (
                "interface I { abstract public $a { get; } }",
                &[
                    "Fatal error: Property in interface cannot be explicitly abstract. All interface members are implicitly abstract @1",
                ],
            ),
            (
                "class A { abstract public $a; }",
                &["Fatal error: Only hooked properties may be declared abstract @1"],
            ),
            (
                "interface I {\n public\n $a;\n}",
                &["Fatal error: Interfaces may only include hooked properties @3"],
            ),
            ("abstract class A { abstract public int $a { get; } }", &[]),
            (
                "class A { public void $a; }",
                &["Fatal error: Property A::$a cannot have type void @1"],
            ),
            (
                "class A { var callable $a; }",
                &["Fatal error: Property A::$a cannot have type callable @1"],
            ),
        ]);
    }

    #[test]
    fn readonly_and_asymmetric_properties_need_types() {
        assert_cases(&[
            (
                "class A { readonly public int $a; readonly public $b; }",
                &["Fatal error: Readonly property A::$b must have type @1"],
            ),
            (
                "readonly class A { public int $a = 1; }",
                &["Fatal error: Readonly property A::$a cannot have default value @1"],
            ),
            (
                "readonly class A { public static $a; }",
                &["Fatal error: Readonly property A::$a must have type @1"],
            ),
            (
                "class A { public readonly static int $a; }",
                &["Fatal error: Static property A::$a cannot be readonly @1"],
            ),
            (
                "class A { public private(set) $a; }",
                &["Fatal error: Property with asymmetric visibility A::$a must have type @1"],
            ),
            (
                "class A { private public(set) int $a; }",
                &[
                    "Fatal error: Visibility of property A::$a must not be weaker than set visibility @1",
                ],
            ),
            (
                "class A { protected private(set) int $a; public protected(set) int $b; }",
                &[],
            ),
            ("class A { private(set) int $a; }", &[]),
            ("class A { public readonly public(set) int $a; }", &[]),
        ]);
    }

    #[test]
    fn promoted_properties_follow_property_rules() {
        assert_cases(&[
            (
                "class A { function __construct(public $a, public $a) {} }",
                &["Fatal error: Redefinition of parameter $a @1"],
            ),
            (
                "class A { function __Construct(public int $a, private ?callable $b = null) {} }",
                &["Fatal error: Property A::$b cannot have type ?callable @1"],
            ),
            (
                "class A { function f() { $g = function (public $b) {}; } }",
                &["Fatal error: Cannot declare promoted property outside a constructor @1"],
            ),
            (
                "abstract class A { abstract function __construct(public $a); }",
                &["Fatal error: Cannot declare promoted property in an abstract constructor @1"],
            ),
            (
                "class A { function __construct(int $a, public ...$b) {} }",
                &["Fatal error: Cannot declare variadic promoted property @1"],
            ),
            (
                "class A { public $a; function __construct(public $a) {} }",
                &["Fatal error: Cannot redeclare A::$a @1"],
            ),
            (
                "readonly class A { function __construct(public $a) {} }",
                &["Fatal error: Readonly property A::$a must have type @1"],
            ),
            (
                "class A { function __construct(private(set) $a) {} }",
                &["Fatal error: Property with asymmetric visibility A::$a must have type @1"],
            ),
            (
                "class A { function __construct(public protected(set) int $a, protected public(set) int $b) {} }",
                &[
                    "Fatal error: Visibility of property A::$b must not be weaker than set visibility @1",
                ],
            ),
            ("trait T { function __construct(public $a) {} }", &[]),
            (
                "readonly class A { function __construct(public int $a) {} public int $b; }",
                &[],
            ),
            (
                "class A { function __construct(final private int $a) {} }",
                &[],
            ),
        ]);
    }

    #[test]
    fn class_constants_are_named_and_declared_once() {
        assert_cases(&[
            (
                "class A { const B = 1, B = 2; }",
                &["Fatal error: Cannot redefine class constant A::B @1"],
            ),
            (
                "class A { const CLASS = 1; }",
                &[
                    "Fatal error: A class constant must not be called 'class'; it is reserved for class name fetching @1",
                ],
            ),
            (
                "interface I { private const class = 1; }",
                &["Fatal error: Access type for interface constant I::class must be public @1"],
            ),
            (
                "class A { private final const CLASS = 1; }",
                &[
                    "Fatal error: Private constant A::CLASS cannot be final as it is not visible to other classes @1",
                ],
            ),
            (
                "new class { const A = 1; const A = 2; };",
                &["Fatal error: Cannot redefine class constant class@anonymous::A @1"],
            ),
            ("class A { const A = 1; } class B { const A = 1; }", &[]),
        ]);
    }

    #[test]
    fn enum_cases_match_the_backing_type() {
        assert_cases(&[
            (
                "enum E: string { case A = \"a\"; case B; }",
                &["Fatal error: Case B of backed enum E must have a value @1"],
            ),
            (
                "enum E { case A; case B = 1; }",
                &["Fatal error: Case B of non-backed enum E must not have a value @1"],
            ),
            (
                "enum E: int { case A = 1; const A = 2; }",
                &["Fatal error: Cannot redefine class constant E::A @1"],
            ),
            (
                "enum E {\n  case A;\n  case\n  A;\n}",
                &["Fatal error: Cannot redefine class constant E::A @4"],
            ),
            (
                "class A { case B = 1; }",
                &["Fatal error: Case can only be used in enums @1"],
            ),
            ("enum E { const A = 1; }", &[]),
        ]);
    }

    #[test]
    fn enum_backing_types_are_int_or_string() {
        for (backing, given) in [
            ("string|int", "string|int"),
            ("mixed", "mixed"),
            ("bool", "bool"),
            ("static", "static"),
            ("\\Foo", "Foo"),
            ("?string", "?string"),
        ] {
            assert_eq!(
                diagnostics(&format!("enum E: {backing} {{}}")),
                [format!(
                    "Fatal error: Enum backing type must be int or string, {given} given @1"
                )],
            );
        }
        assert_cases(&[("enum E: INT { case A = 1; }", &[])]);
    }
}
