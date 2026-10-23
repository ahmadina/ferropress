//! Versioned, machine-readable differential scenarios.
//!
//! A scenario is a TOML document naming the steps to run on both sides, the aspects of each
//! [`Observation`](crate::Observation) to compare, and the normalizations that were reviewed and
//! accepted for it:
//!
//! ```toml
//! schema = 1
//! id = "front-page"
//! description = "An anonymous visitor loads the front page"
//! compare = ["response.status", "response.headers", "response.body", "hooks"]
//!
//! [[steps]]
//! operation = "http"
//! method = "GET"
//! path = "/"
//!
//! [[normalizations]]
//! location = "response.headers.date"
//! reason = "the response date is the wall-clock time of each run"
//! reviewed_by = "maintainer-handle"
//! ```

use std::collections::BTreeSet;
use std::fmt;

use serde::Deserialize;

use crate::error::ScenarioError;
use crate::normalize::{Location, Normalization};
use crate::observation::{Method, RequestPath, Slug};

/// One part of an observation that a scenario can compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[non_exhaustive]
pub enum Aspect {
    /// The status code.
    #[serde(rename = "response.status")]
    ResponseStatus,
    /// Header fields: values per name in order; the order between different names is not significant.
    #[serde(rename = "response.headers")]
    ResponseHeaders,
    /// The exact body bytes.
    #[serde(rename = "response.body")]
    ResponseBody,
    /// The ordered hook trace.
    #[serde(rename = "hooks")]
    Hooks,
    /// SQL statements in execution order.
    #[serde(rename = "queries")]
    Queries,
    /// Outgoing mail in sending order.
    #[serde(rename = "mail")]
    Mail,
    /// The cron schedule after the request.
    #[serde(rename = "cron")]
    Cron,
    /// Database rows added and removed.
    #[serde(rename = "database")]
    Database,
}

impl Aspect {
    /// The name used in scenario files and reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResponseStatus => "response.status",
            Self::ResponseHeaders => "response.headers",
            Self::ResponseBody => "response.body",
            Self::Hooks => "hooks",
            Self::Queries => "queries",
            Self::Mail => "mail",
            Self::Cron => "cron",
            Self::Database => "database",
        }
    }
}

impl fmt::Display for Aspect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One operation performed identically against both sides.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "operation", rename_all = "lowercase", deny_unknown_fields)]
#[non_exhaustive]
pub enum Step {
    /// Send one HTTP request and observe everything it causes.
    Http {
        /// Request method.
        method: Method,
        /// Path and query string.
        path: RequestPath,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScenario {
    schema: u32,
    id: Slug,
    description: String,
    compare: Vec<Aspect>,
    steps: Vec<Step>,
    #[serde(default)]
    normalizations: Vec<RawNormalization>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNormalization {
    location: String,
    reason: String,
    reviewed_by: String,
}

/// A validated scenario.
///
/// Every scenario has at least one step and one compared aspect, no duplicate aspects, and only
/// normalizations that carry a reason and a reviewer and apply to a compared aspect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    id: Slug,
    description: String,
    compare: Vec<Aspect>,
    steps: Vec<Step>,
    normalizations: Vec<Normalization>,
}

impl Scenario {
    /// The scenario format version this crate reads.
    pub const SCHEMA: u32 = 1;

    /// Parses and validates a scenario document.
    ///
    /// # Errors
    ///
    /// Returns [`ScenarioError`] when the document is not valid TOML for this schema or breaks one of
    /// the rules listed on [`Scenario`].
    pub fn from_toml(text: &str) -> Result<Self, ScenarioError> {
        let raw: RawScenario = toml::from_str(text).map_err(ScenarioError::Syntax)?;
        if raw.schema != Self::SCHEMA {
            return Err(ScenarioError::UnsupportedSchema { found: raw.schema });
        }
        if raw.description.trim().is_empty() {
            return Err(ScenarioError::EmptyDescription);
        }
        if raw.steps.is_empty() {
            return Err(ScenarioError::NoSteps);
        }
        if raw.compare.is_empty() {
            return Err(ScenarioError::NoAspects);
        }
        let mut seen = BTreeSet::new();
        if let Some(duplicate) = raw.compare.iter().find(|aspect| !seen.insert(**aspect)) {
            return Err(ScenarioError::DuplicateAspect(*duplicate));
        }
        let normalizations = Self::validate_normalizations(raw.normalizations, &seen)?;
        Ok(Self {
            id: raw.id,
            description: raw.description,
            compare: raw.compare,
            steps: raw.steps,
            normalizations,
        })
    }

    fn validate_normalizations(
        raw: Vec<RawNormalization>,
        compared: &BTreeSet<Aspect>,
    ) -> Result<Vec<Normalization>, ScenarioError> {
        let mut locations = BTreeSet::new();
        raw.into_iter()
            .map(|entry| {
                let location: Location = entry.location.parse()?;
                if entry.reason.trim().is_empty() {
                    return Err(ScenarioError::MissingReason(location));
                }
                if entry.reviewed_by.trim().is_empty() {
                    return Err(ScenarioError::MissingReviewer(location));
                }
                if !compared.contains(&location.aspect()) {
                    return Err(ScenarioError::NormalizationNotCompared(location));
                }
                if !locations.insert(location.clone()) {
                    return Err(ScenarioError::DuplicateNormalization(location));
                }
                Ok(Normalization::new(
                    location,
                    entry.reason,
                    entry.reviewed_by,
                ))
            })
            .collect()
    }

    /// Scenario identifier.
    pub fn id(&self) -> &Slug {
        &self.id
    }

    /// What the scenario exercises, in one sentence.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Compared aspects, in declaration order.
    pub fn compare(&self) -> &[Aspect] {
        &self.compare
    }

    /// Steps, in execution order.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Reviewed normalizations applied to both sides before comparing.
    pub fn normalizations(&self) -> &[Normalization] {
        &self.normalizations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENARIO: &str = r#"
schema = 1
id = "front-page"
description = "An anonymous visitor loads the front page"
compare = ["response.status", "response.headers", "response.body", "hooks", "queries", "mail", "cron", "database"]

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
reason = "the response date is the wall-clock time of each run"
reviewed_by = "maintainer"
"#;

    fn error(text: &str) -> ScenarioError {
        Scenario::from_toml(text).unwrap_err()
    }

    #[test]
    fn valid_scenarios_parse_with_their_parts_in_order() {
        let scenario = Scenario::from_toml(SCENARIO).unwrap();
        assert_eq!(scenario.id().as_str(), "front-page");
        assert_eq!(
            scenario.description(),
            "An anonymous visitor loads the front page"
        );
        assert_eq!(
            scenario
                .compare()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            [
                "response.status",
                "response.headers",
                "response.body",
                "hooks",
                "queries",
                "mail",
                "cron",
                "database"
            ]
        );
        assert_eq!(
            scenario.steps(),
            [
                Step::Http {
                    method: Method::Get,
                    path: "/".parse().unwrap()
                },
                Step::Http {
                    method: Method::Get,
                    path: "/?p=1".parse().unwrap()
                },
            ]
        );
        assert_eq!(scenario.normalizations().len(), 1);
        let normalization = &scenario.normalizations()[0];
        assert_eq!(
            normalization.location(),
            &Location::ResponseHeader("date".to_owned())
        );
        assert_eq!(
            normalization.reason(),
            "the response date is the wall-clock time of each run"
        );
        assert_eq!(normalization.reviewed_by(), "maintainer");
    }

    #[test]
    fn normalizations_are_optional() {
        let start = SCENARIO.find("[[normalizations]]").unwrap();
        let scenario = Scenario::from_toml(&SCENARIO[..start]).unwrap();
        assert!(scenario.normalizations().is_empty());
    }

    #[test]
    fn structural_errors_are_reported() {
        assert!(matches!(
            error(&SCENARIO.replace(r#"method = "GET""#, r#"method = "BREW""#)),
            ScenarioError::Syntax(_)
        ));
        assert!(matches!(
            error(&SCENARIO.replace(
                r#"path = "/""#,
                r#"path = "/"
extra = 1"#
            )),
            ScenarioError::Syntax(_)
        ));
        assert!(matches!(
            error(&SCENARIO.replace(r#"operation = "http""#, r#"operation = "shell""#)),
            ScenarioError::Syntax(_)
        ));
        assert!(matches!(
            error(&SCENARIO.replace(r#"id = "front-page""#, r#"id = "Front Page""#)),
            ScenarioError::Syntax(_)
        ));
        assert!(matches!(
            error(&SCENARIO.replace(r#""hooks","#, r#""cookies","#)),
            ScenarioError::Syntax(_)
        ));
        assert_eq!(
            error(&SCENARIO.replace("schema = 1", "schema = 2")),
            ScenarioError::UnsupportedSchema { found: 2 }
        );
        assert_eq!(
            error(&SCENARIO.replace("An anonymous visitor loads the front page", "  ")),
            ScenarioError::EmptyDescription
        );
    }

    #[test]
    fn steps_and_aspects_must_be_present_and_unique() {
        let start = SCENARIO.find("[[steps]]").unwrap();
        assert_eq!(
            error(&format!("{}steps = []\n", &SCENARIO[..start])),
            ScenarioError::NoSteps
        );
        let compare_line = SCENARIO.lines().find(|l| l.starts_with("compare")).unwrap();
        assert_eq!(
            error(&SCENARIO.replace(compare_line, "compare = []")),
            ScenarioError::NoAspects
        );
        assert_eq!(
            error(&SCENARIO.replace(compare_line, r#"compare = ["hooks", "queries", "hooks"]"#)),
            ScenarioError::DuplicateAspect(Aspect::Hooks)
        );
    }

    #[test]
    fn normalizations_must_be_reviewed_and_targeted() {
        let date = || Location::ResponseHeader("date".to_owned());
        assert_eq!(
            error(&SCENARIO.replace("response.headers.date", "response.body")),
            ScenarioError::InvalidLocation("response.body".to_owned())
        );
        assert_eq!(
            error(&SCENARIO.replace("the response date is the wall-clock time of each run", " ")),
            ScenarioError::MissingReason(date())
        );
        assert_eq!(
            error(&SCENARIO.replace(r#"reviewed_by = "maintainer""#, r#"reviewed_by = """#)),
            ScenarioError::MissingReviewer(date())
        );
        assert_eq!(
            error(&SCENARIO.replace(r#""response.headers", "#, "")),
            ScenarioError::NormalizationNotCompared(date())
        );
        let start = SCENARIO.find("[[normalizations]]").unwrap();
        let doubled = format!("{SCENARIO}{}", &SCENARIO[start..]);
        assert_eq!(
            error(&doubled),
            ScenarioError::DuplicateNormalization(date())
        );
        assert!(matches!(
            error(&SCENARIO.replace(r#"reviewed_by = "maintainer""#, "")),
            ScenarioError::Syntax(_)
        ));
    }
}
