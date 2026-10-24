//! Differential test: every case in `contracts/compiler/cases.jsonl`, captured from the reference
//! PHP by `cargo xtask lab compiler`, is parsed and checked again here. The diagnostics must match
//! the reference's in order, severity, message and line.
//!
//! Cases whose reference diagnostics come from checks not implemented yet are listed by name in
//! `tests/pending.txt`. Cases whose reference diagnostics come from linking classes while compiling
//! are listed separately in `tests/linking.txt`, until the class linker exists (ADR-0007). A listed
//! case that starts to match fails the test until it is removed from its list, so both lists only
//! shrink.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use php_parser::{ErrorClass, ParseOptions, parse};
use serde_json::Value;

type Diagnostic = (String, Vec<u8>, u64);

struct Case {
    name: String,
    source: Vec<u8>,
    diagnostics: Vec<Diagnostic>,
}

/// The bytes of a capture string, which holds one character per byte.
fn bytes(text: &str) -> Result<Vec<u8>, String> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).map_err(|error| format!("{error} in {text:?}")))
        .collect()
}

fn text(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("expected a string, got {value}"))
}

fn parse_case(line: &str) -> Result<Case, String> {
    let value: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    let field = |name: &str| {
        value
            .get(name)
            .ok_or_else(|| format!("missing `{name}` in {value}"))
    };
    let diagnostics = field("diagnostics")?
        .as_array()
        .ok_or("`diagnostics` is not a list")?
        .iter()
        .map(
            |diagnostic| match diagnostic.as_array().map(Vec::as_slice) {
                Some([severity, message, line]) => Ok((
                    text(severity)?.to_owned(),
                    bytes(text(message)?)?,
                    line.as_u64().ok_or("line is not a number")?,
                )),
                _ => Err(format!("malformed diagnostic {diagnostic}")),
            },
        )
        .collect::<Result<_, String>>()?;
    Ok(Case {
        name: text(field("name")?)?.to_owned(),
        source: bytes(text(field("source")?)?)?,
        diagnostics,
    })
}

fn load() -> Result<Vec<Case>, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/compiler/cases.jsonl");
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

/// What we report for `source`, in the reference's terms.
fn ours(source: &[u8]) -> Result<Vec<Diagnostic>, String> {
    let parsed = parse(source, ParseOptions::default()).map_err(|error| error.to_string())?;
    let mut diagnostics: Vec<Diagnostic> = parsed
        .warnings
        .iter()
        .map(|warning| {
            (
                warning.severity.name().to_owned(),
                warning.message.clone().into_bytes(),
                u64::from(warning.line),
            )
        })
        .collect();
    match parsed.result {
        Err(error) => {
            let severity = match error.class {
                ErrorClass::Parse => "Parse error",
                ErrorClass::Compile => "Fatal error",
            };
            let line = error.line.map_or(0, u64::from);
            diagnostics.push((severity.to_owned(), error.message, line));
        }
        Ok(program) => {
            let checked = php_compiler::check(&program, b"");
            diagnostics.extend(checked.warnings.into_iter().chain(checked.fatal).map(
                |diagnostic| {
                    (
                        diagnostic.severity.label().to_owned(),
                        diagnostic.message,
                        u64::from(diagnostic.line),
                    )
                },
            ));
        }
    }
    Ok(diagnostics)
}

fn render(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|(severity, message, line)| {
            format!("{severity}: {} @{line}", String::from_utf8_lossy(message))
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The case names listed in `tests/{file}`.
fn listed(file: &str) -> Result<BTreeSet<String>, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(file);
    let contents =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(contents
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

#[test]
fn compiler_matches_the_reference() -> Result<(), String> {
    let cases = load()?;
    let pending = listed("pending.txt")?;
    let linking = listed("linking.txt")?;
    let both: Vec<&String> = pending.intersection(&linking).collect();
    assert!(
        both.is_empty(),
        "listed as both pending and linking: {both:?}"
    );
    let mut unexpected = String::new();
    let mut resolved = Vec::new();
    let mut differing = Vec::new();
    for case in &cases {
        let expected: Vec<Diagnostic> = case
            .diagnostics
            .iter()
            .map(|(severity, message, line)| {
                let message = String::from_utf8_lossy(message).replace("%file%", "");
                (severity.clone(), message.into_bytes(), *line)
            })
            .collect();
        let got = ours(&case.source)?;
        let matches = got == expected;
        let is_linking = linking.contains(&case.name);
        if !matches && !is_linking {
            differing.push(case.name.clone());
        }
        match (matches, is_linking || pending.contains(&case.name)) {
            (false, false) => {
                let _ = writeln!(
                    unexpected,
                    "{}: {:?}\n  expected {}\n  got      {}",
                    case.name,
                    String::from_utf8_lossy(&case.source),
                    render(&expected),
                    render(&got),
                );
            }
            (true, true) => resolved.push(case.name.clone()),
            _ => {}
        }
    }
    if let Ok(path) = std::env::var("COMPILER_PENDING_OUT") {
        std::fs::write(&path, differing.join("\n") + "\n").map_err(|error| error.to_string())?;
    }
    assert!(
        unexpected.is_empty() && resolved.is_empty(),
        "{} of {} cases differ outside tests/pending.txt and tests/linking.txt:\n{unexpected}\nnow matching, remove from its list: {resolved:?}",
        unexpected
            .lines()
            .filter(|line| !line.starts_with(' '))
            .count(),
        cases.len(),
    );
    Ok(())
}
