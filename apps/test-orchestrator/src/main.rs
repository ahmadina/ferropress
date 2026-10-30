//! `test-orchestrator`: compare Ferropress plugin load to the recorded lab lifecycle.

use std::env;
use std::path::Path;
use std::process::ExitCode;

use test_orchestrator::{describe, known_ids, run_scenario};

fn repo_root() -> &'static Path {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(crate_dir)
}

fn main() -> ExitCode {
    let requested: Vec<String> = env::args().skip(1).collect();
    let ids: Vec<String> = if requested.is_empty() {
        known_ids().map(ToOwned::to_owned).collect()
    } else {
        requested
    };
    let mut failed = false;
    let mut lines = Vec::new();
    for id in ids {
        match run_scenario(repo_root(), &id) {
            Ok(report) => {
                let line = describe(&report);
                if !report.passed() {
                    failed = true;
                }
                lines.push(line);
            }
            Err(error) => {
                failed = true;
                lines.push(format!("{id}: {error}"));
            }
        }
    }
    let text = lines.join("\n");
    if failed {
        print_err(&text);
        ExitCode::FAILURE
    } else {
        print_out(&text);
        ExitCode::SUCCESS
    }
}

#[expect(
    clippy::print_stdout,
    reason = "the orchestrator is a CLI; stdout is its user interface"
)]
fn print_out(text: &str) {
    println!("{text}");
}

#[expect(
    clippy::print_stderr,
    reason = "the orchestrator is a CLI; stderr carries failures"
)]
fn print_err(text: &str) {
    eprintln!("{text}");
}
