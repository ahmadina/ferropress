//! The compiler corpus check: our compile-time diagnostics against the reference PHP on every
//! upstream source file.
//!
//! The lab script prints, per file, the diagnostics compiling it in lint mode reported (see
//! `lab/app/compiler-corpus.php`); this module parses and checks the same text from the host's
//! checkout with `php-parser` and `php-compiler` and lists the files where they differ. Nothing from
//! the upstream sources is written into the repository.

use std::fmt::Write as _;

use anyhow::Context as _;
use php_parser::{ErrorClass, ParseOptions, parse as parse_source};
use serde::Deserialize;

use crate::lexer_corpus::file_section;
use crate::manifest::bytes_sha256;

/// Path of the corpus script inside the lab container.
pub(crate) const SCRIPT: &str = "/opt/ferropress/compiler-corpus.php";
/// Git-ignored file, relative to the repository root, listing the files that differ.
pub(crate) const REPORT: &str = "lab/captures/compiler-corpus.txt";
/// What the script writes in place of the compiled file's name; we compile with an empty one.
const FILE_PLACEHOLDER: &str = "%file%";

/// One diagnostic: severity, message (one character per byte) and line.
type Diagnostic = (String, String, u32);

/// Fragments found only in the diagnostics the reference raises while linking a class to its
/// parent, interfaces and traits during compilation. Our compiler does not link classes yet
/// (ADR-0007), so a file whose first missing diagnostic is one of these differs only by linking.
const LINKING: [&str; 19] = [
    " must be compatible with ",
    " should either be compatible with ",
    "] attribute, but no matching parent ",
    "Cannot override final ",
    " cannot override final constant ",
    "Cannot redeclare static ",
    "Cannot redeclare non static ",
    "Cannot redeclare readonly property ",
    "Cannot redeclare non-readonly property ",
    "Cannot make static method ",
    "Cannot make non static method ",
    "Access level to ",
    "Set access level of ",
    " (as in class ",
    " to match the parent definition in class ",
    " and must therefore be declared abstract or implement the remaining method",
    " cannot extend ",
    "Cannot specify default value for virtual hooked property ",
    " with set hook may not return by reference",
];

/// Fragments found only in diagnostics that depend on the built-in functions, which our compiler
/// does not describe yet (roadmap step 19): their results as temporaries, and the deprecated
/// `$http_response_header` reads that depend on how known callees take arguments.
const BUILT_INS: [&str; 2] = [
    "Cannot use result of built-in function in write context",
    "The predefined locally scoped $http_response_header variable is deprecated",
];

/// Why a file differs only by a diagnostic the compiler cannot produce yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    /// Linking classes to their parents, interfaces and traits.
    Linking,
    /// Descriptions of the built-in functions.
    BuiltIns,
}

impl Cause {
    /// The cause of a diagnostic we lack, when it is one of these.
    fn of(message: &str) -> Option<Self> {
        if LINKING.iter().any(|fragment| message.contains(fragment)) {
            return Some(Self::Linking);
        }
        let redeclares_built_in = message
            .strip_prefix("Cannot redeclare function ")
            .is_some_and(|rest| rest.ends_with("()"));
        (redeclares_built_in || BUILT_INS.iter().any(|fragment| message.contains(fragment)))
            .then_some(Self::BuiltIns)
    }

    /// The prefix of the report lines with this cause.
    const fn prefix(self) -> &'static str {
        match self {
            Self::Linking => "linking",
            Self::BuiltIns => "built-ins",
        }
    }
}

/// What the reference PHP reported for one file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub(crate) path: String,
    pub(crate) sha256: String,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

impl Entry {
    /// Whether the text is a test's FILE section rather than a whole file.
    fn is_test(&self) -> bool {
        std::path::Path::new(&self.path)
            .extension()
            .is_some_and(|extension| extension == "phpt")
    }

    /// Whether the reference stopped the file with a fatal or parse error.
    fn is_rejected(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|(severity, _, _)| severity == "Fatal error" || severity == "Parse error")
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

/// What we report compiling `source`, in the script's terms.
fn ours(source: &[u8]) -> Result<Vec<Diagnostic>, String> {
    let parsed =
        parse_source(source, ParseOptions::default()).map_err(|error| error.to_string())?;
    let mut diagnostics: Vec<Diagnostic> = parsed
        .warnings
        .iter()
        .map(|warning| {
            (
                warning.severity.name().to_owned(),
                warning.message.clone(),
                warning.line,
            )
        })
        .collect();
    match parsed.result {
        Err(error) => {
            let severity = match error.class {
                ErrorClass::Parse => "Parse error",
                ErrorClass::Compile => "Fatal error",
            };
            let line = error.line.unwrap_or(0);
            diagnostics.push((severity.to_owned(), characters(&error.message), line));
        }
        Ok(program) => {
            let checked = php_compiler::check(&program, b"").map_err(|error| error.to_string())?;
            diagnostics.extend(checked.warnings.into_iter().chain(checked.fatal).map(
                |diagnostic| {
                    (
                        diagnostic.severity.label().to_owned(),
                        characters(&diagnostic.message),
                        diagnostic.line,
                    )
                },
            ));
        }
    }
    Ok(diagnostics)
}

/// How a file's diagnostics differ from the reference's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Difference {
    pub(crate) reason: String,
    /// Set when everything before the reference's first diagnostic we lack matches, and that
    /// diagnostic needs what the compiler cannot do yet.
    pub(crate) cause: Option<Cause>,
}

impl Difference {
    fn other(reason: &str) -> Self {
        Self {
            reason: reason.to_owned(),
            cause: None,
        }
    }
}

/// Why `got` differs from `expected`, when only by a missing diagnostic with a known cause.
fn missing_cause(got: &[Diagnostic], expected: &[Diagnostic]) -> Option<Cause> {
    let matching = got
        .iter()
        .zip(expected)
        .take_while(|(got, expected)| got == expected)
        .count();
    if got.len() != matching {
        return None;
    }
    expected
        .get(matching)
        .and_then(|(_, message, _)| Cause::of(message))
}

/// How our diagnostics for `text` (the file's contents) differ from `entry`'s, if they do.
pub(crate) fn difference(entry: &Entry, text: &[u8]) -> Option<Difference> {
    let source = if entry.is_test() {
        match file_section(text) {
            Some(section) => section,
            None => return Some(Difference::other("no FILE section on the host")),
        }
    } else {
        text.to_vec()
    };
    if bytes_sha256(&source) != entry.sha256 {
        return Some(Difference::other("the host text differs from the lab's"));
    }
    let got = match ours(&source) {
        Ok(got) => got,
        Err(error) => return Some(Difference::other(&error)),
    };
    let expected: Vec<Diagnostic> = entry
        .diagnostics
        .iter()
        .map(|(severity, message, line)| {
            (
                severity.clone(),
                message.replace(FILE_PLACEHOLDER, ""),
                *line,
            )
        })
        .collect();
    (got != expected).then(|| Difference {
        reason: format!("diagnostics {got:?}, reference {expected:?}"),
        cause: missing_cause(&got, &expected),
    })
}

/// The outcome of checking every entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) files: usize,
    /// Files the reference stops with a fatal or parse error.
    pub(crate) rejected: usize,
    /// `path: reason` for every file that differs without a known cause, in the script's order.
    pub(crate) differences: Vec<String>,
    /// `path: reason` for every file that differs only by a diagnostic with a known cause, in the
    /// script's order, with that cause.
    pub(crate) deferred: Vec<(Cause, String)>,
}

impl Summary {
    /// How many files differ only because of `cause`.
    pub(crate) fn count(&self, cause: Cause) -> usize {
        self.deferred
            .iter()
            .filter(|(deferred, _)| *deferred == cause)
            .count()
    }

    /// The report file's contents: the differences, then those with a known cause, prefixed by it.
    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.differences {
            let _ = writeln!(out, "{line}");
        }
        for (cause, line) in &self.deferred {
            let _ = writeln!(out, "{}: {line}", cause.prefix());
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
    let mut deferred = Vec::new();
    for entry in entries {
        let text = read(&entry.path).with_context(|| format!("reading {}", entry.path))?;
        if let Some(difference) = difference(entry, &text) {
            let line = format!("{}: {}", entry.path, difference.reason);
            match difference.cause {
                Some(cause) => deferred.push((cause, line)),
                None => differences.push(line),
            }
        }
    }
    Ok(Summary {
        files: entries.len(),
        rejected: entries.iter().filter(|entry| entry.is_rejected()).count(),
        differences,
        deferred,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(entry: &Entry, text: &[u8]) -> Option<String> {
        difference(entry, text).map(|difference| difference.reason)
    }

    fn entry_for(path: &str, source: &[u8], diagnostics: &[(&str, &str, u32)]) -> Entry {
        Entry {
            path: path.to_owned(),
            sha256: bytes_sha256(source),
            diagnostics: diagnostics
                .iter()
                .map(|&(severity, message, line)| (severity.to_owned(), message.to_owned(), line))
                .collect(),
        }
    }

    #[test]
    fn parse_reads_lines_and_rejects_bad_ones() {
        let clean = r#"{"path":"a.php","sha256":"x","diagnostics":[]}"#;
        let failing = r#"{"path":"b.php","sha256":"y","diagnostics":[["Fatal error","m",3]]}"#;
        let entries = parse(&format!("{clean}\n\n{failing}\n")).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[1].diagnostics,
            [("Fatal error".to_owned(), "m".to_owned(), 3)]
        );
        assert!(parse("").is_err());
        assert!(parse("{}").is_err());
    }

    #[test]
    fn difference_compares_text_and_diagnostics() {
        let source = b"<?php echo 1;";
        let entry = entry_for("a.php", source, &[]);
        assert_eq!(difference(&entry, source), None);
        assert_eq!(
            reason(&entry, b"<?php echo 2;").as_deref(),
            Some("the host text differs from the lab's")
        );

        let redeclared = b"<?php function f() {} function f() {}";
        let message = "Cannot redeclare function f() (previously declared in %file%:1)";
        let entry = entry_for("b.php", redeclared, &[("Fatal error", message, 1)]);
        assert_eq!(difference(&entry, redeclared), None);
        let missing = entry_for("b.php", redeclared, &[]);
        assert_eq!(
            reason(&missing, redeclared).as_deref(),
            Some(
                "diagnostics [(\"Fatal error\", \"Cannot redeclare function f() (previously \
                 declared in :1)\", 1)], reference []"
            )
        );
    }

    #[test]
    fn difference_reports_parse_errors_and_warnings_with_their_bytes() {
        let broken = b"<?php \xe9\xe9 \xe9;";
        let message = "syntax error, unexpected identifier \"\u{e9}\"";
        let entry = entry_for("c.php", broken, &[("Parse error", message, 1)]);
        assert_eq!(difference(&entry, broken), None);

        let warned = b"<?php function f($a = 1, $b) {}";
        let message = "f(): Optional parameter $a declared before required parameter $b is \
                       implicitly treated as a required parameter";
        let entry = entry_for("d.php", warned, &[("Deprecated", message, 1)]);
        assert_eq!(difference(&entry, warned), None);
    }

    #[test]
    fn difference_reads_tests_through_their_file_section() {
        let test = b"--TEST--\n--FILE--\n<?php echo 1;\n--EXPECT--\n";
        let entry = entry_for("t.phpt", b"<?php echo 1;\n", &[]);
        assert_eq!(difference(&entry, test), None);
        assert_eq!(
            reason(&entry, b"--TEST--\n").as_deref(),
            Some("no FILE section on the host")
        );
    }

    fn diagnostic(severity: &str, message: &str) -> Diagnostic {
        (severity.to_owned(), message.to_owned(), 1)
    }

    #[test]
    fn deferred_differences_are_told_apart() {
        let linking = diagnostic(
            "Fatal error",
            "Declaration of B::f() must be compatible with A::f($a)",
        );
        let deprecated = diagnostic(
            "Deprecated",
            "Return type of B::f() should either be compatible with A::f(): int, or ...",
        );
        let warning = diagnostic("Warning", "w");
        let built_in = diagnostic("Fatal error", "Cannot redeclare function strlen()");
        let written = diagnostic(
            "Fatal error",
            "Cannot use result of built-in function in write context",
        );
        let other = diagnostic(
            "Fatal error",
            "Cannot redeclare function f() (previously declared in a.php:1)",
        );
        let linked = Some(Cause::Linking);
        assert_eq!(missing_cause(&[], std::slice::from_ref(&linking)), linked);
        assert_eq!(
            missing_cause(&[], std::slice::from_ref(&deprecated)),
            linked
        );
        assert_eq!(
            missing_cause(
                std::slice::from_ref(&warning),
                &[warning.clone(), linking.clone()]
            ),
            linked
        );
        assert_eq!(
            missing_cause(&[], std::slice::from_ref(&built_in)),
            Some(Cause::BuiltIns)
        );
        assert_eq!(
            missing_cause(&[], std::slice::from_ref(&written)),
            Some(Cause::BuiltIns)
        );
        assert_eq!(
            missing_cause(std::slice::from_ref(&other), std::slice::from_ref(&linking)),
            None
        );
        assert_eq!(missing_cause(&[], std::slice::from_ref(&other)), None);
        assert_eq!(missing_cause(std::slice::from_ref(&warning), &[]), None);
        assert_eq!(Cause::BuiltIns.prefix(), "built-ins");
    }

    #[test]
    fn check_summarizes_and_propagates_read_errors() {
        let source = b"<?php echo 1;";
        let good = entry_for("a.php", source, &[]);
        let bad = entry_for("b.php", source, &[("Parse error", "m", 1)]);
        let summary = check(&[good.clone(), bad], |_| Ok(source.to_vec())).unwrap();
        assert_eq!(summary.files, 2);
        assert_eq!(summary.rejected, 1);
        let line = "b.php: diagnostics [], reference [(\"Parse error\", \"m\", 1)]";
        assert_eq!(summary.differences, [line]);
        assert_eq!(summary.render(), format!("{line}\n"));
        assert!(summary.deferred.is_empty());
        let error = check(&[good], |_| Err(std::io::Error::other("gone"))).unwrap_err();
        assert_eq!(error.to_string(), "reading a.php");

        let message = "Class B cannot extend final class A";
        let linked = entry_for("c.php", source, &[("Fatal error", message, 1)]);
        let summary = check(&[linked], |_| Ok(source.to_vec())).unwrap();
        assert!(summary.differences.is_empty());
        let line =
            format!("c.php: diagnostics [], reference [(\"Fatal error\", \"{message}\", 1)]");
        assert_eq!(summary.deferred, [(Cause::Linking, line.clone())]);
        assert_eq!(summary.count(Cause::Linking), 1);
        assert_eq!(summary.count(Cause::BuiltIns), 0);
        assert_eq!(summary.render(), format!("linking: {line}\n"));
    }
}
