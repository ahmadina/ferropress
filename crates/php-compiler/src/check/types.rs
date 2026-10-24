//! Declared types: resolving the names in them, the checks made while compiling them, printing
//! them the way the reference does in diagnostics, and testing compile-time values against them.

use php_parser::ast::{Name, NameKind, SimpleType, Type};
use php_value::Value;

use super::constant::Fetch;
use super::{Checker, Flow};
use crate::diagnostic::Severity;

/// The kinds of values and pseudo-types a declared type admits, one bit each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Mask(u16);

impl Mask {
    pub(super) const NULL: Self = Self(1);
    pub(super) const FALSE: Self = Self(1 << 1);
    pub(super) const TRUE: Self = Self(1 << 2);
    pub(super) const INT: Self = Self(1 << 3);
    pub(super) const FLOAT: Self = Self(1 << 4);
    pub(super) const STRING: Self = Self(1 << 5);
    pub(super) const ARRAY: Self = Self(1 << 6);
    pub(super) const OBJECT: Self = Self(1 << 7);
    const RESOURCE: Self = Self(1 << 8);
    pub(super) const CALLABLE: Self = Self(1 << 9);
    pub(super) const VOID: Self = Self(1 << 10);
    pub(super) const NEVER: Self = Self(1 << 11);
    pub(super) const STATIC: Self = Self(1 << 12);
    const NONE: Self = Self(0);
    const BOOL: Self = Self(Self::FALSE.0 | Self::TRUE.0);
    /// What `mixed` admits: every kind of value.
    pub(super) const ANY: Self = Self(
        Self::NULL.0
            | Self::BOOL.0
            | Self::INT.0
            | Self::FLOAT.0
            | Self::STRING.0
            | Self::ARRAY.0
            | Self::OBJECT.0
            | Self::RESOURCE.0,
    );

    pub(super) const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub(super) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn common(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// The kind of `value`.
    const fn of(value: &Value) -> Self {
        match value {
            Value::Null => Self::NULL,
            Value::Bool(false) => Self::FALSE,
            Value::Bool(true) => Self::TRUE,
            Value::Int(_) => Self::INT,
            Value::Float(_) => Self::FLOAT,
            Value::String(_) => Self::STRING,
            Value::Array(_) => Self::ARRAY,
            Value::Object(_) => Self::OBJECT,
        }
    }
}

/// The builtin type names, matched case-insensitively.
const BUILTIN: [(&[u8], Builtin); 13] = [
    (b"null", Builtin::Mask(Mask::NULL)),
    (b"true", Builtin::Mask(Mask::TRUE)),
    (b"false", Builtin::Mask(Mask::FALSE)),
    (b"int", Builtin::Mask(Mask::INT)),
    (b"float", Builtin::Mask(Mask::FLOAT)),
    (b"string", Builtin::Mask(Mask::STRING)),
    (b"bool", Builtin::Mask(Mask::BOOL)),
    (b"void", Builtin::Mask(Mask::VOID)),
    (b"never", Builtin::Mask(Mask::NEVER)),
    (b"iterable", Builtin::Iterable),
    (b"object", Builtin::Mask(Mask::OBJECT)),
    (b"mixed", Builtin::Mask(Mask::ANY)),
    (b"static", Builtin::Mask(Mask::STATIC)),
];

#[derive(Debug, Clone, Copy)]
enum Builtin {
    Mask(Mask),
    /// `iterable`, an alias of `Traversable|array`.
    Iterable,
}

/// Names that look like a builtin type but are class names, matched case-sensitively, with the
/// builtin they were probably meant to be.
const CONFUSABLE: [(&[u8], Option<&str>); 4] = [
    (b"boolean", Some("bool")),
    (b"integer", Some("int")),
    (b"double", Some("float")),
    (b"resource", None),
];

/// Names no class can have, compared case-insensitively with the last part of a name.
const RESERVED_CLASS_NAMES: [&[u8]; 17] = [
    b"bool",
    b"false",
    b"float",
    b"int",
    b"null",
    b"parent",
    b"self",
    b"static",
    b"string",
    b"true",
    b"void",
    b"never",
    b"iterable",
    b"object",
    b"mixed",
    b"array",
    b"callable",
];

/// What the reference calls a class whose name it cannot print: an anonymous class's name holds
/// a NUL byte, and its messages stop there.
pub(super) const ANONYMOUS_CLASS: &[u8] = b"class@anonymous";

/// Whether `name`'s last part is reserved for builtin types.
pub(super) fn is_reserved_class_name(name: &[u8]) -> bool {
    let last = name
        .iter()
        .rposition(|&byte| byte == b'\\')
        .map_or(name, |slash| &name[slash + 1..]);
    RESERVED_CLASS_NAMES
        .iter()
        .any(|reserved| last.eq_ignore_ascii_case(reserved))
}

/// One class part of a type: a class, or an intersection of classes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Member {
    Class(Vec<u8>),
    Intersection(Vec<Vec<u8>>),
}

/// A compiled type declaration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct DeclaredType {
    mask: Mask,
    members: Vec<Member>,
    /// Whether the type is one intersection, printed without parentheses.
    is_intersection: bool,
    /// Whether the type is `iterable`, whose `Traversable` does not clash with `object`.
    is_iterable: bool,
}

impl DeclaredType {
    /// A type of builtin kinds only.
    pub(super) fn of(mask: Mask) -> Self {
        Self {
            mask,
            ..Self::default()
        }
    }

    pub(super) const fn mask(&self) -> Mask {
        self.mask
    }

    pub(super) const fn is_intersection(&self) -> bool {
        self.is_intersection
    }

    /// The type that also admits `null`.
    pub(super) fn nullable(&self) -> Self {
        Self {
            mask: self.mask.union(Mask::NULL),
            ..self.clone()
        }
    }

    fn class(name: Vec<u8>) -> Self {
        Self {
            members: vec![Member::Class(name)],
            ..Self::default()
        }
    }

    const fn has_classes(&self) -> bool {
        !self.members.is_empty()
    }

    /// Whether the type admits `value` as a default or constant value; an integer is accepted
    /// where a float is.
    pub(super) fn accepts(&self, value: &Value) -> bool {
        let kind = Mask::of(value);
        self.mask.intersects(kind) || (kind == Mask::INT && self.mask.intersects(Mask::FLOAT))
    }

    /// Whether a generator may declare this return type.
    pub(super) fn admits_generators(&self) -> bool {
        if self.mask.intersects(Mask::OBJECT) {
            return true;
        }
        let compatible = |name: &[u8]| {
            [b"Traversable".as_slice(), b"Iterator", b"Generator"]
                .iter()
                .any(|class| name.eq_ignore_ascii_case(class))
        };
        self.members.iter().any(|member| match member {
            Member::Class(name) => compatible(name),
            Member::Intersection(names) => {
                self.is_intersection && names.iter().any(|name| compatible(name))
            }
        })
    }

    /// The type as the reference prints it: classes as written, then builtin types in a fixed
    /// order, then `null` as a `?` prefix on a single type or a `|null` suffix.
    pub(super) fn display(&self) -> Vec<u8> {
        let mut parts: Vec<Vec<u8>> = Vec::new();
        for member in &self.members {
            match member {
                Member::Class(name) => parts.push(name.clone()),
                Member::Intersection(names) if self.is_intersection => parts.push(joined(names)),
                Member::Intersection(names) => {
                    let mut part = b"(".to_vec();
                    part.extend(joined(names));
                    part.push(b')');
                    parts.push(part);
                }
            }
        }
        if self.mask == Mask::ANY {
            parts.push(b"mixed".to_vec());
            return parts.join(&b'|');
        }
        let bool_name = if self.mask.common(Mask::BOOL) == Mask::BOOL {
            Some("bool")
        } else if self.mask.intersects(Mask::FALSE) {
            Some("false")
        } else if self.mask.intersects(Mask::TRUE) {
            Some("true")
        } else {
            None
        };
        let builtins = [
            (Mask::STATIC, Some("static")),
            (Mask::CALLABLE, Some("callable")),
            (Mask::OBJECT, Some("object")),
            (Mask::ARRAY, Some("array")),
            (Mask::STRING, Some("string")),
            (Mask::INT, Some("int")),
            (Mask::FLOAT, Some("float")),
            (Mask::BOOL, bool_name),
            (Mask::VOID, Some("void")),
            (Mask::NEVER, Some("never")),
        ];
        for (bit, name) in builtins {
            if let (true, Some(name)) = (self.mask.intersects(bit), name) {
                parts.push(name.as_bytes().to_vec());
            }
        }
        let mut text = parts.join(&b'|');
        if self.mask.intersects(Mask::NULL) {
            let single = !parts.is_empty() && !text.contains(&b'|') && !text.contains(&b'&');
            if single {
                text.insert(0, b'?');
            } else {
                if !text.is_empty() {
                    text.push(b'|');
                }
                text.extend_from_slice(b"null");
            }
        }
        text
    }
}

/// The name PHP gives the kind of `value` in parameter and class constant messages.
pub(super) const fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// The name PHP gives `value` in property messages, which tell `true` from `false`.
pub(super) const fn value_name(value: &Value) -> &'static str {
    match value {
        Value::Bool(true) => "true",
        Value::Bool(false) => "false",
        other => type_name(other),
    }
}

fn duplicate(text: &[u8]) -> Vec<u8> {
    let mut message = b"Duplicate type ".to_vec();
    message.extend_from_slice(text);
    message.extend_from_slice(b" is redundant");
    message
}

fn joined(names: &[Vec<u8>]) -> Vec<u8> {
    names.join(&b'&')
}

fn more_restrictive(larger: &[u8], smaller: &[u8]) -> Vec<u8> {
    let mut message = b"Type ".to_vec();
    message.extend_from_slice(larger);
    message.extend_from_slice(b" is redundant as it is more restrictive than type ");
    message.extend_from_slice(smaller);
    message
}

impl Checker<'_> {
    /// Resolves the class name `name`, failing as the reference does for a qualified `self`,
    /// `parent` or `static`.
    pub(super) fn class_name(&self, name: &Name) -> Flow<Vec<u8>> {
        let special = [b"self".as_slice(), b"parent", b"static"]
            .iter()
            .any(|special| name.bytes.eq_ignore_ascii_case(special));
        let prefix = match name.kind {
            NameKind::FullyQualified => "\\",
            NameKind::Relative => "namespace\\",
            NameKind::Unqualified => "",
        };
        if special && !prefix.is_empty() {
            let mut message = format!("'{prefix}").into_bytes();
            message.extend_from_slice(&name.bytes);
            message.extend_from_slice(b"' is an invalid class name");
            return self.fatal(message);
        }
        Ok(self.resolve_class(name))
    }

    /// The class `name` refers to: imports replace an unqualified name's first part, and the
    /// current namespace prefixes anything not fully qualified. `self`, `parent` and `static`
    /// stay as written.
    pub(super) fn resolve_class(&self, name: &Name) -> Vec<u8> {
        let bytes = name.bytes.as_slice();
        match name.kind {
            NameKind::FullyQualified => bytes.to_vec(),
            NameKind::Relative => self.qualify(bytes),
            NameKind::Unqualified => {
                let special = [b"self".as_slice(), b"parent", b"static"]
                    .iter()
                    .any(|special| bytes.eq_ignore_ascii_case(special));
                if special {
                    return bytes.to_vec();
                }
                let slash = bytes.iter().position(|&byte| byte == b'\\');
                let first = slash.map_or(bytes, |slash| &bytes[..slash]);
                match self.imports.get(&first.to_ascii_lowercase()) {
                    Some(imported) => {
                        let mut full = imported.clone();
                        full.extend_from_slice(slash.map_or(&[], |slash| &bytes[slash..]));
                        full
                    }
                    None => self.qualify(bytes),
                }
            }
        }
    }

    /// Compiles a type declaration. `force_null` makes the type nullable, as a `null` default
    /// does for a parameter; the flag returned tells whether that changed the type.
    pub(super) fn declared_type(
        &mut self,
        declared: &Type,
        force_null: bool,
    ) -> Flow<(DeclaredType, bool)> {
        let (mut compiled, nullable) = match declared {
            Type::Simple { kind, nullable } => (self.single_type(kind)?, *nullable),
            Type::Union(members) => (self.union_type(members)?, false),
            Type::Intersection(members) => {
                let names = self.intersection_type(members)?;
                let compiled = DeclaredType {
                    members: vec![Member::Intersection(names)],
                    is_intersection: !force_null,
                    ..DeclaredType::default()
                };
                (compiled, false)
            }
        };
        let mask = compiled.mask;
        if mask == Mask::ANY && nullable {
            return self.fatal(
                "Type mixed cannot be marked as nullable since mixed already includes null",
            );
        }
        if mask.intersects(Mask::NULL) && nullable {
            return self.fatal("null cannot be marked as nullable");
        }
        let forced = force_null && !nullable && !mask.intersects(Mask::NULL);
        if nullable || force_null {
            compiled.mask = mask.union(Mask::NULL);
        }
        let standalone = |bit: Mask| {
            compiled.mask.intersects(bit) && (compiled.has_classes() || compiled.mask != bit)
        };
        if standalone(Mask::VOID) {
            return self.fatal("Void can only be used as a standalone type");
        }
        if standalone(Mask::NEVER) {
            return self.fatal("never can only be used as a standalone type");
        }
        Ok((compiled, forced))
    }

    fn single_type(&mut self, kind: &SimpleType) -> Flow<DeclaredType> {
        let name = match kind {
            SimpleType::Array => return Ok(DeclaredType::of(Mask::ARRAY)),
            SimpleType::Callable => return Ok(DeclaredType::of(Mask::CALLABLE)),
            SimpleType::Static => {
                if self.class.is_none() && self.is_scope_known() {
                    return self.fatal("Cannot use \"static\" when no class scope is active");
                }
                return Ok(DeclaredType::of(Mask::STATIC));
            }
            SimpleType::Name(name) => name,
        };
        let builtin = BUILTIN
            .iter()
            .find(|(builtin, _)| name.bytes.eq_ignore_ascii_case(builtin));
        if let Some((_, builtin)) = builtin {
            if name.kind != NameKind::Unqualified {
                let mut message = b"Type declaration '".to_vec();
                message.extend(name.bytes.to_ascii_lowercase());
                message.extend_from_slice(b"' must be unqualified");
                return self.fatal(message);
            }
            return Ok(match builtin {
                Builtin::Mask(mask) => DeclaredType::of(*mask),
                Builtin::Iterable => DeclaredType {
                    mask: Mask::ARRAY,
                    members: vec![Member::Class(b"Traversable".to_vec())],
                    is_iterable: true,
                    ..DeclaredType::default()
                },
            });
        }
        let class = self.type_class_name(name)?;
        self.warn_confusable(name, &class);
        Ok(DeclaredType::class(class))
    }

    /// The class `name` stands for in a type.
    fn type_class_name(&mut self, name: &Name) -> Flow<Vec<u8>> {
        let fetch = match name.kind {
            NameKind::FullyQualified => None,
            NameKind::Unqualified | NameKind::Relative => [Fetch::SelfClass, Fetch::Parent]
                .into_iter()
                .find(|fetch| name.bytes.eq_ignore_ascii_case(fetch.keyword().as_bytes())),
        };
        let Some(fetch) = fetch else {
            let class = self.class_name(name)?;
            if is_reserved_class_name(&class) {
                let mut message = b"Cannot use \"".to_vec();
                message.extend_from_slice(&class);
                message.extend_from_slice(b"\" as a type name as it is reserved");
                return self.fatal(message);
            }
            if class == b"_" {
                self.report(
                    Severity::Deprecated,
                    "Using \"_\" as a type name is deprecated since 8.4",
                );
            }
            return Ok(class);
        };
        self.ensure_valid_fetch(fetch)?;
        let substituted = match (&self.class, self.is_scope_known()) {
            (Some(class), true) if class.name.is_some() => match fetch {
                Fetch::Parent => class.parent.clone(),
                _ => class.name.clone(),
            },
            _ => None,
        };
        Ok(substituted.unwrap_or_else(|| name.bytes.clone()))
    }

    /// Warns when an unqualified class name looks like a misspelled builtin type.
    fn warn_confusable(&mut self, name: &Name, class: &[u8]) {
        if name.kind != NameKind::Unqualified
            || self.imports.contains_key(&name.bytes.to_ascii_lowercase())
        {
            return;
        }
        let Some((_, correct)) = CONFUSABLE
            .iter()
            .find(|(confusable, _)| name.bytes == *confusable)
        else {
            return;
        };
        let mut message = b"\"".to_vec();
        message.extend_from_slice(&name.bytes);
        match correct {
            Some(correct) => {
                message.extend_from_slice(b"\" will be interpreted as a class name. Did you mean \"");
                message.extend_from_slice(correct.as_bytes());
                message.extend_from_slice(b"\"? Write \"\\");
            }
            None => message.extend_from_slice(
                b"\" is not a supported builtin type and will be interpreted as a class name. Write \"\\",
            ),
        }
        message.extend_from_slice(class);
        message.push(b'"');
        if self.namespace.is_some() {
            message.extend_from_slice(b" or import the class with \"use\"");
        }
        message.extend_from_slice(b" to suppress this warning");
        self.report(Severity::Warning, message);
    }

    fn union_type(&mut self, members: &[Type]) -> Flow<DeclaredType> {
        let mut compiled = DeclaredType::default();
        let mut only_iterable_classes = true;
        for member in members {
            let new = match member {
                Type::Intersection(parts) => {
                    only_iterable_classes = false;
                    Member::Intersection(self.intersection_type(parts)?)
                }
                Type::Simple { kind, .. } => {
                    let single = self.single_type(kind)?;
                    if single.mask == Mask::ANY {
                        return self.fatal("Type mixed can only be used as a standalone type");
                    }
                    if single.has_classes() && !single.is_iterable {
                        only_iterable_classes = false;
                    }
                    let overlap = compiled.mask.common(single.mask);
                    if overlap != Mask::NONE {
                        return self.fatal(duplicate(&DeclaredType::of(overlap).display()));
                    }
                    let before = compiled.mask;
                    if (before.intersects(Mask::TRUE) && single.mask == Mask::FALSE)
                        || (before.intersects(Mask::FALSE) && single.mask == Mask::TRUE)
                    {
                        return self
                            .fatal("Type contains both true and false, bool must be used instead");
                    }
                    compiled.mask = before.union(single.mask);
                    match single.members.into_iter().next() {
                        Some(class) => class,
                        None => continue,
                    }
                }
                Type::Union(_) => continue,
            };
            self.ensure_not_redundant(&compiled.members, &new)?;
            compiled.members.push(new);
        }
        let mask = compiled.mask;
        if mask.intersects(Mask::OBJECT)
            && ((!only_iterable_classes && compiled.has_classes()) || mask.intersects(Mask::STATIC))
        {
            let mut message = b"Type ".to_vec();
            message.extend(compiled.display());
            message
                .extend_from_slice(b" contains both object and a class type, which is redundant");
            return self.fatal(message);
        }
        Ok(compiled)
    }

    /// Fails when `new` repeats, or is implied by, one of the `earlier` class members.
    fn ensure_not_redundant(&self, earlier: &[Member], new: &Member) -> Flow {
        for old in earlier {
            match (old, new) {
                (Member::Class(old), Member::Class(new)) if old.eq_ignore_ascii_case(new) => {
                    return self.fatal(duplicate(new));
                }
                (Member::Class(class), Member::Intersection(names))
                | (Member::Intersection(names), Member::Class(class))
                    if names.iter().any(|name| name.eq_ignore_ascii_case(class)) =>
                {
                    return self.fatal(more_restrictive(&joined(names), class));
                }
                (Member::Intersection(old), Member::Intersection(new)) => {
                    let (smaller, larger) = if old.len() < new.len() {
                        (old, new)
                    } else {
                        (new, old)
                    };
                    let contained = smaller
                        .iter()
                        .all(|name| larger.iter().any(|other| other.eq_ignore_ascii_case(name)));
                    if !contained {
                        continue;
                    }
                    if smaller.len() == larger.len() {
                        let mut message = b"Type ".to_vec();
                        message.extend(joined(new));
                        message.extend_from_slice(b" is redundant with type ");
                        message.extend(joined(old));
                        return self.fatal(message);
                    }
                    return self.fatal(more_restrictive(&joined(larger), &joined(smaller)));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn intersection_type(&mut self, members: &[Type]) -> Flow<Vec<Vec<u8>>> {
        let mut names: Vec<Vec<u8>> = Vec::with_capacity(members.len());
        for member in members {
            let Type::Simple { kind, .. } = member else {
                continue;
            };
            let single = self.single_type(kind)?;
            let class = match single.members.as_slice() {
                [Member::Class(class)] if !single.is_iterable => class.clone(),
                _ => {
                    let mut message = b"Type ".to_vec();
                    message.extend(single.display());
                    message.extend_from_slice(b" cannot be part of an intersection type");
                    return self.fatal(message);
                }
            };
            if class.eq_ignore_ascii_case(b"self") || class.eq_ignore_ascii_case(b"parent") {
                let mut message = b"Type ".to_vec();
                message.extend_from_slice(&class);
                message.extend_from_slice(b" cannot be part of an intersection type");
                return self.fatal(message);
            }
            if names.iter().any(|name| name.eq_ignore_ascii_case(&class)) {
                return self.fatal(duplicate(&class));
            }
            names.push(class);
        }
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::{DeclaredType, Mask, Member, is_reserved_class_name, type_name, value_name};
    use crate::check::Checker;
    use php_parser::{ParseOptions, parse};
    use php_value::Value;

    /// Every diagnostic compiling `source` reports, as `severity: message`.
    fn diagnostics(source: &str) -> Vec<String> {
        let source = format!("<?php {source}");
        let program = parse(source.as_bytes(), ParseOptions::default())
            .unwrap()
            .result
            .unwrap();
        let checked = Checker::run(&program);
        checked
            .warnings
            .iter()
            .chain(&checked.fatal)
            .map(|diagnostic| {
                format!(
                    "{}: {}",
                    diagnostic.severity.label(),
                    String::from_utf8_lossy(&diagnostic.message)
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
    fn declared_types_reject_redundant_and_misplaced_parts() {
        assert_cases(&[
            (
                "function f(): ?mixed {}",
                &[
                    "Fatal error: Type mixed cannot be marked as nullable since mixed already includes null",
                ],
            ),
            (
                "function f(): ?null {}",
                &["Fatal error: null cannot be marked as nullable"],
            ),
            (
                "function f(): void|null {}",
                &["Fatal error: Void can only be used as a standalone type"],
            ),
            (
                "function f(): ?never {}",
                &["Fatal error: never can only be used as a standalone type"],
            ),
            (
                "function f(): int|INT {}",
                &["Fatal error: Duplicate type int is redundant"],
            ),
            (
                "function f(): bool|false {}",
                &["Fatal error: Duplicate type false is redundant"],
            ),
            (
                "function f(): true|false {}",
                &["Fatal error: Type contains both true and false, bool must be used instead"],
            ),
            (
                "function f(): mixed|int {}",
                &["Fatal error: Type mixed can only be used as a standalone type"],
            ),
            (
                "function f(A|a $a) {}",
                &["Fatal error: Duplicate type a is redundant"],
            ),
            (
                "function f(iterable|Traversable $a) {}",
                &["Fatal error: Duplicate type Traversable is redundant"],
            ),
            (
                "function f(A|object $a) {}",
                &[
                    "Fatal error: Type A|object contains both object and a class type, which is redundant",
                ],
            ),
            ("function f(iterable|object $a) {}", &[]),
            ("function f(false|null $a, ?false $b) {}", &[]),
            (
                "function f(iterable&A $a) {}",
                &["Fatal error: Type Traversable|array cannot be part of an intersection type"],
            ),
            (
                "function f(int&A $a) {}",
                &["Fatal error: Type int cannot be part of an intersection type"],
            ),
            (
                "function f(A&B&a $a) {}",
                &["Fatal error: Duplicate type a is redundant"],
            ),
            (
                "trait T { function f(A&self $a) {} }",
                &["Fatal error: Type self cannot be part of an intersection type"],
            ),
            (
                "class A { function f(A&self $a) {} }",
                &["Fatal error: Duplicate type A is redundant"],
            ),
            (
                "function f((A&B)|A $a) {}",
                &["Fatal error: Type A&B is redundant as it is more restrictive than type A"],
            ),
            (
                "function f(A|(A&B) $a) {}",
                &["Fatal error: Type A&B is redundant as it is more restrictive than type A"],
            ),
            (
                "function f((A&B)|(B&A) $a) {}",
                &["Fatal error: Type B&A is redundant with type A&B"],
            ),
            (
                "function f((A&B)|(A&B&C) $a) {}",
                &["Fatal error: Type A&B&C is redundant as it is more restrictive than type A&B"],
            ),
            (
                "function f((A&B&C)|(A&B) $a) {}",
                &["Fatal error: Type A&B&C is redundant as it is more restrictive than type A&B"],
            ),
            ("function f((A&B)|(C&D) $a) {}", &[]),
        ]);
    }

    #[test]
    fn type_names_resolve_against_the_scope_and_imports() {
        assert_cases(&[
            (
                "function f(\\int $a) {}",
                &["Fatal error: Type declaration 'int' must be unqualified"],
            ),
            (
                "function f(namespace\\INT $a) {}",
                &["Fatal error: Type declaration 'int' must be unqualified"],
            ),
            (
                "function f(A\\int $a) {}",
                &["Fatal error: Cannot use \"A\\int\" as a type name as it is reserved"],
            ),
            (
                "function f(\\self $a) {}",
                &["Fatal error: '\\self' is an invalid class name"],
            ),
            (
                "function f(self $a) {}",
                &["Fatal error: Cannot use \"self\" when no class scope is active"],
            ),
            (
                "class A { function f(parent $a) {} }",
                &["Fatal error: Cannot use \"parent\" when current class scope has no parent"],
            ),
            (
                "function f(): static {}",
                &["Fatal error: Cannot use \"static\" when no class scope is active"],
            ),
            ("$f = function (): static {};", &[]),
            (
                "function f(_ $a) {}",
                &["Deprecated: Using \"_\" as a type name is deprecated since 8.4"],
            ),
            (
                "function f(boolean $a) {}",
                &[
                    "Warning: \"boolean\" will be interpreted as a class name. Did you mean \"bool\"? Write \"\\boolean\" to suppress this warning",
                ],
            ),
            (
                "namespace A; function f(resource $a) {}",
                &[
                    "Warning: \"resource\" is not a supported builtin type and will be interpreted as a class name. Write \"\\A\\resource\" or import the class with \"use\" to suppress this warning",
                ],
            ),
            ("use A\\B as integer; function f(integer $a) {}", &[]),
            ("function f(Integer $a, \\integer $b) {}", &[]),
            (
                "use X\\Y as Z; function f(Z $a = 1) {}",
                &["Fatal error: Cannot use int as default value for parameter $a of type X\\Y"],
            ),
            (
                "use X\\Y as Z; function f(Z\\W $a = 1) {}",
                &["Fatal error: Cannot use int as default value for parameter $a of type X\\Y\\W"],
            ),
            (
                "use X\\{Y, function g}; function f(Y $a = 1) {}",
                &["Fatal error: Cannot use int as default value for parameter $a of type X\\Y"],
            ),
            (
                "namespace N; use X\\Y; function f(Y|y $a) {}",
                &["Fatal error: Duplicate type X\\Y is redundant"],
            ),
            (
                "namespace N { use X\\Y; } namespace { function f(Y $a = 1) {} }",
                &["Fatal error: Cannot use int as default value for parameter $a of type Y"],
            ),
            (
                "namespace N; function f(namespace\\A $a = 1) {}",
                &["Fatal error: Cannot use int as default value for parameter $a of type N\\A"],
            ),
            (
                "class A extends B { function m(parent $a = 1) {} }",
                &["Fatal error: Cannot use int as default value for parameter $a of type B"],
            ),
            (
                "trait T { function f(self $a = 1) {} }",
                &["Fatal error: Cannot use int as default value for parameter $a of type self"],
            ),
        ]);
    }

    #[test]
    fn parameters_check_defaults_and_order() {
        assert_cases(&[
            (
                "function f(int $a = true) {}",
                &["Fatal error: Cannot use bool as default value for parameter $a of type int"],
            ),
            (
                "function f(callable $a = 'strlen') {}",
                &[
                    "Fatal error: Cannot use string as default value for parameter $a of type callable",
                ],
            ),
            (
                "function f(iterable|null $a = 1) {}",
                &[
                    "Fatal error: Cannot use int as default value for parameter $a of type Traversable|array|null",
                ],
            ),
            (
                "function f(A|B|null|int|false $a = 1.5) {}",
                &[
                    "Fatal error: Cannot use float as default value for parameter $a of type A|B|int|false|null",
                ],
            ),
            (
                "function f((A&B)|null $a = 1) {}",
                &[
                    "Fatal error: Cannot use int as default value for parameter $a of type (A&B)|null",
                ],
            ),
            (
                "function f(string $a = __LINE__) {}",
                &["Fatal error: Cannot use int as default value for parameter $a of type string"],
            ),
            ("function f(float $a = 1, int $b = FOO) {}", &[]),
            (
                "function f(int ...$a = 1) {}",
                &["Fatal error: Variadic parameter cannot have a default value"],
            ),
            (
                "function f(...$a, $b) {}",
                &["Fatal error: Only the last parameter can be variadic"],
            ),
            (
                "function f(void $a) {}",
                &["Fatal error: void cannot be used as a parameter type"],
            ),
            (
                "function f(never $a) {}",
                &["Fatal error: never cannot be used as a parameter type"],
            ),
            (
                "function f(int $a = null, int $b = 1.5) {}",
                &[
                    "Deprecated: f(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead",
                    "Fatal error: Cannot use float as default value for parameter $b of type int",
                ],
            ),
            (
                "namespace N; function f(A&B $a = null) {}",
                &[
                    "Deprecated: N\\f(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead",
                ],
            ),
            (
                "class A { function __construct(public int $a = null) {} }",
                &["Fatal error: Cannot use null as default value for parameter $a of type int"],
            ),
            (
                "new class { function m(int $a = null) {} };",
                &[
                    "Deprecated: class@anonymous(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead",
                ],
            ),
            (
                "function f(int $a = 1, $b = 2, $c, ...$d) {}",
                &[
                    "Deprecated: f(): Optional parameter $a declared before required parameter $c is implicitly treated as a required parameter",
                    "Deprecated: f(): Optional parameter $b declared before required parameter $c is implicitly treated as a required parameter",
                ],
            ),
            (
                "interface I { function f(int $a = null, $b); }",
                &[
                    "Deprecated: I::f(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead",
                ],
            ),
            (
                "function f(mixed $a = null, $b) {}",
                &[
                    "Deprecated: f(): Optional parameter $a declared before required parameter $b is implicitly treated as a required parameter",
                ],
            ),
        ]);
    }

    #[test]
    fn properties_and_class_constants_check_types_and_values() {
        assert_cases(&[
            (
                "class A { public int $a = true; }",
                &["Fatal error: Cannot use true as default value for property A::$a of type int"],
            ),
            (
                "class A { public int $a = 1, $b = false; }",
                &["Fatal error: Cannot use false as default value for property A::$b of type int"],
            ),
            (
                "class A { public int|string $a = null; }",
                &[
                    "Fatal error: Default value for property of type string|int may not be null. Use the nullable type string|int|null to allow null default value",
                ],
            ),
            (
                "class A { public A&B $a = null; }",
                &["Fatal error: Cannot use null as default value for property A::$a of type A&B"],
            ),
            (
                "class A { public float $a = 1; public ?float $b = null; public $c = 'x'; }",
                &[],
            ),
            (
                "new class { public callable $a; };",
                &["Fatal error: Property class@anonymous::$a cannot have type callable"],
            ),
            (
                "class A { public ?callable $a; }",
                &["Fatal error: Property A::$a cannot have type ?callable"],
            ),
            (
                "class A { const int B = true; }",
                &["Fatal error: Cannot use bool as value for class constant A::B of type int"],
            ),
            (
                "class A { const self B = 1; }",
                &["Fatal error: Cannot use int as value for class constant A::B of type A"],
            ),
            (
                "class A { const static B = 1; }",
                &["Fatal error: Cannot use int as value for class constant A::B of type static"],
            ),
            (
                "new class { const int B = 1.5; };",
                &[
                    "Fatal error: Cannot use float as value for class constant class@anonymous::B of type int",
                ],
            ),
            (
                "class A { const mixed B = 1; const ?callable C = 1; }",
                &["Fatal error: Class constant A::C cannot have type ?callable"],
            ),
            ("class A { const float B = 1, C = self::B; }", &[]),
        ]);
    }

    #[test]
    fn returns_are_checked_against_the_return_type() {
        assert_cases(&[
            (
                "function f(): void { return null; }",
                &[
                    "Fatal error: A void function must not return a value (did you mean \"return;\" instead of \"return null;\"?)",
                ],
            ),
            (
                "function f(): void { return \\NULL; }",
                &[
                    "Fatal error: A void function must not return a value (did you mean \"return;\" instead of \"return null;\"?)",
                ],
            ),
            (
                "function f(): void { if (1) { return; } return 2; }",
                &["Fatal error: A void function must not return a value"],
            ),
            (
                "class A { function f() { return function (): void { return 1; }; } }",
                &["Fatal error: A void method must not return a value"],
            ),
            (
                "class A { function f() { function g(): void { return 1; } } }",
                &["Fatal error: A void function must not return a value"],
            ),
            (
                "function f(): never { return; }",
                &["Fatal error: A never-returning function must not return"],
            ),
            (
                "function f(): ?int { return; }",
                &[
                    "Fatal error: A function with return type must return a value (did you mean \"return null;\" instead of \"return;\"?)",
                ],
            ),
            (
                "function f(): int { return; }",
                &["Fatal error: A function with return type must return a value"],
            ),
            (
                "class A { function __TOSTRING() { return; } }",
                &["Fatal error: A method with return type must return a value"],
            ),
            (
                "function f(): int { $g = function () { return; }; return 1; }",
                &[],
            ),
            (
                "$f = fn(): void => 1;",
                &["Fatal error: A void function must not return a value"],
            ),
            (
                "$f = fn(): void => null;",
                &[
                    "Fatal error: A void function must not return a value (did you mean \"return;\" instead of \"return null;\"?)",
                ],
            ),
            ("$f = fn(): never => 1; return;", &[]),
            (
                "function &f(): void {}",
                &["Deprecated: f(): Returning by reference from a void function is deprecated"],
            ),
            ("abstract class A { abstract function f(): void; }", &[]),
        ]);
    }

    #[test]
    fn generators_check_their_return_type_first() {
        assert_cases(&[
            (
                "function f(): void { return 1; yield; }",
                &[
                    "Fatal error: Generator return type must be a supertype of Generator, void given",
                ],
            ),
            (
                "function f(): ?int { yield; }",
                &[
                    "Fatal error: Generator return type must be a supertype of Generator, ?int given",
                ],
            ),
            (
                "class A { function f(): static { yield; } }",
                &[
                    "Fatal error: Generator return type must be a supertype of Generator, static given",
                ],
            ),
            (
                "class A { function __toString() { yield; } }",
                &[
                    "Fatal error: Generator return type must be a supertype of Generator, string given",
                ],
            ),
            ("function f(): Generator { return; yield; }", &[]),
            (
                "function f(): iterable { yield; } function g(): Traversable|int { yield; }",
                &[],
            ),
            (
                "function f(): mixed { yield; } function g(): object { yield; }",
                &[],
            ),
            (
                "function &f() { yield from g(); }",
                &["Fatal error: Cannot use \"yield from\" inside a by-reference generator"],
            ),
            ("function &f() { yield; }", &[]),
        ]);
    }

    fn object() -> php_value::Object {
        php_value::ObjectStore::new()
            .create(php_value::PhpString::new(b"C"))
            .unwrap()
    }

    fn show(declared: &DeclaredType) -> String {
        String::from_utf8_lossy(&declared.display()).into_owned()
    }

    fn classes(names: &[&str]) -> Vec<Vec<u8>> {
        names.iter().map(|name| name.as_bytes().to_vec()).collect()
    }

    #[test]
    fn types_print_classes_then_builtins_in_a_fixed_order() {
        let scalar = DeclaredType::of(Mask::INT.union(Mask::STRING));
        assert_eq!(show(&scalar), "string|int");
        assert_eq!(show(&DeclaredType::of(Mask::INT.union(Mask::NULL))), "?int");
        assert_eq!(show(&DeclaredType::of(Mask::NULL)), "null");
        assert_eq!(show(&DeclaredType::of(Mask::ANY)), "mixed");
        assert_eq!(show(&DeclaredType::of(Mask::BOOL)), "bool");
        assert_eq!(
            show(&DeclaredType::of(Mask::FALSE.union(Mask::NULL))),
            "?false"
        );
        assert_eq!(show(&DeclaredType::of(Mask::TRUE)), "true");
        let all = [
            Mask::STATIC,
            Mask::CALLABLE,
            Mask::OBJECT,
            Mask::ARRAY,
            Mask::FLOAT,
            Mask::VOID,
            Mask::NEVER,
        ]
        .into_iter()
        .fold(Mask::NONE, Mask::union);
        assert_eq!(
            show(&DeclaredType::of(all)),
            "static|callable|object|array|float|void|never"
        );

        let mut union = DeclaredType::class(b"A".to_vec());
        union
            .members
            .push(Member::Intersection(classes(&["B", "C"])));
        union.mask = Mask::INT.union(Mask::NULL);
        assert_eq!(show(&union), "A|(B&C)|int|null");
        let intersection = DeclaredType {
            members: vec![Member::Intersection(classes(&["B", "C"]))],
            is_intersection: true,
            ..DeclaredType::default()
        };
        assert_eq!(show(&intersection), "B&C");
        let nullable = DeclaredType {
            mask: Mask::NULL,
            ..DeclaredType::class(b"A".to_vec())
        };
        assert_eq!(show(&nullable), "?A");
    }

    #[test]
    fn values_are_checked_by_kind() {
        let float = DeclaredType::of(Mask::FLOAT);
        assert!(float.accepts(&Value::Int(1)));
        assert!(float.accepts(&Value::Float(1.5)));
        assert!(!float.accepts(&Value::Null));
        let class = DeclaredType::class(b"A".to_vec());
        assert!(!class.accepts(&Value::Int(1)));
        assert!(DeclaredType::of(Mask::ANY).accepts(&Value::Bool(true)));
        assert!(!DeclaredType::of(Mask::TRUE).accepts(&Value::Bool(false)));
        assert!(DeclaredType::of(Mask::OBJECT).accepts(&Value::Object(object())));
    }

    #[test]
    fn generators_need_an_object_or_iterator_return_type() {
        assert!(DeclaredType::of(Mask::OBJECT).admits_generators());
        assert!(DeclaredType::class(b"iterator".to_vec()).admits_generators());
        assert!(!DeclaredType::of(Mask::STATIC).admits_generators());
        let intersection = DeclaredType {
            members: vec![Member::Intersection(classes(&["Countable", "Traversable"]))],
            is_intersection: true,
            ..DeclaredType::default()
        };
        assert!(intersection.admits_generators());
        let dnf = DeclaredType {
            is_intersection: false,
            ..intersection
        };
        assert!(!dnf.admits_generators());
    }

    #[test]
    fn values_are_named_for_messages() {
        assert_eq!(type_name(&Value::Bool(true)), "bool");
        assert_eq!(value_name(&Value::Bool(true)), "true");
        assert_eq!(value_name(&Value::Bool(false)), "false");
        assert_eq!(value_name(&Value::Null), "null");
        assert_eq!(type_name(&Value::Object(object())), "object");
        assert_eq!(type_name(&Value::Float(1.0)), "float");
    }

    #[test]
    fn reserved_names_are_checked_by_their_last_part() {
        assert!(is_reserved_class_name(b"A\\Int"));
        assert!(is_reserved_class_name(b"callable"));
        assert!(!is_reserved_class_name(b"Integer"));
    }
}
