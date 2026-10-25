//! By-reference argument errors the VM reports itself.

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
fn a_literal_cannot_be_passed_by_reference() -> Result<(), String> {
    let (output, status) = run(b"<?php function foo(&$var) {} foo(1); echo \"Done\\n\";")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("foo(): Argument #1 ($var) could not be passed by reference"),
        "{text}"
    );
    assert!(text.contains("#0 {main}"), "{text}");
    assert!(!text.contains("Done"), "{text}");
    Ok(())
}

#[test]
fn a_variable_can_be_passed_by_reference() -> Result<(), String> {
    let (output, status) = run(b"<?php function inc(&$x) { $x++; } $a = 1; inc($a); echo $a;")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"2");
    Ok(())
}

#[test]
fn a_call_result_is_passed_by_reference_with_a_notice() -> Result<(), String> {
    let (output, status) = run(
        b"<?php function foo(&$ref) { echo $ref; } function bar($value) { return $value; } foo(bar(5));",
    )?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 0);
    assert!(
        text.contains("Notice: Only variables should be passed by reference"),
        "{text}"
    );
    assert!(text.ends_with('5'), "{text}");
    Ok(())
}

#[test]
fn an_expression_cannot_be_passed_by_reference() -> Result<(), String> {
    let (output, status) = run(b"<?php function foo(&$r) { echo $r; } foo(1 + 1);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("could not be passed by reference"), "{text}");
    Ok(())
}
