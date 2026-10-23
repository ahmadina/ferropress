//! `lab check`: run a scenario twice on the reference lab, each run from a fresh install, and compare
//! the runs. Any mismatch is nondeterminism in the reference that differential tests would trip over.

use std::fs;
use std::path::Path;

use anyhow::Context as _;
use test_harness::{Method, Observation, Report, Scenario, Slug, Step};

use crate::capture::{self, HttpClient};
use crate::process::{self, Invocation, Runner};

/// Where scenario documents live, relative to the repository root; each is named `<id>.toml`.
pub(crate) const SCENARIO_DIR: &str = "contracts/scenarios";

/// Longest mismatch value shown in the command output; full values are in the written captures.
const SHOWN_CHARS: usize = 200;

/// Reads and validates the scenario with this id.
pub(crate) fn load_scenario(repo_root: &Path, id: &Slug) -> anyhow::Result<Scenario> {
    let path = repo_root.join(SCENARIO_DIR).join(format!("{id}.toml"));
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Scenario::from_toml(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Resets the lab with `reset`, then performs every step of `scenario` and returns the observations.
/// Capture ids are `check-<run>-<step>`.
pub(crate) fn record(
    env: &[(&'static str, String)],
    reset: &[Invocation],
    scenario: &Scenario,
    run: char,
    runner: &mut impl Runner,
    http: &mut impl HttpClient,
) -> anyhow::Result<Vec<Observation>> {
    process::run_all(runner, reset)?;
    scenario
        .steps()
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let name: Slug = format!("check-{run}-{index}").parse()?;
            match step {
                Step::Http {
                    method: Method::Get,
                    path,
                } => capture::capture(env, &name, path, runner, http)
                    .with_context(|| format!("run {run}, step {index}")),
                other => anyhow::bail!("the lab cannot perform {other:?}"),
            }
        })
        .collect()
}

fn clip(value: &str) -> String {
    match value.char_indices().nth(SHOWN_CHARS) {
        Some((end, _)) => format!("{}…", &value[..end]),
        None => value.to_owned(),
    }
}

/// The command output for a comparison of two reference runs, and whether they matched.
pub(crate) fn describe(scenario: &Scenario, report: &Report) -> (String, bool) {
    let steps = scenario.steps().len();
    if report.passed() {
        return (
            format!(
                "lab check {}: reference is deterministic over {steps} step(s), {} aspect(s) compared",
                report.scenario,
                scenario.compare().len()
            ),
            true,
        );
    }
    let mut text = format!(
        "lab check {}: {} mismatch(es) between two reference runs over {steps} step(s)",
        report.scenario,
        report.mismatch_count()
    );
    for step in &report.steps {
        for mismatch in &step.mismatches {
            let line = format!(
                "\n  - step {}: {} at {}: expected {}, got {}",
                step.index,
                mismatch.aspect,
                mismatch.location,
                clip(&mismatch.expected),
                clip(&mismatch.actual)
            );
            text.push_str(&line);
        }
    }
    (text, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::HttpResponse;
    use crate::process::testing::FakeRunner;
    use test_harness::{Replay, run};

    pub(crate) const SCENARIO: &str = r#"
schema = 1
id = "two-pages"
description = "Two page loads"
compare = ["response.status", "hooks"]

[[steps]]
operation = "http"
method = "GET"
path = "/"

[[steps]]
operation = "http"
method = "GET"
path = "/?p=1"
"#;

    fn scenario() -> Scenario {
        Scenario::from_toml(SCENARIO).unwrap()
    }

    struct StatusHttp(Vec<u16>);

    impl HttpClient for StatusHttp {
        fn get(&mut self, _: &str, _: &[(&str, &str)]) -> anyhow::Result<HttpResponse> {
            Ok(HttpResponse {
                status: self.0.remove(0),
                headers: Vec::new(),
                body: Vec::new(),
            })
        }
    }

    fn lab() -> FakeRunner<impl FnMut(&Invocation) -> std::io::Result<Option<String>>> {
        FakeRunner::new(|invocation: &Invocation| {
            let args = invocation.args.join(" ");
            Ok(Some(if args.contains(" cat ") {
                r#"{"schema":1,"hooks":[],"queries":[],"mail":[],"cron":[]}"#.to_owned()
            } else {
                String::new()
            }))
        })
    }

    fn reset() -> Vec<Invocation> {
        vec![Invocation::new(
            "docker",
            ["compose", "reset"].map(String::from),
        )]
    }

    #[test]
    fn load_scenario_reads_by_id_and_reports_problems() {
        let root = std::env::temp_dir().join(format!("xtask-check-{}", std::process::id()));
        let dir = root.join(SCENARIO_DIR);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("two-pages.toml"), SCENARIO).unwrap();
        fs::write(dir.join("broken.toml"), "schema = 1").unwrap();

        let loaded = load_scenario(&root, &"two-pages".parse().unwrap()).unwrap();
        assert_eq!(loaded, scenario());
        let error = load_scenario(&root, &"broken".parse().unwrap()).unwrap_err();
        assert!(format!("{error:#}").starts_with("parsing "), "{error:#}");
        let error = load_scenario(&root, &"absent".parse().unwrap()).unwrap_err();
        assert!(format!("{error:#}").starts_with("reading "), "{error:#}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn record_resets_then_captures_every_step() {
        let mut runner = lab();
        let observations = record(
            &[],
            &reset(),
            &scenario(),
            'a',
            &mut runner,
            &mut StatusHttp(vec![200, 404]),
        )
        .unwrap();
        assert_eq!(runner.seen[0], "docker compose reset");
        assert!(runner.seen[2].ends_with("rm -f /tmp/ferropress-capture/check-a-0.json"));
        assert!(runner.seen.iter().any(|c| c.ends_with("check-a-1.json")));
        assert_eq!(
            observations
                .iter()
                .map(|o| (
                    o.name.to_string(),
                    o.request.path.to_string(),
                    o.response.status
                ))
                .collect::<Vec<_>>(),
            [
                ("check-a-0".to_owned(), "/".to_owned(), 200),
                ("check-a-1".to_owned(), "/?p=1".to_owned(), 404),
            ]
        );
    }

    #[test]
    fn record_propagates_reset_and_step_failures() {
        let mut failing = FakeRunner::new(|_: &Invocation| Ok(None));
        let error = record(
            &[],
            &reset(),
            &scenario(),
            'a',
            &mut failing,
            &mut StatusHttp(vec![]),
        )
        .unwrap_err();
        assert!(error.to_string().ends_with("reset` failed"), "{error}");

        let mut dump_fails_after_reset = FakeRunner::new({
            let mut calls = 0;
            move |_: &Invocation| {
                calls += 1;
                Ok((calls == 1).then(String::new))
            }
        });
        let error = record(
            &[],
            &reset(),
            &scenario(),
            'b',
            &mut dump_fails_after_reset,
            &mut StatusHttp(vec![]),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "run b, step 0");
    }

    #[test]
    fn describe_reports_success_and_lists_clipped_mismatches() {
        let observe = |statuses: Vec<u16>| {
            record(
                &[],
                &reset(),
                &scenario(),
                'a',
                &mut lab(),
                &mut StatusHttp(statuses),
            )
            .unwrap()
        };
        let same = run(
            &scenario(),
            &mut Replay::new(observe(vec![200, 200])),
            &mut Replay::new(observe(vec![200, 200])),
        )
        .unwrap();
        assert_eq!(
            describe(&scenario(), &same),
            (
                "lab check two-pages: reference is deterministic over 2 step(s), 2 aspect(s) compared"
                    .to_owned(),
                true
            )
        );

        let different = run(
            &scenario(),
            &mut Replay::new(observe(vec![200, 200])),
            &mut Replay::new(observe(vec![200, 500])),
        )
        .unwrap();
        assert_eq!(
            describe(&scenario(), &different),
            (
                "lab check two-pages: 1 mismatch(es) between two reference runs over 2 step(s)\n  \
                 - step 1: response.status at status: expected 200, got 500"
                    .to_owned(),
                false
            )
        );
    }

    #[test]
    fn clip_shortens_long_values_on_char_boundaries() {
        assert_eq!(clip("short"), "short");
        let long = "é".repeat(SHOWN_CHARS + 5);
        assert_eq!(clip(&long), format!("{}…", "é".repeat(SHOWN_CHARS)));
        assert_eq!(clip(&"x".repeat(SHOWN_CHARS)), "x".repeat(SHOWN_CHARS));
    }
}
