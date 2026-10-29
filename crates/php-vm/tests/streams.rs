//! `file_exists`, `file_get_contents` and `file_put_contents`.

use std::fs;
use std::path::PathBuf;

use php_compiler::compile;
use php_parser::{ParseOptions, parse};

fn run(source: &[u8], file: &[u8]) -> Result<(Vec<u8>, u8), String> {
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

fn temp_dir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!(
        "ferropress-stream-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

#[test]
fn file_put_get_and_exists_round_trip() -> Result<(), String> {
    let dir = temp_dir()?;
    let path = dir.join("x.txt");
    let quoted = path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        "<?php echo file_put_contents('{quoted}', 'hi'); echo file_get_contents('{quoted}'); echo file_exists('{quoted}') ? 'y' : 'n';"
    );
    let (output, status) = run(source.as_bytes(), b"/t.php")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"2hiy");
    Ok(())
}

#[test]
fn is_file_and_is_dir_distinguish_paths() -> Result<(), String> {
    let dir = temp_dir()?;
    let file = dir.join("x.txt");
    fs::write(&file, b"hi").map_err(|error| error.to_string())?;
    let quoted_file = file.to_string_lossy().replace('\\', "\\\\");
    let quoted_dir = dir.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        "<?php echo is_file('{quoted_file}') ? 'y' : 'n', is_dir('{quoted_file}') ? 'y' : 'n', is_file('{quoted_dir}') ? 'y' : 'n', is_dir('{quoted_dir}') ? 'y' : 'n', is_file('{quoted_file}.no') ? 'y' : 'n';"
    );
    let (output, status) = run(source.as_bytes(), b"/t.php")?;
    assert_eq!(status, 0);
    assert_eq!(output, b"ynnyn");
    Ok(())
}

#[test]
fn is_file_rejects_wrong_argument_count() -> Result<(), String> {
    let (output, status) = run(b"<?php is_file();", b"/t.php")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(text.contains("expects exactly 1 argument"), "{text}");
    let (output, status) = run(b"<?php is_file(\"a\", 1);", b"/t.php")?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 255);
    assert!(
        text.contains("expects exactly 1 argument, 2 given"),
        "{text}"
    );
    Ok(())
}

#[test]
fn missing_file_get_contents_warns_and_is_false() -> Result<(), String> {
    let (output, status) = run(
        b"<?php echo file_get_contents(\"/tmp/ferropress-missing-stream-xyz\") === false ? \"f\" : \"x\";",
        b"/t.php",
    )?;
    let text = String::from_utf8_lossy(&output);
    assert_eq!(status, 0);
    assert!(text.contains("Failed to open stream"), "{text}");
    assert!(text.contains('f'), "{text}");
    Ok(())
}
