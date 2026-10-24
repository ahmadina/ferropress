//! The names a file declares and imports: namespaces, `use` imports, `declare` directives, the
//! symbols declared at the top of the file, and the generated names of closures.

use std::collections::HashMap;

use php_parser::ast::{
    AttributeGroup, ClassKind, ConstItem, ExprKind, Function, Stmt, StmtKind, TopStatement,
    UseItem, UseKind,
};

use super::attributes::Target;
use super::types::{ANONYMOUS_CLASS, is_reserved_class_name};
use super::{Checker, ClosureName, Code, Flow};
use crate::diagnostic::Severity;

/// Kinds of symbol, as bits, in the record of what the file declared.
const CLASS: u8 = 1;
const FUNCTION: u8 = 1 << 1;
const CONSTANT: u8 = 1 << 2;

/// The `use` imports of the current namespace.
#[derive(Debug, Default)]
pub(super) struct Imports {
    /// Class imports by lowercase alias.
    pub(super) classes: HashMap<Vec<u8>, Vec<u8>>,
    /// Function imports by lowercase alias.
    functions: HashMap<Vec<u8>, Vec<u8>>,
    /// Constant imports by alias, which is case-sensitive.
    constants: HashMap<Vec<u8>, Vec<u8>>,
}

impl Imports {
    fn clear(&mut self) {
        self.classes.clear();
        self.functions.clear();
        self.constants.clear();
    }
}

/// What the file has declared and imported so far.
#[derive(Debug, Default)]
pub(super) struct FileNames {
    pub(super) imports: Imports,
    /// The kinds of symbol declared under each name: classes and functions by lowercase name,
    /// constants by name as written.
    seen: HashMap<Vec<u8>, u8>,
    /// The top-level functions declared so far, by lowercase name, with their line.
    functions: HashMap<Vec<u8>, u32>,
    /// Whether a braced namespace has been declared.
    bracketed: bool,
    /// Whether code is inside a namespace declaration.
    in_namespace: bool,
}

fn use_kind(kind: UseKind) -> (&'static str, u8) {
    match kind {
        UseKind::Class => ("", CLASS),
        UseKind::Function => (" function", FUNCTION),
        UseKind::Constant => (" const", CONSTANT),
    }
}

impl<'a> Checker<'a> {
    pub(super) fn top_statements(
        &mut self,
        statements: &'a [TopStatement],
        file_level: bool,
    ) -> Flow {
        for (index, statement) in statements.iter().enumerate() {
            match statement {
                TopStatement::Namespace { name, body, line } => {
                    self.line = *line;
                    let preceding = file_level.then(|| &statements[..index]);
                    self.namespace_declaration(name.as_deref(), body.is_some(), preceding)?;
                    if let Some(body) = body {
                        self.top_statements(body, false)?;
                        self.names.in_namespace = false;
                        self.names.imports.clear();
                        self.namespace = None;
                    }
                    continue;
                }
                TopStatement::HaltCompiler { line } => {
                    self.line = *line;
                    continue;
                }
                TopStatement::Statement(stmt) => match &stmt.kind {
                    StmtKind::Function(function) => {
                        self.line = stmt.line;
                        self.function_declaration(function, true)?;
                    }
                    StmtKind::Declare { directives, body } => {
                        let preceding = file_level.then(|| &statements[..index]);
                        self.declare(directives, body.as_deref(), preceding)?;
                    }
                    _ => self.stmt(stmt)?,
                },
                TopStatement::Use { kind, items, line } => {
                    self.line = *line;
                    for item in items {
                        self.import(*kind, &[], item)?;
                    }
                }
                TopStatement::GroupUse {
                    kind,
                    prefix,
                    items,
                    line,
                } => {
                    self.line = *line;
                    for item in items {
                        self.import(item.kind.unwrap_or(*kind), prefix, item)?;
                    }
                }
                TopStatement::Const {
                    items,
                    attributes,
                    line,
                } => {
                    self.line = *line;
                    self.constant_declaration(items, attributes)?;
                }
            }
            if self.names.bracketed && !self.names.in_namespace {
                return self.fatal("No code may exist outside of namespace {}");
            }
        }
        Ok(())
    }

    /// Opens a namespace. `preceding` holds the statements before it when it is written at the
    /// top level of the file.
    fn namespace_declaration(
        &mut self,
        name: Option<&[u8]>,
        bracketed: bool,
        preceding: Option<&[TopStatement]>,
    ) -> Flow {
        let mixed = if self.names.bracketed {
            !bracketed
        } else {
            bracketed && self.namespace.is_some()
        };
        if mixed {
            return self.fatal(
                "Cannot mix bracketed namespace declarations with unbracketed namespace declarations",
            );
        }
        if self.names.bracketed && (self.namespace.is_some() || self.names.in_namespace) {
            return self.fatal("Namespace declarations cannot be nested");
        }
        let is_first = if bracketed {
            !self.names.bracketed
        } else {
            self.namespace.is_none()
        };
        let only_declares = preceding.is_some_and(|preceding| {
            preceding.iter().all(|statement| {
                matches!(statement, TopStatement::Statement(stmt)
                    if matches!(stmt.kind, StmtKind::Declare { .. } | StmtKind::Nop))
            })
        });
        if is_first && !only_declares {
            return self.fatal(
                "Namespace declaration statement has to be the very first statement or after any declare call in the script",
            );
        }
        self.namespace = name.map(<[u8]>::to_vec);
        self.names.imports.clear();
        self.names.in_namespace = true;
        self.names.bracketed |= bracketed;
        Ok(())
    }

    fn has_seen(&self, name: &[u8], kind: u8) -> bool {
        self.names
            .seen
            .get(name)
            .is_some_and(|kinds| kinds & kind != 0)
    }

    fn register(&mut self, name: Vec<u8>, kind: u8) {
        *self.names.seen.entry(name).or_default() |= kind;
    }

    /// Records the import `item` of `kind`, which a group `use` writes after `prefix`.
    fn import(&mut self, kind: UseKind, prefix: &[u8], item: &UseItem) -> Flow {
        let mut old = prefix.to_vec();
        if !old.is_empty() {
            old.push(b'\\');
        }
        old.extend_from_slice(&item.name);
        let new = match (&item.alias, old.iter().rposition(|&byte| byte == b'\\')) {
            (Some(alias), _) => alias.clone(),
            (None, Some(slash)) => old[slash + 1..].to_vec(),
            (None, None) => {
                if self.namespace.is_none() {
                    let mut message = b"The use statement with non-compound name '".to_vec();
                    message.extend_from_slice(&old);
                    message.extend_from_slice(b"' has no effect");
                    self.report(Severity::Warning, message);
                }
                old.clone()
            }
        };
        let (label, bit) = use_kind(kind);
        let lookup = if kind == UseKind::Constant {
            new.clone()
        } else {
            new.to_ascii_lowercase()
        };
        if kind == UseKind::Class && is_reserved_class_name(&new) {
            let mut message = b"Cannot use ".to_vec();
            message.extend_from_slice(&old);
            message.extend_from_slice(b" as ");
            message.extend_from_slice(&new);
            message.extend_from_slice(b" because '");
            message.extend_from_slice(&new);
            message.extend_from_slice(b"' is a special class name");
            return self.fatal(message);
        }
        let in_namespace = match &self.namespace {
            Some(namespace) => {
                let mut name = namespace.to_ascii_lowercase();
                name.push(b'\\');
                name.extend_from_slice(&lookup);
                name
            }
            None => lookup.clone(),
        };
        let table = match kind {
            UseKind::Class => &self.names.imports.classes,
            UseKind::Function => &self.names.imports.functions,
            UseKind::Constant => &self.names.imports.constants,
        };
        let declared =
            self.has_seen(&in_namespace, bit) && !old.eq_ignore_ascii_case(&in_namespace);
        if declared || table.contains_key(&lookup) {
            let mut message = format!("Cannot use{label} ").into_bytes();
            message.extend_from_slice(&old);
            message.extend_from_slice(b" as ");
            message.extend_from_slice(&new);
            message.extend_from_slice(b" because the name is already in use");
            return self.fatal(message);
        }
        let table = match kind {
            UseKind::Class => &mut self.names.imports.classes,
            UseKind::Function => &mut self.names.imports.functions,
            UseKind::Constant => &mut self.names.imports.constants,
        };
        table.insert(lookup, old);
        Ok(())
    }

    /// Compiles a named function declaration; one at the top level of the file (or of a
    /// namespace) is also bound while compiling.
    pub(super) fn function_declaration(&mut self, function: &'a Function, top_level: bool) -> Flow {
        let name = self.qualify(&function.name);
        let lower = name.to_ascii_lowercase();
        let imported = self
            .names
            .imports
            .functions
            .get(&function.name.to_ascii_lowercase());
        if imported.is_some_and(|imported| !lower.eq_ignore_ascii_case(imported)) {
            let mut message = b"Cannot redeclare function ".to_vec();
            message.extend_from_slice(&name);
            message.extend_from_slice(b"() (previously declared as local import)");
            return self.fatal(message);
        }
        if lower == b"__autoload" {
            return self
                .fatal("__autoload() is no longer supported, use spl_autoload_register() instead");
        }
        if function.name.eq_ignore_ascii_case(b"assert") {
            return self.fatal(
                "Defining a custom assert() function is not allowed, as the function has special semantics",
            );
        }
        self.register(lower.clone(), FUNCTION);
        let frame = self.function_frame(function, Code::Function(name.clone()))?;
        if !top_level {
            return self.resolve_jumps(&frame.jumps);
        }
        if let Some(&first) = self.names.functions.get(&lower) {
            self.line = function.line;
            let mut message = b"Cannot redeclare function ".to_vec();
            message.extend_from_slice(&name);
            message.extend_from_slice(b"() (previously declared in ");
            message.extend_from_slice(self.file);
            message.extend_from_slice(format!(":{first})").as_bytes());
            return self.fatal(message);
        }
        self.names.functions.insert(lower, function.line);
        self.resolve_jumps(&frame.jumps)
    }

    /// Compiles `declare(directives) body`; `preceding` holds the statements before it when it is
    /// a statement of the file itself rather than of a block, a function or a namespace body.
    pub(super) fn declare(
        &mut self,
        directives: &'a [ConstItem],
        body: Option<&'a Stmt>,
        preceding: Option<&[TopStatement]>,
    ) -> Flow {
        if let Some(first) = directives.first() {
            self.line = first.line;
        }
        let only_declares_before = |allow_empty: bool| {
            preceding.is_some_and(|preceding| {
                preceding.iter().all(|statement| match statement {
                    TopStatement::Statement(stmt) => match stmt.kind {
                        StmtKind::Declare { .. } => true,
                        StmtKind::Nop => allow_empty,
                        _ => false,
                    },
                    _ => false,
                })
            })
        };
        for directive in directives {
            let name = directive.name.as_slice();
            if !matches!(
                directive.value.kind,
                ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::String(_)
            ) {
                let mut message = b"declare(".to_vec();
                message.extend_from_slice(name);
                message.extend_from_slice(b") value must be a literal");
                return self.fatal(message);
            }
            if name.eq_ignore_ascii_case(b"ticks") {
                continue;
            }
            if name.eq_ignore_ascii_case(b"encoding") {
                if !only_declares_before(false) {
                    return self.fatal(
                        "Encoding declaration pragma must be the very first statement in the script",
                    );
                }
                continue;
            }
            if name.eq_ignore_ascii_case(b"strict_types") {
                if !only_declares_before(true) {
                    return self.fatal(
                        "strict_types declaration must be the very first statement in the script",
                    );
                }
                if body.is_some() {
                    return self.fatal("strict_types declaration must not use block mode");
                }
                if !matches!(directive.value.kind, ExprKind::Int(0 | 1)) {
                    return self.fatal("strict_types declaration must have 0 or 1 as its value");
                }
                continue;
            }
            let mut message = b"Unsupported declare '".to_vec();
            message.extend_from_slice(name);
            message.push(b'\'');
            self.report(Severity::Warning, message);
        }
        body.map_or(Ok(()), |body| self.stmt(body))
    }

    /// The checks the reference makes on a named class-like declaration before compiling it.
    pub(super) fn class_declaration(&mut self, kind: ClassKind, name: &[u8]) -> Flow {
        if self.class.is_some() {
            return self.fatal("Class declarations may not be nested");
        }
        let what = match kind {
            ClassKind::Class => "a class name",
            ClassKind::Interface => "an interface name",
            ClassKind::Trait => "a trait name",
            ClassKind::Enum => "an enum name",
        };
        if is_reserved_class_name(name) {
            let mut message = b"Cannot use \"".to_vec();
            message.extend_from_slice(name);
            message.extend_from_slice(format!("\" as {what} as it is reserved").as_bytes());
            return self.fatal(message);
        }
        if name == b"_" {
            self.report(
                Severity::Deprecated,
                format!("Using \"_\" as {what} is deprecated since 8.4"),
            );
        }
        let full = self.qualify(name);
        let lower = full.to_ascii_lowercase();
        let imported = self.names.imports.classes.get(&name.to_ascii_lowercase());
        if imported.is_some_and(|imported| !lower.eq_ignore_ascii_case(imported)) {
            let mut message = b"Cannot redeclare class ".to_vec();
            message.extend_from_slice(&full);
            message.extend_from_slice(b" (previously declared as local import)");
            return self.fatal(message);
        }
        self.register(lower, CLASS);
        Ok(())
    }

    /// Compiles a top-level `const` statement.
    fn constant_declaration(
        &mut self,
        items: &'a [ConstItem],
        attributes: &'a [AttributeGroup],
    ) -> Flow {
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
            let name = self.qualify(&item.name);
            if self
                .names
                .imports
                .constants
                .get(&item.name)
                .is_some_and(|imported| *imported != name)
            {
                let mut message = b"Cannot declare const ".to_vec();
                message.extend_from_slice(&name);
                message.extend_from_slice(b" because the name is already in use");
                return self.fatal(message);
            }
            self.register(name, CONSTANT);
        }
        if !attributes.is_empty() {
            if items.len() > 1 {
                return self.fatal("Cannot apply attributes to multiple constants at once");
            }
            self.attributes(attributes, Target::Constant)?;
        }
        Ok(())
    }

    /// The name the reference generates for a closure declared on `line` in the current code:
    /// `{closure:PARENT:LINE}`, where the parent is the file at the top level, `f()` or
    /// `Class::m()` in a function or method, and the enclosing closure's name in a closure.
    pub(super) fn closure_name(&self, line: u32) -> ClosureName {
        let mut text = b"{closure:".to_vec();
        match &self.frame.code {
            Code::File => text.extend_from_slice(self.file),
            Code::Closure(parent) => {
                text.extend_from_slice(&parent.text);
                if !parent.complete {
                    return ClosureName {
                        text,
                        complete: false,
                    };
                }
            }
            Code::Function(function) => {
                text.extend_from_slice(function);
                text.extend_from_slice(b"()");
            }
            Code::Method {
                class: Some(class),
                name,
            } => {
                text.extend_from_slice(class);
                text.extend_from_slice(b"::");
                text.extend_from_slice(name);
                text.extend_from_slice(b"()");
            }
            Code::Method { class: None, .. } => {
                text.extend_from_slice(ANONYMOUS_CLASS);
                return ClosureName {
                    text,
                    complete: false,
                };
            }
        }
        text.extend_from_slice(format!(":{line}}}").as_bytes());
        ClosureName {
            text,
            complete: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::check::Checker;
    use php_parser::{ParseOptions, parse};

    /// Every diagnostic compiling `source` from `/s/a.php` reports, as `severity: message @line`.
    fn diagnostics(source: &str) -> Vec<String> {
        let source = format!("<?php {source}");
        let program = parse(source.as_bytes(), ParseOptions::default())
            .unwrap()
            .result
            .unwrap();
        let checked = Checker::run(&program, b"/s/a.php");
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
    fn namespaces_must_not_mix_nest_or_follow_code() {
        let mix = "Fatal error: Cannot mix bracketed namespace declarations with unbracketed namespace declarations @1";
        let first = "Fatal error: Namespace declaration statement has to be the very first statement or after any declare call in the script @1";
        let outside = "Fatal error: No code may exist outside of namespace {} @1";
        assert_cases(&[
            ("namespace A; namespace B {}", &[mix]),
            ("namespace A {} namespace B;", &[mix]),
            (
                "namespace A { namespace B {} }",
                &["Fatal error: Namespace declarations cannot be nested @1"],
            ),
            ("echo 1; namespace A;", &[first]),
            ("declare(ticks=1); ; namespace A;", &[]),
            ("namespace A; echo 1; namespace B;", &[]),
            ("namespace A {} namespace {} namespace B {}", &[]),
            ("namespace A {} echo 1;", &[outside]),
            ("namespace A {} use B\\C;", &[outside]),
            ("namespace A {} const C = 1;", &[outside]),
            ("namespace A {} __halt_compiler();", &[]),
        ]);
    }

    #[test]
    fn imports_reject_conflicts_and_warn_when_useless() {
        assert_cases(&[
            (
                "use A;",
                &["Warning: The use statement with non-compound name 'A' has no effect @1"],
            ),
            ("namespace N; use A;", &[]),
            (
                "use A\\B, C\\b;",
                &["Fatal error: Cannot use C\\b as b because the name is already in use @1"],
            ),
            (
                "use function A\\f, B\\F;",
                &[
                    "Fatal error: Cannot use function B\\F as F because the name is already in use @1",
                ],
            ),
            ("use const A\\C, B\\c;", &[]),
            (
                "use const A\\C, B\\C;",
                &["Fatal error: Cannot use const B\\C as C because the name is already in use @1"],
            ),
            (
                "use A\\B as parent;",
                &[
                    "Fatal error: Cannot use A\\B as parent because 'parent' is a special class name @1",
                ],
            ),
            ("use function A\\self;", &[]),
            (
                "namespace N; class B {} use A\\B;",
                &["Fatal error: Cannot use A\\B as B because the name is already in use @1"],
            ),
            ("namespace N; class B {} use N\\b;", &[]),
            (
                "function f() {} use function A\\f;",
                &[
                    "Fatal error: Cannot use function A\\f as f because the name is already in use @1",
                ],
            ),
            ("const C = 1; use const A\\c;", &[]),
            ("namespace N { use A\\B; } namespace M { use C\\B; }", &[]),
            ("use A\\{function f, const F, function F as G, g};", &[]),
            (
                "use A\\{B, function B, C\\B};",
                &["Fatal error: Cannot use A\\C\\B as B because the name is already in use @1"],
            ),
        ]);
    }

    #[test]
    fn declarations_reject_clashes_with_imports() {
        assert_cases(&[
            (
                "use A\\B; class b {}",
                &["Fatal error: Cannot redeclare class b (previously declared as local import) @1"],
            ),
            ("namespace N; use N\\B; class b {}", &[]),
            (
                "use function A\\f; function F() {}",
                &[
                    "Fatal error: Cannot redeclare function F() (previously declared as local import) @1",
                ],
            ),
            ("namespace N; use function N\\f; function f() {}", &[]),
            (
                "namespace N; use const A\\C; const C = 1;",
                &["Fatal error: Cannot declare const N\\C because the name is already in use @1"],
            ),
            ("namespace N; use const N\\C; const C = 1;", &[]),
            ("use const A\\c; const C = 1;", &[]),
        ]);
    }

    #[test]
    fn function_declarations_follow_the_reference_rules() {
        assert_cases(&[
            (
                "function __AutoLoad() {}",
                &[
                    "Fatal error: __autoload() is no longer supported, use spl_autoload_register() instead @1",
                ],
            ),
            ("namespace N; function __autoload() {}", &[]),
            (
                "namespace N; function ASSERT() {}",
                &[
                    "Fatal error: Defining a custom assert() function is not allowed, as the function has special semantics @1",
                ],
            ),
            (
                "function f() {}\n\nfunction F() {\n}",
                &[
                    "Fatal error: Cannot redeclare function F() (previously declared in /s/a.php:1) @3",
                ],
            ),
            (
                "namespace N; function f() {} function f() {}",
                &[
                    "Fatal error: Cannot redeclare function N\\f() (previously declared in /s/a.php:1) @1",
                ],
            ),
            ("function f() { function g() {} function g() {} }", &[]),
            ("if (1) { function f() {} } function f() {}", &[]),
        ]);
    }

    #[test]
    fn class_declarations_check_nesting_and_reserved_names() {
        assert_cases(&[
            (
                "class A { function f() { class B {} } }",
                &["Fatal error: Class declarations may not be nested @1"],
            ),
            (
                "class A { function f() { function g() { class B {} } } }",
                &[],
            ),
            ("class A { function f() { new class {}; } }", &[]),
            (
                "class self {}",
                &["Fatal error: Cannot use \"self\" as a class name as it is reserved @1"],
            ),
            (
                "interface iterable {}",
                &["Fatal error: Cannot use \"iterable\" as an interface name as it is reserved @1"],
            ),
            (
                "trait mixed {}",
                &["Fatal error: Cannot use \"mixed\" as a trait name as it is reserved @1"],
            ),
            (
                "enum never {}",
                &["Fatal error: Cannot use \"never\" as an enum name as it is reserved @1"],
            ),
            (
                "class _ {}",
                &["Deprecated: Using \"_\" as a class name is deprecated since 8.4 @1"],
            ),
        ]);
    }

    #[test]
    fn closures_are_named_after_where_they_are_declared() {
        let message = "(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead";
        assert_cases(&[
            (
                "$f = function (int $a = null) {};",
                &[&format!("Deprecated: {{closure:/s/a.php:1}}{message} @1")],
            ),
            (
                "namespace N; function f() {\n$g = fn(int $a = null) => 1; }",
                &[&format!("Deprecated: {{closure:N\\f():2}}{message} @2")],
            ),
            (
                "class A { function m() { $g = function (int $a = null) {}; } }",
                &[&format!("Deprecated: {{closure:A::m():1}}{message} @1")],
            ),
            (
                "$f = function () {\n$g = function (int $a = null) {}; };",
                &[&format!(
                    "Deprecated: {{closure:{{closure:/s/a.php:1}}:2}}{message} @2"
                )],
            ),
            (
                "new class { function m() { $g = function () { $h = function (int $a = null) {}; }; } };",
                &[&format!(
                    "Deprecated: {{closure:{{closure:class@anonymous{message} @1"
                )],
            ),
            (
                "function f() { return function () { static $a = __FUNCTION__ . __METHOD__; }; }",
                &[],
            ),
        ]);
    }

    #[test]
    fn declare_directives_follow_the_reference_rules() {
        assert_cases(&[
            ("declare(strict_types=1); declare(ticks=1.5);", &[]),
            ("; declare(strict_types=0x1);", &[]),
            ("declare(STRICT_TYPES=1, Ticks=\"a\") ;", &[]),
            (
                "declare(\nfoo=1, Bar=2);",
                &[
                    "Warning: Unsupported declare 'foo' @2",
                    "Warning: Unsupported declare 'Bar' @2",
                ],
            ),
            (
                "declare(ticks=1,\nfoo=-1);",
                &["Fatal error: declare(foo) value must be a literal @1"],
            ),
            (
                "declare(Ticks=\"a$b\");",
                &["Fatal error: declare(Ticks) value must be a literal @1"],
            ),
            (
                "; declare(encoding=\"a\");",
                &[
                    "Fatal error: Encoding declaration pragma must be the very first statement in the script @1",
                ],
            ),
            (
                "const A = 1; declare(strict_types=1);",
                &[
                    "Fatal error: strict_types declaration must be the very first statement in the script @1",
                ],
            ),
            (
                "namespace A { declare(strict_types=1); }",
                &[
                    "Fatal error: strict_types declaration must be the very first statement in the script @1",
                ],
            ),
            (
                "function f() { declare(strict_types=1); }",
                &[
                    "Fatal error: strict_types declaration must be the very first statement in the script @1",
                ],
            ),
            (
                "declare(ticks=1, strict_types=1) {}",
                &["Fatal error: strict_types declaration must not use block mode @1"],
            ),
            (
                "declare(Foo=1, strict_types=1.0);",
                &[
                    "Warning: Unsupported declare 'Foo' @1",
                    "Fatal error: strict_types declaration must have 0 or 1 as its value @1",
                ],
            ),
            (
                "declare(strict_types=2, foo=1);",
                &["Fatal error: strict_types declaration must have 0 or 1 as its value @1"],
            ),
            (
                "declare(foo=1) { $this = 1; }",
                &[
                    "Warning: Unsupported declare 'foo' @1",
                    "Fatal error: Cannot re-assign $this @1",
                ],
            ),
        ]);
    }
}
