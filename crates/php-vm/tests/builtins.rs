//! Built-in string, type and count functions.

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
fn strlen_and_case_map_strings() -> Result<(), String> {
    let (output, status) =
        run(b"<?php echo strlen(\"AbC\"), strtolower(\"AbC\"), strtoupper(\"AbC\");")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"3abcABC");
    Ok(())
}

#[test]
fn count_rejects_a_non_array() -> Result<(), String> {
    let (output, status) = run(b"<?php echo count(1);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("must be of type Countable|array, int given"),
        "{text}"
    );
    Ok(())
}

#[test]
fn gettype_and_is_tests_name_the_value() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo gettype(1), gettype(1.5), gettype(true), gettype(null), gettype(\"a\"); echo is_int(1) ? \"y\" : \"n\", is_string(1) ? \"y\" : \"n\", is_numeric(\"12\") ? \"y\" : \"n\", is_numeric(\"12x\") ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"integerdoublebooleanNULLstringynyn");
    Ok(())
}

#[test]
fn function_exists_finds_user_and_builtin_names() -> Result<(), String> {
    let (output, status) = run(
        b"<?php function f() {} echo function_exists(\"f\") ? \"y\" : \"n\", function_exists(\"strlen\") ? \"y\" : \"n\", function_exists(\"missing\") ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"yyn");
    Ok(())
}

#[test]
fn class_exists_finds_declared_and_builtin_classes() -> Result<(), String> {
    let (output, status) = run(
        b"<?php class C {} echo class_exists(\"C\") ? \"y\" : \"n\", class_exists(\"Exception\") ? \"y\" : \"n\", class_exists(\"Missing\") ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"yyn");
    Ok(())
}

#[test]
fn is_helpers_cover_the_other_types() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo is_null(null) ? \"y\" : \"n\", is_bool(false) ? \"y\" : \"n\", is_float(1.5) ? \"y\" : \"n\", is_object(new Exception(\"x\")) ? \"y\" : \"n\", is_integer(2) ? \"y\" : \"n\", is_array(1) ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"yyyyyn");
    Ok(())
}

#[test]
fn a_computed_callee_finds_a_builtin() -> Result<(), String> {
    let (output, status) = run(b"<?php $n = \"strlen\"; echo $n(\"ab\");")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"2");
    Ok(())
}

#[test]
fn json_round_trips_scalars_and_lists() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo json_encode(1), json_encode(\"a\"), json_encode([1, 2]); echo implode(\",\", json_decode(\"[3,4]\"));",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"1\"a\"[1,2]3,4");
    Ok(())
}

#[test]
fn json_decode_of_invalid_input_is_null() -> Result<(), String> {
    let (output, status) = run(b"<?php echo json_decode(\"{\") === null ? \"n\" : \"x\";")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"n");
    Ok(())
}

#[test]
fn json_encode_maps_objects_assoc_and_scalars() -> Result<(), String> {
    let (output, status) = run(
        b"<?php class C { public $x = 1; } echo json_encode(new C), json_encode([\"a\" => 2]), json_encode(null), json_encode(true), json_encode(1.5);",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"{\"x\":1}{\"a\":2}nulltrue1.5");
    Ok(())
}

#[test]
fn json_decode_reads_scalars_and_objects() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo json_decode(\"true\") ? \"t\" : \"f\", json_decode(\"null\") === null ? \"n\" : \"x\", json_decode(\"1.5\"), count(json_decode(\"{\\\"a\\\":1}\"));",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"tn1.51");
    Ok(())
}

#[test]
fn json_encode_without_arguments_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php json_encode();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("json_encode() expects exactly 1 argument, 0 given"),
        "{text}"
    );
    Ok(())
}

#[test]
fn count_of_an_array_literal() -> Result<(), String> {
    let (output, status) = run(b"<?php echo count([1, 2, 3]);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"3");
    Ok(())
}

#[test]
fn implode_joins_and_explode_splits() -> Result<(), String> {
    let (output, status) = run(b"<?php echo implode(\"-\", explode(\",\", \"a,b,c\"));")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"a-b-c");
    Ok(())
}

#[test]
fn explode_rejects_an_empty_separator() -> Result<(), String> {
    let (output, status) = run(b"<?php explode(\"\", \"ab\");")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("must not be empty"), "{text}");
    Ok(())
}

#[test]
fn math_builtins_compute_scalars() -> Result<(), String> {
    let (output, status) =
        run(b"<?php echo abs(-3), min(3, 1, 2), max(3, 1, 2), intval(1.9), strval(2);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"31312");
    Ok(())
}

#[test]
fn intdiv_and_rounding() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo intdiv(7, 2), intval(floor(1.5)), intval(ceil(1.2)), intval(round(1.5));",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"3122");
    Ok(())
}

#[test]
fn intdiv_by_zero_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php intdiv(1, 0);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Division by zero"), "{text}");
    Ok(())
}

#[test]
fn min_without_arguments_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php min();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("expects at least 1 argument"), "{text}");
    Ok(())
}

#[test]
fn strlen_without_arguments_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php strlen();")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("strlen() expects exactly 1 argument, 0 given"),
        "{text}"
    );
    Ok(())
}
