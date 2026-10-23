//! Syntax facts the compiler reports on that the assert rendering does not show.

use php_parser::ParseOptions;
use php_parser::ast::{ClassMember, ExprKind, Program, Stmt, StmtKind, StringPart, TopStatement};

#[expect(clippy::panic, reason = "test helper failing the calling test")]
fn program(source: &str) -> Program {
    let Ok(parsed) = php_parser::parse(source.as_bytes(), ParseOptions::default()) else {
        panic!("source fits");
    };
    let Ok(program) = parsed.result else {
        panic!("source parses");
    };
    program
}

fn statements(program: &Program) -> Vec<&Stmt> {
    program
        .statements
        .iter()
        .filter_map(|statement| match statement {
            TopStatement::Statement(stmt) => Some(stmt),
            _ => None,
        })
        .collect()
}

#[test]
fn warnings_carry_the_line_they_are_reported_on() {
    let source = "<?php\n\"a\n\\400\";\n$a = <<<X\n  a\n  \\500 $b\n  X;\ndeclare(encoding='a');";
    let Ok(parsed) = php_parser::parse(source.as_bytes(), ParseOptions::default()) else {
        panic!("source fits");
    };
    let lines: Vec<u32> = parsed.warnings.iter().map(|warning| warning.line).collect();
    assert_eq!(lines, [3, 6, 8]);
}

#[test]
fn dollar_brace_interpolation_keeps_its_form() {
    let program = program("<?php echo \"$a {$b} ${c} ${d[0]} ${e . f}\";");
    let [stmt] = statements(&program)[..] else {
        panic!("one statement");
    };
    let StmtKind::Echo(expr) = &stmt.kind else {
        panic!("echo");
    };
    let ExprKind::Interpolated(parts) = &expr.kind else {
        panic!("interpolated string");
    };
    let forms: Vec<&str> = parts
        .iter()
        .filter_map(|part| match part {
            StringPart::Literal(_) => None,
            StringPart::Expr(_) => Some("plain"),
            StringPart::BracedName(_) => Some("name"),
            StringPart::BracedExpr(_) => Some("expr"),
        })
        .collect();
    assert_eq!(forms, ["plain", "plain", "name", "name", "expr"]);
    assert!(parts[0].expr().is_some());
    assert!(parts[1].expr().is_none());
    assert!(parts.iter().filter_map(StringPart::expr).count() == 5);
}

#[test]
fn switch_cases_record_their_line_and_terminator() {
    let program = program("<?php switch ($a) {\ncase 1;\ndefault:\n}");
    let [stmt] = statements(&program)[..] else {
        panic!("one statement");
    };
    let StmtKind::Switch { cases, .. } = &stmt.kind else {
        panic!("switch");
    };
    let facts: Vec<(bool, bool, u32)> = cases
        .iter()
        .map(|case| (case.condition.is_some(), case.semicolon, case.line))
        .collect();
    assert_eq!(facts, [(true, true, 2), (false, false, 3)]);
}

#[test]
fn parameters_take_the_line_of_their_type_or_variable() {
    let program = program("<?php function f(\n#[A]\nint\n$a,\n&\n$b) {}");
    let [stmt] = statements(&program)[..] else {
        panic!("one statement");
    };
    let StmtKind::Function(function) = &stmt.kind else {
        panic!("function");
    };
    let lines: Vec<u32> = function.params.iter().map(|param| param.line).collect();
    assert_eq!(lines, [3, 6]);
}

#[test]
fn properties_take_the_line_of_their_variable() {
    let program = program("<?php class C {\npublic int\n$a = 1,\n$b;\npublic $c { get => 1; }\n}");
    let [stmt] = statements(&program)[..] else {
        panic!("one statement");
    };
    let StmtKind::Class(class) = &stmt.kind else {
        panic!("class");
    };
    let lines: Vec<u32> = class
        .members
        .iter()
        .flat_map(|member| match member {
            ClassMember::Property(group) => group.properties.iter().map(|p| p.line).collect(),
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(lines, [3, 4, 5]);
}
