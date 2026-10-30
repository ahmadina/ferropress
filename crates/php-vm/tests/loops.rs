//! Counted loops, inlined leaves and dedicated string opcodes.

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
fn an_integer_count_loop_matches_the_sum() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $n = 0; $i = 0; while ($i < 10) { $n = $n + ($i * 3) - ($i % 2); $i = $i + 1; } echo $n;",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"130");
    Ok(())
}

#[test]
fn overflow_in_a_count_loop_becomes_float() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $n = 9223372036854775807; $i = 0; while ($i < 1) { $n = $n + 2; $i = $i + 1; } echo gettype($n);",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"double");
    Ok(())
}

#[test]
fn a_non_integer_counter_still_runs() -> Result<(), String> {
    let (output, status) =
        run(b"<?php $n = 0; $i = \"0\"; while ($i < 3) { $n = $n + 1; $i = $i + 1; } echo $n;")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"3");
    Ok(())
}

#[test]
fn modulus_zero_in_a_loop_throws() -> Result<(), String> {
    let (output, status) =
        run(b"<?php $i = 0; $n = 1; while ($i < 1) { $n = $n % 0; $i = $i + 1; }")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Modulo by zero"), "{text}");
    Ok(())
}

#[test]
fn an_inlined_leaf_adds_one() -> Result<(), String> {
    let (output, status) = run(
        b"<?php function add1($n) { return $n + 1; } $n = 0; $i = 0; while ($i < 5) { $n = add1($n); $i = $i + 1; } echo $n;",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"5");
    Ok(())
}

#[test]
fn a_by_ref_leaf_is_not_inlined() -> Result<(), String> {
    let (output, status) =
        run(b"<?php function bump(&$n) { $n = $n + 1; return $n; } $n = 2; echo bump($n), $n;")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"33");
    Ok(())
}

#[test]
fn strlen_strpos_and_contains_in_a_loop() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $s = \"abcdefghijklmnopqrstuvwxyz\"; $n = 0; $i = 0; while ($i < 3) { $n = $n + strlen($s) + strpos($s, \"m\"); $n = $n + (str_contains($s, \"q\") ? 1 : 0); $i = $i + 1; } echo $n;",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"117");
    Ok(())
}

#[test]
fn strlen_of_an_array_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php strlen([]);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("must be of type string"), "{text}");
    Ok(())
}

#[test]
fn appending_in_a_count_loop_builds_the_list() -> Result<(), String> {
    let (output, status) = run(
        b"<?php $a = []; $i = 0; while ($i < 4) { $a[] = $i; $i = $i + 1; } $total = 0; foreach ($a as $value) { $total = $total + $value; } echo count($a), \" \", $total;",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"4 6");
    Ok(())
}

#[test]
fn appending_to_a_scalar_throws() -> Result<(), String> {
    let (output, status) =
        run(b"<?php $a = 1; $i = 0; while ($i < 1) { $a[] = $i; $i = $i + 1; }")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("scalar value as an array"), "{text}");
    Ok(())
}

#[test]
fn an_empty_foreach_leaves_the_value() -> Result<(), String> {
    let (output, status) =
        run(b"<?php $value = 9; $total = 0; foreach ([] as $value) { $total += $value; } echo $value, $total;")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"90");
    Ok(())
}
