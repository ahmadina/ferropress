//! Fakes for tests that use the harness, enabled by the `test-util` feature.

use std::collections::VecDeque;

use crate::observation::{Method, Observation, Request, RequestPath, Response, Slug, Trace};
use crate::run::Target;
use crate::scenario::Step;

/// A [`ScriptedTarget`] was asked for more observations than it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the scripted target has no observations left")]
pub struct ScriptExhausted;

/// A target that returns prepared observations in order and records every step it was asked for.
#[derive(Debug, Clone, Default)]
pub struct ScriptedTarget {
    script: VecDeque<Observation>,
    /// Scenario, step index and step of every request, in order.
    pub seen: Vec<(Slug, usize, Step)>,
}

impl ScriptedTarget {
    /// A target that answers with `observations`, one per step.
    pub fn new(observations: impl IntoIterator<Item = Observation>) -> Self {
        Self {
            script: observations.into_iter().collect(),
            seen: Vec::new(),
        }
    }
}

impl Target for ScriptedTarget {
    type Error = ScriptExhausted;

    fn observe(
        &mut self,
        scenario: &Slug,
        index: usize,
        step: &Step,
    ) -> Result<Observation, Self::Error> {
        self.seen.push((scenario.clone(), index, step.clone()));
        self.script.pop_front().ok_or(ScriptExhausted)
    }
}

/// Lowercase hex SHA-256 of the empty byte string.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// A `GET` observation with status 200, no headers, an empty body and nothing recorded.
pub fn observation(name: Slug, path: RequestPath) -> Observation {
    Observation {
        schema: Observation::SCHEMA,
        name,
        request: Request {
            method: Method::Get,
            path,
        },
        response: Response {
            status: 200,
            headers: Vec::new(),
            body: String::new(),
            body_sha256: EMPTY_SHA256.to_owned(),
        },
        trace: Trace {
            schema: Trace::SCHEMA,
            hooks: Vec::new(),
            files: Vec::new(),
            queries: Vec::new(),
            mail: Vec::new(),
            cron: Vec::new(),
        },
        database: crate::DatabaseDiff::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest as _;
    use std::fmt::Write as _;

    #[test]
    fn observation_is_an_empty_successful_get() {
        let observation = observation("probe".parse().unwrap(), "/x".parse().unwrap());
        assert_eq!(observation.name.as_str(), "probe");
        assert_eq!(observation.request.path.as_str(), "/x");
        assert_eq!(
            observation.summary(),
            "status 200, 0 hooks, 0 files, 0 queries, 0 mails, 0 cron events, 0 tables changed"
        );
        let hex = sha2::Sha256::digest(b"")
            .iter()
            .fold(String::new(), |mut hex, byte| {
                write!(hex, "{byte:02x}").unwrap();
                hex
            });
        assert_eq!(observation.response.body_sha256, hex);
    }
}
