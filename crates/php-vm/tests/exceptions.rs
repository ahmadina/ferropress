//! `throw` and `try`/`catch`.

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
fn a_caught_exception_runs_the_handler() -> Result<(), String> {
    let (output, status) = run(
        b"<?php try { echo 1; throw new Exception(\"x\"); echo 2; } catch (Exception $e) { echo \"c\"; }",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"1c");
    Ok(())
}

#[test]
fn an_uncaught_exception_prints_its_message() -> Result<(), String> {
    let (output, status) = run(b"<?php throw new Exception(\"x\");")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Uncaught Exception: x"), "{text}");
    Ok(())
}

#[test]
fn a_vm_error_can_be_caught_as_error() -> Result<(), String> {
    let (output, status) = run(b"<?php try { echo 1 % 0; } catch (Error $e) { echo \"z\"; }")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"z");
    Ok(())
}

#[test]
fn throwing_a_non_object_fails() -> Result<(), String> {
    let (output, status) = run(b"<?php throw 1;")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Can only throw objects"), "{text}");
    Ok(())
}

#[test]
fn throwing_a_user_class_is_rejected() -> Result<(), String> {
    let (output, status) = run(b"<?php class E {} throw new E;")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("Cannot throw objects that do not implement Throwable"),
        "{text}"
    );
    Ok(())
}

#[test]
fn a_function_throw_unwinds_to_the_caller() -> Result<(), String> {
    let (output, status) = run(
        b"<?php function f() { throw new Exception(\"in\"); } try { f(); echo \"n\"; } catch (Exception $e) { echo \"c\"; }",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"c");
    Ok(())
}

#[test]
fn finally_runs_after_try() -> Result<(), String> {
    let (output, status) = run(b"<?php try { echo 1; } finally { echo 2; }")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"12");
    Ok(())
}

#[test]
fn finally_runs_after_a_caught_throw() -> Result<(), String> {
    let (output, status) = run(
        b"<?php try { throw new Exception(\"x\"); } catch (Exception $e) { echo \"c\"; } finally { echo \"f\"; }",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"cf");
    Ok(())
}

#[test]
fn finally_runs_before_an_uncaught_throw() -> Result<(), String> {
    let (output, status) =
        run(b"<?php try { throw new Exception(\"x\"); } finally { echo \"f\"; }")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.starts_with('f'), "{text}");
    assert!(text.contains("Uncaught Exception: x"), "{text}");
    Ok(())
}

#[test]
fn finally_runs_before_return() -> Result<(), String> {
    let (output, status) =
        run(b"<?php function f() { try { return 1; } finally { echo \"f\"; } } echo f();")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"f1");
    Ok(())
}

#[test]
fn the_first_matching_catch_wins() -> Result<(), String> {
    let (output, status) = run(
        b"<?php try { throw new Exception(\"x\"); } catch (Error $e) { echo \"e\"; } catch (Exception $e) { echo \"x\"; }",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"x");
    Ok(())
}
