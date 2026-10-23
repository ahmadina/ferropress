//! The parser oracle: how the reference PHP parses source text.
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
pub(crate) const PARSER_DIR: &str = "contracts/parser";
/// Path of the oracle script inside the lab container.
pub(crate) const SCRIPT: &str = "/opt/ferropress/parser-oracle.php";

const SCHEMA: u32 = 1;
const ERROR_CLASSES: [&str; 2] = ["ParseError", "CompileError"];
const RENDERING_START: &str = "assert(function () {\n";
const RENDERING_END: &str = "} && false)";

/// The error parsing a case raised: exception class, message and line (`None` when PHP reported the
/// line of the code that asked for the parse instead of a source line).
pub(crate) type RecordedError = (String, String, Option<u32>);

/// A warning parsing a case raised: severity (`Warning`, `Deprecated`, …) and message.
pub(crate) type RecordedWarning = (String, String);

/// One case: a source text and everything the reference PHP reported for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Case {
    pub(crate) name: String,
    pub(crate) source: String,
    pub(crate) error: Option<RecordedError>,
    pub(crate) warnings: Vec<RecordedWarning>,
    /// The engine's rendering of the source placed in a closure body, when it has one.
    pub(crate) rendering: Option<String>,
}

impl Case {
    fn strings(&self) -> impl Iterator<Item = &str> {
        let error = self
            .error
            .iter()
            .flat_map(|(class, message, _)| [class.as_str(), message.as_str()]);
        [self.name.as_str(), self.source.as_str()]
            .into_iter()
            .chain(error)
            .chain(
                self.warnings
                    .iter()
                    .flat_map(|(severity, message)| [severity.as_str(), message.as_str()]),
            )
            .chain(self.rendering.as_deref())
    }

    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !self
            .strings()
            .all(|text| text.chars().all(|c| u32::from(c) <= 0xFF))
        {
            problems.push("holds a non-byte character".to_owned());
        }
        if let Some((class, message, line)) = &self.error {
            if !ERROR_CLASSES.contains(&class.as_str()) {
                problems.push(format!("raises unknown class {class}"));
            }
            if message.is_empty() || *line == Some(0) {
                problems.push("has an empty error message or line 0".to_owned());
            }
            if self.rendering.is_some() {
                problems.push("has both an error and a rendering".to_owned());
            }
        }
        if let Some(rendering) = &self.rendering
            && !(rendering.starts_with(RENDERING_START) && rendering.ends_with(RENDERING_END))
        {
            problems.push("has a rendering that is not a closure in assert()".to_owned());
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
    pub(crate) cases: Vec<Case>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: u32,
    php_version: String,
    short_open_tag: bool,
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
        let errors = self
            .cases
            .iter()
            .filter(|case| case.error.is_some())
            .count();
        let renderings = self
            .cases
            .iter()
            .filter(|case| case.rendering.is_some())
            .count();
        format!(
            "{} cases, {errors} errors, {renderings} renderings",
            self.cases.len()
        )
    }

    fn render(&self) -> anyhow::Result<String> {
        let header = Header {
            schema: self.schema,
            php_version: self.php_version.clone(),
            short_open_tag: self.short_open_tag,
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
        serde_json::from_str(output).context("parsing the parser oracle's output")?;
    let problems = oracle.problems();
    anyhow::ensure!(
        problems.is_empty(),
        "the parser oracle output is invalid:\n  - {}",
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

    const OUTPUT: &str = r#"{"schema": 1, "php_version": "8.5.12", "short_open_tag": true, "cases": [
        {"name": "echo", "source": "<?php 'a';", "error": null, "warnings": [],
         "rendering": "assert(function () {\n    'a';\n} && false)"},
        {"name": "broken", "source": "<?php )", "error": ["ParseError", "Unmatched ')'", 1],
         "warnings": [["Warning", "w"]], "rendering": null}
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
                cases,
            })
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-parser-{}-{name}", std::process::id()));
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
        assert_eq!(oracle.summary(), "2 cases, 1 errors, 1 renderings");
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
            ["case `echo` repeats an earlier one"]
        );
        assert_eq!(
            problems_of(|o| o.cases[0].source.push('ā')),
            ["case `echo` holds a non-byte character"]
        );
        assert_eq!(
            problems_of(|o| o.cases[1].error = Some(("Error".into(), "x".into(), None))),
            ["case `broken` raises unknown class Error"]
        );
        assert_eq!(
            problems_of(|o| o.cases[1].error = Some(("ParseError".into(), String::new(), Some(0)))),
            ["case `broken` has an empty error message or line 0"]
        );
        assert_eq!(
            problems_of(|o| o.cases[1].rendering = o.cases[0].rendering.clone()),
            ["case `broken` has both an error and a rendering"]
        );
        assert_eq!(
            problems_of(|o| o.cases[0].rendering = Some("'a';".into())),
            ["case `echo` has a rendering that is not a closure in assert()"]
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
            Some(r#"{"schema":1,"php_version":"8.5.12","short_open_tag":true,"section":"cases"}"#)
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
            .join(PARSER_DIR);
        let oracle = Oracle::load(&dir).unwrap();
        assert_eq!(oracle.problems(), Vec::<String>::new());
    }
}
