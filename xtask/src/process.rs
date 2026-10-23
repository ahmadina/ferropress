//! External program invocations behind a small trait, so command sequencing is testable without the
//! programs themselves.

use std::fmt;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// One external program invocation, run from the repository root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub(crate) program: &'static str,
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<(&'static str, String)>,
}

impl Invocation {
    pub(crate) fn new<I, S>(program: &'static str, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program,
            args: args.into_iter().map(Into::into).collect(),
            env: Vec::new(),
        }
    }
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

/// Executes invocations.
pub(crate) trait Runner {
    /// Runs an invocation with inherited output; reports whether it exited successfully.
    fn run(&mut self, invocation: &Invocation) -> io::Result<bool>;

    /// Runs an invocation and returns its stdout, or `None` if it exited unsuccessfully.
    fn capture(&mut self, invocation: &Invocation) -> io::Result<Option<String>>;
}

/// Runs invocations as child processes in the repository root.
pub(crate) struct SystemRunner<'a> {
    pub(crate) repo_root: &'a Path,
}

impl SystemRunner<'_> {
    fn command(&self, invocation: &Invocation) -> Command {
        let mut command = Command::new(invocation.program);
        command
            .args(&invocation.args)
            .envs(invocation.env.iter().map(|(key, value)| (key, value)))
            .current_dir(self.repo_root);
        command
    }
}

impl Runner for SystemRunner<'_> {
    fn run(&mut self, invocation: &Invocation) -> io::Result<bool> {
        self.command(invocation)
            .status()
            .map(|status| status.success())
    }

    fn capture(&mut self, invocation: &Invocation) -> io::Result<Option<String>> {
        let output = self.command(invocation).stderr(Stdio::inherit()).output()?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
    }
}

fn spawn_error(invocation: &Invocation, error: &io::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "could not run `{invocation}` ({error}); is `{}` installed and running?",
        invocation.program
    )
}

/// Runs one invocation and fails unless it exits successfully.
pub(crate) fn run_checked(runner: &mut impl Runner, invocation: &Invocation) -> anyhow::Result<()> {
    let succeeded = runner
        .run(invocation)
        .map_err(|error| spawn_error(invocation, &error))?;
    anyhow::ensure!(succeeded, "`{invocation}` failed");
    Ok(())
}

/// Runs invocations in order and stops at the first failure.
pub(crate) fn run_all(runner: &mut impl Runner, plan: &[Invocation]) -> anyhow::Result<()> {
    plan.iter()
        .try_for_each(|invocation| run_checked(runner, invocation))
}

/// Runs one invocation and returns its trimmed stdout, failing unless it exits successfully.
pub(crate) fn capture_checked(
    runner: &mut impl Runner,
    invocation: &Invocation,
) -> anyhow::Result<String> {
    let output = runner
        .capture(invocation)
        .map_err(|error| spawn_error(invocation, &error))?;
    let stdout = output.ok_or_else(|| anyhow::anyhow!("`{invocation}` failed"))?;
    Ok(stdout.trim().to_owned())
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A runner whose behavior is a closure: `Ok(Some(stdout))` succeeds, `Ok(None)` fails.
    pub(crate) struct FakeRunner<F> {
        pub(crate) respond: F,
        pub(crate) seen: Vec<String>,
    }

    impl<F> FakeRunner<F>
    where
        F: FnMut(&Invocation) -> io::Result<Option<String>>,
    {
        pub(crate) fn new(respond: F) -> Self {
            Self {
                respond,
                seen: Vec::new(),
            }
        }
    }

    impl<F> Runner for FakeRunner<F>
    where
        F: FnMut(&Invocation) -> io::Result<Option<String>>,
    {
        fn run(&mut self, invocation: &Invocation) -> io::Result<bool> {
            self.capture(invocation).map(|output| output.is_some())
        }

        fn capture(&mut self, invocation: &Invocation) -> io::Result<Option<String>> {
            self.seen.push(invocation.to_string());
            (self.respond)(invocation)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::FakeRunner;
    use super::*;

    fn invocations() -> [Invocation; 2] {
        [
            Invocation::new("tool", ["first"]),
            Invocation::new("tool", ["second", "--flag"]),
        ]
    }

    #[test]
    fn invocation_renders_program_and_args() {
        assert_eq!(invocations()[1].to_string(), "tool second --flag");
        assert_eq!(
            Invocation::new("tool", Vec::<String>::new()).to_string(),
            "tool"
        );
    }

    #[test]
    fn run_all_runs_in_order() {
        let mut runner = FakeRunner::new(|_: &Invocation| Ok(Some(String::new())));
        run_all(&mut runner, &invocations()).unwrap();
        assert_eq!(runner.seen, ["tool first", "tool second --flag"]);
    }

    #[test]
    fn run_all_stops_at_the_first_failure() {
        let mut runner = FakeRunner::new(|_: &Invocation| Ok(None));
        let error = run_all(&mut runner, &invocations()).unwrap_err();
        assert_eq!(error.to_string(), "`tool first` failed");
        assert_eq!(runner.seen.len(), 1);
    }

    #[test]
    fn spawn_failures_are_explained() {
        let mut runner = FakeRunner::new(|_: &Invocation| Err(io::ErrorKind::NotFound.into()));
        let invocation = Invocation::new("tool", ["x"]);
        let error = run_checked(&mut runner, &invocation)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("could not run `tool x`"), "{error}");
        assert!(
            error.ends_with("is `tool` installed and running?"),
            "{error}"
        );
        let error = capture_checked(&mut runner, &invocation)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("could not run `tool x`"), "{error}");
    }

    #[test]
    fn capture_checked_trims_output_and_reports_failure() {
        let mut ok = FakeRunner::new(|_: &Invocation| Ok(Some("  abc\n".to_owned())));
        assert_eq!(capture_checked(&mut ok, &invocations()[0]).unwrap(), "abc");
        let mut failing = FakeRunner::new(|_: &Invocation| Ok(None));
        let error = capture_checked(&mut failing, &invocations()[0]).unwrap_err();
        assert_eq!(error.to_string(), "`tool first` failed");
    }

    #[test]
    fn system_runner_reports_status_output_and_spawn_errors() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut runner = SystemRunner { repo_root };
        let mut same = Invocation::new(
            "git",
            ["diff", "--quiet", "--no-index", "Cargo.toml", "Cargo.toml"],
        );
        same.env.push(("XTASK_PROBE", "1".to_owned()));
        let different = Invocation::new(
            "git",
            ["diff", "--quiet", "--no-index", "Cargo.toml", "src/main.rs"],
        );
        assert!(runner.run(&same).unwrap());
        assert!(!runner.run(&different).unwrap());

        let toplevel = Invocation::new("git", ["rev-parse", "--is-inside-work-tree"]);
        assert_eq!(
            runner.capture(&toplevel).unwrap().as_deref(),
            Some("true\n")
        );
        assert_eq!(runner.capture(&different).unwrap(), None);

        let missing = Invocation::new("ferropress-missing-program", Vec::<String>::new());
        assert!(runner.run(&missing).is_err());
        assert!(runner.capture(&missing).is_err());
    }
}
