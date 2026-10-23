//! `lab capture`: record one request against the reference lab as a JSON artifact — the HTTP response,
//! the ordered hook trace, SQL statements, outgoing mail, the cron schedule and database row changes.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::dbdiff::{self, DatabaseDiff};
use crate::lab;
use crate::manifest;
use crate::process::{self, Runner};

/// Where capture artifacts are written, relative to the repository root (git-ignored).
pub(crate) const CAPTURE_DIR: &str = "lab/captures";

const CAPTURE_SCHEMA: u32 = 1;
const TRACE_SCHEMA: u32 = 1;
const CAPTURE_HEADER: &str = "x-ferropress-capture";
const USER_AGENT: &str = "ferropress-lab-capture";
/// Inside the app container: where the capture plugin writes its trace.
const CONTAINER_TRACE_DIR: &str = "/tmp/ferropress-capture";
const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const NAME_MAX_LEN: usize = 64;
/// Runs inside the database container, which already holds the lab credentials in its environment.
const DUMP_SCRIPT: &str = r#"MYSQL_PWD="$MYSQL_PASSWORD" exec mysqldump --user="$MYSQL_USER" --skip-dump-date --skip-extended-insert --order-by-primary --compact --no-create-info --no-tablespaces --skip-triggers "$MYSQL_DATABASE""#;

/// Capture identifier: lowercase ASCII letters, digits and hyphens, starting with a letter or digit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CaptureName(String);

impl CaptureName {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for CaptureName {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let valid_start = value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
        let valid_bytes = value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if valid_start && valid_bytes && value.len() <= NAME_MAX_LEN {
            Ok(Self(value.to_owned()))
        } else {
            Err(format!(
                "`{value}` must be 1-{NAME_MAX_LEN} lowercase letters, digits or hyphens, \
                 starting with a letter or digit"
            ))
        }
    }
}

impl fmt::Display for CaptureName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Request target: an absolute path with optional query, without spaces, control characters or fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestPath(String);

impl FromStr for RequestPath {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let valid = value.starts_with('/')
            && !value.starts_with("//")
            && !value.contains('#')
            && !value.chars().any(|c| c.is_whitespace() || c.is_control());
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(format!(
                "`{value}` must start with a single `/` and contain no spaces, control characters or `#`"
            ))
        }
    }
}

impl fmt::Display for RequestPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum HookKind {
    Action,
    Filter,
}

/// One hook invocation, in firing order.
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HookEvent {
    pub(crate) kind: HookKind,
    pub(crate) name: String,
    /// Number of arguments passed to the hook's callbacks.
    pub(crate) args: u32,
    /// Hook nesting depth; 1 for a hook fired outside any other hook.
    pub(crate) depth: u32,
}

/// A message handed to the mail API; the lab never delivers it.
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mail {
    pub(crate) to: Vec<String>,
    pub(crate) subject: String,
    pub(crate) message: String,
    pub(crate) headers: Vec<String>,
    pub(crate) attachments: Vec<String>,
}

/// A scheduled event, without its wall-clock run time.
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CronEvent {
    pub(crate) hook: String,
    pub(crate) schedule: Option<String>,
    pub(crate) interval: Option<u64>,
    pub(crate) args: Vec<serde_json::Value>,
}

/// What the capture plugin records inside the request.
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Trace {
    pub(crate) schema: u32,
    pub(crate) hooks: Vec<HookEvent>,
    pub(crate) queries: Vec<String>,
    pub(crate) mail: Vec<Mail>,
    pub(crate) cron: Vec<CronEvent>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Request {
    pub(crate) method: &'static str,
    pub(crate) path: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    /// Body as UTF-8, with invalid sequences replaced; `body_sha256` covers the exact bytes.
    pub(crate) body: String,
    pub(crate) body_sha256: String,
}

/// The complete capture artifact.
#[derive(Debug, PartialEq, Serialize)]
pub(crate) struct Capture {
    pub(crate) schema: u32,
    pub(crate) name: String,
    pub(crate) request: Request,
    pub(crate) response: Response,
    pub(crate) trace: Trace,
    pub(crate) database: DatabaseDiff,
}

impl Capture {
    /// One-line summary for the command output.
    pub(crate) fn summary(&self) -> String {
        format!(
            "status {}, {} hooks, {} queries, {} mails, {} cron events, {} tables changed",
            self.response.status,
            self.trace.hooks.len(),
            self.trace.queries.len(),
            self.trace.mail.len(),
            self.trace.cron.len(),
            self.database.len()
        )
    }
}

/// A raw HTTP response.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HttpResponse {
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

/// Sends HTTP requests; abstracted so captures can be tested without the lab.
pub(crate) trait HttpClient {
    /// Sends a GET request without following redirects; any status is a successful response.
    fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> anyhow::Result<HttpResponse>;
}

/// Blocking HTTP client for the local lab.
pub(crate) struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    pub(crate) fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build()
            .into();
        Self { agent }
    }
}

impl HttpClient for UreqClient {
    fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> anyhow::Result<HttpResponse> {
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let mut response = request.call().with_context(|| format!("GET {url}"))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY_BYTES)
            .read_to_vec()
            .with_context(|| format!("reading the body of GET {url}"))?;
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

fn dump_database(
    runner: &mut impl Runner,
    env: &[(&'static str, String)],
) -> anyhow::Result<String> {
    process::capture_checked(
        runner,
        &lab::compose(env, ["exec", "-T", "database", "sh", "-c", DUMP_SCRIPT]),
    )
    .context("dumping the lab database")
}

/// Records `GET path` against the running lab stack.
pub(crate) fn capture(
    env: &[(&'static str, String)],
    name: &CaptureName,
    path: &RequestPath,
    runner: &mut impl Runner,
    http: &mut impl HttpClient,
) -> anyhow::Result<Capture> {
    let trace_file = format!("{CONTAINER_TRACE_DIR}/{name}.json");
    let before = dump_database(runner, env)?;
    process::run_checked(
        runner,
        &lab::compose(env, ["exec", "-T", "app", "rm", "-f", &trace_file]),
    )?;

    let url = format!("{}{path}", lab::APP_URL.trim_end_matches('/'));
    let response = http.get(
        &url,
        &[(CAPTURE_HEADER, name.as_str()), ("user-agent", USER_AGENT)],
    )?;

    let raw_trace = process::capture_checked(
        runner,
        &lab::compose(env, ["exec", "-T", "app", "cat", &trace_file]),
    )
    .context(
        "reading the capture plugin's trace; rebuild the lab image with `cargo xtask lab up`",
    )?;
    let trace: Trace =
        serde_json::from_str(&raw_trace).context("parsing the capture plugin's trace")?;
    anyhow::ensure!(
        trace.schema == TRACE_SCHEMA,
        "the capture plugin wrote trace schema {}, expected {TRACE_SCHEMA}",
        trace.schema
    );
    let after = dump_database(runner, env)?;

    Ok(Capture {
        schema: CAPTURE_SCHEMA,
        name: name.as_str().to_owned(),
        request: Request {
            method: "GET",
            path: path.to_string(),
        },
        response: Response {
            status: response.status,
            headers: response.headers,
            body_sha256: manifest::bytes_sha256(&response.body),
            body: String::from_utf8_lossy(&response.body).into_owned(),
        },
        trace,
        database: dbdiff::diff(&before, &after),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::Invocation;
    use crate::process::testing::FakeRunner;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    const TRACE: &str = r#"{
        "schema": 1,
        "hooks": [
            {"kind": "action", "name": "init", "args": 0, "depth": 1},
            {"kind": "filter", "name": "the_title", "args": 2, "depth": 2}
        ],
        "queries": ["SELECT 1"],
        "mail": [{"to": ["a@example.test"], "subject": "Hi", "message": "Body", "headers": [], "attachments": []}],
        "cron": [{"hook": "daily_job", "schedule": "daily", "interval": 86400, "args": [1, "x"]}]
    }"#;

    fn name(value: &str) -> CaptureName {
        value.parse().unwrap()
    }

    fn path(value: &str) -> RequestPath {
        value.parse().unwrap()
    }

    #[test]
    fn capture_names_are_validated() {
        for good in ["home", "a", "post-42", "0"] {
            assert_eq!(name(good).as_str(), good);
            assert_eq!(name(good).to_string(), good);
        }
        let too_long = "a".repeat(NAME_MAX_LEN + 1);
        for bad in ["", "-a", "Home", "a_b", "a b", "../x", too_long.as_str()] {
            let error = bad.parse::<CaptureName>().unwrap_err();
            assert!(error.contains("lowercase letters"), "{bad}: {error}");
        }
    }

    #[test]
    fn request_paths_are_validated() {
        for good in ["/", "/?p=1", "/a/b?x=y&z=%20"] {
            assert_eq!(path(good).to_string(), good);
        }
        for bad in ["", "a", "//evil.example", "/a b", "/a#frag", "/a\n"] {
            let error = bad.parse::<RequestPath>().unwrap_err();
            assert!(
                error.contains("must start with a single"),
                "{bad:?}: {error}"
            );
        }
    }

    #[test]
    fn trace_parses_and_rejects_unknown_fields() {
        let trace: Trace = serde_json::from_str(TRACE).unwrap();
        assert_eq!(trace.hooks[1].kind, HookKind::Filter);
        assert_eq!(trace.cron[0].interval, Some(86_400));
        let extra = TRACE.replace(r#""queries""#, r#""extra": 1, "queries""#);
        assert!(serde_json::from_str::<Trace>(&extra).is_err());
    }

    struct FakeHttp {
        response: Option<anyhow::Result<HttpResponse>>,
        requests: Vec<(String, Vec<(String, String)>)>,
    }

    impl HttpClient for FakeHttp {
        fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> anyhow::Result<HttpResponse> {
            self.requests.push((
                url.to_owned(),
                headers
                    .iter()
                    .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                    .collect(),
            ));
            self.response.take().unwrap()
        }
    }

    fn http_ok(body: &[u8]) -> FakeHttp {
        FakeHttp {
            response: Some(Ok(HttpResponse {
                status: 200,
                headers: vec![("content-type".into(), "text/html".into())],
                body: body.to_vec(),
            })),
            requests: Vec::new(),
        }
    }

    /// Answers the lab invocations: two different database dumps, then `rm` and `cat` of the trace.
    fn lab_responder(
        trace: impl Into<String>,
    ) -> impl FnMut(&Invocation) -> std::io::Result<Option<String>> {
        let trace = trace.into();
        let mut dumps = 0;
        move |invocation: &Invocation| {
            let args = invocation.args.join(" ");
            Ok(Some(if args.contains("mysqldump") {
                dumps += 1;
                if dumps == 1 {
                    "INSERT INTO `options` VALUES (1,'old');".to_owned()
                } else {
                    "INSERT INTO `options` VALUES (1,'new');".to_owned()
                }
            } else if args.contains(" cat ") {
                trace.clone()
            } else {
                String::new()
            }))
        }
    }

    fn env() -> Vec<(&'static str, String)> {
        vec![("LAB_DB_IMAGE", "mysql:1@sha256:00".to_owned())]
    }

    #[test]
    fn capture_combines_response_trace_and_database_diff() {
        let mut runner = FakeRunner::new(lab_responder(TRACE));
        let mut http = http_ok(b"<html>\xff</html>");
        let capture = capture(
            &env(),
            &name("home"),
            &path("/?p=1"),
            &mut runner,
            &mut http,
        )
        .unwrap();

        assert_eq!(
            http.requests,
            [(
                "http://127.0.0.1:8080/?p=1".to_owned(),
                vec![
                    ("x-ferropress-capture".to_owned(), "home".to_owned()),
                    ("user-agent".to_owned(), "ferropress-lab-capture".to_owned()),
                ]
            )]
        );
        assert_eq!(runner.seen.len(), 4);
        assert!(runner.seen[0].contains("exec -T database sh -c MYSQL_PWD="));
        assert_eq!(
            runner.seen[1],
            "docker compose --file lab/compose.yaml exec -T app rm -f /tmp/ferropress-capture/home.json"
        );
        assert_eq!(
            runner.seen[2],
            "docker compose --file lab/compose.yaml exec -T app cat /tmp/ferropress-capture/home.json"
        );

        assert_eq!(capture.request.path, "/?p=1");
        assert_eq!(capture.response.status, 200);
        assert_eq!(capture.response.body, "<html>\u{fffd}</html>");
        assert_eq!(
            capture.response.body_sha256,
            manifest::bytes_sha256(b"<html>\xff</html>")
        );
        assert_eq!(capture.trace.hooks.len(), 2);
        assert_eq!(capture.database["options"].added, ["(1,'new')"]);
        assert_eq!(
            capture.summary(),
            "status 200, 2 hooks, 1 queries, 1 mails, 1 cron events, 1 tables changed"
        );
    }

    #[test]
    fn capture_serializes_with_stable_field_names() {
        let mut runner = FakeRunner::new(lab_responder(TRACE));
        let capture = capture(
            &env(),
            &name("home"),
            &path("/"),
            &mut runner,
            &mut http_ok(b"ok"),
        )
        .unwrap();
        let json: serde_json::Value = serde_json::to_value(&capture).unwrap();
        assert_eq!(json["schema"], 1);
        assert_eq!(json["request"]["method"], "GET");
        assert_eq!(json["response"]["headers"][0][0], "content-type");
        assert_eq!(json["trace"]["hooks"][0]["kind"], "action");
        assert_eq!(json["database"]["options"]["removed"][0], "(1,'old')");
    }

    #[test]
    fn capture_rejects_bad_traces() {
        let wrong_schema = TRACE.replacen(r#""schema": 1"#, r#""schema": 2"#, 1);
        let mut runner = FakeRunner::new(lab_responder(wrong_schema));
        let error = capture(
            &env(),
            &name("x"),
            &path("/"),
            &mut runner,
            &mut http_ok(b""),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "the capture plugin wrote trace schema 2, expected 1");

        let mut runner = FakeRunner::new(lab_responder("not json"));
        let error = capture(
            &env(),
            &name("x"),
            &path("/"),
            &mut runner,
            &mut http_ok(b""),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "parsing the capture plugin's trace");
    }

    #[test]
    fn capture_propagates_lab_and_http_failures() {
        let mut failing = FakeRunner::new(|_: &Invocation| Ok(None));
        let error = capture(
            &env(),
            &name("x"),
            &path("/"),
            &mut failing,
            &mut http_ok(b""),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "dumping the lab database");

        let mut http = FakeHttp {
            response: Some(Err(anyhow::anyhow!("connection refused"))),
            requests: Vec::new(),
        };
        let mut runner = FakeRunner::new(lab_responder(TRACE));
        let error = capture(&env(), &name("x"), &path("/"), &mut runner, &mut http)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "connection refused");

        let mut no_trace = FakeRunner::new(|invocation: &Invocation| {
            let args = invocation.args.join(" ");
            Ok((!args.contains(" cat ")).then(String::new))
        });
        let error = capture(
            &env(),
            &name("x"),
            &path("/"),
            &mut no_trace,
            &mut http_ok(b""),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.starts_with("reading the capture plugin's trace"),
            "{error}"
        );
    }

    /// Serves one canned HTTP response on a local port and reports the request it received.
    fn serve_once(response: &'static str) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                request.push_str(&line);
            }
            sender.send(request).unwrap();
            let mut stream = stream;
            stream.write_all(response.as_bytes()).unwrap();
        });
        (format!("http://{address}"), receiver)
    }

    #[test]
    fn ureq_client_returns_redirects_unfollowed_with_headers_and_body() {
        let (base, request) = serve_once(
            "HTTP/1.1 302 Found\r\nLocation: /next\r\nX-Test: a\r\nContent-Length: 2\r\n\r\nok",
        );
        let response = UreqClient::new()
            .get(&format!("{base}/start?x=1"), &[(CAPTURE_HEADER, "probe")])
            .unwrap();
        assert_eq!(response.status, 302);
        assert!(
            response
                .headers
                .contains(&("location".to_owned(), "/next".to_owned()))
        );
        assert!(
            response
                .headers
                .contains(&("x-test".to_owned(), "a".to_owned()))
        );
        assert_eq!(response.body, b"ok");
        let request = request.recv().unwrap();
        assert!(
            request.starts_with("GET /start?x=1 HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(
            request.contains("x-ferropress-capture: probe\r\n"),
            "{request}"
        );
    }

    #[test]
    fn ureq_client_treats_error_statuses_as_responses() {
        let (base, _request) =
            serve_once("HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n");
        let response = UreqClient::new().get(&base, &[]).unwrap();
        assert_eq!(response.status, 500);
        assert!(response.body.is_empty());
    }

    #[test]
    fn ureq_client_reports_connection_failures() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let error = UreqClient::new()
            .get(&format!("http://{address}/"), &[])
            .unwrap_err();
        assert!(error.to_string().starts_with("GET http://"), "{error}");
    }
}
