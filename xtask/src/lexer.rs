//! The lexer oracle: how the reference PHP splits source text into tokens.
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
pub(crate) const LEXER_DIR: &str = "contracts/lexer";
/// Path of the oracle script inside the lab container.
pub(crate) const SCRIPT: &str = "/opt/ferropress/lexer-oracle.php";

const SCHEMA: u32 = 1;
const CONSTANT_STRING: &str = "T_CONSTANT_ENCAPSED_STRING";

/// One token as the tokenizer reported it: name (the character itself for one-character tokens),
/// length in bytes and line.
pub(crate) type RecordedToken = (String, usize, u32);

/// One case: a source text and everything the reference PHP reported for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Case {
    pub(crate) name: String,
    pub(crate) short_open_tag: bool,
    pub(crate) source: String,
    pub(crate) tokens: Vec<RecordedToken>,
    pub(crate) warnings: Vec<String>,
    pub(crate) parse_error: Option<String>,
    /// Value or error of each constant string literal, in token order.
    pub(crate) literals: Vec<(Option<String>, Option<String>)>,
}

impl Case {
    fn strings(&self) -> impl Iterator<Item = &str> {
        let tokens = self.tokens.iter().map(|(name, _, _)| name.as_str());
        let literals = self
            .literals
            .iter()
            .flat_map(|(value, error)| value.iter().chain(error).map(String::as_str));
        [self.name.as_str(), self.source.as_str()]
            .into_iter()
            .chain(tokens)
            .chain(self.warnings.iter().map(String::as_str))
            .chain(self.parse_error.as_deref())
            .chain(literals)
    }

    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !self
            .strings()
            .all(|text| text.chars().all(|c| u32::from(c) <= 0xFF))
        {
            problems.push("holds a non-byte character".to_owned());
        }
        let length: usize = self.tokens.iter().map(|(_, length, _)| length).sum();
        let source_length = self.source.chars().count();
        if length != source_length {
            problems.push(format!("tokens cover {length} bytes of {source_length}"));
        }
        let lines: Vec<u32> = self.tokens.iter().map(|(_, _, line)| *line).collect();
        if lines.first().is_some_and(|&line| line == 0)
            || lines.windows(2).any(|pair| pair[1] < pair[0])
        {
            problems.push("token lines decrease or start at 0".to_owned());
        }
        let strings = self
            .tokens
            .iter()
            .filter(|(name, _, _)| name == CONSTANT_STRING)
            .count();
        if strings != self.literals.len() {
            problems.push(format!(
                "{} literals for {strings} constant strings",
                self.literals.len()
            ));
        }
        if self
            .literals
            .iter()
            .any(|(value, error)| value.is_some() == error.is_some())
        {
            problems.push("a literal has both or neither of value and error".to_owned());
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
            let label = format!(
                "case `{}` (short_open_tag {})",
                case.name, case.short_open_tag
            );
            if !seen.insert((&case.name, case.short_open_tag)) {
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
        let tokens: usize = self.cases.iter().map(|case| case.tokens.len()).sum();
        let errors = self
            .cases
            .iter()
            .filter(|case| case.parse_error.is_some())
            .count();
        format!(
            "{} cases, {tokens} tokens, {errors} parse errors",
            self.cases.len()
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
    let oracle: Oracle =
        serde_json::from_str(output).context("parsing the lexer oracle's output")?;
    let problems = oracle.problems();
    anyhow::ensure!(
        problems.is_empty(),
        "the lexer oracle output is invalid:\n  - {}",
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
        {"name": "echo", "short_open_tag": true, "source": "<?php 'a';", "warnings": [],
         "tokens": [["T_OPEN_TAG", 6, 1], ["T_CONSTANT_ENCAPSED_STRING", 3, 1], [";", 1, 1]],
         "parse_error": null, "literals": [["a", null]]},
        {"name": "echo", "short_open_tag": false, "source": "<?php )\n", "warnings": [],
         "tokens": [["T_OPEN_TAG", 6, 1], [")", 1, 1], ["T_WHITESPACE", 1, 1]],
         "parse_error": "Unmatched ')'", "literals": []}
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
        let dir = std::env::temp_dir().join(format!("xtask-lexer-{}-{name}", std::process::id()));
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
        assert_eq!(oracle.summary(), "2 cases, 6 tokens, 1 parse errors");
    }

    #[test]
    fn parse_rejects_malformed_output() {
        assert!(parse("[]").is_err());
        assert!(parse(&OUTPUT.replace("\"warnings\"", "\"extra\": 1, \"warnings\"")).is_err());
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
            ["case `echo` (short_open_tag true) repeats an earlier one"]
        );
        assert_eq!(
            problems_of(|o| o.cases[1].source.push('ā')),
            [
                "case `echo` (short_open_tag false) holds a non-byte character",
                "case `echo` (short_open_tag false) tokens cover 8 bytes of 9"
            ]
        );
        assert_eq!(
            problems_of(|o| o.cases[1].tokens[2].2 = 0),
            ["case `echo` (short_open_tag false) token lines decrease or start at 0"]
        );
        assert_eq!(
            problems_of(|o| o.cases[0].literals.clear()),
            ["case `echo` (short_open_tag true) 0 literals for 1 constant strings"]
        );
        assert_eq!(
            problems_of(|o| o.cases[0].literals[0].1 = Some("x".to_owned())),
            ["case `echo` (short_open_tag true) a literal has both or neither of value and error"]
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
            .join(LEXER_DIR);
        let oracle = Oracle::load(&dir).unwrap();
        assert_eq!(oracle.problems(), Vec::<String>::new());
    }
}
