//! The lexer corpus check: our lexer against the reference PHP on every upstream source file.
//!
//! The lab script prints one digest line per file (see `lab/app/lexer-corpus.php`); this module
//! tokenizes the same text from the host's checkout with `php-lexer`, builds the same digest and
//! lists the files that differ. Nothing from the upstream sources is written into the repository.

use std::fmt::Write as _;

use anyhow::Context as _;
use php_lexer::{Diagnostic, LexerOptions, TokenKind, tokenize};
use serde::Deserialize;

use crate::manifest::bytes_sha256;

/// Path of the corpus script inside the lab container.
pub(crate) const SCRIPT: &str = "/opt/ferropress/lexer-corpus.php";
/// Directory, relative to the repository root, holding the upstream checkouts.
pub(crate) const UPSTREAM_DIR: &str = "lab/upstream";
/// Git-ignored file, relative to the repository root, listing the files that differ.
pub(crate) const REPORT: &str = "lab/captures/lexer-corpus.txt";

/// What the reference PHP reported for one file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub(crate) path: String,
    pub(crate) sha256: String,
    pub(crate) tokens: usize,
    pub(crate) digest: String,
    pub(crate) warnings: Vec<String>,
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

fn is_section_marker(line: &[u8]) -> bool {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    line.len() > 4
        && line.starts_with(b"--")
        && line.ends_with(b"--")
        && line[2..line.len() - 2]
            .iter()
            .all(|&byte| byte.is_ascii_uppercase() || byte == b'_')
}

/// The named section of a test: the lines after `--NAME--` up to the next section marker, or `None`
/// when the test has no such section.
pub(crate) fn section(text: &[u8], name: &[u8]) -> Option<Vec<u8>> {
    let mut marker = Vec::with_capacity(name.len() + 4);
    marker.extend_from_slice(b"--");
    marker.extend_from_slice(name);
    marker.extend_from_slice(b"--");
    let lines: Vec<&[u8]> = text.split(|&byte| byte == b'\n').collect();
    let start = lines.iter().position(|line| {
        let end = line
            .iter()
            .rposition(|&byte| byte != b'\r')
            .map_or(0, |last| last + 1);
        line[..end] == marker
    })? + 1;
    for (index, line) in lines.iter().enumerate().skip(start) {
        if is_section_marker(line) {
            let mut body = Vec::new();
            for kept in &lines[start..index] {
                body.extend_from_slice(kept);
                body.push(b'\n');
            }
            return Some(body);
        }
    }
    Some(lines[start..].join(&b'\n'))
}

/// The FILE section of a test, or `None` when the test has none.
pub(crate) fn file_section(text: &[u8]) -> Option<Vec<u8>> {
    section(text, b"FILE").or_else(|| section(text, b"FILEEOF"))
}

/// Why our reading of `text` (the file's contents) differs from `entry`, if it does.
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
    let result = match tokenize(&source, LexerOptions::default()) {
        Ok(result) => result,
        Err(error) => return Some(error.to_string()),
    };
    let mut listing = String::new();
    for token in &result.tokens {
        let name = match token.kind {
            TokenKind::Char(byte) => char::from(byte).to_string(),
            named => named.name().unwrap_or_default().to_owned(),
        };
        let _ = writeln!(
            listing,
            "{name}\t{}\t{}",
            token.end - token.start,
            token.line
        );
    }
    let warnings: Vec<String> = result
        .diagnostics
        .iter()
        .filter_map(|diagnostic| match diagnostic {
            Diagnostic::Warning { warning, .. } => Some(warning.to_string()),
            Diagnostic::Error { .. } => None,
        })
        .collect();
    if result.tokens.len() != entry.tokens || bytes_sha256(listing.as_bytes()) != entry.digest {
        return Some(format!(
            "tokens differ ({} here, {} in the reference)",
            result.tokens.len(),
            entry.tokens
        ));
    }
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
    pub(crate) tokens: usize,
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
        tokens: entries.iter().map(|entry| entry.tokens).sum(),
        differences,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_for(path: &str, source: &[u8], listing: &str, tokens: usize) -> Entry {
        Entry {
            path: path.to_owned(),
            sha256: bytes_sha256(source),
            tokens,
            digest: bytes_sha256(listing.as_bytes()),
            warnings: Vec::new(),
        }
    }

    const LISTING: &str = "T_OPEN_TAG\t6\t1\nT_ECHO\t4\t1\n;\t1\t1\n";

    #[test]
    fn parse_reads_lines_and_rejects_bad_ones() {
        let line = r#"{"path":"a.php","sha256":"x","tokens":1,"digest":"y","warnings":[]}"#;
        let entries = parse(&format!("{line}\n\n{line}\n")).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(parse("").is_err());
        assert!(parse("{}").is_err());
    }

    #[test]
    fn file_sections() {
        let test = b"--TEST--\nx\n--FILE--\n<?php\necho 1;\n--EXPECT--\n1\n";
        assert_eq!(file_section(test), Some(b"<?php\necho 1;\n".to_vec()));
        let crlf = b"--FILE--\r\n<?php\r\n--EXPECTF--\r\n";
        assert_eq!(file_section(crlf), Some(b"<?php\r\n".to_vec()));
        let to_end = b"--FILE--\n<?php\n-- not --\n--lower--\n";
        assert_eq!(
            file_section(to_end),
            Some(b"<?php\n-- not --\n--lower--\n".to_vec())
        );
        assert_eq!(file_section(b"--FILE--\n--EXPECT--"), Some(Vec::new()));
        assert_eq!(
            file_section(b"--FILEEOF--\n<?php echo 1;\n--EXPECT--\n"),
            Some(b"<?php echo 1;\n".to_vec())
        );
        assert_eq!(file_section(b"--TEST--\n"), None);
        assert!(!is_section_marker(b"----"));
        assert_eq!(
            section(b"--EXPECT--\n1\n--CLEAN--\n", b"EXPECT"),
            Some(b"1\n".to_vec())
        );
    }

    #[test]
    fn difference_compares_text_tokens_and_warnings() {
        let source = b"<?php echo;";
        let entry = entry_for("a.php", source, LISTING, 3);
        assert_eq!(difference(&entry, source), None);
        assert_eq!(
            difference(&entry, b"<?php print;").as_deref(),
            Some("the host text differs from the lab's")
        );
        let wrong = entry_for("a.php", source, "T_OPEN_TAG\t6\t1\n", 1);
        assert_eq!(
            difference(&wrong, source).as_deref(),
            Some("tokens differ (3 here, 1 in the reference)")
        );
        let mut warned = entry.clone();
        warned.warnings.push("w".to_owned());
        assert_eq!(
            difference(&warned, source).as_deref(),
            Some(r#"warnings [], reference ["w"]"#)
        );
        let octal = b"<?php \"\\400\";";
        let mut warning = entry_for(
            "b.php",
            octal,
            "T_OPEN_TAG\t6\t1\nT_CONSTANT_ENCAPSED_STRING\t6\t1\n;\t1\t1\n",
            3,
        );
        warning.warnings =
            vec![r"Octal escape sequence overflow \400 is greater than \377".to_owned()];
        assert_eq!(difference(&warning, octal), None);
    }

    #[test]
    fn difference_reads_tests_through_their_file_section() {
        let test = b"--TEST--\n--FILE--\n<?php echo;\n--EXPECT--\n";
        let entry = entry_for(
            "t.phpt",
            b"<?php echo;\n",
            &format!("{LISTING}T_WHITESPACE\t1\t1\n"),
            4,
        );
        assert_eq!(difference(&entry, test), None);
        assert_eq!(
            difference(&entry, b"--TEST--\n").as_deref(),
            Some("no FILE section on the host")
        );
    }

    #[test]
    fn check_summarizes_and_propagates_read_errors() {
        let source = b"<?php echo;";
        let good = entry_for("a.php", source, LISTING, 3);
        let bad = entry_for("b.php", source, "", 0);
        let summary = check(&[good.clone(), bad], |_| Ok(source.to_vec())).unwrap();
        assert_eq!(summary.files, 2);
        assert_eq!(summary.tokens, 3);
        assert_eq!(
            summary.differences,
            ["b.php: tokens differ (3 here, 0 in the reference)"]
        );
        assert_eq!(
            summary.render(),
            "b.php: tokens differ (3 here, 0 in the reference)\n"
        );
        let error = check(&[good], |_| Err(std::io::Error::other("gone"))).unwrap_err();
        assert_eq!(error.to_string(), "reading a.php");
    }
}
