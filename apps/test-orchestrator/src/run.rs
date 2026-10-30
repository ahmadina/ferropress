//! Wires a scenario's reference lifecycle to a Ferropress boot and compares them.

use std::fs;
use std::path::Path;

use test_harness::{
    Method, Observation, Profile, Replay, Request, RequestPath, Response, RunError, Scenario, Slug,
    Step, Target, Trace,
};

use crate::boot;
use crate::error::OrchestratorError;
use crate::lifecycle;

/// Where scenario documents live, relative to the repository root.
pub const SCENARIO_DIR: &str = "contracts/scenarios";
/// Where recorded lifecycles live, relative to the repository root.
pub const LIFECYCLE_DIR: &str = "contracts/lifecycle";

/// SHA-256 of the empty body both sides use; this scenario compares hooks and files only.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Scenarios this binary can run: each id maps to a recorded lifecycle subject.
pub const KNOWN: &[(&str, &str)] = &[
    ("plugin-hook-trace", "single-front-page"),
    ("plugin-hook-trace-network", "network-main-site"),
];

/// Scenario identifiers this binary can run, in declaration order.
pub fn known_ids() -> impl Iterator<Item = &'static str> {
    KNOWN.iter().map(|(id, _)| *id)
}

fn lifecycle_id(scenario: &Slug) -> Result<&'static str, OrchestratorError> {
    KNOWN
        .iter()
        .find(|(id, _)| *id == scenario.as_str())
        .map(|(_, lifecycle)| *lifecycle)
        .ok_or_else(|| OrchestratorError::Unsupported(scenario.clone()))
}

/// Reads `contracts/scenarios/{id}.toml`.
///
/// # Errors
///
/// [`OrchestratorError::Read`] when the file is missing; [`OrchestratorError::Scenario`] when it is
/// not a valid scenario document.
pub fn load_scenario(repo_root: &Path, id: &str) -> Result<Scenario, OrchestratorError> {
    let path = repo_root.join(SCENARIO_DIR).join(format!("{id}.toml"));
    let text =
        fs::read_to_string(&path).map_err(|source| OrchestratorError::Read { path, source })?;
    Ok(Scenario::from_toml(&text)?)
}

/// Compares the recorded lifecycle to a Ferropress boot for `id`.
///
/// # Errors
///
/// [`OrchestratorError`] when the scenario is unknown, a file cannot be read, a plugin cannot
/// load, or a target reports the wrong request.
pub fn run_scenario(repo_root: &Path, id: &str) -> Result<test_harness::Report, OrchestratorError> {
    let scenario = load_scenario(repo_root, id)?;
    let subject = lifecycle_id(scenario.id())?;
    let lifecycle_path = repo_root
        .join(LIFECYCLE_DIR)
        .join(format!("{subject}.jsonl"));
    let text = fs::read_to_string(&lifecycle_path).map_err(|source| OrchestratorError::Read {
        path: lifecycle_path,
        source,
    })?;
    let extracted = lifecycle::extract(subject, &text)?;
    let step = http_step(&scenario)?;
    let expected = observation(scenario.id().clone(), step, extracted);
    let mut reference = Replay::new(vec![expected]);
    let mut candidate = Ferropress {
        repo_root,
        profile: scenario.profile(),
    };
    test_harness::run(&scenario, &mut reference, &mut candidate).map_err(|error| match error {
        RunError::Reference { source, .. } => OrchestratorError::Replay(source),
        RunError::Candidate { source, .. } => source,
        RunError::WrongRequest { side, step } => OrchestratorError::WrongRequest { side, step },
        other => OrchestratorError::Parse {
            path: id.to_owned(),
            message: other.to_string(),
        },
    })
}

fn http_step(scenario: &Scenario) -> Result<&RequestPath, OrchestratorError> {
    match scenario.steps().first() {
        Some(Step::Http {
            method: Method::Get,
            path,
        }) => Ok(path),
        _ => Err(OrchestratorError::Parse {
            path: scenario.id().to_string(),
            message: "the first step is not GET".into(),
        }),
    }
}

fn observation(name: Slug, path: &RequestPath, extracted: lifecycle::PluginLoad) -> Observation {
    Observation {
        schema: Observation::SCHEMA,
        name,
        request: Request {
            method: Method::Get,
            path: path.clone(),
        },
        response: Response {
            status: 200,
            headers: Vec::new(),
            body: String::new(),
            body_sha256: EMPTY_SHA256.to_owned(),
        },
        trace: Trace {
            schema: Trace::SCHEMA,
            hooks: extracted.hooks,
            files: extracted.files,
            queries: Vec::new(),
            mail: Vec::new(),
            cron: Vec::new(),
        },
        database: test_harness::DatabaseDiff::new(),
    }
}

struct Ferropress<'a> {
    repo_root: &'a Path,
    profile: Profile,
}

impl Target for Ferropress<'_> {
    type Error = OrchestratorError;

    fn observe(
        &mut self,
        scenario: &Scenario,
        _index: usize,
        step: &Step,
    ) -> Result<Observation, Self::Error> {
        let Step::Http {
            method: Method::Get,
            path,
        } = step
        else {
            return Err(OrchestratorError::Parse {
                path: scenario.id().to_string(),
                message: "only GET steps are supported".into(),
            });
        };
        boot::observe(self.repo_root, scenario.id(), path, self.profile)
    }
}

/// One-line outcome, plus mismatch lines when the run failed.
#[must_use]
pub fn describe(report: &test_harness::Report) -> String {
    if report.passed() {
        return format!(
            "{}: passed ({} step(s), {} mismatch(es))",
            report.scenario,
            report.steps.len(),
            report.mismatch_count()
        );
    }
    let mut text = format!(
        "{}: {} mismatch(es) over {} step(s)",
        report.scenario,
        report.mismatch_count(),
        report.steps.len()
    );
    for step in &report.steps {
        for mismatch in &step.mismatches {
            text.push_str("\n  - step ");
            text.push_str(&step.index.to_string());
            text.push_str(": ");
            text.push_str(&mismatch.to_string());
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_harness::{Aspect, Mismatch, Report, StepReport};

    #[test]
    fn known_ids_are_the_plugin_load_scenarios() {
        assert_eq!(
            known_ids().collect::<Vec<_>>(),
            ["plugin-hook-trace", "plugin-hook-trace-network"]
        );
    }

    #[test]
    fn describe_lists_mismatches() {
        let passed = Report {
            scenario: "plugin-hook-trace".parse().unwrap(),
            steps: vec![StepReport {
                index: 0,
                step: Step::Http {
                    method: Method::Get,
                    path: "/".parse().unwrap(),
                },
                mismatches: Vec::new(),
            }],
        };
        assert_eq!(
            describe(&passed),
            "plugin-hook-trace: passed (1 step(s), 0 mismatch(es))"
        );

        let failed = Report {
            scenario: "plugin-hook-trace".parse().unwrap(),
            steps: vec![StepReport {
                index: 0,
                step: Step::Http {
                    method: Method::Get,
                    path: "/".parse().unwrap(),
                },
                mismatches: vec![Mismatch {
                    aspect: Aspect::Hooks,
                    location: "hooks[0]".into(),
                    expected: "action mu_plugin_loaded (args 1, depth 1)".into(),
                    actual: "none".into(),
                }],
            }],
        };
        assert!(describe(&failed).contains("1 mismatch(es)"));
        assert!(describe(&failed).contains("hooks[0]"));
    }
}
