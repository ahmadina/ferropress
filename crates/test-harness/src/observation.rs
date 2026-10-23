//! What one request makes observable, on either side of a differential test.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{InvalidRequestPath, InvalidSlug};

const SLUG_MAX_LEN: usize = 64;

/// Identifier for captures and scenarios: 1–64 lowercase ASCII letters, digits and hyphens, starting
/// with a letter or digit, so it is always safe as a file name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct Slug(String);

impl Slug {
    /// The identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_valid(value: &str) -> bool {
        let valid_start = value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
        valid_start
            && value.len() <= SLUG_MAX_LEN
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    }
}

impl TryFrom<String> for Slug {
    type Error = InvalidSlug;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if Self::is_valid(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidSlug(value))
        }
    }
}

impl FromStr for Slug {
    type Err = InvalidSlug;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value.to_owned())
    }
}

impl From<Slug> for String {
    fn from(slug: Slug) -> Self {
        slug.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Request target: an absolute path with optional query string, without spaces, control characters or
/// fragment, and never scheme-relative (`//host`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct RequestPath(String);

impl RequestPath {
    /// The path and query text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_valid(value: &str) -> bool {
        value.starts_with('/')
            && !value.starts_with("//")
            && !value.contains('#')
            && !value.chars().any(|c| c.is_whitespace() || c.is_control())
    }
}

impl TryFrom<String> for RequestPath {
    type Error = InvalidRequestPath;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if Self::is_valid(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidRequestPath(value))
        }
    }
}

impl FromStr for RequestPath {
    type Err = InvalidRequestPath;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value.to_owned())
    }
}

impl From<RequestPath> for String {
    fn from(path: RequestPath) -> Self {
        path.0
    }
}

impl fmt::Display for RequestPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// HTTP request method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[non_exhaustive]
pub enum Method {
    /// `GET`
    #[serde(rename = "GET")]
    Get,
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Get => "GET",
        })
    }
}

/// Whether a hook was fired as an action or applied as a filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HookKind {
    /// Fired for side effects; return values are discarded.
    Action,
    /// Applied to a value that callbacks may replace.
    Filter,
}

/// One hook invocation, in firing order.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookEvent {
    /// Action or filter.
    pub kind: HookKind,
    /// Hook name.
    pub name: String,
    /// Number of arguments passed to the hook's callbacks.
    pub args: u32,
    /// Nesting depth; 1 for a hook fired outside any other hook.
    pub depth: u32,
}

impl fmt::Display for HookEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            HookKind::Action => "action",
            HookKind::Filter => "filter",
        };
        write!(
            f,
            "{kind} {} (args {}, depth {})",
            self.name, self.args, self.depth
        )
    }
}

/// A source file loaded during the request, positioned in the hook trace.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileLoad {
    /// Path relative to the installation root.
    pub path: String,
    /// Index of the first hook event recorded after the file was loaded; equal to the number of hook
    /// events for files loaded after the last one.
    pub before_hook: u32,
}

impl fmt::Display for FileLoad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (before hook {})", self.path, self.before_hook)
    }
}

/// A message handed to the mail API; it is recorded, never delivered.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mail {
    /// Recipients, in the order given.
    pub to: Vec<String>,
    /// Subject line.
    pub subject: String,
    /// Message body.
    pub message: String,
    /// Extra header lines, in the order given.
    pub headers: Vec<String>,
    /// Attachment paths, in the order given.
    pub attachments: Vec<String>,
}

/// A scheduled event, without its wall-clock run time.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CronEvent {
    /// Hook fired when the event runs.
    pub hook: String,
    /// Recurrence name, or `None` for a single event.
    pub schedule: Option<String>,
    /// Recurrence interval in seconds, or `None` for a single event.
    pub interval: Option<u64>,
    /// Arguments passed to the hook.
    pub args: Vec<serde_json::Value>,
}

/// What is recorded inside the request: hooks, loaded files, SQL, mail and the cron schedule after the
/// request.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Trace {
    /// Format version; see [`Trace::SCHEMA`].
    pub schema: u32,
    /// Hook invocations in firing order.
    pub hooks: Vec<HookEvent>,
    /// Source files in load order.
    pub files: Vec<FileLoad>,
    /// SQL statements in execution order.
    pub queries: Vec<String>,
    /// Outgoing mail in sending order.
    pub mail: Vec<Mail>,
    /// Scheduled events, sorted by hook name then arguments.
    pub cron: Vec<CronEvent>,
}

impl Trace {
    /// The trace format version this crate reads and writes.
    pub const SCHEMA: u32 = 2;
}

/// The request that produced an observation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// HTTP method.
    pub method: Method,
    /// Path and query string.
    pub path: RequestPath,
}

/// The HTTP response as received.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    /// Status code.
    pub status: u16,
    /// Header fields in received order, names lowercase.
    pub headers: Vec<(String, String)>,
    /// Body as UTF-8 with invalid sequences replaced; `body_sha256` covers the exact bytes.
    pub body: String,
    /// Lowercase hex SHA-256 of the exact body bytes.
    pub body_sha256: String,
}

/// Rows that differ in one table; each list is sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TableDiff {
    /// Rows present only after the request, as SQL value tuples.
    pub added: Vec<String>,
    /// Rows present only before the request, as SQL value tuples.
    pub removed: Vec<String>,
}

/// Changed tables by name; unchanged tables are omitted. A changed row is one removal plus one addition.
pub type DatabaseDiff = BTreeMap<String, TableDiff>;

/// Everything one request made observable.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Format version; see [`Observation::SCHEMA`].
    pub schema: u32,
    /// Capture name.
    pub name: Slug,
    /// The request sent.
    pub request: Request,
    /// The response received.
    pub response: Response,
    /// Hooks, files, SQL, mail and cron recorded during the request.
    pub trace: Trace,
    /// Database rows changed by the request.
    pub database: DatabaseDiff,
}

impl Observation {
    /// The observation format version this crate reads and writes.
    pub const SCHEMA: u32 = 2;

    /// One-line summary, e.g. for command output.
    pub fn summary(&self) -> String {
        format!(
            "status {}, {} hooks, {} files, {} queries, {} mails, {} cron events, {} tables changed",
            self.response.status,
            self.trace.hooks.len(),
            self.trace.files.len(),
            self.trace.queries.len(),
            self.trace.mail.len(),
            self.trace.cron.len(),
            self.database.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const OBSERVATION: &str = r#"{
        "schema": 2,
        "name": "home",
        "request": {"method": "GET", "path": "/?p=1"},
        "response": {
            "status": 200,
            "headers": [["content-type", "text/html"]],
            "body": "ok",
            "body_sha256": "2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df"
        },
        "trace": {
            "schema": 2,
            "hooks": [
                {"kind": "action", "name": "init", "args": 0, "depth": 1},
                {"kind": "filter", "name": "the_title", "args": 2, "depth": 2}
            ],
            "files": [{"path": "index.php", "before_hook": 0}],
            "queries": ["SELECT 1"],
            "mail": [{"to": ["a@example.test"], "subject": "Hi", "message": "Body", "headers": [], "attachments": []}],
            "cron": [{"hook": "daily_job", "schedule": "daily", "interval": 86400, "args": [1, "x"]}]
        },
        "database": {"options": {"added": ["(1,'new')"], "removed": ["(1,'old')"]}}
    }"#;

    fn observation() -> Observation {
        serde_json::from_str(OBSERVATION).unwrap()
    }

    #[test]
    fn slugs_are_validated() {
        for good in ["home", "a", "post-42", "0"] {
            let slug: Slug = good.parse().unwrap();
            assert_eq!(slug.as_str(), good);
            assert_eq!(slug.to_string(), good);
            assert_eq!(String::from(slug), good);
        }
        let too_long = "a".repeat(SLUG_MAX_LEN + 1);
        for bad in ["", "-a", "Home", "a_b", "a b", "../x", too_long.as_str()] {
            let error = bad.parse::<Slug>().unwrap_err();
            assert_eq!(error, InvalidSlug(bad.to_owned()));
            assert!(error.to_string().contains("lowercase letters"), "{error}");
        }
        assert!("a".repeat(SLUG_MAX_LEN).parse::<Slug>().is_ok());
    }

    #[test]
    fn request_paths_are_validated() {
        for good in ["/", "/?p=1", "/a/b?x=y&z=%20"] {
            let path: RequestPath = good.parse().unwrap();
            assert_eq!(path.as_str(), good);
            assert_eq!(path.to_string(), good);
            assert_eq!(String::from(path), good);
        }
        for bad in ["", "a", "//evil.example", "/a b", "/a#frag", "/a\n"] {
            let error = bad.parse::<RequestPath>().unwrap_err();
            assert_eq!(error, InvalidRequestPath(bad.to_owned()));
            assert!(error.to_string().contains("single `/`"), "{error}");
        }
    }

    #[test]
    fn observations_round_trip_through_json() {
        let parsed = observation();
        assert_eq!(parsed.name.as_str(), "home");
        assert_eq!(parsed.request.method, Method::Get);
        assert_eq!(parsed.trace.hooks[1].kind, HookKind::Filter);
        assert_eq!(parsed.trace.cron[0].interval, Some(86_400));
        assert_eq!(parsed.database["options"].removed, ["(1,'old')"]);

        let json = serde_json::to_value(&parsed).unwrap();
        assert_eq!(json["request"]["method"], "GET");
        assert_eq!(json["response"]["headers"][0][1], "text/html");
        assert_eq!(json["trace"]["hooks"][0]["kind"], "action");
        let reparsed: Observation = serde_json::from_value(json).unwrap();
        assert_eq!(reparsed, parsed);
    }

    #[test]
    fn invalid_or_unknown_fields_are_rejected() {
        let cases = [
            OBSERVATION.replacen(r#""home""#, r#""Home""#, 1),
            OBSERVATION.replacen(r#""/?p=1""#, r#""p=1""#, 1),
            OBSERVATION.replacen(r#""GET""#, r#""BREW""#, 1),
            OBSERVATION.replacen(r#""queries""#, r#""extra": 1, "queries""#, 1),
            OBSERVATION.replacen(r#""schema": 2,"#, r#""schema": 2, "extra": 1,"#, 1),
            OBSERVATION.replacen(r#""before_hook": 0"#, r#""before_hook": -1"#, 1),
        ];
        for case in cases {
            assert!(
                serde_json::from_str::<Observation>(&case).is_err(),
                "{case}"
            );
        }
    }

    #[test]
    fn display_and_summary_are_readable() {
        let parsed = observation();
        assert_eq!(parsed.request.method.to_string(), "GET");
        assert_eq!(
            parsed.trace.hooks[0].to_string(),
            "action init (args 0, depth 1)"
        );
        assert_eq!(
            parsed.trace.hooks[1].to_string(),
            "filter the_title (args 2, depth 2)"
        );
        assert_eq!(
            parsed.trace.files[0].to_string(),
            "index.php (before hook 0)"
        );
        assert_eq!(
            parsed.summary(),
            "status 200, 2 hooks, 1 files, 1 queries, 1 mails, 1 cron events, 1 tables changed"
        );
        assert_eq!(Observation::SCHEMA, 2);
        assert_eq!(Trace::SCHEMA, 2);
    }

    proptest! {
        #[test]
        fn slug_parsing_matches_its_rules(value in "\\PC{0,70}") {
            let expected = value.len() <= SLUG_MAX_LEN
                && value.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            prop_assert_eq!(value.parse::<Slug>().is_ok(), expected);
        }

        #[test]
        fn valid_slugs_round_trip(value in "[a-z0-9][a-z0-9-]{0,63}") {
            let slug: Slug = value.parse().unwrap();
            let json = serde_json::to_string(&slug).unwrap();
            prop_assert_eq!(serde_json::from_str::<Slug>(&json).unwrap(), slug);
        }
    }
}
