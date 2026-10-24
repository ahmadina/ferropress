//! The operator oracle: what the reference PHP's operators and conversions return for a grid of
//! operands.
//!
//! The lab script prints a header line (the operands, and the tables of messages and errors the
//! results refer to by index) followed by one line per unary operation, and one per binary operator
//! and left operand. This module validates the output and stores it as `cases.jsonl`, with
//! `"section":"cases"` added to the header. Byte strings hold one character per byte, with the
//! byte's value as its code point.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Directory, relative to the repository root, holding the oracle file.
pub(crate) const OPERATORS_DIR: &str = "contracts/operators";
/// Path of the oracle script inside the lab container.
pub(crate) const SCRIPT: &str = "/opt/ferropress/operator-oracle.php";

const SCHEMA: u32 = 1;
const UNARY: [&str; 11] = [
    "neg", "plus", "bitnot", "not", "int", "float", "string", "bool", "array", "preinc", "predec",
];
const BINARY: [&str; 17] = [
    "add",
    "sub",
    "mul",
    "div",
    "mod",
    "pow",
    "shl",
    "shr",
    "bitor",
    "bitand",
    "bitxor",
    "concat",
    "eq",
    "identical",
    "lt",
    "le",
    "spaceship",
];
const SEVERITIES: [&str; 3] = ["Warning", "Notice", "Deprecated"];

/// What one operation did: its result (absent when it threw), the indexes of the messages it raised
/// and the index of the error it threw.
pub(crate) type Outcome = (Option<Value>, Vec<usize>, Option<usize>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: u32,
    php_version: String,
    operands: Vec<Value>,
    messages: Vec<(String, String)>,
    errors: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    section: Option<String>,
}

/// The results of one operation over the grid: a unary operation, or a binary operator with a fixed
/// left operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Line {
    pub(crate) op: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) left: Option<usize>,
    pub(crate) results: Vec<Outcome>,
}

/// Everything the oracle script recorded.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Oracle {
    header: Header,
    pub(crate) lines: Vec<Line>,
}

fn is_bytes(text: &str) -> bool {
    text.chars().all(|c| u32::from(c) <= 0xFF)
}

/// Every reason `value` is not a well-formed tagged value.
fn value_problems(value: &Value) -> Vec<String> {
    let fine = match value.as_array().map(Vec::as_slice) {
        Some([tag]) => tag == "null",
        Some([tag, Value::Bool(_)]) => tag == "bool",
        Some([tag, number]) if tag == "int" => number.is_i64(),
        Some([tag, Value::String(bits)]) if tag == "float" => {
            bits.len() == 16 && bits.bytes().all(|byte| byte.is_ascii_hexdigit())
        }
        Some([tag, Value::String(text)]) if tag == "str" => is_bytes(text),
        Some([tag, Value::Array(entries)]) if tag == "array" => {
            let mut problems = Vec::new();
            for entry in entries {
                match entry.as_array().map(Vec::as_slice) {
                    Some([key, item]) if key.is_i64() || key.as_str().is_some_and(is_bytes) => {
                        problems.extend(value_problems(item));
                    }
                    _ => problems.push(format!("malformed array entry {entry}")),
                }
            }
            return problems;
        }
        _ => false,
    };
    if fine {
        Vec::new()
    } else {
        vec![format!("malformed value {value}")]
    }
}

impl Oracle {
    /// Every reason the recorded results cannot be used as an oracle.
    pub(crate) fn problems(&self) -> Vec<String> {
        let header = &self.header;
        let mut problems = Vec::new();
        if header.schema != SCHEMA {
            problems.push(format!("schema {} (expected {SCHEMA})", header.schema));
        }
        if header.php_version.is_empty() {
            problems.push("no PHP version".to_owned());
        }
        if header.operands.is_empty() {
            problems.push("no operands".to_owned());
        }
        problems.extend(header.operands.iter().flat_map(value_problems));
        for (severity, message) in &header.messages {
            if !SEVERITIES.contains(&severity.as_str()) || message.is_empty() || !is_bytes(message)
            {
                problems.push(format!("malformed message {severity}: {message}"));
            }
        }
        for (class, message) in &header.errors {
            if class.is_empty() || !is_bytes(message) {
                problems.push(format!("malformed error {class}: {message}"));
            }
        }
        let mut seen = HashSet::new();
        for line in &self.lines {
            let label = match line.left {
                Some(left) => format!("`{}` with left operand {left}", line.op),
                None => format!("`{}`", line.op),
            };
            let known = match line.left {
                None => UNARY.contains(&line.op.as_str()),
                Some(left) => BINARY.contains(&line.op.as_str()) && left < header.operands.len(),
            };
            if !known {
                problems.push(format!("{label} is not an operation over the operands"));
            }
            if !seen.insert((&line.op, line.left)) {
                problems.push(format!("{label} repeats an earlier line"));
            }
            if line.results.len() != header.operands.len() {
                problems.push(format!("{label} has {} results", line.results.len()));
            }
            for (result, messages, error) in &line.results {
                if result.is_some() == error.is_some() {
                    problems.push(format!(
                        "{label} needs exactly one of a result and an error"
                    ));
                }
                problems.extend(result.iter().flat_map(value_problems));
                if messages.iter().any(|&index| index >= header.messages.len())
                    || error.is_some_and(|index| index >= header.errors.len())
                {
                    problems.push(format!("{label} refers to a missing message or error"));
                }
            }
        }
        for op in UNARY.iter().chain(&BINARY) {
            let lines = self.lines.iter().filter(|line| line.op == *op).count();
            let expected = if UNARY.contains(op) {
                1
            } else {
                header.operands.len()
            };
            if lines != expected {
                problems.push(format!("`{op}` has {lines} lines (expected {expected})"));
            }
        }
        problems
    }

    pub(crate) fn php_version(&self) -> &str {
        &self.header.php_version
    }

    /// Short description of what was recorded.
    pub(crate) fn summary(&self) -> String {
        let outcomes = || self.lines.iter().flat_map(|line| &line.results);
        format!(
            "{} operands, {} results, {} thrown errors, {} with messages",
            self.header.operands.len(),
            outcomes().count(),
            outcomes().filter(|(_, _, error)| error.is_some()).count(),
            outcomes()
                .filter(|(_, messages, _)| !messages.is_empty())
                .count()
        )
    }

    fn render(&self) -> anyhow::Result<String> {
        let header = Header {
            section: Some("cases".to_owned()),
            ..self.header.clone()
        };
        let mut out = serde_json::to_string(&header)?;
        out.push('\n');
        for line in &self.lines {
            let line = serde_json::to_string(line).context("serializing a line")?;
            writeln!(out, "{line}").context("formatting")?;
        }
        Ok(out)
    }
}

fn read(output: &str) -> anyhow::Result<Oracle> {
    let mut lines = output.lines();
    let header: Header = serde_json::from_str(lines.next().unwrap_or_default())
        .context("parsing the operator oracle's header")?;
    let lines = lines
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line)
                .with_context(|| format!("operator oracle line {}", index + 2))
        })
        .collect::<anyhow::Result<_>>()?;
    Ok(Oracle { header, lines })
}

/// Parses and validates the oracle script's output.
pub(crate) fn parse(output: &str) -> anyhow::Result<Oracle> {
    let oracle = read(output)?;
    anyhow::ensure!(
        oracle.header.section.is_none(),
        "the operator oracle's header already has a section"
    );
    let problems = oracle.problems();
    anyhow::ensure!(
        problems.is_empty(),
        "the operator oracle output is invalid:\n  - {}",
        problems.join("\n  - ")
    );
    Ok(oracle)
}

/// Writes `cases.jsonl` into `dir`, replacing an earlier one.
pub(crate) fn write(oracle: &Oracle, dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("cases.jsonl");
    fs::write(&path, oracle.render()?).with_context(|| format!("writing {}", path.display()))
}

/// Oracle output for a grid of two operands, with every operation's lines.
#[cfg(test)]
pub(crate) fn sample_output() -> String {
    let mut out = String::from(
        r#"{"schema":1,"php_version":"8.5.12","operands":[["int",1],["str","a"]],"messages":[["Warning","w"]],"errors":[["TypeError","t"]]}"#,
    );
    let results = r#"[[["int",1],[],null],[null,[0],0]]"#;
    for op in UNARY {
        write!(out, "\n{{\"op\":\"{op}\",\"results\":{results}}}").unwrap();
    }
    for op in BINARY {
        for left in 0..2 {
            write!(
                out,
                "\n{{\"op\":\"{op}\",\"left\":{left},\"results\":{results}}}"
            )
            .unwrap();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Oracle {
        /// Reads the file written by [`write`] from `dir`.
        pub(crate) fn load(dir: &Path) -> anyhow::Result<Self> {
            let path = dir.join("cases.jsonl");
            let text =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let mut oracle = read(&text)?;
            anyhow::ensure!(
                oracle.header.section.as_deref() == Some("cases"),
                "{} has no cases section",
                path.display()
            );
            oracle.header.section = None;
            Ok(oracle)
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("xtask-operators-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn problems_of(edit: impl FnOnce(&mut Oracle)) -> Vec<String> {
        let mut oracle = parse(&sample_output()).unwrap();
        edit(&mut oracle);
        oracle.problems()
    }

    #[test]
    fn parse_accepts_well_formed_sample_output() {
        let oracle = parse(&sample_output()).unwrap();
        assert_eq!(oracle.php_version(), "8.5.12");
        assert_eq!(
            oracle.summary(),
            "2 operands, 90 results, 45 thrown errors, 45 with messages"
        );
    }

    #[test]
    fn parse_rejects_malformed_sample_output() {
        assert!(parse("[]").is_err());
        assert!(parse(&format!("{}\n[]", sample_output())).is_err());
        assert!(
            parse(&sample_output().replacen("\"schema\":1", "\"section\":\"x\",\"schema\":1", 1))
                .is_err()
        );
        let error =
            parse(&sample_output().replacen("\"schema\":1", "\"schema\":2", 1)).unwrap_err();
        assert!(
            error.to_string().contains("schema 2 (expected 1)"),
            "{error}"
        );
    }

    #[test]
    fn problems_name_every_inconsistency() {
        assert!(problems_of(|_| {}).is_empty());
        assert_eq!(
            problems_of(|o| o.header.php_version.clear()),
            ["no PHP version"]
        );
        assert!(problems_of(|o| o.header.operands.clear()).contains(&"no operands".to_owned()));
        let malformed = [
            serde_json::json!(["int", "1"]),
            serde_json::json!(["float", "zz"]),
            serde_json::json!(["str", "ā"]),
            serde_json::json!(["array", [[1.5, ["null"]]]]),
            serde_json::json!(["bool"]),
        ];
        for value in malformed {
            let expected = value_problems(&value);
            assert_eq!(expected.len(), 1, "{value}");
            assert_eq!(problems_of(|o| o.header.operands[0] = value), expected);
        }
        assert!(
            value_problems(&serde_json::json!([
                "array",
                [[0, ["bool", true]], ["k", ["float", "3ff0000000000000"]]]
            ]))
            .is_empty()
        );
        assert_eq!(
            problems_of(|o| o.header.messages[0].0 = "Fatal".into()),
            ["malformed message Fatal: w"]
        );
        assert_eq!(
            problems_of(|o| o.header.errors[0].0.clear()),
            ["malformed error : t"]
        );
        assert_eq!(
            problems_of(|o| o.lines[0].op = "nope".into()),
            [
                "`nope` is not an operation over the operands",
                "`neg` has 0 lines (expected 1)"
            ]
        );
        assert_eq!(
            problems_of(|o| o.lines[0].left = Some(0)),
            ["`neg` with left operand 0 is not an operation over the operands",]
        );
        assert_eq!(
            problems_of(|o| o.lines.push(o.lines[0].clone())),
            [
                "`neg` repeats an earlier line",
                "`neg` has 2 lines (expected 1)"
            ]
        );
        assert_eq!(
            problems_of(|o| o.lines[0].results.truncate(1)),
            ["`neg` has 1 results"]
        );
        assert_eq!(
            problems_of(|o| o.lines[0].results[0].2 = Some(0)),
            ["`neg` needs exactly one of a result and an error"]
        );
        assert_eq!(
            problems_of(|o| o.lines[0].results[1].1 = vec![3]),
            ["`neg` refers to a missing message or error"]
        );
    }

    #[test]
    fn written_file_loads_back_identically() {
        let dir = temp_dir("roundtrip");
        let oracle = parse(&sample_output()).unwrap();
        write(&oracle, &dir).unwrap();
        assert_eq!(Oracle::load(&dir).unwrap(), oracle);
        let text = fs::read_to_string(dir.join("cases.jsonl")).unwrap();
        assert!(
            text.lines()
                .next()
                .unwrap()
                .ends_with(r#""section":"cases"}"#)
        );
        fs::write(dir.join("cases.jsonl"), sample_output()).unwrap();
        assert!(Oracle::load(&dir).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_reports_unwritable_directories() {
        let dir = temp_dir("blocked");
        fs::write(&dir, b"file").unwrap();
        let error = write(&parse(&sample_output()).unwrap(), &dir).unwrap_err();
        assert!(error.to_string().starts_with("creating "), "{error}");
        fs::remove_file(&dir).unwrap();
    }

    #[test]
    fn committed_captures_are_valid() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(OPERATORS_DIR);
        let oracle = Oracle::load(&dir).unwrap();
        assert_eq!(oracle.problems(), Vec::<String>::new());
    }
}
