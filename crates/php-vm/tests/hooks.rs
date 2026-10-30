//! Reentrant hook list and reflection natives (ADR-0003).

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
fn call_user_func_runs_a_named_function() -> Result<(), String> {
    let (output, status) =
        run(b"<?php function inc($x) { return $x + 1; } echo call_user_func(\"inc\", 3);")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"4");
    Ok(())
}

#[test]
fn apply_walks_registered_hooks() -> Result<(), String> {
    let (output, status) = run(
        b"<?php function a($v) { return $v . \"a\"; } function b($v) { return $v . \"b\"; } bridge_add(\"a\"); bridge_add(\"b\"); echo bridge_apply(\"x\");",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"xab");
    Ok(())
}

#[test]
fn remove_returns_whether_the_hook_was_present() -> Result<(), String> {
    let (output, status) = run(
        b"<?php function a($v) { return $v; } echo bridge_remove(\"a\") ? \"y\" : \"n\"; bridge_add(\"A\"); echo bridge_remove(\"a\") ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"ny");
    Ok(())
}

#[test]
fn apply_rejects_extra_arguments() -> Result<(), String> {
    let (output, status) = run(b"<?php bridge_apply(1, 2);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("expects at most 1 argument"), "{text}");
    Ok(())
}

#[test]
fn spl_object_id_rejects_a_non_object() -> Result<(), String> {
    let (output, status) = run(b"<?php spl_object_id(1);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("spl_object_id(): Argument #1 ($object) must be of type object"),
        "{text}"
    );
    Ok(())
}

#[test]
fn get_parent_class_accepts_a_class_name() -> Result<(), String> {
    let (output, status) = run(
        b"<?php class P {} class C extends P {} echo get_parent_class(\"C\"), get_parent_class(\"P\") === false ? \"n\" : \"y\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"Pn");
    Ok(())
}
