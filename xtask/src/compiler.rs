//! The compiler oracle: what the reference PHP reports while compiling source text without running it.
//!
//! The lab script prints every case as one JSON document; this module validates it and stores it as
//! `cases.jsonl`, starting with a header line. Byte strings hold one character per byte, with the
//! byte's value as its code point.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

/// Directory, relative to the repository root, holding the oracle file.
pub(crate) const COMPILER_DIR: &str = "contracts/compiler";
/// Path of the oracle script inside the lab container.
pub(crate) const SCRIPT: &str = "/opt/ferropress/compiler-oracle.php";

const SCHEMA: u32 = 1;
const SEVERITIES: [&str; 5] = [
    "Fatal error",
    "Parse error",
    "Warning",
    "Deprecated",
    "Notice",
];
/// Severities that end compilation; nothing may be reported after them.
const FATAL: [&str; 2] = ["Fatal error", "Parse error"];

/// One diagnostic compiling a case reported: severity, message and line.
pub(crate) type Diagnostic = (String, String, u32);

/// One case: a source text and every diagnostic the reference PHP reported compiling it, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Case {
    pub(crate) name: String,
    pub(crate) source: String,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

impl Case {
    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let strings = [self.name.as_str(), self.source.as_str()]
            .into_iter()
            .chain(
                self.diagnostics
                    .iter()
                    .map(|(_, message, _)| message.as_str()),
            );
        if !strings
            .into_iter()
            .all(|text| text.chars().all(|c| u32::from(c) <= 0xFF))
        {
            problems.push("holds a non-byte character".to_owned());
        }
        for (index, (severity, message, line)) in self.diagnostics.iter().enumerate() {
            if !SEVERITIES.contains(&severity.as_str()) {
                problems.push(format!("reports unknown severity {severity}"));
            }
            if message.is_empty() || *line == 0 {
                problems.push("has an empty message or line 0".to_owned());
            }
            if FATAL.contains(&severity.as_str()) && index + 1 != self.diagnostics.len() {
                problems.push("reports something after a fatal error".to_owned());
            }
        }
        problems
    }
}

/// Everything the oracle script recorded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Oracle {
    pub(crate) schema: u32,
    pub(crate) php_version: String,
    pub(crate) short_open_tag: bool,
    pub(crate) zend_assertions: i8,
    pub(crate) cases: Vec<Case>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: u32,
    php_version: String,
    short_open_tag: bool,
    zend_assertions: i8,
    section: String,
}

impl Oracle {
    /// Every reason the recorded cases cannot be used as an oracle.
    pub(crate) fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema != SCHEMA {
            problems.push(format!("schema {} (expected {SCHEMA})", self.schema));
        }
        if self.php_version.is_empty() {
            problems.push("no PHP version".to_owned());
        }
        if self.cases.is_empty() {
            problems.push("no cases".to_owned());
        }
        let mut seen = HashSet::new();
        for case in &self.cases {
            let label = format!("case `{}`", case.name);
            if !seen.insert(&case.name) {
                problems.push(format!("{label} repeats an earlier one"));
            }
            problems.extend(
                case.problems()
                    .into_iter()
                    .map(|problem| format!("{label} {problem}")),
            );
        }
        problems
    }

    /// Short description of what was recorded.
    pub(crate) fn summary(&self) -> String {
        let count = |severity: &str| {
            self.cases
                .iter()
                .flat_map(|case| &case.diagnostics)
                .filter(|(found, _, _)| found == severity)
                .count()
        };
        format!(
            "{} cases, {} fatal errors, {} warnings, {} deprecations",
            self.cases.len(),
            count("Fatal error"),
            count("Warning"),
            count("Deprecated")
        )
    }

    fn render(&self) -> anyhow::Result<String> {
        let header = Header {
            schema: self.schema,
            php_version: self.php_version.clone(),
            short_open_tag: self.short_open_tag,
            zend_assertions: self.zend_assertions,
            section: "cases".to_owned(),
        };
        let mut out = serde_json::to_string(&header)?;
        out.push('\n');
        for case in &self.cases {
            let line = serde_json::to_string(case).context("serializing a case")?;
            writeln!(out, "{line}").context("formatting")?;
        }
        Ok(out)
    }
}

/// Parses and validates the oracle script's output.
pub(crate) fn parse(output: &str) -> anyhow::Result<Oracle> {
    let oracle: Oracle =
        serde_json::from_str(output).context("parsing the compiler oracle's output")?;
    let problems = oracle.problems();
    anyhow::ensure!(
        problems.is_empty(),
        "the compiler oracle output is invalid:\n  - {}",
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

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: &str = r#"{"schema": 1, "php_version": "8.5.12", "short_open_tag": true,
        "zend_assertions": 1, "cases": [
        {"name": "fine", "source": "<?php 1;", "diagnostics": []},
        {"name": "broken", "source": "<?php $a[];", "diagnostics": [
            ["Deprecated", "d", 1], ["Warning", "w", 1], ["Fatal error", "Cannot use [] for reading", 1]]}
    ]}"#;

    impl Oracle {
        /// Reads the file written by [`write`] from `dir`.
        pub(crate) fn load(dir: &Path) -> anyhow::Result<Self> {
            let path = dir.join("cases.jsonl");
            let text =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let mut lines = text.lines();
            let header: Header = serde_json::from_str(lines.next().unwrap_or_default())
                .with_context(|| format!("parsing the header of {}", path.display()))?;
            anyhow::ensure!(
                header.section == "cases" && header.schema == SCHEMA,
                "{} declares section `{}` schema {}",
                path.display(),
                header.section,
                header.schema
            );
            let cases = lines
                .enumerate()
                .map(|(index, line)| {
                    serde_json::from_str(line)
                        .with_context(|| format!("cases.jsonl line {}", index + 2))
                })
                .collect::<anyhow::Result<_>>()?;
            Ok(Self {
                schema: SCHEMA,
                php_version: header.php_version,
                short_open_tag: header.short_open_tag,
                zend_assertions: header.zend_assertions,
                cases,
            })
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("xtask-compiler-{}-{name}", std::process::id()));
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
        assert_eq!(oracle.cases.len(), 2);
        assert_eq!(
            oracle.summary(),
            "2 cases, 1 fatal errors, 1 warnings, 1 deprecations"
        );
    }

    #[test]
    fn parse_rejects_malformed_output() {
        assert!(parse("[]").is_err());
        assert!(
            parse(&OUTPUT.replace("\"diagnostics\": []", "\"extra\": 1, \"diagnostics\": []"))
                .is_err()
        );
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
        assert_eq!(problems_of(|o| o.cases.clear()), ["no cases"]);
        assert_eq!(
            problems_of(|o| o.cases.push(o.cases[0].clone())),
            ["case `fine` repeats an earlier one"]
        );
        assert_eq!(
            problems_of(|o| o.cases[0].source.push('ā')),
            ["case `fine` holds a non-byte character"]
        );
        assert_eq!(
            problems_of(|o| o.cases[1].diagnostics[0].0 = "Error".into()),
            ["case `broken` reports unknown severity Error"]
        );
        assert_eq!(
            problems_of(|o| o.cases[1].diagnostics[1].2 = 0),
            ["case `broken` has an empty message or line 0"]
        );
        assert_eq!(
            problems_of(|o| o.cases[1].diagnostics.swap(1, 2)),
            ["case `broken` reports something after a fatal error"]
        );
    }

    #[test]
    fn written_file_loads_back_identically() {
        let dir = temp_dir("roundtrip");
        let oracle = parse(OUTPUT).unwrap();
        write(&oracle, &dir).unwrap();
        assert_eq!(Oracle::load(&dir).unwrap(), oracle);
        let text = fs::read_to_string(dir.join("cases.jsonl")).unwrap();
        assert_eq!(
            text.lines().next(),
            Some(
                r#"{"schema":1,"php_version":"8.5.12","short_open_tag":true,"zend_assertions":1,"section":"cases"}"#
            )
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
            .join(COMPILER_DIR);
        let oracle = Oracle::load(&dir).unwrap();
        assert_eq!(oracle.problems(), Vec::<String>::new());
    }
}
