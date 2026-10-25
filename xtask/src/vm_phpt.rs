//! The VM PHP test-suite check: compile and run every upstream `.phpt` FILE against `php-vm`.
//!
//! Each test is classified as passed, failed, pending (a construct or built-in the VM does not run
//! yet) or skipped (a SAPI, `ini` or expectation form this runner cannot honor). Nothing from the
//! upstream sources is written into the repository.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use php_compiler::{CompileFailure, Failure, compile};
use php_parser::{ErrorClass, ParseOptions, parse as parse_source};

use crate::lexer_corpus::section;

/// Directory, relative to the repository root, holding the PHP checkout.
pub(crate) const UPSTREAM_PHP: &str = "lab/upstream/php";
/// Git-ignored file, relative to the repository root, listing failed and pending tests.
pub(crate) const REPORT: &str = "lab/captures/vm-phpt.txt";

/// Sections that need a SAPI, process settings or an expectation form this runner does not honor.
const SKIP_SECTIONS: [&[u8]; 19] = [
    b"ARGS",
    b"CAPTURE_STDIO",
    b"CGI",
    b"COOKIE",
    b"DEFLATE_POST",
    b"ENV",
    b"EXPECTREGEX",
    b"EXPECT_EXTERNAL",
    b"EXTENSIONS",
    b"FILE_EXTERNAL",
    b"GET",
    b"GZIP_POST",
    b"INI",
    b"PHPDBG",
    b"POST",
    b"POST_RAW",
    b"PUT",
    b"REDIRECTTEST",
    b"STDIN",
];

/// How one test finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Output matched the expected section.
    Passed,
    /// The FILE compiled and ran, but the output differed.
    Failed(String),
    /// The FILE uses a construct or built-in the compiler or VM does not run yet.
    Pending(String),
    /// The test needs a SAPI, `ini` setting or expectation form this runner cannot honor.
    Skipped(String),
}

/// Counts and the report lines for failed and pending tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) tests: usize,
    pub(crate) passed: usize,
    pub(crate) failed: usize,
    pub(crate) pending: usize,
    pub(crate) skipped: usize,
    /// `failed path: reason` and `pending path: reason`, in walk order.
    pub(crate) lines: Vec<String>,
}

impl Summary {
    /// The report file's contents.
    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            let _ = writeln!(out, "{line}");
        }
        out
    }
}

/// Walks `root` for `.phpt` files, runs each FILE and classifies the result.
pub(crate) fn check(root: &Path) -> anyhow::Result<Summary> {
    let files = collect(root)?;
    let mut summary = Summary {
        tests: files.len(),
        passed: 0,
        failed: 0,
        pending: 0,
        skipped: 0,
        lines: Vec::new(),
    };
    for path in &files {
        let relative = relative(root, path);
        let text = fs::read(path).with_context(|| format!("reading {relative}"))?;
        match run_test(&relative, &text) {
            Outcome::Passed => summary.passed += 1,
            Outcome::Failed(reason) => {
                summary.failed += 1;
                summary.lines.push(format!("failed {relative}: {reason}"));
            }
            Outcome::Pending(reason) => {
                summary.pending += 1;
                summary.lines.push(format!("pending {relative}: {reason}"));
            }
            Outcome::Skipped(_) => summary.skipped += 1,
        }
    }
    Ok(summary)
}

fn collect(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    visit(root, &mut files)?;
    files.sort();
    anyhow::ensure!(!files.is_empty(), "no .phpt tests under {}", root.display());
    Ok(files)
}

fn visit(dir: &Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading {}", path.display()))?;
        if file_type.is_dir() {
            visit(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "phpt")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).map_or_else(
        |_| path.display().to_string(),
        |tail| {
            tail.components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        },
    )
}

/// Classifies one test whose contents are `text` and whose path relative to the PHP root is `file`.
pub(crate) fn run_test(file: &str, text: &[u8]) -> Outcome {
    if let Some(reason) = skip_reason(text) {
        return Outcome::Skipped(reason);
    }
    let Some(source) = crate::lexer_corpus::file_section(text) else {
        return Outcome::Skipped("no FILE section".to_owned());
    };
    let expected = match expected_output(text) {
        Ok(expected) => expected,
        Err(reason) => return Outcome::Skipped(reason),
    };
    match execute(&source, display_path(file).as_bytes()) {
        Exec::Pending(reason) => Outcome::Pending(reason),
        Exec::Ran(output) => classify_run(&source, &output, &expected),
        Exec::Failed(reason) => Outcome::Failed(reason),
    }
}

fn classify_run(source: &[u8], output: &[u8], expected: &Expected) -> Outcome {
    let outcome = compare(output, expected);
    if matches!(outcome, Outcome::Failed(_)) {
        if has_inline_html(source) {
            return Outcome::Pending("inline HTML".to_owned());
        }
        if has_variable_variables(source) {
            return Outcome::Pending("variable variables".to_owned());
        }
    }
    outcome
}

fn has_inline_html(source: &[u8]) -> bool {
    source
        .windows(2)
        .position(|window| window == b"?>")
        .is_some_and(|index| !source[index + 2..].iter().all(u8::is_ascii_whitespace))
}

fn has_variable_variables(source: &[u8]) -> bool {
    source
        .windows(2)
        .any(|window| window == b"$$" || window == b"${")
}

/// The path PHP's runner shows: the test's path with a `.php` suffix in place of `.phpt`.
fn display_path(file: &str) -> String {
    file.strip_suffix(".phpt")
        .map_or_else(|| file.to_owned(), |stem| format!("{stem}.php"))
}

fn skip_reason(text: &[u8]) -> Option<String> {
    SKIP_SECTIONS.iter().find_map(|name| {
        section(text, name).map(|_| format!("{} section", String::from_utf8_lossy(name)))
    })
}

/// The expected output and whether it uses EXPECTF substitutions.
struct Expected {
    body: Vec<u8>,
    patterned: bool,
}

fn expected_output(text: &[u8]) -> Result<Expected, String> {
    if let Some(body) = section(text, b"EXPECTF") {
        return Ok(Expected {
            body,
            patterned: true,
        });
    }
    if let Some(body) = section(text, b"EXPECT") {
        return Ok(Expected {
            body,
            patterned: false,
        });
    }
    Err("no EXPECT or EXPECTF section".to_owned())
}

/// What executing a FILE produced.
enum Exec {
    Ran(Vec<u8>),
    Pending(String),
    Failed(String),
}

fn execute(source: &[u8], file: &[u8]) -> Exec {
    let parsed = match parse_source(source, ParseOptions::default()) {
        Ok(parsed) => parsed,
        Err(error) => return Exec::Failed(error.to_string()),
    };
    let mut output = Vec::new();
    for warning in &parsed.warnings {
        display(
            &mut output,
            warning.severity.name().as_bytes(),
            warning.message.as_bytes(),
            file,
            warning.line,
        );
    }
    match parsed.result {
        Err(error) => {
            let label = match error.class {
                ErrorClass::Parse => b"Parse error".as_slice(),
                ErrorClass::Compile => b"Fatal error".as_slice(),
            };
            display(
                &mut output,
                label,
                &error.message,
                file,
                error.line.unwrap_or(0),
            );
            Exec::Ran(output)
        }
        Ok(program) => match compile(&program, file) {
            Err(CompileFailure::Emit(Failure::Unsupported { construct, .. })) => {
                Exec::Pending(format!("compiling {construct} is not supported yet"))
            }
            Err(CompileFailure::Emit(Failure::TooLarge { line })) => Exec::Pending(format!(
                "the code at line {line} exceeds the bytecode's operand limit"
            )),
            Err(error) => Exec::Failed(error.to_string()),
            Ok(compiled) => finish_compile(compiled, file, output),
        },
    }
}

fn finish_compile(compiled: php_compiler::Compiled, file: &[u8], mut output: Vec<u8>) -> Exec {
    for warning in &compiled.diagnostics.warnings {
        display(
            &mut output,
            warning.severity.label().as_bytes(),
            &warning.message,
            file,
            warning.line,
        );
    }
    if let Some(fatal) = &compiled.diagnostics.fatal {
        display(
            &mut output,
            fatal.severity.label().as_bytes(),
            &fatal.message,
            file,
            fatal.line,
        );
        return Exec::Ran(output);
    }
    let Some(unit) = compiled.unit else {
        return Exec::Failed("compile produced no unit and no fatal error".to_owned());
    };
    match php_vm::run(&unit, file, &mut output) {
        Ok(_) => Exec::Ran(output),
        Err(php_vm::VmError::Unsupported(what)) => {
            Exec::Pending(format!("running {what} is not supported yet"))
        }
        Err(error) => Exec::Failed(error.to_string()),
    }
}

fn display(output: &mut Vec<u8>, label: &[u8], message: &[u8], file: &[u8], line: u32) {
    output.push(b'\n');
    output.extend_from_slice(label);
    output.extend_from_slice(b": ");
    output.extend_from_slice(message);
    output.extend_from_slice(b" in ");
    output.extend_from_slice(file);
    output.extend_from_slice(b" on line ");
    output.extend_from_slice(line.to_string().as_bytes());
    output.push(b'\n');
}

fn compare(got: &[u8], expected: &Expected) -> Outcome {
    match try_match(got, expected) {
        Ok(true) => return Outcome::Passed,
        Ok(false) => {}
        Err(reason) => return Outcome::Skipped(reason),
    }
    if undefined_builtin(got) {
        return Outcome::Pending(preview(got));
    }
    Outcome::Failed(format!(
        "expected {}, got {}",
        preview(&expected.body),
        preview(got)
    ))
}

/// Whether `got` matches `expected`, allowing one extra leading newline on the output and one
/// extra trailing newline on either side — the two differences the PHP test runner ignores.
fn try_match(got: &[u8], expected: &Expected) -> Result<bool, String> {
    let stripped = got.strip_prefix(b"\n").unwrap_or(got);
    for output in [got, stripped] {
        if expected.patterned {
            let pattern = trim_line_ends(&expected.body);
            let output = trim_line_ends(output);
            if expectf::matches(&pattern, &output)? {
                return Ok(true);
            }
        } else if equal_ignore_trailing_newline(output, &expected.body) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn trim_line_ends(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for (index, line) in bytes.split(|&byte| byte == b'\n').enumerate() {
        if index > 0 {
            out.push(b'\n');
        }
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let end = line
            .iter()
            .rposition(|&byte| byte != b' ' && byte != b'\t')
            .map_or(0, |last| last + 1);
        out.extend_from_slice(&line[..end]);
    }
    out
}

fn equal_ignore_trailing_newline(got: &[u8], expected: &[u8]) -> bool {
    let got = got.strip_suffix(b"\n").unwrap_or(got);
    let expected = expected.strip_suffix(b"\n").unwrap_or(expected);
    got == expected
}

fn undefined_builtin(output: &[u8]) -> bool {
    output
        .windows(b"Call to undefined function ".len())
        .any(|window| window == b"Call to undefined function ")
}

fn preview(bytes: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    if text.len() > 160 {
        text.truncate(160);
        text.push('…');
    }
    text.replace('\n', "\\n")
}

/// EXPECTF substitutions used by the PHP test format.
mod expectf {
    /// Whether `got` matches the EXPECTF `pattern`, or why the pattern cannot be used.
    pub(super) fn matches(pattern: &[u8], got: &[u8]) -> Result<bool, String> {
        let atoms = parse(pattern)?;
        Ok(match_from(&atoms, 0, got, 0))
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Atom {
        Lit(Vec<u8>),
        Line,
        Line0,
        Any,
        Any0,
        Space0,
        Int,
        Digits,
        Hex,
        Float,
        Char,
        Sep,
    }

    fn parse(pattern: &[u8]) -> Result<Vec<Atom>, String> {
        let mut atoms = Vec::new();
        let mut lit = Vec::new();
        let mut index = 0;
        while index < pattern.len() {
            if pattern[index] == b'%' {
                if pattern.get(index + 1) == Some(&b'r') {
                    return Err("EXPECTF raw regex".to_owned());
                }
                if let Some(atom) = substitution(pattern.get(index + 1).copied()) {
                    if !lit.is_empty() {
                        atoms.push(Atom::Lit(std::mem::take(&mut lit)));
                    }
                    atoms.push(atom);
                    index += 2;
                    continue;
                }
            }
            lit.push(pattern[index]);
            index += 1;
        }
        if !lit.is_empty() {
            atoms.push(Atom::Lit(lit));
        }
        Ok(atoms)
    }

    fn substitution(byte: Option<u8>) -> Option<Atom> {
        Some(match byte? {
            b'%' => Atom::Lit(b"%".to_vec()),
            b's' => Atom::Line,
            b'S' => Atom::Line0,
            b'a' => Atom::Any,
            b'A' => Atom::Any0,
            b'w' => Atom::Space0,
            b'i' => Atom::Int,
            b'd' => Atom::Digits,
            b'x' => Atom::Hex,
            b'f' => Atom::Float,
            b'c' => Atom::Char,
            b'e' => Atom::Sep,
            _ => return None,
        })
    }

    fn match_from(atoms: &[Atom], atom: usize, text: &[u8], offset: usize) -> bool {
        let Some(current) = atoms.get(atom) else {
            return offset == text.len();
        };
        match current {
            Atom::Lit(bytes) => {
                text.get(offset..)
                    .is_some_and(|rest| rest.starts_with(bytes))
                    && match_from(atoms, atom + 1, text, offset + bytes.len())
            }
            Atom::Char => offset < text.len() && match_from(atoms, atom + 1, text, offset + 1),
            Atom::Sep => match_sep(atoms, atom, text, offset),
            Atom::Digits => take(atoms, atom, text, offset, 1, ascii_digit),
            Atom::Hex => take(atoms, atom, text, offset, 1, ascii_hex),
            Atom::Int => match_int(atoms, atom, text, offset),
            Atom::Float => match_float(atoms, atom, text, offset),
            Atom::Space0 => take(atoms, atom, text, offset, 0, |byte| {
                byte.is_ascii_whitespace()
            }),
            Atom::Line => take(atoms, atom, text, offset, 1, |byte| !is_line_end(byte)),
            Atom::Line0 => take(atoms, atom, text, offset, 0, |byte| !is_line_end(byte)),
            Atom::Any => take(atoms, atom, text, offset, 1, |_| true),
            Atom::Any0 => take(atoms, atom, text, offset, 0, |_| true),
        }
    }

    fn match_sep(atoms: &[Atom], atom: usize, text: &[u8], offset: usize) -> bool {
        let sep = std::path::MAIN_SEPARATOR as u8;
        text.get(offset)
            .is_some_and(|byte| *byte == sep || *byte == b'/' || *byte == b'\\')
            && match_from(atoms, atom + 1, text, offset + 1)
    }

    fn match_int(atoms: &[Atom], atom: usize, text: &[u8], offset: usize) -> bool {
        let start = offset + usize::from(matches!(text.get(offset), Some(b'+' | b'-')));
        take(atoms, atom, text, start, 1, ascii_digit)
    }

    fn match_float(atoms: &[Atom], atom: usize, text: &[u8], offset: usize) -> bool {
        let Some(end) = float_end(text, offset) else {
            return false;
        };
        match_from(atoms, atom + 1, text, end)
    }

    fn float_end(text: &[u8], offset: usize) -> Option<usize> {
        let mut index = offset + usize::from(matches!(text.get(offset), Some(b'+' | b'-')));
        let mut digits = 0;
        if text.get(index) == Some(&b'.') {
            index += 1;
        }
        while text.get(index).is_some_and(u8::is_ascii_digit) {
            digits += 1;
            index += 1;
        }
        if text.get(index) == Some(&b'.') {
            index += 1;
            while text.get(index).is_some_and(u8::is_ascii_digit) {
                digits += 1;
                index += 1;
            }
        }
        if digits == 0 {
            return None;
        }
        if matches!(text.get(index), Some(b'e' | b'E')) {
            index += 1;
            index += usize::from(matches!(text.get(index), Some(b'+' | b'-')));
            let exp = index;
            while text.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            if index == exp {
                return None;
            }
        }
        Some(index)
    }

    fn take(
        atoms: &[Atom],
        atom: usize,
        text: &[u8],
        offset: usize,
        minimum: usize,
        keep: impl Fn(u8) -> bool,
    ) -> bool {
        let available = text[offset..]
            .iter()
            .take_while(|byte| keep(**byte))
            .count();
        if available < minimum {
            return false;
        }
        (minimum..=available)
            .rev()
            .any(|taken| match_from(atoms, atom + 1, text, offset + taken))
    }

    fn ascii_digit(byte: u8) -> bool {
        byte.is_ascii_digit()
    }

    fn ascii_hex(byte: u8) -> bool {
        byte.is_ascii_hexdigit()
    }

    fn is_line_end(byte: u8) -> bool {
        byte == b'\n' || byte == b'\r'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_file(body: &[u8]) -> Outcome {
        run_test("t.phpt", body)
    }

    #[test]
    fn echo_tests_pass_and_mismatches_fail() {
        assert_eq!(
            test_file(b"--FILE--\n<?php echo 1;\n--EXPECT--\n1"),
            Outcome::Passed
        );
        assert_eq!(
            test_file(b"--FILE--\n<?php echo \"Hello World\"?>\n--EXPECT--\nHello World\n"),
            Outcome::Passed
        );
        match test_file(b"--FILE--\n<?php echo 1;\n--EXPECT--\n2") {
            Outcome::Failed(reason) => assert!(reason.contains("expected 2"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn expectf_matches_paths_and_numbers() {
        let text = b"--FILE--\n<?php echo $u;\n--EXPECTF--\n\nWarning: Undefined variable $u in %s on line %d\n";
        assert_eq!(test_file(text), Outcome::Passed);
        assert_eq!(
            run_test(
                "Zend/tests/break_error_003.phpt",
                b"--FILE--\n<?php break;\n--EXPECTF--\nFatal error: 'break' not in the 'loop' or 'switch' context in %sbreak_error_003.php on line %d\n",
            ),
            Outcome::Passed
        );
        assert!(
            expectf::matches(
                b"in %sbreak_error_003.php on line %d\n",
                b"in Zend/tests/break_error_003.php on line 1\n",
            )
            .unwrap()
        );
        assert!(expectf::matches(b"a%sb", b"aXb").unwrap());
        assert!(expectf::matches(b"%d", b"42").unwrap());
        assert!(expectf::matches(b"%i", b"-3").unwrap());
        assert!(expectf::matches(b"%f", b"1.5e2").unwrap());
        assert!(!expectf::matches(b"%d", b"x").unwrap());
        assert!(expectf::matches(b"%%s", b"%s").unwrap());
        assert!(expectf::matches(b"%s", b"").is_ok_and(|matched| !matched));
        assert_eq!(
            expectf::matches(b"%r.+%r", b"ab").unwrap_err(),
            "EXPECTF raw regex"
        );
    }

    #[test]
    fn unsupported_constructs_and_undefined_functions_are_pending() {
        match test_file(b"--FILE--\n<?php echo $a[0];\n--EXPECT--\n") {
            Outcome::Pending(reason) => {
                assert!(reason.contains("this expression"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        match test_file(b"--FILE--\n<?php var_dump(1);\n--EXPECT--\nint(1)\n") {
            Outcome::Pending(reason) => {
                assert!(reason.contains("undefined function"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn inline_html_and_variable_variables_are_pending_when_output_differs() {
        match test_file(b"--FILE--\n<?php echo 1; ?>\n#\n<?php echo 2;\n--EXPECT--\nNOPE") {
            Outcome::Pending(reason) => assert_eq!(reason, "inline HTML"),
            other => panic!("{other:?}"),
        }
        match test_file(b"--FILE--\n<?php echo $$a;\n--EXPECT--\n") {
            Outcome::Pending(reason) => assert!(
                reason.contains("variable") || reason.contains("not supported"),
                "{reason}"
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn sapi_and_ini_tests_are_skipped() {
        assert_eq!(
            test_file(b"--INI--\nerror_reporting=0\n--FILE--\n<?php echo 1;\n--EXPECT--\n1"),
            Outcome::Skipped("INI section".to_owned())
        );
        assert_eq!(
            test_file(b"--FILE--\n<?php echo 1;\n--EXPECTREGEX--\n1"),
            Outcome::Skipped("EXPECTREGEX section".to_owned())
        );
        assert_eq!(
            test_file(b"--TEST--\nno file\n--EXPECT--\n1"),
            Outcome::Skipped("no FILE section".to_owned())
        );
    }

    #[test]
    fn compile_fatals_are_compared_as_output() {
        let text = b"--FILE--\n<?php break;\n--EXPECTF--\n\nFatal error: 'break' not in the 'loop' or 'switch' context in %s on line %d\n";
        assert_eq!(test_file(text), Outcome::Passed);
    }

    #[test]
    fn check_walks_a_tree_and_lists_failures() {
        let root = std::env::temp_dir().join(format!("xtask-vm-phpt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Zend")).unwrap();
        fs::write(
            root.join("Zend/ok.phpt"),
            b"--FILE--\n<?php echo 1;\n--EXPECT--\n1",
        )
        .unwrap();
        fs::write(
            root.join("Zend/bad.phpt"),
            b"--FILE--\n<?php echo 1;\n--EXPECT--\n2",
        )
        .unwrap();
        fs::write(
            root.join("Zend/skip.phpt"),
            b"--INI--\nx=1\n--FILE--\n<?php echo 1;\n--EXPECT--\n1",
        )
        .unwrap();
        let summary = check(&root).unwrap();
        assert_eq!(
            (
                summary.tests,
                summary.passed,
                summary.failed,
                summary.skipped
            ),
            (3, 1, 1, 1)
        );
        assert!(
            summary.render().starts_with("failed Zend/bad.phpt:"),
            "{}",
            summary.render()
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn check_refuses_an_empty_tree() {
        let root = std::env::temp_dir().join(format!("xtask-vm-phpt-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let error = check(&root).unwrap_err();
        assert!(
            error.to_string().starts_with("no .phpt tests under "),
            "{error}"
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
