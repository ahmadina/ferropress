//! Repository automation for Ferropress, invoked as `cargo xtask <command>`.

mod capture;
mod cli;
mod dbdiff;
mod fetch;
mod lab;
mod layers;
mod manifest;
mod process;
mod workspace;

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use anyhow::Context as _;
use capture::{HttpClient, UreqClient};
use clap::Parser as _;
use cli::{Cli, Command, LabCommand};
use lab::LabAction;
use layers::Finding;
use manifest::{ArchiveStatus, Manifest, Problem};
use process::{Runner, SystemRunner};
use test_harness::{RequestPath, Slug};

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

fn manifest_report(
    manifest: &Manifest,
    problems: &[Problem],
    archives: &[(&str, ArchiveStatus)],
) -> Report {
    if !problems.is_empty() {
        let mut text = format!("manifest invalid: {} problem(s)", problems.len());
        for problem in problems {
            text.push_str("\n  - ");
            text.push_str(&problem.to_string());
        }
        return Report {
            text,
            success: false,
        };
    }
    let mut success = true;
    let mut text = format!(
        "manifest valid: {} source(s), {} archive(s), {} image(s)",
        manifest.source.len(),
        manifest.archive.len(),
        manifest.image.len()
    );
    for (id, status) in archives {
        let line = match status {
            ArchiveStatus::Verified => format!("archive `{id}`: sha256 verified"),
            ArchiveStatus::NotFetched => format!("archive `{id}`: not downloaded"),
            ArchiveStatus::Mismatch { actual } => {
                success = false;
                format!("archive `{id}`: sha256 mismatch, file hashes to {actual}")
            }
        };
        text.push_str("\n  - ");
        text.push_str(&line);
    }
    Report { text, success }
}

fn load_manifest(repo_root: &Path) -> anyhow::Result<Manifest> {
    let path = repo_root.join(manifest::MANIFEST_PATH);
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    manifest::parse(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Loads the manifest; a manifest with validation problems yields the failing report instead.
fn load_valid_manifest(repo_root: &Path) -> anyhow::Result<Result<Manifest, Report>> {
    let manifest = load_manifest(repo_root)?;
    let problems = manifest::validate(&manifest);
    if problems.is_empty() {
        Ok(Ok(manifest))
    } else {
        Ok(Err(manifest_report(&manifest, &problems, &[])))
    }
}

fn verify_manifest(repo_root: &Path) -> anyhow::Result<Report> {
    let manifest = load_manifest(repo_root)?;
    let problems = manifest::validate(&manifest);
    let mut archives = Vec::new();
    if problems.is_empty() {
        for archive in &manifest.archive {
            let status = manifest::verify_archive(repo_root, archive)
                .with_context(|| format!("hashing archive `{}`", archive.id))?;
            archives.push((archive.id.as_str(), status));
        }
    }
    Ok(manifest_report(&manifest, &problems, &archives))
}

fn run_lab(
    repo_root: &Path,
    action: LabAction,
    runner: &mut impl Runner,
) -> anyhow::Result<Report> {
    let manifest = match load_valid_manifest(repo_root)? {
        Ok(manifest) => manifest,
        Err(report) => return Ok(report),
    };
    let plan = lab::plan(action, &manifest)?;
    process::run_all(runner, &plan)?;
    let text = match action {
        LabAction::Up | LabAction::Reset => format!(
            "lab {}: stack is healthy at {}",
            action.name(),
            lab::APP_URL
        ),
        LabAction::Down => "lab down: stack stopped".to_owned(),
        LabAction::Install => format!("lab install: site is installed at {}", lab::APP_URL),
    };
    Ok(Report {
        text,
        success: true,
    })
}

fn run_lab_fetch(repo_root: &Path, runner: &mut impl Runner) -> anyhow::Result<Report> {
    let manifest = match load_valid_manifest(repo_root)? {
        Ok(manifest) => manifest,
        Err(report) => return Ok(report),
    };
    let lines = fetch::fetch(repo_root, &manifest, runner)?;
    let mut text = format!("lab fetch: {} entr(ies) at pinned versions", lines.len());
    for line in &lines {
        text.push_str("\n  - ");
        text.push_str(line);
    }
    Ok(Report {
        text,
        success: true,
    })
}

fn run_lab_capture(
    repo_root: &Path,
    name: &Slug,
    path: &RequestPath,
    runner: &mut impl Runner,
    http: &mut impl HttpClient,
) -> anyhow::Result<Report> {
    let manifest = match load_valid_manifest(repo_root)? {
        Ok(manifest) => manifest,
        Err(report) => return Ok(report),
    };
    let env = lab::lab_env(&manifest)?;
    let capture = capture::capture(&env, name, path, runner, http)?;
    let relative = format!("{}/{name}.json", capture::CAPTURE_DIR);
    let output = repo_root.join(&relative);
    let dir = repo_root.join(capture::CAPTURE_DIR);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let json = serde_json::to_vec_pretty(&capture).context("serializing the capture")?;
    fs::write(&output, json).with_context(|| format!("writing {}", output.display()))?;
    Ok(Report {
        text: format!("lab capture: wrote {relative} ({})", capture.summary()),
        success: true,
    })
}

/// The repository root: the parent of this crate's directory.
fn repo_root() -> &'static Path {
    let xtask_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask_dir.parent().unwrap_or(xtask_dir)
}

fn system_runner() -> SystemRunner<'static> {
    SystemRunner {
        repo_root: repo_root(),
    }
}

fn run(command: &Command) -> anyhow::Result<Report> {
    match command {
        Command::CheckLayers => check_layers(),
        Command::VerifyManifest => verify_manifest(repo_root()),
        Command::Lab(lab) => {
            let action = match lab {
                LabCommand::Up => LabAction::Up,
                LabCommand::Down => LabAction::Down,
                LabCommand::Reset => LabAction::Reset,
                LabCommand::Install => LabAction::Install,
                LabCommand::Fetch => return run_lab_fetch(repo_root(), &mut system_runner()),
                LabCommand::Capture { name, path } => {
                    return run_lab_capture(
                        repo_root(),
                        name,
                        path,
                        &mut system_runner(),
                        &mut UreqClient::new(),
                    );
                }
            };
            run_lab(repo_root(), action, &mut system_runner())
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli.command) {
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
    use crate::process::testing::FakeRunner;
    use crate::workspace::DependencyKind;

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

    #[test]
    fn repo_root_contains_the_manifest() {
        assert!(repo_root().join(manifest::MANIFEST_PATH).is_file());
    }

    #[test]
    fn committed_manifest_verifies() {
        let report = run(&Command::VerifyManifest).unwrap();
        assert!(report.success, "{}", report.text);
        assert!(
            report.text.starts_with("manifest valid:"),
            "{}",
            report.text
        );
    }

    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn manifest_text(schema: u32) -> String {
        format!(
            r#"schema = {schema}

[[archive]]
id = "release"
version = "1.0"
url = "https://example.org/release.tar.gz"
sha256 = "{ABC_SHA256}"
file = "lab/packages/release.tar.gz"
"#
        )
    }

    struct TempRepo(std::path::PathBuf);

    impl TempRepo {
        fn new(name: &str, manifest: Option<&str>) -> Self {
            let path =
                std::env::temp_dir().join(format!("xtask-main-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join("contracts")).unwrap();
            fs::create_dir_all(path.join("lab/packages")).unwrap();
            if let Some(text) = manifest {
                fs::write(path.join(manifest::MANIFEST_PATH), text).unwrap();
            }
            Self(path)
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn verify_manifest_reports_archive_states() {
        let repo = TempRepo::new("states", Some(&manifest_text(1)));
        let report = verify_manifest(&repo.0).unwrap();
        assert!(report.success);
        assert_eq!(
            report.text,
            "manifest valid: 0 source(s), 1 archive(s), 0 image(s)\n  - archive `release`: not downloaded"
        );

        let archive = repo.0.join("lab/packages/release.tar.gz");
        fs::write(&archive, b"abc").unwrap();
        let report = verify_manifest(&repo.0).unwrap();
        assert!(report.success);
        assert!(report.text.ends_with("archive `release`: sha256 verified"));

        fs::write(&archive, b"tampered").unwrap();
        let report = verify_manifest(&repo.0).unwrap();
        assert!(!report.success);
        assert!(
            report
                .text
                .contains("archive `release`: sha256 mismatch, file hashes to ")
        );
    }

    #[test]
    fn verify_manifest_lists_problems_without_hashing() {
        let repo = TempRepo::new("invalid", Some(&manifest_text(7)));
        let report = verify_manifest(&repo.0).unwrap();
        assert!(!report.success);
        assert_eq!(
            report.text,
            "manifest invalid: 1 problem(s)\n  - `manifest`.schema: schema 7 is not supported (expected 1)"
        );
    }

    #[test]
    fn verify_manifest_fails_on_missing_or_malformed_file() {
        let missing = TempRepo::new("missing", None);
        let error = verify_manifest(&missing.0).unwrap_err();
        assert!(format!("{error:#}").starts_with("reading "), "{error:#}");

        let malformed = TempRepo::new("malformed", Some("schema = ["));
        let error = verify_manifest(&malformed.0).unwrap_err();
        assert!(format!("{error:#}").starts_with("parsing "), "{error:#}");
    }

    fn lab_manifest_text() -> String {
        let digest = format!("sha256:{ABC_SHA256}");
        format!(
            r#"{archive}
[[image]]
id = "php-runtime"
reference = "php:1-apache"
digest = "{digest}"

[[image]]
id = "database"
reference = "mysql:1"
digest = "{digest}"
"#,
            archive = manifest_text(1).replace("id = \"release\"", "id = \"platform-release\"")
        )
    }

    fn recording(
        succeed: bool,
    ) -> FakeRunner<impl FnMut(&process::Invocation) -> std::io::Result<Option<String>>> {
        FakeRunner::new(move |_: &process::Invocation| Ok(succeed.then(String::new)))
    }

    #[test]
    fn lab_reports_each_action() {
        let repo = TempRepo::new("lab-ok", Some(&lab_manifest_text()));
        let expected = [
            (
                LabAction::Up,
                1,
                "lab up: stack is healthy at http://127.0.0.1:8080/",
            ),
            (LabAction::Down, 1, "lab down: stack stopped"),
            (
                LabAction::Reset,
                3,
                "lab reset: stack is healthy at http://127.0.0.1:8080/",
            ),
            (
                LabAction::Install,
                1,
                "lab install: site is installed at http://127.0.0.1:8080/",
            ),
        ];
        for (action, calls, text) in expected {
            let mut runner = recording(true);
            let report = run_lab(&repo.0, action, &mut runner).unwrap();
            assert_eq!(report.text, text);
            assert!(report.success);
            assert_eq!(runner.seen.len(), calls);
        }
    }

    #[test]
    fn lab_refuses_an_invalid_manifest_without_running_anything() {
        let repo = TempRepo::new("lab-invalid", Some(&manifest_text(3)));
        let mut runner = recording(true);
        let report = run_lab(&repo.0, LabAction::Up, &mut runner).unwrap();
        assert!(!report.success);
        assert!(report.text.starts_with("manifest invalid:"));
        assert_eq!(runner.seen.len(), 0);
    }

    #[test]
    fn lab_propagates_missing_entries_and_failures() {
        let incomplete = TempRepo::new("lab-incomplete", Some(&manifest_text(1)));
        let error = run_lab(&incomplete.0, LabAction::Up, &mut recording(true)).unwrap_err();
        assert!(
            error.to_string().starts_with("the manifest has no"),
            "{error}"
        );

        let repo = TempRepo::new("lab-fail", Some(&lab_manifest_text()));
        let error = run_lab(&repo.0, LabAction::Down, &mut recording(false)).unwrap_err();
        assert!(error.to_string().ends_with("down` failed"), "{error}");

        let missing = TempRepo::new("lab-missing", None);
        assert!(run_lab(&missing.0, LabAction::Up, &mut recording(true)).is_err());
    }

    #[test]
    fn lab_fetch_reports_each_entry() {
        let repo = TempRepo::new("fetch-ok", Some(&manifest_text(1)));
        let root = repo.0.clone();
        let mut runner = FakeRunner::new(move |invocation: &process::Invocation| {
            let output = &invocation.args[invocation.args.len() - 2];
            fs::write(root.join(output), b"abc").map(|()| Some(String::new()))
        });
        let report = run_lab_fetch(&repo.0, &mut runner).unwrap();
        assert!(report.success);
        assert_eq!(
            report.text,
            "lab fetch: 1 entr(ies) at pinned versions\n  - archive `release`: downloaded and verified"
        );
    }

    #[test]
    fn lab_fetch_refuses_invalid_manifest_and_propagates_failures() {
        let invalid = TempRepo::new("fetch-invalid", Some(&manifest_text(4)));
        let mut runner = recording(true);
        let report = run_lab_fetch(&invalid.0, &mut runner).unwrap();
        assert!(!report.success);
        assert!(runner.seen.is_empty());

        let repo = TempRepo::new("fetch-fail", Some(&manifest_text(1)));
        let error = run_lab_fetch(&repo.0, &mut recording(false)).unwrap_err();
        assert!(error.to_string().starts_with("`curl --fail"), "{error}");

        let missing = TempRepo::new("fetch-missing", None);
        assert!(run_lab_fetch(&missing.0, &mut recording(true)).is_err());
    }

    struct OkHttp;

    impl HttpClient for OkHttp {
        fn get(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> anyhow::Result<capture::HttpResponse> {
            Ok(capture::HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: b"ok".to_vec(),
            })
        }
    }

    fn capture_runner()
    -> FakeRunner<impl FnMut(&process::Invocation) -> std::io::Result<Option<String>>> {
        FakeRunner::new(|invocation: &process::Invocation| {
            let args = invocation.args.join(" ");
            Ok(Some(if args.contains(" cat ") {
                r#"{"schema":1,"hooks":[],"queries":[],"mail":[],"cron":[]}"#.to_owned()
            } else {
                String::new()
            }))
        })
    }

    #[test]
    fn lab_capture_writes_the_artifact() {
        let repo = TempRepo::new("capture-ok", Some(&lab_manifest_text()));
        let name: Slug = "home".parse().unwrap();
        let path: RequestPath = "/".parse().unwrap();
        let report =
            run_lab_capture(&repo.0, &name, &path, &mut capture_runner(), &mut OkHttp).unwrap();
        assert!(report.success);
        assert_eq!(
            report.text,
            "lab capture: wrote lab/captures/home.json (status 200, 0 hooks, 0 queries, 0 mails, \
             0 cron events, 0 tables changed)"
        );
        let written: serde_json::Value =
            serde_json::from_slice(&fs::read(repo.0.join("lab/captures/home.json")).unwrap())
                .unwrap();
        assert_eq!(written["name"], "home");
        assert_eq!(written["response"]["body"], "ok");
    }

    #[test]
    fn lab_capture_refuses_invalid_manifest_and_propagates_failures() {
        let name: Slug = "home".parse().unwrap();
        let path: RequestPath = "/".parse().unwrap();

        let invalid = TempRepo::new("capture-invalid", Some(&manifest_text(5)));
        let mut runner = capture_runner();
        let report = run_lab_capture(&invalid.0, &name, &path, &mut runner, &mut OkHttp).unwrap();
        assert!(!report.success);
        assert!(runner.seen.is_empty());

        let incomplete = TempRepo::new("capture-incomplete", Some(&manifest_text(1)));
        let error = run_lab_capture(
            &incomplete.0,
            &name,
            &path,
            &mut capture_runner(),
            &mut OkHttp,
        )
        .unwrap_err();
        assert!(
            error.to_string().starts_with("the manifest has no"),
            "{error}"
        );

        let repo = TempRepo::new("capture-fail", Some(&lab_manifest_text()));
        let error =
            run_lab_capture(&repo.0, &name, &path, &mut recording(false), &mut OkHttp).unwrap_err();
        assert_eq!(error.to_string(), "dumping the lab database");

        let blocked = TempRepo::new("capture-blocked", Some(&lab_manifest_text()));
        fs::create_dir_all(blocked.0.join("lab")).unwrap();
        fs::write(blocked.0.join("lab/captures"), b"not a directory").unwrap();
        let error = run_lab_capture(&blocked.0, &name, &path, &mut capture_runner(), &mut OkHttp)
            .unwrap_err();
        assert!(error.to_string().starts_with("creating "), "{error}");
    }

    #[test]
    fn verify_manifest_propagates_hashing_errors() {
        let repo = TempRepo::new("hash-error", Some(&manifest_text(1)));
        fs::create_dir_all(repo.0.join("lab/packages/release.tar.gz")).unwrap();
        let error = verify_manifest(&repo.0).unwrap_err();
        assert!(
            format!("{error:#}").starts_with("hashing archive `release`"),
            "{error:#}"
        );
    }
}
