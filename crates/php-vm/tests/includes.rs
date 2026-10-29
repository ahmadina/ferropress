//! `include` and `require`.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

use php_compiler::compile;
use php_parser::{ParseOptions, parse};

fn compile_run(source: &[u8], file: &[u8]) -> Result<(Vec<u8>, u8), String> {
    let program = parse(source, ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    let compiled = compile(&program, file).map_err(|error| error.to_string())?;
    let unit = compiled
        .unit
        .ok_or_else(|| format!("compile error {:?}", compiled.diagnostics.fatal))?;
    let mut output = Vec::new();
    let status = php_vm::run(&unit, file, &mut output).map_err(|error| error.to_string())?;
    Ok((output, status))
}

fn temp_pair(main: &[u8], included: &[u8]) -> Result<(PathBuf, PathBuf), String> {
    let dir = std::env::temp_dir().join(format!(
        "ferropress-inc-{}-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let main_path = dir.join("main.php");
    let inc_path = dir.join("inc.php");
    fs::write(&main_path, main).map_err(|error| error.to_string())?;
    fs::write(&inc_path, included).map_err(|error| error.to_string())?;
    Ok((main_path, inc_path))
}

#[test]
fn include_runs_the_file_and_returns_its_value() -> Result<(), String> {
    let (main, _) = temp_pair(
        b"<?php echo include \"inc.php\";",
        b"<?php echo \"i\"; return 2;",
    )?;
    let source = fs::read(&main).map_err(|error| error.to_string())?;
    let (output, status) = compile_run(&source, main.to_string_lossy().as_bytes())?;
    assert_eq!(status, 0);
    assert_eq!(output, b"i2");
    Ok(())
}

#[test]
fn include_once_skips_a_second_load() -> Result<(), String> {
    let (main, _) = temp_pair(
        b"<?php include_once \"inc.php\"; include_once \"inc.php\";",
        b"<?php echo \"i\";",
    )?;
    let source = fs::read(&main).map_err(|error| error.to_string())?;
    let (output, status) = compile_run(&source, main.to_string_lossy().as_bytes())?;
    assert_eq!(status, 0);
    assert_eq!(output, b"i");
    Ok(())
}

#[test]
fn a_missing_include_warns_and_returns_false() -> Result<(), String> {
    let (output, status) =
        compile_run(b"<?php echo include \"missing-ferropress.php\";", b"/t.php")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 0);
    assert!(text.contains("Failed to open stream"), "{text}");
    Ok(())
}

#[test]
fn a_missing_require_throws() -> Result<(), String> {
    let (output, status) = compile_run(b"<?php require \"missing-ferropress.php\";", b"/t.php")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("Failed opening required"), "{text}");
    Ok(())
}
