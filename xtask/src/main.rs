//! Repository automation for Ferropress, invoked as `cargo xtask <command>`.

mod layers;
mod workspace;

use std::fmt;
use std::process::ExitCode;

use layers::Finding;

const USAGE: &str = "usage: cargo xtask <command>

commands:
  check-layers  verify that crate dependencies follow the workspace layer rules
  help          show this message";

/// Exit status for invalid command-line usage.
const USAGE_EXIT_CODE: u8 = 2;

#[derive(Debug, PartialEq, Eq)]
enum Command {
    CheckLayers,
    Help,
}

#[derive(Debug, PartialEq, Eq)]
enum UsageError {
    MissingCommand,
    UnknownCommand(String),
    UnexpectedArgument(String),
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCommand => f.write_str("no command given"),
            Self::UnknownCommand(command) => write!(f, "unknown command `{command}`"),
            Self::UnexpectedArgument(argument) => write!(f, "unexpected argument `{argument}`"),
        }
    }
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, UsageError> {
    let mut args = args.into_iter();
    let command = match args.next().as_deref() {
        None => return Err(UsageError::MissingCommand),
        Some("check-layers") => Command::CheckLayers,
        Some("help" | "-h" | "--help") => Command::Help,
        Some(other) => return Err(UsageError::UnknownCommand(other.to_owned())),
    };
    match args.next() {
        Some(extra) => Err(UsageError::UnexpectedArgument(extra)),
        None => Ok(command),
    }
}

/// Outcome of a command: the text to show and whether it succeeded.
#[derive(Debug, PartialEq, Eq)]
struct Report {
    text: String,
    success: bool,
}

fn layer_report(member_count: usize, findings: &[Finding]) -> Report {
    if findings.is_empty() {
        return Report {
            text: format!("layer check passed: {member_count} crate(s), no violations"),
            success: true,
        };
    }
    let mut text = format!("layer check failed: {} violation(s)", findings.len());
    for finding in findings {
        text.push_str("\n  - ");
        text.push_str(&finding.to_string());
    }
    Report {
        text,
        success: false,
    }
}

fn check_layers() -> anyhow::Result<Report> {
    let members = workspace::load()?;
    let findings = layers::check(&members);
    Ok(layer_report(members.len(), &findings))
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            print_out(USAGE);
            ExitCode::SUCCESS
        }
        Ok(Command::CheckLayers) => match check_layers() {
            Ok(report) if report.success => {
                print_out(&report.text);
                ExitCode::SUCCESS
            }
            Ok(report) => {
                print_err(&report.text);
                ExitCode::FAILURE
            }
            Err(error) => {
                print_err(&format!("error: {error:#}"));
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            print_err(&format!("error: {error}\n\n{USAGE}"));
            ExitCode::from(USAGE_EXIT_CODE)
        }
    }
}

#[expect(
    clippy::print_stdout,
    reason = "xtask is a CLI; stdout is its user interface"
)]
fn print_out(text: &str) {
    println!("{text}");
}

#[expect(
    clippy::print_stderr,
    reason = "xtask is a CLI; stderr carries its diagnostics"
)]
fn print_err(text: &str) {
    eprintln!("{text}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::Layer;
    use crate::workspace::DependencyKind;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|&arg| arg.to_owned()).collect()
    }

    #[test]
    fn parses_known_commands() {
        assert_eq!(parse(args(&["check-layers"])), Ok(Command::CheckLayers));
        for help in ["help", "-h", "--help"] {
            assert_eq!(parse(args(&[help])), Ok(Command::Help), "{help}");
        }
    }

    #[test]
    fn rejects_missing_unknown_and_extra_arguments() {
        assert_eq!(parse(args(&[])), Err(UsageError::MissingCommand));
        assert_eq!(
            parse(args(&["build"])),
            Err(UsageError::UnknownCommand("build".into()))
        );
        assert_eq!(
            parse(args(&["check-layers", "--fix"])),
            Err(UsageError::UnexpectedArgument("--fix".into()))
        );
    }

    #[test]
    fn usage_errors_render_readably() {
        assert_eq!(UsageError::MissingCommand.to_string(), "no command given");
        assert_eq!(
            UsageError::UnknownCommand("x".into()).to_string(),
            "unknown command `x`"
        );
        assert_eq!(
            UsageError::UnexpectedArgument("y".into()).to_string(),
            "unexpected argument `y`"
        );
    }

    #[test]
    fn report_passes_without_findings() {
        assert_eq!(
            layer_report(3, &[]),
            Report {
                text: "layer check passed: 3 crate(s), no violations".into(),
                success: true,
            }
        );
    }

    #[test]
    fn report_lists_every_finding() {
        let findings = [
            Finding::Unclassified {
                krate: "utils".into(),
            },
            Finding::ForbiddenDependency {
                from: "cms-hooks".into(),
                from_layer: Layer::Domain,
                to: "infra-mysql".into(),
                to_layer: Layer::Infrastructure,
                kind: DependencyKind::Normal,
            },
        ];
        let report = layer_report(2, &findings);
        assert!(!report.success);
        let lines: Vec<&str> = report.text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "layer check failed: 2 violation(s)");
        assert!(lines[1].starts_with("  - crate `utils` fits no layer"));
        assert!(lines[2].starts_with("  - `cms-hooks` (domain) must not depend on `infra-mysql`"));
    }

    #[test]
    fn check_layers_passes_on_this_workspace() {
        let report = check_layers().unwrap();
        assert!(report.success, "{}", report.text);
    }
}
