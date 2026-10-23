//! Differential test: every case in `contracts/lexer/cases.jsonl`, captured from the reference PHP by
//! `cargo xtask lab lexer`, is tokenized again here and must produce the same tokens, lines,
//! warnings, scanner errors and literal values.

use std::fmt::Write as _;
use std::path::Path;

use php_lexer::{
    Diagnostic, LexerOptions, Quote, Token, TokenKind, decode_double_quoted, decode_single_quoted,
    tokenize,
};
use serde_json::Value;

/// Cases whose difference from the reference is understood and accepted, by name.
const KNOWN_DIFFERENCES: &[&str] = &[];

/// Starts of the messages only the scanner raises.
const SCANNER_MESSAGES: &[&str] = &[
    "Unmatched '",
    "Unclosed '",
    "Unterminated comment starting line ",
    "Invalid numeric literal",
    "Invalid UTF-8 codepoint escape sequence",
    "Invalid indentation - tabs and spaces cannot be mixed",
    "Invalid body indentation level",
];

struct Case {
    name: String,
    short_open_tag: bool,
    source: Vec<u8>,
    tokens: Vec<(String, usize, u64)>,
    warnings: Vec<String>,
    parse_error: Option<String>,
    literals: Vec<Result<Vec<u8>, String>>,
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

fn list<'v>(value: &'v Value, name: &str) -> Result<&'v [Value], String> {
    field(value, name)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("`{name}` is not a list"))
}

fn number(value: &Value) -> Result<u64, String> {
    value
        .as_u64()
        .ok_or_else(|| format!("expected a number, got {value}"))
}

fn parse_case(line: &str) -> Result<Case, String> {
    let value: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    let tokens = list(&value, "tokens")?
        .iter()
        .map(|token| match token.as_array().map(Vec::as_slice) {
            Some([name, length, line]) => Ok((
                text(name)?.to_owned(),
                usize::try_from(number(length)?).map_err(|error| error.to_string())?,
                number(line)?,
            )),
            _ => Err(format!("malformed token {token}")),
        })
        .collect::<Result<_, String>>()?;
    let literals = list(&value, "literals")?
        .iter()
        .map(|literal| match literal.as_array().map(Vec::as_slice) {
            Some([Value::String(value), Value::Null]) => Ok(Ok(bytes(value)?)),
            Some([Value::Null, Value::String(error)]) => Ok(Err(error.clone())),
            _ => Err(format!("malformed literal {literal}")),
        })
        .collect::<Result<_, String>>()?;
    let warnings = list(&value, "warnings")?
        .iter()
        .map(|warning| text(warning).map(str::to_owned))
        .collect::<Result<_, String>>()?;
    let parse_error = match field(&value, "parse_error")? {
        Value::Null => None,
        error => Some(text(error)?.to_owned()),
    };
    Ok(Case {
        name: text(field(&value, "name")?)?.to_owned(),
        short_open_tag: field(&value, "short_open_tag")?
            .as_bool()
            .ok_or("`short_open_tag` is not a boolean")?,
        source: bytes(text(field(&value, "source")?)?)?,
        tokens,
        warnings,
        parse_error,
        literals,
    })
}

fn load() -> Result<Vec<Case>, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/lexer/cases.jsonl");
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

fn name_of(kind: TokenKind) -> String {
    match kind {
        TokenKind::Char(byte) => char::from(byte).to_string(),
        named => named.name().unwrap_or_default().to_owned(),
    }
}

fn decode(text: &[u8]) -> Result<Vec<u8>, String> {
    let unprefixed = match text {
        [b'b' | b'B', rest @ ..] => rest,
        _ => text,
    };
    let body = unprefixed
        .get(1..unprefixed.len().saturating_sub(1))
        .unwrap_or_default();
    if unprefixed.first() == Some(&b'\'') {
        Ok(decode_single_quoted(body))
    } else {
        decode_double_quoted(body, Quote::Double)
            .map(|decoded| decoded.bytes)
            .map_err(|failure| failure.error.to_string())
    }
}

/// Every way our reading of the case differs from the reference.
fn differences(case: &Case) -> Result<Vec<String>, String> {
    let options = LexerOptions {
        short_open_tag: case.short_open_tag,
        ..LexerOptions::default()
    };
    let result = tokenize(&case.source, options).map_err(|error| error.to_string())?;
    let mut found = Vec::new();
    let ours: Vec<(String, usize, u64)> = result
        .tokens
        .iter()
        .map(|token| {
            (
                name_of(token.kind),
                token.text(&case.source).len(),
                u64::from(token.line),
            )
        })
        .collect();
    if ours != case.tokens {
        let first = ours
            .iter()
            .zip(&case.tokens)
            .position(|(a, b)| a != b)
            .unwrap_or(ours.len().min(case.tokens.len()));
        found.push(format!(
            "tokens differ from #{first}: ours {:?}, reference {:?}",
            &ours[first.min(ours.len())..ours.len().min(first + 4)],
            &case.tokens[first.min(case.tokens.len())..case.tokens.len().min(first + 4)]
        ));
    }
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    for diagnostic in &result.diagnostics {
        match diagnostic {
            Diagnostic::Warning { warning, .. } => warnings.push(warning.to_string()),
            Diagnostic::Error { error, .. } => errors.push(error.to_string()),
        }
    }
    if warnings != case.warnings {
        found.push(format!(
            "warnings {warnings:?}, reference {:?}",
            case.warnings
        ));
    }
    match &case.parse_error {
        None if !errors.is_empty() => {
            found.push(format!("errors {errors:?} but the reference compiles"));
        }
        Some(message)
            if SCANNER_MESSAGES
                .iter()
                .any(|start| message.starts_with(start))
                && !case.source.windows(3).any(|window| window == b"<<<") =>
        {
            if errors.first() != Some(message) {
                found.push(format!(
                    "first error {:?}, reference {message:?}",
                    errors.first()
                ));
            }
        }
        _ => {}
    }
    let strings: Vec<&Token> = result
        .tokens
        .iter()
        .filter(|token| token.kind == TokenKind::ConstantString)
        .collect();
    if strings.len() == case.literals.len() {
        for (token, expected) in strings.iter().zip(&case.literals) {
            let decoded = decode(token.text(&case.source));
            if &decoded != expected {
                found.push(format!(
                    "literal {:?} decodes to {decoded:?}, reference {expected:?}",
                    String::from_utf8_lossy(token.text(&case.source))
                ));
            }
        }
    }
    Ok(found)
}

#[test]
fn lexer_matches_the_reference() {
    let cases = load().expect("the capture file loads");
    assert!(cases.len() > 4000, "only {} cases", cases.len());
    let error_checks = cases
        .iter()
        .filter(|case| {
            case.parse_error.as_deref().is_some_and(|message| {
                SCANNER_MESSAGES
                    .iter()
                    .any(|start| message.starts_with(start))
            }) && !case.source.windows(3).any(|window| window == b"<<<")
        })
        .count();
    let clean = cases
        .iter()
        .filter(|case| case.parse_error.is_none())
        .count();
    let warnings: usize = cases.iter().map(|case| case.warnings.len()).sum();
    let literals: usize = cases.iter().map(|case| case.literals.len()).sum();
    assert!(
        error_checks > 300 && clean > 500 && warnings > 50 && literals > 1000,
        "too few checks: {error_checks} scanner errors, {clean} clean cases, {warnings} warnings, {literals} literals"
    );
    let mut report = String::new();
    let mut failures = 0;
    let mut known = 0;
    for case in &cases {
        let found = differences(case).expect("the case tokenizes");
        if found.is_empty() {
            continue;
        }
        if KNOWN_DIFFERENCES.contains(&case.name.as_str()) {
            known += 1;
            continue;
        }
        failures += 1;
        if failures <= 25 {
            let _ = writeln!(
                report,
                "{} (short_open_tag {}) {:?}\n  {}",
                case.name,
                case.short_open_tag,
                String::from_utf8_lossy(&case.source),
                found.join("\n  ")
            );
        }
    }
    assert_eq!(
        failures,
        0,
        "{failures} of {} cases differ:\n{report}",
        cases.len()
    );
    assert_eq!(
        known,
        KNOWN_DIFFERENCES.len(),
        "a known difference no longer differs"
    );
}
