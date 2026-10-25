//! Differential test: every script in `contracts/vm/cases.jsonl`, run by the reference PHP CLI and
//! recorded by `cargo xtask lab vm`, is compiled and run again here. The output must match the
//! reference's byte for byte, and so must the exit status.

use std::fmt::Write as _;
use std::path::Path;

use php_parser::{ParseOptions, parse};
use serde_json::Value;

/// The path the oracle puts in place of each case's file.
const FILE: &[u8] = b"%file%";

struct Case {
    name: String,
    source: Vec<u8>,
    output: Vec<u8>,
    status: u8,
}

/// The bytes of a capture string, which holds one character per byte.
fn bytes(text: &str) -> Result<Vec<u8>, String> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).map_err(|error| format!("{error} in {text:?}")))
        .collect()
}

fn parse_case(line: &str) -> Result<Case, String> {
    let value: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    let text = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("missing string `{name}` in {value}"))
    };
    let status = value
        .get("status")
        .and_then(Value::as_u64)
        .and_then(|status| u8::try_from(status).ok())
        .ok_or_else(|| format!("missing status in {value}"))?;
    Ok(Case {
        name: text("name")?.to_owned(),
        source: bytes(text("source")?)?,
        output: bytes(text("output")?)?,
        status,
    })
}

fn load() -> Result<Vec<Case>, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/vm/cases.jsonl");
    let contents =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut lines = contents.lines();
    let header = lines.next().ok_or("empty capture file")?;
    if !header.contains(r#""section":"cases""#) {
        return Err(format!("unexpected header {header}"));
    }
    lines
        .enumerate()
        .map(|(index, line)| {
            parse_case(line).map_err(|error| format!("line {}: {error}", index + 2))
        })
        .collect()
}

/// What running `source` printed and its exit status.
fn run(source: &[u8]) -> Result<(Vec<u8>, u8), String> {
    let program = parse(source, ParseOptions::default())
        .map_err(|error| error.to_string())?
        .result
        .map_err(|error| format!("{error:?}"))?;
    let compiled = php_compiler::compile(&program, FILE).map_err(|error| error.to_string())?;
    let unit = compiled
        .unit
        .ok_or_else(|| format!("compile error {:?}", compiled.diagnostics.fatal))?;
    let mut output = Vec::new();
    let status = php_vm::run(&unit, FILE, &mut output).map_err(|error| error.to_string())?;
    Ok((output, status))
}

#[test]
fn scripts_run_as_the_reference_runs_them() -> Result<(), String> {
    let cases = load()?;
    assert!(!cases.is_empty());
    let mut report = String::new();
    for case in &cases {
        match run(&case.source) {
            Ok((output, status)) if output == case.output && status == case.status => {}
            Ok((output, status)) => {
                let _ = writeln!(
                    report,
                    "{}: expected status {} output {:?}\n  got status {status} output {:?}",
                    case.name,
                    case.status,
                    String::from_utf8_lossy(&case.output),
                    String::from_utf8_lossy(&output)
                );
            }
            Err(error) => {
                let _ = writeln!(report, "{}: {error}", case.name);
            }
        }
    }
    assert!(report.is_empty(), "{report}");
    Ok(())
}
