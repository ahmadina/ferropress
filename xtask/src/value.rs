//! The value-model oracle: how the reference PHP stores values under assignments, references and unsets.
//!
//! The lab script runs every scenario in a fresh PHP process and prints all of them as one JSON
//! document; this module validates it and stores it as `scenarios.jsonl`, starting with a header line.
//! Byte strings hold one character per byte, with the byte's value as its code point.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// Directory, relative to the repository root, holding the oracle file.
pub(crate) const VALUE_DIR: &str = "contracts/value";
/// Path of the oracle script inside the lab container.
pub(crate) const SCRIPT: &str = "/opt/ferropress/value-oracle.php";

const SCHEMA: u32 = 1;
const OPERATIONS: [&str; 4] = ["assign", "bind", "unset", "dump"];
const EVENTS: [&str; 4] = ["dump", "error", "warning", "deprecated"];

/// One scenario: the operations run and the events they produced, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scenario {
    pub(crate) ops: Vec<Json>,
    pub(crate) events: Vec<Json>,
}

/// Everything the oracle script recorded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Oracle {
    pub(crate) schema: u32,
    pub(crate) php_version: String,
    pub(crate) scenarios: Vec<Scenario>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: u32,
    php_version: String,
    section: String,
}

fn tag(item: &Json) -> Option<&str> {
    item.as_array()?.first()?.as_str()
}

fn holds_bytes(json: &Json) -> bool {
    match json {
        Json::String(text) => text.chars().all(|c| u32::from(c) <= 0xFF),
        Json::Array(items) => items.iter().all(holds_bytes),
        Json::Object(fields) => fields.values().all(holds_bytes),
        Json::Null | Json::Bool(_) | Json::Number(_) => true,
    }
}

impl Oracle {
    /// Every reason the recorded scenarios cannot be used as an oracle.
    pub(crate) fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema != SCHEMA {
            problems.push(format!("schema {} (expected {SCHEMA})", self.schema));
        }
        if self.php_version.is_empty() {
            problems.push("no PHP version".to_owned());
        }
        if self.scenarios.is_empty() {
            problems.push("no scenarios".to_owned());
        }
        let mut seen = HashSet::new();
        for (index, scenario) in self.scenarios.iter().enumerate() {
            let number = index + 1;
            if !seen.insert(&scenario.ops) {
                problems.push(format!("scenario {number} repeats an earlier one"));
            }
            if scenario.events.is_empty() {
                problems.push(format!("scenario {number} recorded no events"));
            }
            if let Some(op) = scenario
                .ops
                .iter()
                .find(|op| !tag(op).is_some_and(|name| OPERATIONS.contains(&name)))
            {
                problems.push(format!("scenario {number} has an unknown operation {op}"));
            }
            if let Some(event) = scenario
                .events
                .iter()
                .find(|event| !tag(event).is_some_and(|name| EVENTS.contains(&name)))
            {
                problems.push(format!("scenario {number} has an unexpected event {event}"));
            }
            if !scenario.ops.iter().chain(&scenario.events).all(holds_bytes) {
                problems.push(format!("scenario {number} holds a non-byte character"));
            }
        }
        problems
    }

    /// Short description of what was recorded.
    pub(crate) fn summary(&self) -> String {
        let events: usize = self.scenarios.iter().map(|s| s.events.len()).sum();
        format!("{} scenarios, {events} events", self.scenarios.len())
    }

    fn render(&self) -> anyhow::Result<String> {
        let header = Header {
            schema: self.schema,
            php_version: self.php_version.clone(),
            section: "scenarios".to_owned(),
        };
        let mut out = serde_json::to_string(&header)?;
        out.push('\n');
        for scenario in &self.scenarios {
            let line = serde_json::to_string(scenario).context("serializing a scenario")?;
            writeln!(out, "{line}").context("formatting")?;
        }
        Ok(out)
    }
}

/// Parses and validates the oracle script's output.
pub(crate) fn parse(output: &str) -> anyhow::Result<Oracle> {
    let oracle: Oracle =
        serde_json::from_str(output).context("parsing the value oracle's output")?;
    let problems = oracle.problems();
    anyhow::ensure!(
        problems.is_empty(),
        "the value oracle output is invalid:\n  - {}",
        problems.join("\n  - ")
    );
    Ok(oracle)
}

/// Writes `scenarios.jsonl` into `dir`, replacing an earlier one.
pub(crate) fn write(oracle: &Oracle, dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("scenarios.jsonl");
    fs::write(&path, oracle.render()?).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: &str = r#"{"schema": 1, "php_version": "8.5.12", "scenarios": [
        {"ops": [["assign", "a", [0], ["int", 1]], ["dump", "a"]],
         "events": [["dump", "a", ["array", [[0, ["int", 1]]]]]]},
        {"ops": [["unset", "a", [0]], ["dump", "a"]],
         "events": [["warning", "Undefined variable $v_a"], ["dump", "a", null]]}
    ]}"#;

    impl Oracle {
        /// Reads the file written by [`write`] from `dir`.
        pub(crate) fn load(dir: &Path) -> anyhow::Result<Self> {
            let path = dir.join("scenarios.jsonl");
            let text =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let mut lines = text.lines();
            let header: Header = serde_json::from_str(lines.next().unwrap_or_default())
                .with_context(|| format!("parsing the header of {}", path.display()))?;
            anyhow::ensure!(
                header.section == "scenarios" && header.schema == SCHEMA,
                "{} declares section `{}` schema {}",
                path.display(),
                header.section,
                header.schema
            );
            let scenarios = lines
                .enumerate()
                .map(|(index, line)| {
                    serde_json::from_str(line)
                        .with_context(|| format!("scenarios.jsonl line {}", index + 2))
                })
                .collect::<anyhow::Result<_>>()?;
            Ok(Self {
                schema: SCHEMA,
                php_version: header.php_version,
                scenarios,
            })
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-value-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn problems_of(edit: impl FnOnce(&mut Oracle)) -> Vec<String> {
        let mut oracle = parse(OUTPUT).unwrap();
        edit(&mut oracle);
        oracle.problems()
    }

    #[test]
    fn parse_accepts_well_formed_output() {
        let oracle = parse(OUTPUT).unwrap();
        assert_eq!(oracle.scenarios.len(), 2);
        assert_eq!(oracle.summary(), "2 scenarios, 3 events");
    }

    #[test]
    fn parse_rejects_malformed_output() {
        assert!(parse("[]").is_err());
        assert!(parse(&OUTPUT.replace("\"ops\"", "\"extra\": 1, \"ops\"")).is_err());
        let error = parse(&OUTPUT.replace("\"schema\": 1", "\"schema\": 2")).unwrap_err();
        assert!(
            error.to_string().contains("schema 2 (expected 1)"),
            "{error}"
        );
    }

    #[test]
    fn problems_name_every_inconsistency() {
        assert!(problems_of(|_| {}).is_empty());
        assert_eq!(problems_of(|o| o.php_version.clear()), ["no PHP version"]);
        assert_eq!(problems_of(|o| o.scenarios.clear()), ["no scenarios"]);
        assert_eq!(
            problems_of(|o| o.scenarios.push(o.scenarios[0].clone())),
            ["scenario 3 repeats an earlier one"]
        );
        assert_eq!(
            problems_of(|o| o.scenarios[1].events.clear()),
            ["scenario 2 recorded no events"]
        );
        assert_eq!(
            problems_of(|o| o.scenarios[0].ops.push(serde_json::json!(["swap"]))),
            [r#"scenario 1 has an unknown operation ["swap"]"#]
        );
        assert_eq!(
            problems_of(|o| o.scenarios[1]
                .events
                .push(serde_json::json!(["level-8", "x"]))),
            [r#"scenario 2 has an unexpected event ["level-8","x"]"#]
        );
        assert_eq!(
            problems_of(|o| o.scenarios[1]
                .events
                .push(serde_json::json!({"x": ["warning", "ā"]}))),
            [
                r#"scenario 2 has an unexpected event {"x":["warning","ā"]}"#,
                "scenario 2 holds a non-byte character"
            ]
        );
    }

    #[test]
    fn written_file_loads_back_identically() {
        let dir = temp_dir("roundtrip");
        let oracle = parse(OUTPUT).unwrap();
        write(&oracle, &dir).unwrap();
        assert_eq!(Oracle::load(&dir).unwrap(), oracle);
        let text = fs::read_to_string(dir.join("scenarios.jsonl")).unwrap();
        assert_eq!(
            text.lines().next(),
            Some(r#"{"schema":1,"php_version":"8.5.12","section":"scenarios"}"#)
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_reports_unwritable_directories() {
        let dir = temp_dir("blocked");
        fs::write(&dir, b"file").unwrap();
        let error = write(&parse(OUTPUT).unwrap(), &dir).unwrap_err();
        assert!(error.to_string().starts_with("creating "), "{error}");
        fs::remove_file(&dir).unwrap();
    }

    #[test]
    fn committed_captures_are_valid() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(VALUE_DIR);
        let oracle = Oracle::load(&dir).unwrap();
        assert_eq!(oracle.problems(), Vec::<String>::new());
    }
}
