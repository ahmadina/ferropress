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
            "code {main} temporaries=0",
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
            "  10    3  JumpIfNotLess $i, 3, @16",
            "  11    3  JumpIfFalse $i, @14",
            "  12    3  Jump @15",
            "  13    3  Jump @14",
            "  14    3  Jump @16",
            "  15    3  IncJump $i, @10",
            "  16    4  Jump @18",
            "  17    4  Jump @16",
            "  18    4  Return 1",
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
            "   0    5  Invoke/2 T0 = \"add\", 1, $x",
            "   1    5  Echo T0",
            "   2    6  Add T1 = $x, 1",
            "   3    6  Invoke/1 \"Add\", T1",
            "   4    7  Return 1",
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
fn a_call_argument_is_marked_on_send() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php f(g());")?,
        listing(&[
            "code {main} temporaries=1",
            "   0    1  Invoke T0 = \"g\"",
            "   1    1  Invoke/5 \"f\", T0",
            "   2    1  Return 1",
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
    let cases: [(&str, &str); 40] = [
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
        ("<?php function f() { yield from 1; }", "yield from"),
        (
            "<?php if (1) { function f() {} }",
            "conditional function declarations",
        ),
        (
            "<?php function f() { function g() {} }",
            "conditional function declarations",
        ),
        ("<?php foreach ($a as &$b) {}", "foreach by reference"),
        ("<?php switch ($a) {}", "switch"),
        ("<?php function f() { global $a; }", "global"),
        ("<?php function f() { static $a; }", "static variables"),
        ("<?php unset($a);", "unset"),
        ("<?php a: goto a;", "goto"),
        ("<?php declare(ticks=1);", "declare"),
        (
            "<?php class A { private $a; }",
            "static, private, typed or hooked properties",
        ),
        (
            "<?php class A { use T { T::m as n; } }",
            "trait adaptations",
        ),
        ("<?php enum E: int { case A = 1; }", "backed enums"),
        ("<?php class A { public $a = 1 + 1; }", "property defaults"),
        ("<?php class A { const C = 1; }", "class constants"),
        ("<?php abstract class A {}", "abstract and readonly classes"),
        (
            "<?php class A extends namespace\\B {}",
            "namespace-relative names",
        ),
        ("<?php $o = new class {};", "anonymous classes"),
        ("<?php echo A;", "constants"),
        ("<?php $a ??= 1;", "??="),
        ("<?php echo $a ?? 1;", "??"),
        ("<?php echo 1 |> f(...);", "|>"),
        ("<?php echo @$a;", "@"),
        ("<?php echo $this;", "$this"),
        ("<?php echo $_GET;", "superglobals"),
        ("<?php echo $$a;", "variable variables"),
        ("<?php $a[0][1] = 1;", "assignments to this target"),
        ("<?php echo isset($a[0][1]);", "isset of this target"),
        ("<?php eval(\"1\");", "eval"),
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
    let calls: [(&str, &str); 3] = [
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
fn closures_and_arrows_are_code_objects_with_captures() -> Result<(), String> {
    assert_eq!(
        disassemble(
            "<?php\n$a = 1;\n$f = function($x) use ($a, &$b) { return $x + $a; };\necho $f(2);\n\
             $g = fn($y) => $y + $a;\necho $g(3);\n"
        )?,
        listing(&[
            "code {main} temporaries=4",
            "   0    2  Assign $a, 1",
            "   1    3  MakeClosure T0 = 0",
            "   2    3  Capture T0, $a, 1",
            "   3    3  Capture/1 T0, $b, 2",
            "   4    3  Assign $f, T0",
            "   5    4  InitCallValue $f",
            "   6    4  Send 2",
            "   7    4  Call T1 =",
            "   8    4  Echo T1",
            "   9    5  MakeClosure T2 = 1",
            "  10    5  Capture T2, $a, 1",
            "  11    5  Assign $g, T2",
            "  12    6  InitCallValue $g",
            "  13    6  Send 3",
            "  14    6  Call T3 =",
            "  15    6  Echo T3",
            "  16    6  Return 1",
            "",
            "code {closure:/t.php:3}($x) temporaries=1",
            "   0    3  Add T0 = $x, $a",
            "   1    3  Return T0",
            "   2    3  Return",
            "",
            "code {closure:/t.php:5}($y) temporaries=1",
            "   0    5  Add T0 = $y, $a",
            "   1    5  Return T0",
        ])
    );
    Ok(())
}

#[test]
fn classes_emit_new_properties_and_methods() -> Result<(), String> {
    assert_eq!(
        disassemble(
            "<?php\nclass C {\n  public $a = 1;\n  function m($x) { return $x + $this->a; }\n}\n\
             $o = new C;\necho $o->m(2);\n$o->a = 3;\necho $o->a;\n"
        )?,
        listing(&[
            "code {main} temporaries=3",
            "   0    6  New T0 = \"C\"",
            "   1    6  Assign $o, T0",
            "   2    7  InitMethodCall $o, \"m\", \"m\"",
            "   3    7  Send 2",
            "   4    7  Call T1 =",
            "   5    7  Echo T1",
            "   6    8  StoreProp $o, \"a\", 3",
            "   7    9  LoadProp T2 = $o, \"a\"",
            "   8    9  Echo T2",
            "   9    9  Return 1",
            "",
            "code C->m($x) temporaries=2",
            "   0    4  LoadProp T0 = $this, \"a\"",
            "   1    4  Add T1 = $x, T0",
            "   2    4  Return T1",
            "   3    4  Return",
        ])
    );
    Ok(())
}

#[test]
fn a_child_class_emits_parent_calls() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php\nclass C extends P {\n  function m() { return parent::m(); }\n}\n")?,
        listing(&[
            "code {main} temporaries=0",
            "   0    3  Return 1",
            "",
            "code C->m temporaries=1",
            "   0    3  InitStaticCall \"parent\", \"m\", \"m\"",
            "   1    3  Call T0 =",
            "   2    3  Return T0",
            "   3    3  Return",
        ])
    );
    Ok(())
}

#[test]
fn a_generator_yields_and_marks_the_code() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php function g() { yield 1; yield $k => 2; }")?,
        listing(&[
            "code {main} temporaries=0",
            "   0    1  Return 1",
            "",
            "code g generator temporaries=0",
            "   0    1  Yield 1",
            "   1    1  Yield 2, $k",
            "   2    1  Return",
        ])
    );
    Ok(())
}

#[test]
fn static_calls_emit_init_static_call() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php echo Fiber::suspend(1);")?,
        listing(&[
            "code {main} temporaries=1",
            "   0    1  InitStaticCall \"Fiber\", \"suspend\", \"suspend\"",
            "   1    1  Send 1",
            "   2    1  Call T0 =",
            "   3    1  Echo T0",
            "   4    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn foreach_emits_ready_pick_and_step() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php foreach ($a as $k => $v) { echo $v; }")?,
        listing(&[
            "code {main} temporaries=2",
            "   0    1  Copy T0 = 0",
            "   1    1  IterReady T1 = $a, T0",
            "   2    1  JumpIfFalse T1, @8",
            "   3    1  IterPick/1 $k = $a, T0",
            "   4    1  IterPick $v = $a, T0",
            "   5    1  Echo $v",
            "   6    1  Add T0 = T0, 1",
            "   7    1  Jump @1",
            "   8    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn isset_and_empty_emit_quiet_checks() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php echo isset($a); echo isset($a[0]); echo empty($a); echo empty(0);")?,
        listing(&[
            "code {main} temporaries=4",
            "   0    1  Isset T0 = $a",
            "   1    1  Echo T0",
            "   2    1  IssetDim T1 = $a, 0",
            "   3    1  Echo T1",
            "   4    1  Isset/1 T2 = $a",
            "   5    1  Echo T2",
            "   6    1  Isset/1 T3 = 0",
            "   7    1  Echo T3",
            "   8    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn array_offsets_emit_fetch_and_store() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php $a = [1]; echo $a[0]; $a[1] = 2; $a[] = 3;")?,
        listing(&[
            "code {main} temporaries=2",
            "   0    1  NewArray T0 =",
            "   1    1  ArrayItem T0, 1",
            "   2    1  Assign $a, T0",
            "   3    1  FetchDim T1 = $a, 0",
            "   4    1  Echo T1",
            "   5    1  StoreDim $a, 1, 2",
            "   6    1  StoreDim $a, 3",
            "   7    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn array_literals_emit_new_array_and_items() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php echo [1, \"a\" => 2];")?,
        listing(&[
            "code {main} temporaries=1",
            "   0    1  NewArray T0 =",
            "   1    1  ArrayItem T0, 1",
            "   2    1  ArrayItem T0, 2, \"a\"",
            "   3    1  Echo T0",
            "   4    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn a_class_inside_a_closure_emits_bind_class() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php $f = function() { class C {} };")?,
        listing(&[
            "code {main} temporaries=1",
            "   0    1  MakeClosure T0 = 0",
            "   1    1  Assign $f, T0",
            "   2    1  Return 1",
            "",
            "code {closure:/t.php:1} temporaries=0",
            "   0    1  BindClass 0",
            "   1    1  Return",
        ])
    );
    Ok(())
}

#[test]
fn interfaces_traits_and_enums_emit() -> Result<(), String> {
    assert_eq!(
        disassemble(
            "<?php interface I { function m(); } class C implements I { function m() { echo 1; } } echo (new C) instanceof I;"
        )?,
        listing(&[
            "code {main} temporaries=2",
            "   0    1  New T0 = \"C\"",
            "   1    1  InstanceOf T1 = T0, \"I\"",
            "   2    1  Echo T1",
            "   3    1  Return 1",
            "",
            "code C->m temporaries=0",
            "   0    1  Echo 1",
            "   1    1  Return",
        ])
    );
    assert_eq!(
        disassemble("<?php trait T { function m() { echo 1; } } class C { use T; } (new C)->m();")?,
        listing(&[
            "code {main} temporaries=1",
            "   0    1  New T0 = \"C\"",
            "   1    1  InitMethodCall T0, \"m\", \"m\"",
            "   2    1  Call",
            "   3    1  Return 1",
            "",
            "code T->m temporaries=0",
            "   0    1  Echo 1",
            "   1    1  Return",
        ])
    );
    assert_eq!(
        disassemble("<?php enum E { case A; } echo E::A instanceof E;")?,
        listing(&[
            "code {main} temporaries=2",
            "   0    1  ClassConst T0 = \"E\", \"A\"",
            "   1    1  InstanceOf T1 = T0, \"E\"",
            "   2    1  Echo T1",
            "   3    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn include_and_require_emit_an_include_instruction() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php echo include \"a.php\"; require_once $p;")?,
        listing(&[
            "code {main} temporaries=1",
            "   0    1  Include T0 = \"a.php\"",
            "   1    1  Echo T0",
            "   2    1  Include/3 $p",
            "   3    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn try_finally_emits_end_finally() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php try { echo 1; } finally { echo 2; }")?,
        listing(&[
            "code {main} temporaries=0",
            "   0    1  Echo 1",
            "   1    1  Jump @2",
            "   2    1  Echo 2",
            "   3    1  EndFinally",
            "   4    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn try_catch_and_throw_use_a_handler_table() -> Result<(), String> {
    assert_eq!(
        disassemble(
            "<?php try { echo 1; throw new Exception(\"x\"); } catch (Exception $e) { echo \"c\"; }"
        )?,
        listing(&[
            "code {main} temporaries=1",
            "   0    1  Echo 1",
            "   1    1  New T0 = \"Exception\"",
            "   2    1  InitMethodCall/1 T0, \"__construct\", \"__construct\"",
            "   3    1  Send \"x\"",
            "   4    1  Call",
            "   5    1  Throw T0",
            "   6    1  Jump @9",
            "   7    1  Echo \"c\"",
            "   8    1  Jump @9",
            "   9    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn compare_while_and_add_assign_fuse() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php while ($i < 3) { $n = $n + $i - 1; }")?,
        listing(&[
            "code {main} temporaries=0",
            "   0    1  JumpIfNotLess $i, 3, @4",
            "   1    1  AddAssign $n, $i",
            "   2    1  SubAssign $n, 1",
            "   3    1  Jump @0",
            "   4    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn plus_one_assignment_is_an_increment() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php $i = $i + 1; $i += 1; $i = 1 + $i;")?,
        listing(&[
            "code {main} temporaries=0",
            "   0    1  PreIncrement $i",
            "   1    1  PreIncrement $i",
            "   2    1  PreIncrement $i",
            "   3    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn a_computed_callee_uses_the_value() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php $n = \"add\"; echo $n(1, $x);")?,
        listing(&[
            "code {main} temporaries=1",
            "   0    1  Assign $n, \"add\"",
            "   1    1  InitCallValue $n",
            "   2    1  Send 1",
            "   3    1  SendVar $x",
            "   4    1  Call T0 =",
            "   5    1  Echo T0",
            "   6    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn a_leaf_add_is_inlined() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php function add1($n) { return $n + 1; }\necho add1($x);\n")?,
        listing(&[
            "code {main} temporaries=1",
            "   0    2  Add T0 = $x, 1",
            "   1    2  Echo T0",
            "   2    2  Return 1",
            "",
            "code add1($n) temporaries=1",
            "   0    1  Add T0 = $n, 1",
            "   1    1  Return T0",
            "   2    1  Return",
        ])
    );
    Ok(())
}

#[test]
fn strlen_strpos_and_count_are_opcodes() -> Result<(), String> {
    assert_eq!(
        disassemble(
            "<?php echo strlen($s), strpos($s, \"m\"), str_contains($s, \"q\"), count($a);"
        )?,
        listing(&[
            "code {main} temporaries=4",
            "   0    1  Strlen T0 = $s",
            "   1    1  Echo T0",
            "   2    1  Strpos T1 = $s, \"m\"",
            "   3    1  Echo T1",
            "   4    1  StrContains T2 = $s, \"q\"",
            "   5    1  Echo T2",
            "   6    1  Count T3 = $a",
            "   7    1  Echo T3",
            "   8    1  Return 1",
        ])
    );
    Ok(())
}

#[test]
fn strlen_without_arguments_stays_an_invoke() -> Result<(), String> {
    assert_eq!(
        disassemble("<?php strlen();")?,
        listing(&[
            "code {main} temporaries=0",
            "   0    1  Invoke \"strlen\"",
            "   1    1  Return 1",
        ])
    );
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
