//! Time `example/bench/` on the Rust VM and on a PHP CLI, then print a comparison table.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::example;

/// Directory of loop-heavy scripts, relative to the repository root.
pub(crate) const DIR: &str = "example/bench";

const WARMUPS: u32 = 1;
const SAMPLES: u32 = 5;

/// One timed side of a comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Side {
    pub(crate) elapsed: Duration,
    pub(crate) output: Vec<u8>,
    pub(crate) status: u8,
}

/// Ferropress versus PHP for one script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) name: String,
    pub(crate) ferropress: Side,
    pub(crate) php: Result<Side, String>,
}

/// How PHP is invoked; tests inject a fake.
pub(crate) trait PhpRunner {
    /// Runs `script` and returns what it printed and how long the process took.
    fn run(&mut self, script: &Path) -> Result<Side, String>;
}

/// The `php` binary on `PATH`, with CLI opcache off so each run compiles.
pub(crate) struct SystemPhp {
    program: PathBuf,
}

impl SystemPhp {
    /// `php` on `PATH`, or `None` when it is not installed.
    pub(crate) fn detect() -> Option<Self> {
        let output = Command::new("php").arg("-v").output().ok()?;
        if !output.status.success() {
            return None;
        }
        Some(Self {
            program: PathBuf::from("php"),
        })
    }
}

/// Compile and run `$argv[1]`; write elapsed nanoseconds to stderr (process start is excluded).
const PHP_HARNESS: &str =
    "$t = hrtime(true); include $argv[1]; fwrite(STDERR, (string) (hrtime(true) - $t));";

impl PhpRunner for SystemPhp {
    fn run(&mut self, script: &Path) -> Result<Side, String> {
        let cwd = script.parent().unwrap_or(script);
        let file = script
            .file_name()
            .ok_or_else(|| format!("{} has no file name", script.display()))?;
        let output = Command::new(&self.program)
            .args([
                "-d",
                "display_errors=1",
                "-d",
                "opcache.enable_cli=0",
                "-r",
                PHP_HARNESS,
                "--",
            ])
            .arg(file)
            .current_dir(cwd)
            .output()
            .map_err(|error| format!("php: {error}"))?;
        let status = output.status.code().unwrap_or(255);
        let status = u8::try_from(status).unwrap_or(255);
        let elapsed = parse_hrtime(&output.stderr)?;
        Ok(Side {
            elapsed,
            output: output.stdout,
            status,
        })
    }
}

fn parse_hrtime(stderr: &[u8]) -> Result<Duration, String> {
    let text = String::from_utf8_lossy(stderr);
    let ns: u64 = text.trim().parse().map_err(|error| {
        format!(
            "php harness did not write a nanosecond count ({error}): {}",
            text.trim()
        )
    })?;
    Ok(Duration::from_nanos(ns))
}

/// Every `*.php` directly in `example/bench/`, in name order.
pub(crate) fn list(repo_root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let dir = repo_root.join(DIR);
    let mut scripts = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "php") && path.is_file() {
            scripts.push(path);
        }
    }
    scripts.sort();
    Ok(scripts)
}

/// Resolves a user path from `example/bench/`, then like [`example::resolve`].
pub(crate) fn resolve(repo_root: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return example::resolve(repo_root, path);
    }
    let from_bench = repo_root.join(DIR).join(path);
    if from_bench.is_file() {
        return Ok(from_bench);
    }
    example::resolve(repo_root, path)
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn time_ferropress(script: &Path) -> Result<Side, String> {
    let start = Instant::now();
    let run = example::run_script(script)?;
    Ok(Side {
        elapsed: start.elapsed(),
        output: run.output,
        status: run.status,
    })
}

fn sample_side<F>(mut once: F) -> Result<Side, String>
where
    F: FnMut() -> Result<Side, String>,
{
    for _ in 0..WARMUPS {
        once()?;
    }
    let mut samples = Vec::with_capacity(SAMPLES as usize);
    let mut last = None;
    for _ in 0..SAMPLES {
        let side = once()?;
        samples.push(side.elapsed);
        last = Some(side);
    }
    let mut side = last.ok_or_else(|| "no samples".to_owned())?;
    side.elapsed = median(samples);
    Ok(side)
}

/// Times `scripts` on the VM and on `php`.
pub(crate) fn compare(
    scripts: &[PathBuf],
    mut php: Option<&mut dyn PhpRunner>,
) -> Result<Vec<Row>, String> {
    let mut rows = Vec::with_capacity(scripts.len());
    for script in scripts {
        let name = script.file_name().map_or_else(
            || script.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let ferropress = sample_side(|| time_ferropress(script))?;
        let php = match php.as_mut() {
            Some(runner) => sample_side(|| runner.run(script)),
            None => Err("php not on PATH".into()),
        };
        rows.push(Row {
            name,
            ferropress,
            php,
        });
    }
    Ok(rows)
}

fn millis(duration: Duration) -> String {
    format!("{:.2} ms", duration.as_secs_f64() * 1000.0)
}

fn outputs_match(ferropress: &Side, php: &Side) -> bool {
    ferropress.status == php.status && ferropress.output == php.output
}

/// Comparison table, plus a debug-build warning when timed in `dev`.
pub(crate) fn render(rows: &[Row], php_version: Option<&str>) -> (String, bool) {
    let mut text = String::new();
    if cfg!(debug_assertions) {
        text.push_str(
            "warning: debug build; time with `cargo run --release -p xtask -- example-bench`\n",
        );
    }
    match php_version {
        Some(version) => {
            text.push_str(version.trim());
            text.push('\n');
        }
        None => text.push_str("php not on PATH; Ferropress times only\n"),
    }
    let _ = writeln!(
        text,
        "{:<22} {:>12} {:>12} {:>8} {:>6}",
        "script", "ferropress", "php", "ratio", "match"
    );
    let mut success = true;
    for row in rows {
        if row.ferropress.status != 0 {
            success = false;
        }
        let (php, ratio, matched) = match &row.php {
            Ok(php) => {
                if php.status != 0 {
                    success = false;
                }
                let ratio = if php.elapsed.is_zero() {
                    "n/a".into()
                } else {
                    format!(
                        "{:.2}x",
                        row.ferropress.elapsed.as_secs_f64() / php.elapsed.as_secs_f64()
                    )
                };
                let matched = if outputs_match(&row.ferropress, php) {
                    "yes"
                } else {
                    success = false;
                    "no"
                };
                (millis(php.elapsed), ratio, matched)
            }
            Err(error) => (error.clone(), "-".into(), "-"),
        };
        let _ = writeln!(
            text,
            "{:<22} {:>12} {:>12} {:>8} {:>6}",
            row.name,
            millis(row.ferropress.elapsed),
            php,
            ratio,
            matched
        );
    }
    (text, success)
}

/// First line of `php -v`, if `php` runs.
pub(crate) fn php_version() -> Option<String> {
    let output = Command::new("php").arg("-v").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .next()
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn repo() -> Result<&'static Path, String> {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or_else(|| "xtask lives in the repo".into())
    }

    struct ScriptedPhp {
        side: Side,
    }

    impl PhpRunner for ScriptedPhp {
        fn run(&mut self, _script: &Path) -> Result<Side, String> {
            Ok(self.side.clone())
        }
    }

    #[test]
    fn lists_loop_scripts() -> Result<(), String> {
        let scripts = list(repo()?).map_err(|error| error.to_string())?;
        let names: Vec<String> = scripts
            .iter()
            .filter_map(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect();
        assert!(names.contains(&"arithmetic.php".into()), "{names:?}");
        Ok(())
    }

    #[test]
    fn resolves_a_bare_bench_name() -> Result<(), String> {
        let path = resolve(repo()?, Path::new("arithmetic.php"))?;
        assert!(path.ends_with("example/bench/arithmetic.php"));
        Ok(())
    }

    #[test]
    fn parse_hrtime_reads_nanoseconds() {
        assert_eq!(
            parse_hrtime(b"1500000\n").unwrap(),
            Duration::from_nanos(1_500_000)
        );
        assert!(parse_hrtime(b"not-a-number").is_err());
    }

    #[test]
    fn median_is_the_middle_sample() {
        assert_eq!(
            median(vec![
                Duration::from_millis(3),
                Duration::from_millis(1),
                Duration::from_millis(2),
            ]),
            Duration::from_millis(2)
        );
    }

    #[test]
    fn render_marks_a_mismatch_and_a_missing_php() {
        let row = Row {
            name: "a.php".into(),
            ferropress: Side {
                elapsed: Duration::from_millis(20),
                output: b"1\n".to_vec(),
                status: 0,
            },
            php: Ok(Side {
                elapsed: Duration::from_millis(10),
                output: b"2\n".to_vec(),
                status: 0,
            }),
        };
        let (text, success) = render(&[row], Some("PHP 8.4.19"));
        assert!(!success);
        assert!(text.contains("2.00x"), "{text}");
        assert!(text.contains("no"), "{text}");

        let missing = Row {
            name: "a.php".into(),
            ferropress: Side {
                elapsed: Duration::from_millis(1),
                output: b"1\n".to_vec(),
                status: 0,
            },
            php: Err("php not on PATH".into()),
        };
        let (text, success) = render(&[missing], None);
        assert!(success);
        assert!(text.contains("php not on PATH"), "{text}");
    }

    #[test]
    fn compare_uses_the_injected_php() -> Result<(), String> {
        let script = repo()?.join("example/hello.php");
        let mut php = ScriptedPhp {
            side: Side {
                elapsed: Duration::from_millis(1),
                output: b"Hello from Ferropress\n2 + 3 = 5\nREADY\n".to_vec(),
                status: 0,
            },
        };
        let rows = compare(&[script], Some(&mut php))?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ferropress.status, 0);
        assert_eq!(rows[0].ferropress.output, php.side.output);
        assert!(outputs_match(&rows[0].ferropress, rows[0].php.as_ref()?));
        Ok(())
    }
}
