//! The bytecode programs compile to, compared by disassembly.

use php_compiler::{CompileFailure, Failure, compile};
use php_parser::{ParseOptions, parse};

/// The disassembly of the unit `source` compiles to.
fn disassemble(source: &str) -> Result<String, String> {
    let program = parse(source.as_bytes(), ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    let compiled = compile(&program, b"/t.php").map_err(|error| error.to_string())?;
    compiled
        .unit
        .map(|unit| unit.to_string())
        .ok_or_else(|| format!("{:?}", compiled.diagnostics.fatal))
}

/// The failure compiling `source` stops with.
fn failure(source: &str) -> Result<Failure, String> {
    let program = parse(source.as_bytes(), ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    match compile(&program, b"/t.php") {
        Err(CompileFailure::Emit(failure)) => Ok(failure),
        other => Err(format!("{other:?}")),
    }
}

/// `lines` joined into a disassembly.
fn listing(lines: &[&str]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[test]
fn echo_and_arithmetic() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php\n$a = 1;\necho $a + 2 * $a, \"x\";\n")?,
        listing(&[
            "code {main} temporaries=2",
            "   0    2  Assign $a, 1",
            "   1    3  Mul T0 = 2, $a",
            "   2    3  Add T1 = $a, T0",
            "   3    3  Echo T1",
            "   4    3  Echo \"x\"",
            "   5    3  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn comparisons_swap_greater_and_unary_operators_multiply() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php echo $a > $b, $a >= 1, -$a, +$a, !$a, ~$a, $a xor $b;")?,
        listing(&[
            "code {main} temporaries=7",
            "   0    1  Less T0 = $b, $a",
            "   1    1  Echo T0",
            "   2    1  LessOrEqual T1 = 1, $a",
            "   3    1  Echo T1",
            "   4    1  Mul T2 = $a, -1",
            "   5    1  Echo T2",
            "   6    1  Mul T3 = $a, 1",
            "   7    1  Echo T3",
            "   8    1  BoolNot T4 = $a",
            "   9    1  Echo T4",
            "  10    1  BitNot T5 = $a",
            "  11    1  Echo T5",
            "  12    1  BoolXor T6 = $a, $b",
            "  13    1  Echo T6",
            "  14    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn logical_operators_short_circuit() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php echo $a && $b; echo $a || $b;")?,
        listing(&[
            "code {main} temporaries=2",
            "   0    1  ToBool T0 = $a",
            "   1    1  JumpIfFalse T0, @3",
            "   2    1  ToBool T0 = $b",
            "   3    1  Echo T0",
            "   4    1  ToBool T1 = $a",
            "   5    1  JumpIfTrue T1, @7",
            "   6    1  ToBool T1 = $b",
            "   7    1  Echo T1",
            "   8    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn ternaries_copy_the_chosen_value() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php echo $a ? 1 : 2; echo $a ?: 3;")?,
        listing(&[
            "code {main} temporaries=2",
            "   0    1  JumpIfFalse $a, @3",
            "   1    1  Copy T0 = 1",
            "   2    1  Jump @4",
            "   3    1  Copy T0 = 2",
            "   4    1  Echo T0",
            "   5    1  Copy T1 = $a",
            "   6    1  JumpIfTrue T1, @8",
            "   7    1  Copy T1 = 3",
            "   8    1  Echo T1",
            "   9    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn interpolation_joins_like_the_reference() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php echo \"$a-{$b}\"; echo \"x$a\"; echo \"$a\";")?,
        listing(&[
            "code {main} temporaries=3",
            "   0    1  ToString T0 = $a",
            "   1    1  Concat T0 = T0, \"-\"",
            "   2    1  Concat T0 = T0, $b",
            "   3    1  Echo T0",
            "   4    1  Concat T1 = \"x\", $a",
            "   5    1  Echo T1",
            "   6    1  ToString T2 = $a",
            "   7    1  Echo T2",
            "   8    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn assignments_increments_and_print() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php $a .= 'x'; echo $b = $a; $i++; echo $i--, ++$i; print 1; $a;")?,
        listing(&[
            "code {main} temporaries=4",
            "   0    1  Concat T0 = $a, \"x\"",
            "   1    1  Assign $a, T0",
            "   2    1  Assign T1 = $b, $a",
            "   3    1  Echo T1",
            "   4    1  PreIncrement $i",
            "   5    1  PostDecrement T2 = $i",
            "   6    1  Echo T2",
            "   7    1  PreIncrement T3 = $i",
            "   8    1  Echo T3",
            "   9    1  Echo 1",
            "  10    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn keyword_constants_are_literals() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php echo TRUE, \\false, null, 1.5;")?,
        listing(&[
            "code {main} temporaries=0",
            "   0    1  Echo true",
            "   1    1  Echo false",
            "   2    1  Echo null",
            "   3    1  Echo 1.5",
            "   4    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn if_elseif_else() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php if ($a) { echo 1; } elseif ($b) { echo 2; } else { echo 3; }")?,
        listing(&[
            "code {main} temporaries=0",
            "   0    1  JumpIfFalse $a, @3",
            "   1    1  Echo 1",
            "   2    1  Jump @7",
            "   3    1  JumpIfFalse $b, @6",
            "   4    1  Echo 2",
            "   5    1  Jump @7",
            "   6    1  Echo 3",
            "   7    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn loops_with_break_and_continue() -> Result<(), String> {
    assert_eq!(
        disassemble(
            "<?php while ($a) { if ($b) break; continue; }\ndo { echo 1; continue; } while ($a);\n\
             for ($i = 0; $i < 3; $i++) { if ($i) continue; break 1; }\nfor (;;) { break; }"
        )?,
        listing(&[
            "code {main} temporaries=1",
            "   0    1  JumpIfFalse $a, @6",
            "   1    1  JumpIfFalse $b, @4",
            "   2    1  Jump @6",
            "   3    1  Jump @4",
            "   4    1  Jump @0",
            "   5    1  Jump @0",
            "   6    2  Echo 1",
            "   7    2  Jump @8",
            "   8    2  JumpIfTrue $a, @6",
            "   9    3  Assign $i, 0",
            "  10    3  Less T0 = $i, 3",
            "  11    3  JumpIfFalse T0, @18",
            "  12    3  JumpIfFalse $i, @15",
            "  13    3  Jump @16",
            "  14    3  Jump @15",
            "  15    3  Jump @18",
            "  16    3  PreIncrement $i",
            "  17    3  Jump @10",
            "  18    4  Jump @20",
            "  19    4  Jump @18",
            "  20    4  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn breaks_leave_nested_loops() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php while (1) { while (2) { break 2; } }")?,
        listing(&[
            "code {main} temporaries=0",
            "   0    1  JumpIfFalse 1, @5",
            "   1    1  JumpIfFalse 2, @4",
            "   2    1  Jump @5",
            "   3    1  Jump @1",
            "   4    1  Jump @0",
            "   5    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn functions_and_calls() -> Result<(), String> {
    assert_eq!(
        disassemble(
            "<?php\nfunction Add($a, &$b, ...$c) {\n  return $a + $b;\n}\necho add(1, $x);\n\
             \\Add($x + 1);\nfunction g() {}\n"
        )?,
        listing(&[
            "code {main} temporaries=2",
            "   0    5  InitCall \"add\", \"add\"",
            "   1    5  Send 1",
            "   2    5  SendVar $x",
            "   3    5  Call T0 =",
            "   4    5  Echo T0",
            "   5    6  InitCall \"add\", \"Add\"",
            "   6    6  Add T1 = $x, 1",
            "   7    6  Send T1",
            "   8    6  Call",
            "   9    7  Return 1",
            "",
            "code Add($a, &$b, ...$c) temporaries=1",
            "   0    3  Add T0 = $a, $b",
            "   1    3  Return T0",
            "   2    3  Return",
            "",
            "code g temporaries=0",
            "   0    7  Return",
        ])
    );
    Ok(())
}

#[test]
fn fatal_programs_have_no_unit() {
    assert!(disassemble("<?php break;").is_err());
}

#[test]
fn constructs_not_compiled_yet_are_named() -> Result<(), String> {
    let cases: [(&str, &str); 33] = [
        ("<?php namespace A;", "namespaces"),
        ("<?php use A\\B;", "use imports"),
        ("<?php use A\\{B};", "use imports"),
        ("<?php const A = 1;", "constant declarations"),
        ("<?php function &f() {}", "functions returning by reference"),
        ("<?php function f(): int {}", "return types"),
        ("<?php #[A] function f() {}", "attributes"),
        ("<?php function f(int $a) {}", "parameter types"),
        ("<?php function f($a = 1) {}", "parameter defaults"),
        ("<?php function f(#[A] $a) {}", "attributes"),
        (
            "<?php function f(&...$a) {}",
            "by-reference variadic parameters",
        ),
        ("<?php function f() { yield; }", "generators"),
        (
            "<?php if (1) { function f() {} }",
            "conditional function declarations",
        ),
        (
            "<?php function f() { function g() {} }",
            "conditional function declarations",
        ),
        ("<?php foreach ($a as $b) {}", "foreach"),
        ("<?php switch ($a) {}", "switch"),
        ("<?php function f() { global $a; }", "global"),
        ("<?php function f() { static $a; }", "static variables"),
        ("<?php unset($a);", "unset"),
        ("<?php a: goto a;", "goto"),
        ("<?php try {} finally {}", "try"),
        ("<?php declare(ticks=1);", "declare"),
        ("<?php class A {}", "classes"),
        ("<?php echo A;", "constants"),
        ("<?php $a ??= 1;", "??="),
        ("<?php echo $a ?? 1;", "??"),
        ("<?php echo 1 |> f(...);", "|>"),
        ("<?php echo @$a;", "@"),
        ("<?php echo $this;", "$this"),
        ("<?php echo $_GET;", "superglobals"),
        ("<?php echo $$a;", "variable variables"),
        ("<?php $a[] = 1;", "assignments to this target"),
        ("<?php echo [];", "this expression"),
    ];
    for (source, construct) in cases {
        match failure(source)? {
            Failure::Unsupported {
                construct: named, ..
            } => {
                assert_eq!(named, construct, "{source}");
            }
            other @ Failure::TooLarge { .. } => return Err(format!("{source}: {other}")),
        }
    }
    let calls: [(&str, &str); 4] = [
        ("<?php $f();", "calls of computed callees"),
        ("<?php namespace\\f();", "namespace-relative names"),
        ("<?php f(...);", "first-class callables"),
        ("<?php f(a: 1);", "named and unpacked arguments"),
    ];
    for (source, construct) in calls {
        match failure(source) {
            Ok(Failure::Unsupported {
                construct: named,
                line,
            }) => {
                assert_eq!((named, line), (construct, 1), "{source}");
            }
            other => return Err(format!("{source}: {other:?}")),
        }
    }
    Ok(())
}

#[test]
fn failures_describe_themselves() {
    let unsupported = Failure::Unsupported {
        construct: "goto",
        line: 3,
    };
    assert_eq!(
        unsupported.to_string(),
        "compiling goto is not supported yet (line 3)"
    );
    assert_eq!(
        Failure::TooLarge { line: 2 }.to_string(),
        "the code at line 2 exceeds the bytecode's operand limit"
    );
    assert_eq!(
        CompileFailure::from(unsupported).to_string(),
        "compiling goto is not supported yet (line 3)"
    );
}
