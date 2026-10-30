//! Run PHP scripts from `example/` on the Rust VM.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use php_compiler::compile;
use php_parser::{ParseOptions, parse};

/// Directory under the repository root that holds the scripts.
pub(crate) const DIR: &str = "example";

/// A script that ran, or why it did not.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Run {
    /// Path shown to the user, relative to the repository when possible.
    pub(crate) name: String,
    /// Bytes the script printed, including warnings and uncaught errors.
    pub(crate) output: Vec<u8>,
    /// 0 when the script finished, 255 after an uncaught error.
    pub(crate) status: u8,
}

/// Every `*.php` directly in `example/`, in name order.
pub(crate) fn list(repo_root: &Path) -> io::Result<Vec<PathBuf>> {
    let dir = repo_root.join(DIR);
    let mut scripts = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "php") && path.is_file() {
            scripts.push(path);
        }
    }
    scripts.sort();
    Ok(scripts)
}

/// Resolves a user path: absolute, from the repo root, or from `example/`.
pub(crate) fn resolve(repo_root: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return existing(path);
    }
    let from_root = repo_root.join(path);
    if from_root.is_file() {
        return Ok(from_root);
    }
    let from_example = repo_root.join(DIR).join(path);
    if from_example.is_file() {
        return Ok(from_example);
    }
    Err(format!("example not found: {}", path.display()))
}

fn existing(path: &Path) -> Result<PathBuf, String> {
    if path.is_file() {
        Ok(path.to_path_buf())
    } else {
        Err(format!("example not found: {}", path.display()))
    }
}

/// Parses, compiles and runs `script` on the VM.
pub(crate) fn run_script(script: &Path) -> Result<Run, String> {
    let source = fs::read(script).map_err(|error| format!("{}: {error}", script.display()))?;
    let file = path_bytes(script);
    let name = script.file_name().map_or_else(
        || script.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let parsed = parse(&source, ParseOptions::default()).map_err(|error| error.to_string())?;
    let program = parsed.result.map_err(|error| error.to_string())?;
    let compiled = compile(&program, &file).map_err(|error| error.to_string())?;
    if let Some(fatal) = compiled.diagnostics.fatal {
        return Err(format!(
            "{name}: {}",
            String::from_utf8_lossy(&fatal.message)
        ));
    }
    let unit = compiled
        .unit
        .ok_or_else(|| format!("{name}: compile produced no unit"))?;
    let mut output = Vec::new();
    for warning in &compiled.diagnostics.warnings {
        output.extend_from_slice(warning.severity.label().as_bytes());
        output.extend_from_slice(b": ");
        output.extend_from_slice(&warning.message);
        output.push(b'\n');
    }
    let status = php_vm::run(&unit, &file, &mut output).map_err(|error| error.to_string())?;
    Ok(Run {
        name,
        output,
        status,
    })
}

fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

/// Formats one or more runs the way the CLI prints them.
pub(crate) fn render(runs: &[Run]) -> (String, bool) {
    let mut text = String::new();
    let mut success = true;
    for (index, run) in runs.iter().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        if runs.len() > 1 {
            text.push_str("== ");
            text.push_str(&run.name);
            text.push_str(" ==\n");
        }
        text.push_str(&String::from_utf8_lossy(&run.output));
        if run.status != 0 {
            success = false;
        }
    }
    (text, success)
}

#[cfg(test)]
mod tests {
    use super::{Run, list, render, resolve, run_script};
    use std::path::Path;

    fn repo() -> Result<&'static Path, String> {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or_else(|| "xtask lives in the repo".into())
    }

    #[test]
    fn lists_top_level_php_scripts() -> Result<(), String> {
        let scripts = list(repo()?).map_err(|error| error.to_string())?;
        let names: Vec<String> = scripts
            .iter()
            .filter_map(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect();
        assert!(names.contains(&"hello.php".into()), "{names:?}");
        assert!(
            !names.iter().any(|name| name == "greet.php"),
            "lib scripts must not be listed: {names:?}"
        );
        Ok(())
    }

    #[test]
    fn resolves_a_bare_script_name() -> Result<(), String> {
        let path = resolve(repo()?, Path::new("hello.php"))?;
        assert!(path.ends_with("example/hello.php"));
        Ok(())
    }

    #[test]
    fn a_missing_script_is_named() -> Result<(), String> {
        let error = match resolve(repo()?, Path::new("missing.php")) {
            Err(error) => error,
            Ok(path) => return Err(format!("expected missing, got {}", path.display())),
        };
        assert!(error.contains("example not found"), "{error}");
        Ok(())
    }

    #[test]
    fn hello_prints_the_greeting() -> Result<(), String> {
        let run = run_script(&repo()?.join("example/hello.php"))?;
        assert_eq!(run.status, 0);
        assert_eq!(
            String::from_utf8_lossy(&run.output),
            "Hello from Ferropress\n2 + 3 = 5\nREADY\n"
        );
        Ok(())
    }

    #[test]
    fn every_top_level_example_finishes() -> Result<(), String> {
        let scripts = list(repo()?).map_err(|error| error.to_string())?;
        assert!(!scripts.is_empty());
        for script in scripts {
            let run = run_script(&script)?;
            assert_eq!(
                run.status,
                0,
                "{}:\n{}",
                run.name,
                String::from_utf8_lossy(&run.output)
            );
        }
        Ok(())
    }

    #[test]
    fn several_runs_are_headed() {
        let (text, success) = render(&[
            Run {
                name: "a.php".into(),
                output: b"one\n".to_vec(),
                status: 0,
            },
            Run {
                name: "b.php".into(),
                output: b"two\n".to_vec(),
                status: 0,
            },
        ]);
        assert!(success);
        assert_eq!(text, "== a.php ==\none\n\n== b.php ==\ntwo\n");
    }
}
