//! Runs a scenario against a reference and a candidate target and reports mismatches per step.

use std::fmt;

use crate::compare::{Mismatch, compare};
use crate::observation::{Observation, Slug};
use crate::scenario::{Scenario, Step};

/// A system that can perform scenario steps and report what each one made observable.
pub trait Target {
    /// Why a step could not be performed.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Performs `step`, the `index`-th step (from 0) of `scenario`, and returns its observation.
    ///
    /// # Errors
    ///
    /// Returns [`Target::Error`] when the step cannot be performed or observed.
    fn observe(
        &mut self,
        scenario: &Slug,
        index: usize,
        step: &Step,
    ) -> Result<Observation, Self::Error>;
}

/// Which side of the comparison a target is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The reference platform.
    Reference,
    /// The implementation under test.
    Candidate,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Reference => "reference",
            Self::Candidate => "candidate",
        })
    }
}

/// A scenario run that could not complete.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunError<R, C>
where
    R: std::error::Error + 'static,
    C: std::error::Error + 'static,
{
    /// The reference target failed a step.
    #[error("the reference target failed step {step}")]
    Reference {
        /// Step index, from 0.
        step: usize,
        /// The target's error.
        #[source]
        source: R,
    },
    /// The candidate target failed a step.
    #[error("the candidate target failed step {step}")]
    Candidate {
        /// Step index, from 0.
        step: usize,
        /// The target's error.
        #[source]
        source: C,
    },
    /// A target reported an observation of a different request than the step sent.
    #[error("the {side} target observed a different request than step {step} sent")]
    WrongRequest {
        /// The misbehaving side.
        side: Side,
        /// Step index, from 0.
        step: usize,
    },
}

/// The outcome of one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepReport {
    /// Step index, from 0.
    pub index: usize,
    /// The step performed.
    pub step: Step,
    /// Differences after normalization, in aspect order.
    pub mismatches: Vec<Mismatch>,
}

/// The outcome of a scenario run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The scenario run.
    pub scenario: Slug,
    /// One report per step, in order.
    pub steps: Vec<StepReport>,
}

impl Report {
    /// Whether every step matched on every compared aspect.
    pub fn passed(&self) -> bool {
        self.steps.iter().all(|step| step.mismatches.is_empty())
    }

    /// Total mismatches across all steps.
    pub fn mismatch_count(&self) -> usize {
        self.steps.iter().map(|step| step.mismatches.len()).sum()
    }
}

fn matches_step(observation: &Observation, step: &Step) -> bool {
    match step {
        Step::Http { method, path } => {
            observation.request.method == *method && observation.request.path == *path
        }
    }
}

/// Performs every step of `scenario` on both targets, applies its normalizations to both
/// observations and compares the scenario's aspects.
///
/// # Errors
///
/// Returns [`RunError`] when either target fails a step or reports an observation of a different
/// request; mismatches are not errors and are returned in the [`Report`].
pub fn run<R: Target, C: Target>(
    scenario: &Scenario,
    reference: &mut R,
    candidate: &mut C,
) -> Result<Report, RunError<R::Error, C::Error>> {
    let id = scenario.id();
    let mut steps = Vec::with_capacity(scenario.steps().len());
    for (index, step) in scenario.steps().iter().enumerate() {
        let mut expected =
            reference
                .observe(id, index, step)
                .map_err(|source| RunError::Reference {
                    step: index,
                    source,
                })?;
        if !matches_step(&expected, step) {
            return Err(RunError::WrongRequest {
                side: Side::Reference,
                step: index,
            });
        }
        let mut actual =
            candidate
                .observe(id, index, step)
                .map_err(|source| RunError::Candidate {
                    step: index,
                    source,
                })?;
        if !matches_step(&actual, step) {
            return Err(RunError::WrongRequest {
                side: Side::Candidate,
                step: index,
            });
        }
        for normalization in scenario.normalizations() {
            normalization.apply(&mut expected);
            normalization.apply(&mut actual);
        }
        steps.push(StepReport {
            index,
            step: step.clone(),
            mismatches: compare(scenario.compare(), &expected, &actual),
        });
    }
    Ok(Report {
        scenario: id.clone(),
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakes::{self, ScriptExhausted, ScriptedTarget};
    use crate::scenario::Aspect;
    use std::error::Error as _;

    const SCENARIO: &str = r#"
schema = 1
id = "two-pages"
description = "Two page loads"
compare = ["response.status", "response.headers"]

[[steps]]
operation = "http"
method = "GET"
path = "/"

[[steps]]
operation = "http"
method = "GET"
path = "/?p=1"

[[normalizations]]
location = "response.headers.date"
reason = "wall-clock time of each run"
reviewed_by = "maintainer"
"#;

    fn scenario() -> Scenario {
        Scenario::from_toml(SCENARIO).unwrap()
    }

    fn page(path: &str, status: u16, date: &str) -> Observation {
        let mut observation =
            fakes::observation("two-pages".parse().unwrap(), path.parse().unwrap());
        observation.response.status = status;
        observation.response.headers = vec![("date".to_owned(), date.to_owned())];
        observation
    }

    #[test]
    fn normalized_differences_pass() {
        let mut reference = ScriptedTarget::new([page("/", 200, "Mon"), page("/?p=1", 200, "Mon")]);
        let mut candidate = ScriptedTarget::new([page("/", 200, "Tue"), page("/?p=1", 200, "Tue")]);
        let report = run(&scenario(), &mut reference, &mut candidate).unwrap();
        assert!(report.passed());
        assert_eq!(report.mismatch_count(), 0);
        assert_eq!(report.scenario.as_str(), "two-pages");
        assert_eq!(
            report.steps.iter().map(|s| s.index).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(report.steps[1].step, scenario().steps()[1]);
        let seen: Vec<(String, usize)> = reference
            .seen
            .iter()
            .map(|(id, index, _)| (id.to_string(), *index))
            .collect();
        assert_eq!(
            seen,
            [("two-pages".to_owned(), 0), ("two-pages".to_owned(), 1)]
        );
        assert_eq!(candidate.seen.len(), 2);
    }

    #[test]
    fn mismatches_are_reported_per_step() {
        let mut reference = ScriptedTarget::new([page("/", 200, "Mon"), page("/?p=1", 200, "Mon")]);
        let mut candidate = ScriptedTarget::new([page("/", 200, "Tue"), page("/?p=1", 404, "Tue")]);
        let report = run(&scenario(), &mut reference, &mut candidate).unwrap();
        assert!(!report.passed());
        assert_eq!(report.mismatch_count(), 1);
        assert!(report.steps[0].mismatches.is_empty());
        assert_eq!(report.steps[1].mismatches[0].aspect, Aspect::ResponseStatus);
    }

    #[test]
    fn target_failures_stop_the_run_with_the_side_and_step() {
        let mut reference = ScriptedTarget::new([page("/", 200, "Mon")]);
        let mut candidate = ScriptedTarget::new([page("/", 200, "Mon"), page("/?p=1", 200, "Mon")]);
        let error = run(&scenario(), &mut reference, &mut candidate).unwrap_err();
        assert_eq!(error.to_string(), "the reference target failed step 1");
        assert!(matches!(
            error,
            RunError::Reference {
                step: 1,
                source: ScriptExhausted
            }
        ));
        assert_eq!(
            error.source().map(ToString::to_string),
            Some("the scripted target has no observations left".to_owned())
        );

        let mut reference = ScriptedTarget::new([page("/", 200, "Mon")]);
        let mut candidate = ScriptedTarget::new([]);
        let error = run(&scenario(), &mut reference, &mut candidate).unwrap_err();
        assert_eq!(error.to_string(), "the candidate target failed step 0");
    }

    #[test]
    fn observations_of_other_requests_are_rejected() {
        let mut reference = ScriptedTarget::new([page("/other", 200, "Mon")]);
        let mut candidate = ScriptedTarget::new([page("/", 200, "Mon")]);
        let error = run(&scenario(), &mut reference, &mut candidate).unwrap_err();
        assert!(matches!(
            error,
            RunError::WrongRequest {
                side: Side::Reference,
                step: 0
            }
        ));
        assert_eq!(
            error.to_string(),
            "the reference target observed a different request than step 0 sent"
        );

        let mut reference = ScriptedTarget::new([page("/", 200, "Mon")]);
        let mut candidate = ScriptedTarget::new([page("/?p=1", 200, "Mon")]);
        let error = run(&scenario(), &mut reference, &mut candidate).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the candidate target observed a different request than step 0 sent"
        );
    }
}
