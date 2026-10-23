//! The parser corpus check: our parser against the reference PHP on every upstream source file.
//!
//! The lab script prints, per file, the error and warnings parsing raised (see
//! `lab/app/parser-corpus.php`); this module parses the same text from the host's checkout with
//! `php-parser` and lists the files where the outcome differs. Nothing from the upstream sources is
//! written into the repository.

use std::fmt::Write as _;

use anyhow::Context as _;
use php_parser::{ParseOptions, parse as parse_source};
use serde::Deserialize;

use crate::lexer_corpus::file_section;
use crate::manifest::bytes_sha256;

/// Path of the corpus script inside the lab container.
pub(crate) const SCRIPT: &str = "/opt/ferropress/parser-corpus.php";
/// Git-ignored file, relative to the repository root, listing the files that differ.
pub(crate) const REPORT: &str = "lab/captures/parser-corpus.txt";

/// What the reference PHP reported for one file. Messages hold one character per byte.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub(crate) path: String,
    pub(crate) sha256: String,
    pub(crate) error: Option<(String, String, Option<u32>)>,
    pub(crate) warnings: Vec<(String, String)>,
}

impl Entry {
    /// Whether the text is a test's FILE section rather than a whole file.
    fn is_test(&self) -> bool {
        std::path::Path::new(&self.path)
            .extension()
            .is_some_and(|extension| extension == "phpt")
    }
}

/// Parses the script's output, one entry per non-empty line.
pub(crate) fn parse(output: &str) -> anyhow::Result<Vec<Entry>> {
    let entries: Vec<Entry> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line).with_context(|| format!("corpus line {}", index + 1))
        })
        .collect::<anyhow::Result<_>>()?;
    anyhow::ensure!(!entries.is_empty(), "the corpus script reported no files");
    Ok(entries)
}

/// Bytes as the script writes them: one character per byte.
fn characters(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}

/// Why our parse of `text` (the file's contents) differs from `entry`, if it does.
pub(crate) fn difference(entry: &Entry, text: &[u8]) -> Option<String> {
    let source = if entry.is_test() {
        match file_section(text) {
            Some(section) => section,
            None => return Some("no FILE section on the host".to_owned()),
        }
    } else {
        text.to_vec()
    };
    if bytes_sha256(&source) != entry.sha256 {
        return Some("the host text differs from the lab's".to_owned());
    }
    let parsed = match parse_source(&source, ParseOptions::default()) {
        Ok(parsed) => parsed,
        Err(error) => return Some(error.to_string()),
    };
    let error = parsed.result.err().map(|error| {
        (
            error.class.name().to_owned(),
            characters(&error.message),
            error.line,
        )
    });
    if error != entry.error {
        return Some(format!("error {error:?}, reference {:?}", entry.error));
    }
    let warnings: Vec<(String, String)> = parsed
        .warnings
        .iter()
        .map(|warning| (warning.severity.name().to_owned(), warning.message.clone()))
        .collect();
    if warnings != entry.warnings {
        return Some(format!(
            "warnings {warnings:?}, reference {:?}",
            entry.warnings
        ));
    }
    None
}

/// The outcome of checking every entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) files: usize,
    /// Files the reference rejects with an error.
    pub(crate) errors: usize,
    /// `path: reason` for every file that differs, in the script's order.
    pub(crate) differences: Vec<String>,
}

impl Summary {
    /// The report file's contents.
    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.differences {
            let _ = writeln!(out, "{line}");
        }
        out
    }
}

/// Checks every entry against the text `read` returns for its path.
pub(crate) fn check(
    entries: &[Entry],
    mut read: impl FnMut(&str) -> std::io::Result<Vec<u8>>,
) -> anyhow::Result<Summary> {
    let mut differences = Vec::new();
    for entry in entries {
        let text = read(&entry.path).with_context(|| format!("reading {}", entry.path))?;
        if let Some(reason) = difference(entry, &text) {
            differences.push(format!("{}: {reason}", entry.path));
        }
    }
    Ok(Summary {
        files: entries.len(),
        errors: entries.iter().filter(|entry| entry.error.is_some()).count(),
        differences,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_for(path: &str, source: &[u8]) -> Entry {
        Entry {
            path: path.to_owned(),
            sha256: bytes_sha256(source),
            error: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn parse_reads_lines_and_rejects_bad_ones() {
        let clean = r#"{"path":"a.php","sha256":"x","error":null,"warnings":[]}"#;
        let failing = r#"{"path":"b.php","sha256":"y","error":["ParseError","m",3],"warnings":[["Warning","w"]]}"#;
        let entries = parse(&format!("{clean}\n\n{failing}\n")).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[1].error,
            Some(("ParseError".to_owned(), "m".to_owned(), Some(3)))
        );
        assert_eq!(
            entries[1].warnings,
            [("Warning".to_owned(), "w".to_owned())]
        );
        assert!(parse("").is_err());
        assert!(parse("{}").is_err());
    }

    #[test]
    fn difference_compares_text_errors_and_warnings() {
        let source = b"<?php echo 1;";
        let entry = entry_for("a.php", source);
        assert_eq!(difference(&entry, source), None);
        assert_eq!(
            difference(&entry, b"<?php echo 2;").as_deref(),
            Some("the host text differs from the lab's")
        );

        let broken = b"<?php echo \xe9 +;";
        let mut rejected = entry_for("b.php", broken);
        assert_eq!(
            difference(&rejected, broken).as_deref(),
            Some(
                "error Some((\"ParseError\", \"syntax error, unexpected token \\\";\\\"\", \
                 Some(1))), reference None"
            )
        );
        rejected.error = Some((
            "ParseError".to_owned(),
            "syntax error, unexpected token \";\"".to_owned(),
            Some(1),
        ));
        assert_eq!(difference(&rejected, broken), None);

        let mut warned = entry.clone();
        warned.warnings.push(("Warning".to_owned(), "w".to_owned()));
        assert_eq!(
            difference(&warned, source).as_deref(),
            Some(r#"warnings [], reference [("Warning", "w")]"#)
        );
    }

    #[test]
    fn errors_keep_their_bytes_as_characters() {
        let source = b"<?php \xe9\xe9 \xe9;";
        let mut entry = entry_for("c.php", source);
        entry.error = Some((
            "ParseError".to_owned(),
            "syntax error, unexpected identifier \"\u{e9}\"".to_owned(),
            Some(1),
        ));
        assert_eq!(difference(&entry, source), None);
    }

    #[test]
    fn difference_reads_tests_through_their_file_section() {
        let test = b"--TEST--\n--FILE--\n<?php echo 1;\n--EXPECT--\n";
        let entry = entry_for("t.phpt", b"<?php echo 1;\n");
        assert_eq!(difference(&entry, test), None);
        assert_eq!(
            difference(&entry, b"--TEST--\n").as_deref(),
            Some("no FILE section on the host")
        );
    }

    #[test]
    fn check_summarizes_and_propagates_read_errors() {
        let source = b"<?php echo 1;";
        let good = entry_for("a.php", source);
        let mut bad = entry_for("b.php", source);
        bad.error = Some(("ParseError".to_owned(), "m".to_owned(), None));
        let summary = check(&[good.clone(), bad], |_| Ok(source.to_vec())).unwrap();
        assert_eq!(summary.files, 2);
        assert_eq!(summary.errors, 1);
        let line = "b.php: error None, reference Some((\"ParseError\", \"m\", None))";
        assert_eq!(summary.differences, [line]);
        assert_eq!(summary.render(), format!("{line}\n"));
        let error = check(&[good], |_| Err(std::io::Error::other("gone"))).unwrap_err();
        assert_eq!(error.to_string(), "reading a.php");
    }
}
