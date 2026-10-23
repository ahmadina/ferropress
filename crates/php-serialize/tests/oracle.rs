//! Differential tests against the reference PHP outcomes recorded in `contracts/serialize/`.
//!
//! Regenerate the captures with `cargo xtask lab serialize`. Byte strings in the captures hold one
//! character per byte, with the byte's value as its code point.

use std::fs;
use std::path::PathBuf;

use php_serialize::{DecodeOptions, Document, Key, Severity, Value, Writer, decode_with};
use serde::Deserialize;

const PREFIX: &str = "unserialize(): ";
/// Emitted by the runtime for `C:` tokens whose class cannot be instantiated; not a format concern.
const NO_UNSERIALIZER: &str = "Class __PHP_Incomplete_Class has no unserializer";
/// Enum lookups the runtime performs after the format has been read.
const ENUM_LOOKUPS: [&str; 2] = [
    "unserialize(): Class '",
    "unserialize(): Undefined constant ",
];
const MAX_REPORTED: usize = 20;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Level {
    Warning,
    Deprecated,
}

#[derive(Debug, Deserialize)]
struct DecodeCase {
    input: String,
    #[serde(default)]
    max_depth: Option<usize>,
    refused: bool,
    messages: Vec<(Level, String)>,
    #[serde(default)]
    serialized: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FloatCase {
    bits: String,
    serialized: String,
}

fn captures(section: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/serialize")
        .join(format!("{section}.jsonl"));
    fs::read_to_string(&path).unwrap_or_default()
}

fn cases<T: for<'de> Deserialize<'de>>(section: &str) -> Vec<Result<T, String>> {
    captures(section)
        .lines()
        .skip(1)
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line)
                .map_err(|error| format!("{section} case {}: {error}", index + 1))
        })
        .collect()
}

fn bytes(text: &str) -> Option<Vec<u8>> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).ok())
        .collect()
}

fn text(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

fn level_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Deprecated => "deprecated",
        Severity::Warning => "warning",
    }
}

fn reference_messages(case: &DecodeCase) -> Vec<String> {
    case.messages
        .iter()
        .filter(|(_, message)| message != NO_UNSERIALIZER)
        .map(|(level, message)| {
            let name = match level {
                Level::Warning => "warning",
                Level::Deprecated => "deprecated",
            };
            format!("{name}: {message}")
        })
        .collect()
}

fn diagnostic_messages<'a>(
    diagnostics: impl IntoIterator<Item = &'a php_serialize::Diagnostic<'a>>,
) -> Vec<String> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let prefix = if diagnostic.has_function_prefix() {
                PREFIX
            } else {
                ""
            };
            format!(
                "{}: {prefix}{}",
                level_name(diagnostic.severity()),
                text(&diagnostic.message())
            )
        })
        .collect()
}

/// Whether the runtime, not the format, decides how this document is written back.
fn runtime_rewrites(value: &Value<'_, '_>) -> bool {
    match value {
        Value::Custom { .. } | Value::Reference(_) | Value::ObjectReference(_) => true,
        Value::Array(entries) => entries_rewritten(entries.clone(), false),
        Value::Object { properties, .. } => entries_rewritten(properties.clone(), true),
        _ => false,
    }
}

fn entries_rewritten<'d, 'a: 'd>(
    entries: impl Iterator<Item = (Key<'d>, Value<'d, 'a>)>,
    object: bool,
) -> bool {
    let mut seen = Vec::new();
    for (key, value) in entries {
        let normal = match key {
            Key::Int(number) => {
                if object {
                    return true;
                }
                number.to_string().into_bytes()
            }
            Key::String(name) => {
                let canonical = std::str::from_utf8(name)
                    .ok()
                    .and_then(|s| s.parse::<i64>().ok())
                    .is_some_and(|number| number.to_string().as_bytes() == name);
                if canonical && !object {
                    return true;
                }
                name.to_vec()
            }
        };
        if seen.contains(&normal) || runtime_rewrites(&value) {
            return true;
        }
        seen.push(normal);
    }
    false
}

/// Compares one decode case; returns a description of every difference.
fn compare(case: &DecodeCase) -> Option<String> {
    let Some(input) = bytes(&case.input) else {
        return Some("input holds a non-byte character".to_owned());
    };
    let options = case.max_depth.map_or_else(DecodeOptions::default, |depth| {
        DecodeOptions::default().with_max_depth(depth)
    });
    let outcome = decode_with(&input, options);
    let expected = reference_messages(case);
    let len = input.len();

    if input.is_empty() {
        return match outcome {
            Err(error) if error.offset() == 0 && expected.is_empty() => None,
            _ => Some("empty input must be refused silently".to_owned()),
        };
    }

    if let Some(lookup) = expected
        .iter()
        .position(|message| ENUM_LOOKUPS.iter().any(|start| message.contains(start)))
    {
        let refused_at = expected.last().and_then(|last| error_offset(last));
        let (offset, diagnostics) = match &outcome {
            Ok(document) => (None, diagnostic_messages(document.diagnostics())),
            Err(error) => (
                Some(error.offset()),
                diagnostic_messages(error.diagnostics()),
            ),
        };
        let prefix_matches = diagnostics.get(..lookup) == expected.get(..lookup);
        let beyond = match (offset, refused_at) {
            (None, _) => true,
            (Some(ours), Some(theirs)) => ours > theirs,
            (Some(_), None) => false,
        };
        return (!prefix_matches || !beyond).then(|| {
            format!("enum lookup refused at {refused_at:?}; we stopped at {offset:?} with {diagnostics:?}")
        });
    }

    let (actual, document) = match &outcome {
        Ok(document) => {
            let mut messages = diagnostic_messages(document.diagnostics());
            if document.consumed() < len {
                messages.push(format!(
                    "warning: {PREFIX}Extra data starting at offset {} of {len} bytes",
                    document.consumed()
                ));
            }
            (messages, Some(document))
        }
        Err(error) => {
            let mut messages = diagnostic_messages(error.diagnostics());
            messages.push(format!(
                "warning: {PREFIX}Error at offset {} of {len} bytes",
                error.offset()
            ));
            (messages, None)
        }
    };
    if actual != expected || case.refused != document.is_none() {
        return Some(format!("expected {expected:?}, got {actual:?}"));
    }
    let (Some(document), Some(serialized)) = (document, &case.serialized) else {
        return None;
    };
    compare_written(document, &input, serialized)
}

fn compare_written(document: &Document<'_>, input: &[u8], serialized: &str) -> Option<String> {
    let reference = bytes(serialized)?;
    let ours = document.to_bytes();
    let required = reference == input[..document.consumed()] || !runtime_rewrites(&document.root());
    (required && ours != reference).then(|| {
        format!(
            "written back as {:?}, reference wrote {serialized:?}",
            text(&ours)
        )
    })
}

fn error_offset(message: &str) -> Option<usize> {
    message
        .strip_prefix("warning: unserialize(): Error at offset ")?
        .split(' ')
        .next()?
        .parse()
        .ok()
}

#[test]
fn reader_matches_the_reference_on_every_captured_input() {
    let cases = cases::<DecodeCase>("decode");
    assert!(
        cases.len() > 1000,
        "too few captured decode cases: {}",
        cases.len()
    );
    let mismatches: Vec<String> = cases
        .iter()
        .filter_map(|case| match case {
            Ok(case) => compare(case).map(|problem| format!("{:?}: {problem}", case.input)),
            Err(error) => Some(error.clone()),
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} cases differ:\n{}",
        mismatches.len(),
        cases.len(),
        mismatches[..mismatches.len().min(MAX_REPORTED)].join("\n")
    );
}

#[test]
fn writer_matches_the_reference_on_every_captured_float() {
    let cases = cases::<FloatCase>("floats");
    assert!(
        cases.len() > 1000,
        "too few captured floats: {}",
        cases.len()
    );
    let mismatches: Vec<String> = cases
        .iter()
        .filter_map(|case| {
            let case = match case {
                Ok(case) => case,
                Err(error) => return Some(error.clone()),
            };
            let Ok(bits) = u64::from_str_radix(&case.bits, 16) else {
                return Some(format!("{}: bad bits", case.bits));
            };
            let mut writer = Writer::new();
            writer.float(f64::from_bits(bits));
            (writer.as_bytes() != case.serialized.as_bytes()).then(|| {
                format!(
                    "{}: wrote {:?}, reference wrote {:?}",
                    case.bits,
                    text(writer.as_bytes()),
                    case.serialized
                )
            })
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} floats differ:\n{}",
        mismatches.len(),
        cases.len(),
        mismatches[..mismatches.len().min(MAX_REPORTED)].join("\n")
    );
}
