//! The serialized-format oracle: how the reference PHP reads and writes serialized values.
//!
//! The lab script builds its own deterministic corpus and prints every outcome as one JSON document;
//! this module validates it and stores it as `decode.jsonl` and `floats.jsonl`, each starting with a
//! header line. Byte strings hold one character per byte, with the byte's value as its code point.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

/// Directory, relative to the repository root, holding the oracle files.
pub(crate) const SERIALIZE_DIR: &str = "contracts/serialize";
/// Path of the oracle script inside the lab container.
pub(crate) const SCRIPT: &str = "/opt/ferropress/serialize-oracle.php";

const SCHEMA: u32 = 1;

/// Kind of a message the reference emitted while reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Level {
    Warning,
    Deprecated,
}

/// One input as the reference read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecodeCase {
    pub(crate) input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) max_depth: Option<u64>,
    pub(crate) refused: bool,
    pub(crate) messages: Vec<(Level, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) serialized: Option<String>,
}

/// One float, by IEEE 754 bit pattern, as the reference writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FloatCase {
    pub(crate) bits: String,
    pub(crate) serialized: String,
}

/// Everything the oracle script recorded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Oracle {
    pub(crate) schema: u32,
    pub(crate) php_version: String,
    pub(crate) decode: Vec<DecodeCase>,
    pub(crate) floats: Vec<FloatCase>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: u32,
    php_version: String,
    section: String,
}

fn holds_bytes(text: &str) -> bool {
    text.chars().all(|c| u32::from(c) <= 0xFF)
}

impl Oracle {
    /// Every reason the recorded outcomes cannot be used as an oracle.
    pub(crate) fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema != SCHEMA {
            problems.push(format!("schema {} (expected {SCHEMA})", self.schema));
        }
        if self.php_version.is_empty() {
            problems.push("no PHP version".to_owned());
        }
        if self.decode.is_empty() || self.floats.is_empty() {
            problems.push("no decode or no float cases".to_owned());
        }
        let mut inputs = HashSet::new();
        for (index, case) in self.decode.iter().enumerate() {
            let line = index + 1;
            if !inputs.insert((&case.input, case.max_depth)) {
                problems.push(format!("decode case {line} repeats an earlier input"));
            }
            if case.refused == case.serialized.is_some() {
                problems.push(format!(
                    "decode case {line} must have a serialized value exactly when accepted"
                ));
            }
            let texts = std::iter::once(&case.input)
                .chain(case.serialized.as_ref())
                .chain(case.messages.iter().map(|(_, text)| text));
            if !texts.into_iter().all(|text| holds_bytes(text)) {
                problems.push(format!("decode case {line} holds a non-byte character"));
            }
        }
        let mut patterns = HashSet::new();
        for (index, case) in self.floats.iter().enumerate() {
            let line = index + 1;
            let well_formed = case.bits.len() == 16
                && case
                    .bits
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !well_formed {
                problems.push(format!(
                    "float case {line} bits must be 16 lowercase hex digits"
                ));
            } else if !patterns.insert(&case.bits) {
                problems.push(format!("float case {line} repeats bits {}", case.bits));
            }
            if !case.serialized.is_ascii() {
                problems.push(format!("float case {line} is not ASCII"));
            }
        }
        problems
    }

    /// Short description of what was recorded.
    pub(crate) fn summary(&self) -> String {
        let refused = self.decode.iter().filter(|case| case.refused).count();
        format!(
            "{} decode cases ({refused} refused), {} floats",
            self.decode.len(),
            self.floats.len()
        )
    }

    fn section<T: Serialize>(&self, name: &str, items: &[T]) -> anyhow::Result<String> {
        let header = Header {
            schema: self.schema,
            php_version: self.php_version.clone(),
            section: name.to_owned(),
        };
        let mut out = serde_json::to_string(&header)?;
        out.push('\n');
        for item in items {
            let line = serde_json::to_string(item)
                .with_context(|| format!("serializing a {name} case"))?;
            writeln!(out, "{line}").context("formatting")?;
        }
        Ok(out)
    }
}

/// Parses and validates the oracle script's output.
pub(crate) fn parse(output: &str) -> anyhow::Result<Oracle> {
    let oracle: Oracle =
        serde_json::from_str(output).context("parsing the serialize oracle's output")?;
    let problems = oracle.problems();
    anyhow::ensure!(
        problems.is_empty(),
        "the serialize oracle output is invalid:\n  - {}",
        problems.join("\n  - ")
    );
    Ok(oracle)
}

/// Writes `decode.jsonl` and `floats.jsonl` into `dir`, replacing earlier ones.
pub(crate) fn write(oracle: &Oracle, dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let files = [
        ("decode", oracle.section("decode", &oracle.decode)?),
        ("floats", oracle.section("floats", &oracle.floats)?),
    ];
    for (section, text) in files {
        let path = dir.join(format!("{section}.jsonl"));
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;

    const SECTIONS: [&str; 2] = ["decode", "floats"];

    impl Oracle {
        /// Reads the files written by [`write`] from `dir`.
        pub(crate) fn load(dir: &Path) -> anyhow::Result<Self> {
            let mut texts = Vec::with_capacity(SECTIONS.len());
            let mut version: Option<String> = None;
            for section in SECTIONS {
                let path = dir.join(format!("{section}.jsonl"));
                let text = fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let header: Header = serde_json::from_str(text.lines().next().unwrap_or_default())
                    .with_context(|| format!("parsing the header of {}", path.display()))?;
                anyhow::ensure!(
                    header.section == section && header.schema == SCHEMA,
                    "{} declares section `{}` schema {} (expected `{section}` schema {SCHEMA})",
                    path.display(),
                    header.section,
                    header.schema
                );
                if let Some(expected) = &version {
                    anyhow::ensure!(
                        *expected == header.php_version,
                        "{} is for PHP {} but decode.jsonl is for {expected}",
                        path.display(),
                        header.php_version
                    );
                }
                version = Some(header.php_version);
                texts.push(text);
            }
            Ok(Self {
                schema: SCHEMA,
                php_version: version.unwrap_or_default(),
                decode: cases(&texts[0], "decode")?,
                floats: cases(&texts[1], "floats")?,
            })
        }
    }

    fn cases<T: DeserializeOwned>(text: &str, section: &str) -> anyhow::Result<Vec<T>> {
        text.lines()
            .enumerate()
            .skip(1)
            .map(|(index, line)| {
                serde_json::from_str(line)
                    .with_context(|| format!("{section}.jsonl line {}", index + 1))
            })
            .collect()
    }

    const OUTPUT: &str = r#"{"schema": 1, "php_version": "8.5.12",
        "decode": [
            {"input": "i:1;", "refused": false, "messages": [], "serialized": "i:1;"},
            {"input": "s:1:\"\u00ff\";", "max_depth": 3, "refused": false, "messages": [], "serialized": "s:1:\"\u00ff\";"},
            {"input": "x", "refused": true, "messages": [["warning", "unserialize(): Error at offset 0 of 1 bytes"]]},
            {"input": "S:1:\"a\";", "refused": false, "messages": [["deprecated", "unserialize(): ..."]], "serialized": "s:1:\"a\";"}
        ],
        "floats": [{"bits": "3ff0000000000000", "serialized": "d:1;"}]}"#;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("xtask-serialize-{}-{name}", std::process::id()));
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
        assert_eq!(oracle.decode.len(), 4);
        assert_eq!(oracle.decode[1].max_depth, Some(3));
        assert_eq!(oracle.decode[3].messages[0].0, Level::Deprecated);
        assert_eq!(
            oracle.summary(),
            "4 decode cases (1 refused), 1 floats".to_owned()
        );
    }

    #[test]
    fn parse_rejects_malformed_and_unknown_fields() {
        assert!(parse("[]").is_err());
        let unknown = OUTPUT.replace("\"refused\": true", "\"refused\": true, \"extra\": 1");
        assert!(parse(&unknown).is_err());
        let level = OUTPUT.replace("[\"warning\"", "[\"notice\"");
        assert!(parse(&level).is_err());
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
        assert_eq!(
            problems_of(|o| o.floats.clear()),
            ["no decode or no float cases"]
        );
        assert_eq!(
            problems_of(|o| o.decode.push(o.decode[0].clone())),
            ["decode case 5 repeats an earlier input"]
        );
        assert!(
            problems_of(|o| {
                let mut other_depth = o.decode[0].clone();
                other_depth.max_depth = Some(1);
                o.decode.push(other_depth);
            })
            .is_empty()
        );
        assert_eq!(
            problems_of(|o| o.decode[2].serialized = Some(String::new())),
            ["decode case 3 must have a serialized value exactly when accepted"]
        );
        assert_eq!(
            problems_of(|o| o.decode[0].serialized = None),
            ["decode case 1 must have a serialized value exactly when accepted"]
        );
        assert_eq!(
            problems_of(|o| o.decode[2].messages[0].1.push('\u{100}')),
            ["decode case 3 holds a non-byte character"]
        );
        assert_eq!(
            problems_of(|o| o.floats.push(o.floats[0].clone())),
            ["float case 2 repeats bits 3ff0000000000000"]
        );
        for bits in ["3FF0000000000000", "3ff", "3ff000000000000g"] {
            assert_eq!(
                problems_of(|o| o.floats[0].bits = bits.to_owned()),
                ["float case 1 bits must be 16 lowercase hex digits"],
                "{bits}"
            );
        }
        assert_eq!(
            problems_of(|o| o.floats[0].serialized.push('é')),
            ["float case 1 is not ASCII"]
        );
    }

    #[test]
    fn written_files_load_back_identically() {
        let dir = temp_dir("roundtrip");
        let oracle = parse(OUTPUT).unwrap();
        write(&oracle, &dir).unwrap();
        assert_eq!(Oracle::load(&dir).unwrap(), oracle);
        let decode = fs::read_to_string(dir.join("decode.jsonl")).unwrap();
        assert_eq!(
            decode.lines().next(),
            Some(r#"{"schema":1,"php_version":"8.5.12","section":"decode"}"#)
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_rejects_missing_or_inconsistent_files() {
        let dir = temp_dir("inconsistent");
        let oracle = parse(OUTPUT).unwrap();
        assert!(Oracle::load(&dir).is_err());
        write(&oracle, &dir).unwrap();

        let floats = dir.join("floats.jsonl");
        let text = fs::read_to_string(&floats).unwrap();
        fs::write(&floats, text.replace("8.5.12", "8.4.0")).unwrap();
        let error = Oracle::load(&dir).unwrap_err();
        assert!(error.to_string().contains("is for PHP 8.4.0"), "{error}");

        fs::write(&floats, text.replace("\"floats\"", "\"decode\"")).unwrap();
        let error = Oracle::load(&dir).unwrap_err();
        assert!(error.to_string().contains("declares section"), "{error}");

        fs::write(&floats, "not json\n").unwrap();
        let error = Oracle::load(&dir).unwrap_err();
        assert!(
            error.to_string().starts_with("parsing the header"),
            "{error}"
        );

        fs::write(&floats, text.clone() + "{}\n").unwrap();
        let error = Oracle::load(&dir).unwrap_err();
        assert_eq!(error.to_string(), "floats.jsonl line 3");
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
            .join(SERIALIZE_DIR);
        let oracle = Oracle::load(&dir).unwrap();
        assert_eq!(oracle.problems(), Vec::<String>::new());
    }
}
