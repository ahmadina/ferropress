//! Autoloading: `spl_autoload_register` and deferred class binding.

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
fn autoload_defines_a_class_for_new() -> Result<(), String> {
    let (output, status) = run(
        b"<?php spl_autoload_register(function($c) { if ($c === \"Foo\") { class Foo { function m() { echo 7; } } } }); $o = new Foo; $o->m();",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"7");
    Ok(())
}

#[test]
fn a_missing_class_is_still_missing_after_an_empty_loader() -> Result<(), String> {
    let (output, status) = run(b"<?php spl_autoload_register(function($c) {}); new Missing;")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Class \"Missing\" not found"), "{text}");
    Ok(())
}

#[test]
fn class_exists_runs_the_loader() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo class_exists(\"Foo\") ? \"y\" : \"n\"; spl_autoload_register(function($c) { if ($c === \"Foo\") { class Foo {} } }); echo class_exists(\"Foo\") ? \"y\" : \"n\"; echo class_exists(\"Foo\", false) ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"nyy");
    Ok(())
}

#[test]
fn instanceof_does_not_run_the_loader() -> Result<(), String> {
    let (output, status) = run(
        b"<?php class A {} $o = new A; spl_autoload_register(function($c) { if ($c === \"Foo\") { class Foo {} } }); echo $o instanceof Foo ? \"y\" : \"n\"; echo class_exists(\"Foo\", false) ? \"y\" : \"n\";",
    )?;
    assert_eq!(status, 0);
    assert_eq!(output, b"nn");
    Ok(())
}

#[test]
fn a_non_callable_register_throws() -> Result<(), String> {
    let (output, status) = run(b"<?php spl_autoload_register(1);")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("must be a valid callback"), "{text}");
    Ok(())
}

#[test]
fn include_registers_a_class_on_the_caller() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!(
        "ferropress-al-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let main = dir.join("main.php");
    let inc = dir.join("inc.php");
    std::fs::write(&inc, b"<?php class Foo { function m() { echo 4; } }")
        .map_err(|error| error.to_string())?;
    std::fs::write(&main, b"<?php include \"inc.php\"; $o = new Foo; $o->m();")
        .map_err(|error| error.to_string())?;
    let source = std::fs::read(&main).map_err(|error| error.to_string())?;
    let program = parse(&source, ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    let file = main.to_string_lossy();
    let compiled = compile(&program, file.as_bytes()).map_err(|error| error.to_string())?;
    let unit = compiled
        .unit
        .ok_or_else(|| format!("compile error {:?}", compiled.diagnostics.fatal))?;
    let mut output = Vec::new();
    let status =
        php_vm::run(&unit, file.as_bytes(), &mut output).map_err(|error| error.to_string())?;
    assert_eq!(status, 0);
    assert_eq!(output, b"4");
    Ok(())
}
