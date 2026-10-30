//! Closures, arrow functions and computed callees.

use php_compiler::compile;
use php_parser::{ParseOptions, parse};

fn run(source: &[u8]) -> Result<(Vec<u8>, u8), String> {
    let program = parse(source, ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    let compiled = compile(&program, b"/t.php").map_err(|error| error.to_string())?;
    let unit = compiled
        .unit
        .ok_or_else(|| format!("compile error {:?}", compiled.diagnostics.fatal))?;
    let mut output = Vec::new();
    let status = php_vm::run(&unit, b"/t.php", &mut output).map_err(|error| error.to_string())?;
    Ok((output, status))
}

#[test]
fn a_closure_is_called_with_its_arguments() -> Result<(), String> {
    let (output, status) = run(b"<?php $f = function($x) { return $x + 1; }; echo $f(2);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"3");
    Ok(())
}

#[test]
fn an_arrow_captures_by_value() -> Result<(), String> {
    let (output, status) = run(b"<?php $a = 10; $f = fn($x) => $x + $a; $a = 1; echo $f(2);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"12");
    Ok(())
}

#[test]
fn use_by_value_is_a_snapshot() -> Result<(), String> {
    let (output, status) =
        run(b"<?php $a = 10; $f = function($x) use ($a) { return $x + $a; }; $a = 1; echo $f(2);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"12");
    Ok(())
}

#[test]
fn use_by_reference_shares_the_variable() -> Result<(), String> {
    let (output, status) =
        run(b"<?php $a = 10; $f = function() use (&$a) { $a++; }; $f(); echo $a;")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"11");
    Ok(())
}

#[test]
fn a_string_names_a_function() -> Result<(), String> {
    let (output, status) =
        run(b"<?php function add($a, $b) { return $a + $b; } $n = \"ADD\"; echo $n(2, 3);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"5");
    Ok(())
}

#[test]
fn a_non_callable_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php $f = 1; $f();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Value of type int is not callable"), "{text}");
    Ok(())
}

#[test]
fn echoing_a_closure_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php $f = function() {}; echo $f;")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Object of class Closure could not be converted to string"),
        "{text}"
    );
    Ok(())
}

#[test]
fn an_undefined_use_warns_at_creation() -> Result<(), String> {
    let (output, status) =
        run(b"<?php $f = function() use ($missing) { return $missing; }; echo $f();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 0);
    assert!(text.contains("Undefined variable $missing"), "{text}");
    Ok(())
}

#[test]
fn a_nested_closure_returns_another() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $f = function() { return function($x) { return $x; }; }; $g = $f(); echo $g(7);",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"7");
    Ok(())
}

#[test]
fn a_closure_error_names_the_closure() -> Result<(), String> {
    let (output, status) = run(b"<?php $f = function() { echo 1 % 0; }; $f();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("{closure:/t.php:1}"), "{text}");
    Ok(())
}

#[test]
fn an_immediately_invoked_closure_runs() -> Result<(), String> {
    let (output, status) = run(b"<?php echo (function($x) { return $x; })(5);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"5");
    Ok(())
}

#[test]
fn a_discarded_closure_still_captures() -> Result<(), String> {
    let (output, status) = run(b"<?php function() use ($missing) {}; echo \"ok\";")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 0);
    assert!(text.contains("Undefined variable $missing"), "{text}");
    assert!(text.ends_with("ok"), "{text}");
    Ok(())
}

#[test]
fn use_by_reference_creates_an_undefined_variable() -> Result<(), String> {
    let (output, status) = run(b"<?php $f = function() use (&$a) { $a = 3; }; $f(); echo $a;")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"3");
    Ok(())
}

#[test]
fn a_leading_backslash_is_stripped_from_a_string_callee() -> Result<(), String> {
    let (output, status) =
        run(b"<?php function add($a, $b) { return $a + $b; } $n = \"\\\\add\"; echo $n(2, 3);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"5");
    Ok(())
}

#[test]
fn an_undefined_string_function_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php $n = \"missing\"; $n();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Call to undefined function missing()"),
        "{text}"
    );
    Ok(())
}

#[test]
fn plus_one_assignment_increments() -> Result<(), String> {
    let (output, status) = run(b"<?php $i = 0; $i = $i + 1; $i += 1; $i = 1 + $i; echo $i;")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"3");
    Ok(())
}

#[test]
fn a_zero_arg_named_call_returns() -> Result<(), String> {
    let (output, status) = run(b"<?php function g() { return 7; } echo g();")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"7");
    Ok(())
}

#[test]
fn a_named_call_keeps_the_written_name() -> Result<(), String> {
    let (output, status) = run(b"<?php Missing();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Call to undefined function Missing()"),
        "{text}"
    );
    Ok(())
}

#[test]
fn null_is_not_callable() -> Result<(), String> {
    let (output, status) = run(b"<?php $f();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Undefined variable $f"), "{text}");
    assert!(
        text.contains("Value of type null is not callable"),
        "{text}"
    );
    Ok(())
}
