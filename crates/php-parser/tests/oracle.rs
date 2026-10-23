//! Differential test: every case in `contracts/parser/cases.jsonl`, captured from the reference PHP
//! by `cargo xtask lab parser`, is parsed again here. Parse and compile errors must match in class,
//! message and line, warnings must match in order, and a valid program must print back (as the body
//! of a closure inside `assert()`) exactly as the reference prints it.

use std::fmt::Write as _;
use std::path::Path;

use php_lexer::{LexerOptions, TokenKind, tokenize};
use php_parser::ast::{Arg, Args, ExprKind, StmtKind, TopStatement};
use php_parser::{ParseOptions, export, parse};
use serde_json::Value;

struct Case {
    name: String,
    source: Vec<u8>,
    error: Option<(String, Vec<u8>, Option<u64>)>,
    warnings: Vec<(String, String)>,
    rendering: Option<Vec<u8>>,
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

fn field<'v>(value: &'v Value, name: &str) -> Result<&'v Value, String> {
    value
        .get(name)
        .ok_or_else(|| format!("missing `{name}` in {value}"))
}

fn parse_case(line: &str) -> Result<Case, String> {
    let value: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    let error = match field(&value, "error")? {
        Value::Null => None,
        Value::Array(parts) => match parts.as_slice() {
            [class, message, line] => Some((
                text(class)?.to_owned(),
                bytes(text(message)?)?,
                line.as_u64(),
            )),
            _ => return Err(format!("malformed error {parts:?}")),
        },
        other => return Err(format!("malformed error {other}")),
    };
    let warnings = field(&value, "warnings")?
        .as_array()
        .ok_or("`warnings` is not a list")?
        .iter()
        .map(|warning| match warning.as_array().map(Vec::as_slice) {
            Some([severity, message]) => {
                Ok((text(severity)?.to_owned(), text(message)?.to_owned()))
            }
            _ => Err(format!("malformed warning {warning}")),
        })
        .collect::<Result<_, String>>()?;
    let rendering = match field(&value, "rendering")? {
        Value::Null => None,
        rendering => Some(bytes(text(rendering)?)?),
    };
    Ok(Case {
        name: text(field(&value, "name")?)?.to_owned(),
        source: bytes(text(field(&value, "source")?)?)?,
        error,
        warnings,
        rendering,
    })
}

fn load() -> Result<Vec<Case>, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/parser/cases.jsonl");
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

/// The source the reference rendered: the case placed in a closure inside `assert()`.
fn wrapped(source: &[u8]) -> Vec<u8> {
    let tokens = tokenize(source, LexerOptions::default())
        .map(|result| result.tokens)
        .unwrap_or_default();
    let html_at_end = tokens
        .last()
        .is_none_or(|token| matches!(token.kind, TokenKind::InlineHtml | TokenKind::CloseTag));
    let mut out = b"<?php assert(function () {\n?>".to_vec();
    out.extend_from_slice(source);
    if html_at_end {
        out.extend_from_slice(b"<?php ");
    }
    out.extend_from_slice(b"\n} && false);");
    out
}

/// Our rendering of the wrapped source.
fn rendering(source: &[u8]) -> Result<Vec<u8>, String> {
    let parsed = parse(&wrapped(source), ParseOptions::default()).map_err(|e| e.to_string())?;
    let program = parsed.result.map_err(|error| format!("wrapped: {error}"))?;
    let Some(TopStatement::Statement(statement)) = program.statements.first() else {
        return Err("wrapped: no statement".to_owned());
    };
    let StmtKind::Expr(call) = &statement.kind else {
        return Err("wrapped: not an expression".to_owned());
    };
    let ExprKind::Call {
        args: Args::List(args),
        ..
    } = &call.kind
    else {
        return Err("wrapped: not a call".to_owned());
    };
    let Some(Arg::Positional(argument)) = args.first() else {
        return Err("wrapped: no argument".to_owned());
    };
    let mut out = b"assert(".to_vec();
    out.extend_from_slice(&export(argument));
    out.push(b')');
    Ok(out)
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Every way our parse of the case differs from the reference.
fn differences(case: &Case) -> Result<Vec<String>, String> {
    let parsed = parse(&case.source, ParseOptions::default()).map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    let warnings: Vec<(String, String)> = parsed
        .warnings
        .iter()
        .map(|warning| (warning.severity.name().to_owned(), warning.message.clone()))
        .collect();
    if warnings != case.warnings {
        found.push(format!(
            "warnings {warnings:?}, reference {:?}",
            case.warnings
        ));
    }
    match (&parsed.result, &case.error) {
        (Ok(_), None) => {}
        (Ok(_), Some((class, message, line))) => found.push(format!(
            "parsed, reference {class}: {} (line {line:?})",
            lossy(message)
        )),
        (Err(error), None) => found.push(format!("{error}, reference parses")),
        (Err(error), Some((class, message, line))) => {
            if error.class.name() != class
                || error.line.map(u64::from) != *line
                || error.message != *message
            {
                found.push(format!(
                    "{}: {} (line {:?}), reference {class}: {} (line {line:?})",
                    error.class.name(),
                    lossy(&error.message),
                    error.line,
                    lossy(message)
                ));
            }
        }
    }
    if let Some(expected) = &case.rendering {
        match rendering(&case.source) {
            Ok(ours) if ours == *expected => {}
            Ok(ours) => found.push(format!(
                "rendering\n    ours      {:?}\n    reference {:?}",
                lossy(&ours),
                lossy(expected)
            )),
            Err(error) => found.push(format!("rendering failed: {error}")),
        }
    }
    Ok(found)
}

#[test]
fn parser_matches_the_reference() {
    let cases = load().expect("the capture file loads");
    assert!(cases.len() > 7000, "only {} cases", cases.len());
    let errors = cases.iter().filter(|case| case.error.is_some()).count();
    let renderings = cases.iter().filter(|case| case.rendering.is_some()).count();
    let warnings: usize = cases.iter().map(|case| case.warnings.len()).sum();
    assert!(
        errors > 2500 && renderings > 4000 && warnings > 30,
        "too few checks: {errors} errors, {renderings} renderings, {warnings} warnings"
    );
    let mut report = String::new();
    let mut failures = 0;
    for case in &cases {
        let found = differences(case).expect("the case parses");
        if found.is_empty() {
            continue;
        }
        failures += 1;
        let _ = writeln!(
            report,
            "{} {:?}\n  {}",
            case.name,
            lossy(&case.source),
            found.join("\n  ")
        );
    }
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("parser-oracle-report.txt");
    let _ = std::fs::write(&path, &report);
    let shown: String = report.lines().take(60).collect::<Vec<_>>().join("\n");
    assert_eq!(
        failures,
        0,
        "{failures} of {} cases differ (full report: {}):\n{shown}",
        cases.len(),
        path.display()
    );
}
