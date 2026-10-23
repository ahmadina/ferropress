//! Lifecycle of the reference lab stack (`lab/compose.yaml`), driven by pinned manifest values.

use std::fmt;
use std::io;
use std::path::Path;
use std::process::Command;

use crate::manifest::Manifest;

/// Compose file location, relative to the repository root.
pub(crate) const COMPOSE_FILE: &str = "lab/compose.yaml";
/// Address the lab application is published on.
pub(crate) const APP_URL: &str = "http://127.0.0.1:8080/";

const PHP_IMAGE_ID: &str = "php-runtime";
const DATABASE_IMAGE_ID: &str = "database";
const PLATFORM_ARCHIVE_ID: &str = "platform-release";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LabAction {
    /// Build and start the stack, waiting until every service is healthy.
    Up,
    /// Stop the stack, keeping the database volume.
    Down,
    /// Stop the stack, delete the database volume and start it again from scratch.
    Reset,
}

impl LabAction {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Reset => "reset",
        }
    }
}

/// One external program invocation.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub(crate) program: &'static str,
    pub(crate) args: Vec<&'static str>,
    pub(crate) env: Vec<(&'static str, String)>,
}

impl fmt::Display for Invocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.program)?;
        for arg in &self.args {
            write!(f, " {arg}")?;
        }
        Ok(())
    }
}

/// A manifest entry the lab needs but cannot find.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MissingEntry {
    kind: &'static str,
    id: &'static str,
}

impl fmt::Display for MissingEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the manifest has no {} with id `{}`", self.kind, self.id)
    }
}

impl std::error::Error for MissingEntry {}

fn lab_env(manifest: &Manifest) -> Result<Vec<(&'static str, String)>, MissingEntry> {
    let image = |id: &'static str| {
        manifest
            .image
            .iter()
            .find(|image| image.id == id)
            .map(|image| format!("{}@{}", image.reference, image.digest))
            .ok_or(MissingEntry { kind: "image", id })
    };
    let archive = manifest
        .archive
        .iter()
        .find(|archive| archive.id == PLATFORM_ARCHIVE_ID)
        .ok_or(MissingEntry {
            kind: "archive",
            id: PLATFORM_ARCHIVE_ID,
        })?;
    Ok(vec![
        ("LAB_PHP_IMAGE", image(PHP_IMAGE_ID)?),
        ("LAB_DB_IMAGE", image(DATABASE_IMAGE_ID)?),
        ("LAB_PLATFORM_URL", archive.url.clone()),
        ("LAB_PLATFORM_SHA256", archive.sha256.clone()),
    ])
}

/// The `docker compose` invocations that perform `action`, in order.
pub(crate) fn plan(
    action: LabAction,
    manifest: &Manifest,
) -> Result<Vec<Invocation>, MissingEntry> {
    let env = lab_env(manifest)?;
    let compose = |subcommand: &[&'static str]| {
        let mut args = vec!["compose", "--file", COMPOSE_FILE];
        args.extend_from_slice(subcommand);
        Invocation {
            program: "docker",
            args,
            env: env.clone(),
        }
    };
    let up = || compose(&["up", "--build", "--detach", "--wait"]);
    Ok(match action {
        LabAction::Up => vec![up()],
        LabAction::Down => vec![compose(&["down"])],
        LabAction::Reset => vec![compose(&["down", "--volumes"]), up()],
    })
}

/// Executes invocations; abstracted so the sequencing can be tested without Docker.
pub(crate) trait Runner {
    /// Runs one invocation and reports whether it exited successfully.
    fn run(&mut self, invocation: &Invocation) -> io::Result<bool>;
}

/// Runs invocations as child processes in the repository root, inheriting stdout and stderr.
pub(crate) struct SystemRunner<'a> {
    pub(crate) repo_root: &'a Path,
}

impl Runner for SystemRunner<'_> {
    fn run(&mut self, invocation: &Invocation) -> io::Result<bool> {
        Command::new(invocation.program)
            .args(&invocation.args)
            .envs(invocation.env.iter().map(|(key, value)| (key, value)))
            .current_dir(self.repo_root)
            .status()
            .map(|status| status.success())
    }
}

/// Runs the plan in order and stops at the first failure.
pub(crate) fn execute(plan: &[Invocation], runner: &mut impl Runner) -> anyhow::Result<()> {
    for invocation in plan {
        let succeeded = runner.run(invocation).map_err(|error| {
            anyhow::anyhow!(
                "could not run `{invocation}` ({error}); is Docker installed and running?"
            )
        })?;
        anyhow::ensure!(succeeded, "`{invocation}` failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse;

    const DIGEST: &str = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn manifest() -> Manifest {
        parse(&format!(
            r#"schema = 1

[[archive]]
id = "platform-release"
version = "1.0"
url = "https://example.org/release.tar.gz"
sha256 = "{hash}"
file = "lab/packages/release.tar.gz"

[[image]]
id = "php-runtime"
reference = "docker.io/library/php:1-apache"
digest = "{DIGEST}"

[[image]]
id = "database"
reference = "docker.io/library/mysql:1"
digest = "{DIGEST}"
"#,
            hash = DIGEST.trim_start_matches("sha256:")
        ))
        .unwrap()
    }

    fn rendered(plan: &[Invocation]) -> Vec<String> {
        plan.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn env_carries_pinned_values() {
        let plan = plan(LabAction::Up, &manifest()).unwrap();
        assert_eq!(
            plan[0].env,
            [
                (
                    "LAB_PHP_IMAGE",
                    format!("docker.io/library/php:1-apache@{DIGEST}")
                ),
                (
                    "LAB_DB_IMAGE",
                    format!("docker.io/library/mysql:1@{DIGEST}")
                ),
                (
                    "LAB_PLATFORM_URL",
                    "https://example.org/release.tar.gz".to_owned()
                ),
                (
                    "LAB_PLATFORM_SHA256",
                    DIGEST.trim_start_matches("sha256:").to_owned()
                ),
            ]
        );
    }

    #[test]
    fn actions_map_to_compose_invocations() {
        let manifest = manifest();
        assert_eq!(
            rendered(&plan(LabAction::Up, &manifest).unwrap()),
            ["docker compose --file lab/compose.yaml up --build --detach --wait"]
        );
        assert_eq!(
            rendered(&plan(LabAction::Down, &manifest).unwrap()),
            ["docker compose --file lab/compose.yaml down"]
        );
        assert_eq!(
            rendered(&plan(LabAction::Reset, &manifest).unwrap()),
            [
                "docker compose --file lab/compose.yaml down --volumes",
                "docker compose --file lab/compose.yaml up --build --detach --wait",
            ]
        );
    }

    #[test]
    fn action_names() {
        assert_eq!(
            [LabAction::Up, LabAction::Down, LabAction::Reset].map(LabAction::name),
            ["up", "down", "reset"]
        );
    }

    #[test]
    fn missing_entries_are_reported() {
        let mut without_archive = manifest();
        without_archive.archive.clear();
        let error = plan(LabAction::Up, &without_archive).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the manifest has no archive with id `platform-release`"
        );

        let mut without_php = manifest();
        without_php.image.retain(|image| image.id != PHP_IMAGE_ID);
        assert_eq!(
            plan(LabAction::Up, &without_php).unwrap_err(),
            MissingEntry {
                kind: "image",
                id: PHP_IMAGE_ID
            }
        );

        let mut without_database = manifest();
        without_database
            .image
            .retain(|image| image.id != DATABASE_IMAGE_ID);
        assert_eq!(
            plan(LabAction::Down, &without_database).unwrap_err(),
            MissingEntry {
                kind: "image",
                id: DATABASE_IMAGE_ID
            }
        );
    }

    #[test]
    fn committed_manifest_has_every_lab_entry() {
        let manifest = parse(include_str!("../../contracts/reference-manifest.toml")).unwrap();
        for action in [LabAction::Up, LabAction::Down, LabAction::Reset] {
            assert!(plan(action, &manifest).is_ok(), "{}", action.name());
        }
    }

    /// Records invocations and answers with scripted outcomes.
    struct ScriptedRunner {
        outcomes: Vec<io::Result<bool>>,
        seen: Vec<String>,
    }

    impl Runner for ScriptedRunner {
        fn run(&mut self, invocation: &Invocation) -> io::Result<bool> {
            self.seen.push(invocation.to_string());
            self.outcomes.remove(0)
        }
    }

    fn scripted(outcomes: Vec<io::Result<bool>>) -> ScriptedRunner {
        ScriptedRunner {
            outcomes,
            seen: Vec::new(),
        }
    }

    #[test]
    fn execute_runs_every_step_in_order() {
        let plan = plan(LabAction::Reset, &manifest()).unwrap();
        let mut runner = scripted(vec![Ok(true), Ok(true)]);
        execute(&plan, &mut runner).unwrap();
        assert_eq!(runner.seen, rendered(&plan));
    }

    #[test]
    fn execute_stops_at_the_first_failed_step() {
        let plan = plan(LabAction::Reset, &manifest()).unwrap();
        let mut runner = scripted(vec![Ok(false)]);
        let error = execute(&plan, &mut runner).unwrap_err();
        assert_eq!(
            error.to_string(),
            "`docker compose --file lab/compose.yaml down --volumes` failed"
        );
        assert_eq!(runner.seen.len(), 1);
    }

    #[test]
    fn execute_explains_spawn_failures() {
        let plan = plan(LabAction::Up, &manifest()).unwrap();
        let mut runner = scripted(vec![Err(io::ErrorKind::NotFound.into())]);
        let error = execute(&plan, &mut runner).unwrap_err().to_string();
        assert!(
            error.starts_with("could not run `docker compose"),
            "{error}"
        );
        assert!(
            error.ends_with("is Docker installed and running?"),
            "{error}"
        );
    }

    #[test]
    fn system_runner_reports_exit_status_and_spawn_errors() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut runner = SystemRunner { repo_root };
        let invocation = |program, args| Invocation {
            program,
            args,
            env: vec![("LAB_PROBE", "1".to_owned())],
        };
        let same = vec!["diff", "--quiet", "--no-index", "Cargo.toml", "Cargo.toml"];
        let different = vec!["diff", "--quiet", "--no-index", "Cargo.toml", "src/main.rs"];
        assert!(runner.run(&invocation("git", same)).unwrap());
        assert!(!runner.run(&invocation("git", different)).unwrap());
        assert!(
            runner
                .run(&invocation("ferropress-missing-program", vec![]))
                .is_err()
        );
    }
}
