//! The VM oracle: what short scripts print and their exit status when the reference PHP CLI runs them.
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
pub(crate) const VM_DIR: &str = "contracts/vm";
/// Path of the oracle script inside the lab container.
pub(crate) const SCRIPT: &str = "/opt/ferropress/vm-oracle.php";

const SCHEMA: u32 = 1;

/// One case: a script, what running it printed and its exit status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Case {
    pub(crate) name: String,
    pub(crate) source: String,
    pub(crate) output: String,
    pub(crate) status: u8,
}

/// Everything the oracle script recorded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Oracle {
    pub(crate) schema: u32,
    pub(crate) php_version: String,
    pub(crate) cases: Vec<Case>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: u32,
    php_version: String,
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
            if !seen.insert(&case.name) {
                problems.push(format!("case `{}` repeats an earlier one", case.name));
            }
            let bytes_only = [&case.name, &case.source, &case.output]
                .into_iter()
                .all(|text| text.chars().all(|c| u32::from(c) <= 0xFF));
            if !bytes_only {
                problems.push(format!("case `{}` holds a non-byte character", case.name));
            }
        }
        problems
    }

    /// Short description of what was recorded.
    pub(crate) fn summary(&self) -> String {
        let failed = self.cases.iter().filter(|case| case.status != 0).count();
        format!(
            "{} cases, {} ending with a non-zero status",
            self.cases.len(),
            failed
        )
    }

    fn render(&self) -> anyhow::Result<String> {
        let header = Header {
            schema: self.schema,
            php_version: self.php_version.clone(),
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
    let oracle: Oracle = serde_json::from_str(output).context("parsing the VM oracle's output")?;
    let problems = oracle.problems();
    anyhow::ensure!(
        problems.is_empty(),
        "the VM oracle output is invalid:\n  - {}",
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

    const OUTPUT: &str = r#"{"schema": 1, "php_version": "8.5.12", "cases": [
        {"name": "fine", "source": "<?php\necho 1;", "output": "1", "status": 0},
        {"name": "broken", "source": "<?php\necho 1 % 0;", "output": "\nFatal error: x", "status": 255}
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
                cases,
            })
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-vm-{}-{name}", std::process::id()));
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
        assert_eq!(oracle.summary(), "2 cases, 1 ending with a non-zero status");
    }

    #[test]
    fn parse_rejects_malformed_output() {
        assert!(parse("[]").is_err());
        assert!(parse(&OUTPUT.replace("\"status\": 0", "\"status\": 256")).is_err());
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
            problems_of(|o| o.cases[0].output.push('ā')),
            ["case `fine` holds a non-byte character"]
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
            Some(r#"{"schema":1,"php_version":"8.5.12","section":"cases"}"#)
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
            .join(VM_DIR);
        let oracle = Oracle::load(&dir).unwrap();
        assert_eq!(oracle.problems(), Vec::<String>::new());
    }
}
