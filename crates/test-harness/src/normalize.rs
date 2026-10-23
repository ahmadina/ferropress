//! Reviewed normalizations: targeted rewrites of values that are proven to differ between runs.
//!
//! A normalization never removes data. It replaces the value at its location with a fixed marker on
//! both sides, so presence, count and position are still compared.

use std::fmt;
use std::str::FromStr;

use crate::error::ScenarioError;
use crate::observation::Observation;
use crate::scenario::Aspect;

/// Value written in place of a normalized value.
pub const NORMALIZED: &str = "<normalized>";

const HEADER_PREFIX: &str = "response.headers.";

/// Where a normalization applies.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Location {
    /// Every value of the response header with this lowercase name, e.g. `response.headers.date`.
    ResponseHeader(String),
}

impl Location {
    /// The compared aspect this location belongs to.
    pub const fn aspect(&self) -> Aspect {
        match self {
            Self::ResponseHeader(_) => Aspect::ResponseHeaders,
        }
    }

    /// Whether `byte` may appear in a lowercase header name (an RFC 9110 token without uppercase).
    fn is_header_name_byte(byte: u8) -> bool {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"!#$%&'*+-.^_`|~".contains(&byte)
    }
}

impl FromStr for Location {
    type Err = ScenarioError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.strip_prefix(HEADER_PREFIX) {
            Some(name) if !name.is_empty() && name.bytes().all(Self::is_header_name_byte) => {
                Ok(Self::ResponseHeader(name.to_owned()))
            }
            _ => Err(ScenarioError::InvalidLocation(value.to_owned())),
        }
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResponseHeader(name) => write!(f, "{HEADER_PREFIX}{name}"),
        }
    }
}

/// A normalization accepted in review, with the reason it is safe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalization {
    location: Location,
    reason: String,
    reviewed_by: String,
}

impl Normalization {
    pub(crate) fn new(location: Location, reason: String, reviewed_by: String) -> Self {
        Self {
            location,
            reason,
            reviewed_by,
        }
    }

    /// Where it applies.
    pub fn location(&self) -> &Location {
        &self.location
    }

    /// Why the value legitimately differs between runs.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Who reviewed and accepted it.
    pub fn reviewed_by(&self) -> &str {
        &self.reviewed_by
    }

    /// Replaces every value at the location with [`NORMALIZED`].
    pub fn apply(&self, observation: &mut Observation) {
        match &self.location {
            Location::ResponseHeader(name) => {
                for (header, value) in &mut observation.response.headers {
                    if header == name {
                        NORMALIZED.clone_into(value);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observation::{Method, Request, Response, Trace};
    use proptest::prelude::*;

    fn observation(headers: &[(&str, &str)]) -> Observation {
        Observation {
            schema: Observation::SCHEMA,
            name: "probe".parse().unwrap(),
            request: Request {
                method: Method::Get,
                path: "/".parse().unwrap(),
            },
            response: Response {
                status: 200,
                headers: headers
                    .iter()
                    .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                    .collect(),
                body: String::new(),
                body_sha256: String::new(),
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

    fn header(name: &str) -> Normalization {
        Normalization::new(
            Location::ResponseHeader(name.to_owned()),
            "reason".to_owned(),
            "reviewer".to_owned(),
        )
    }

    #[test]
    fn header_locations_parse_and_display() {
        for name in ["date", "x-powered-by", "etag", "a!#$%&'*+-.^_`|~9"] {
            let text = format!("response.headers.{name}");
            let location: Location = text.parse().unwrap();
            assert_eq!(location, Location::ResponseHeader(name.to_owned()));
            assert_eq!(location.to_string(), text);
            assert_eq!(location.aspect(), Aspect::ResponseHeaders);
        }
        for bad in [
            "",
            "response.headers.",
            "response.headers.Date",
            "response.headers.a b",
            "response.headers.a:b",
            "response.body",
            "hooks",
        ] {
            assert_eq!(
                bad.parse::<Location>(),
                Err(ScenarioError::InvalidLocation(bad.to_owned()))
            );
        }
    }

    #[test]
    fn header_normalization_masks_every_value_and_keeps_positions() {
        let mut target = observation(&[
            ("date", "Fri, 23 Oct 2026 10:00:00 GMT"),
            ("content-type", "text/html"),
            ("date", "second"),
        ]);
        header("date").apply(&mut target);
        assert_eq!(
            target.response.headers,
            observation(&[
                ("date", NORMALIZED),
                ("content-type", "text/html"),
                ("date", NORMALIZED),
            ])
            .response
            .headers
        );
    }

    #[test]
    fn header_normalization_ignores_other_and_absent_headers() {
        let original = observation(&[("content-type", "text/html"), ("x-date", "1")]);
        let mut target = original.clone();
        header("date").apply(&mut target);
        assert_eq!(target, original);
    }

    #[test]
    fn accessors_return_the_reviewed_fields() {
        let normalization = header("date");
        assert_eq!(
            normalization.location(),
            &Location::ResponseHeader("date".to_owned())
        );
        assert_eq!(normalization.reason(), "reason");
        assert_eq!(normalization.reviewed_by(), "reviewer");
    }

    proptest! {
        #[test]
        fn header_normalization_is_idempotent(
            headers in proptest::collection::vec(("(date|etag|server)", "[ -~]{0,8}"), 0..6)
        ) {
            let pairs: Vec<(&str, &str)> =
                headers.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
            let mut once = observation(&pairs);
            header("date").apply(&mut once);
            let mut twice = once.clone();
            header("date").apply(&mut twice);
            prop_assert_eq!(&once, &twice);
            prop_assert_eq!(once.response.headers.len(), pairs.len());
        }
    }
}
