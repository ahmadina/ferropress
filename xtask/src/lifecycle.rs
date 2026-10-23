//! `lab lifecycle`: record the reference request lifecycle — source files loaded and actions fired, in
//! order, from the first boot hook through shutdown — for each site profile, and write it into
//! `contracts/lifecycle/`.
//!
//! Each subject is recorded in its steady state, after one warm-up visit, so one-off work done by the
//! first visit after an install is not part of the contract. Filters are left out: the full hook trace
//! is compared by scenarios, while the lifecycle is the ordering backbone native code must reproduce.

use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use test_harness::{HookKind, Profile, RequestPath, Slug, Trace};

use crate::capture::{self, HttpClient};
use crate::process::{self, Invocation, Runner};

/// Where lifecycle files live, relative to the repository root; each is named `<id>.jsonl`.
pub(crate) const LIFECYCLE_DIR: &str = "contracts/lifecycle";
/// The lifecycle file format version.
pub(crate) const SCHEMA: u32 = 1;

/// One request whose lifecycle is recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Subject {
    pub(crate) id: &'static str,
    pub(crate) profile: Profile,
    pub(crate) path: &'static str,
}

/// Every recorded subject, in recording order.
pub(crate) const SUBJECTS: [Subject; 3] = [
    Subject {
        id: "single-front-page",
        profile: Profile::Single,
        path: "/",
    },
    Subject {
        id: "network-main-site",
        profile: Profile::Network,
        path: "/",
    },
    Subject {
        id: "network-second-site",
        profile: Profile::Network,
        path: "/second/",
    },
];

/// One step of the lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "event", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Event {
    /// A source file was loaded; the path is relative to the installation root.
    Load { path: String },
    /// An action fired.
    Action { name: String, args: u32, depth: u32 },
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load { path } => write!(f, "load {path}"),
            Self::Action { name, args, depth } => {
                write!(f, "action {name} (args {args}, depth {depth})")
            }
        }
    }
}

/// The first line of a lifecycle file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Header {
    pub(crate) schema: u32,
    pub(crate) platform_version: String,
    pub(crate) id: String,
    pub(crate) profile: Profile,
    pub(crate) path: String,
}

/// A recorded lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Lifecycle {
    pub(crate) header: Header,
    pub(crate) events: Vec<Event>,
}

impl Lifecycle {
    /// The file contents: the header line, then one event per line.
    pub(crate) fn render(&self) -> anyhow::Result<String> {
        let mut text = serde_json::to_string(&self.header)?;
        text.push('\n');
        for event in &self.events {
            text.push_str(&serde_json::to_string(event)?);
            text.push('\n');
        }
        Ok(text)
    }

    /// Parses the contents written by [`Lifecycle::render`].
    #[cfg(test)]
    pub(crate) fn parse(text: &str) -> anyhow::Result<Self> {
        let mut lines = text.lines();
        let header: Header = serde_json::from_str(lines.next().unwrap_or_default())
            .context("parsing the lifecycle header")?;
        anyhow::ensure!(
            header.schema == SCHEMA,
            "lifecycle schema {} is not supported (expected {SCHEMA})",
            header.schema
        );
        let events = lines
            .enumerate()
            .map(|(index, line)| {
                serde_json::from_str(line)
                    .with_context(|| format!("parsing lifecycle event {index}"))
            })
            .collect::<anyhow::Result<Vec<Event>>>()?;
        Ok(Self { header, events })
    }

    /// One-line summary, e.g. for command output.
    pub(crate) fn summary(&self) -> String {
        let loads = self
            .events
            .iter()
            .filter(|event| matches!(event, Event::Load { .. }))
            .count();
        format!(
            "{}: {} actions, {loads} files",
            self.header.id,
            self.events.len() - loads
        )
    }
}

/// The trace's actions with every file load placed before the first hook recorded after it.
pub(crate) fn events(trace: &Trace) -> Vec<Event> {
    let mut files = trace.files.iter().peekable();
    let load = |file: &test_harness::FileLoad| Event::Load {
        path: file.path.clone(),
    };
    let mut events = Vec::new();
    for (index, hook) in trace.hooks.iter().enumerate() {
        while let Some(file) = files
            .next_if(|file| usize::try_from(file.before_hook).is_ok_and(|before| before <= index))
        {
            events.push(load(file));
        }
        if hook.kind == HookKind::Action {
            events.push(Event::Action {
                name: hook.name.clone(),
                args: hook.args,
                depth: hook.depth,
            });
        }
    }
    events.extend(files.map(load));
    events
}

/// Resets the lab with `reset`, then records every subject after one warm-up visit. Capture ids are
/// `lifecycle-<run>-<subject>` and `lifecycle-<run>-<subject>-warm`.
pub(crate) fn record(
    env: &[(&'static str, String)],
    reset: &[Invocation],
    run: char,
    runner: &mut impl Runner,
    http: &mut impl HttpClient,
) -> anyhow::Result<Vec<Vec<Event>>> {
    process::run_all(runner, reset)?;
    SUBJECTS
        .iter()
        .enumerate()
        .map(|(index, subject)| {
            let path: RequestPath = subject.path.parse()?;
            let warm: Slug = format!("lifecycle-{run}-{index}-warm").parse()?;
            let name: Slug = format!("lifecycle-{run}-{index}").parse()?;
            capture::capture(env, subject.profile, &warm, &path, runner, http)
                .and_then(|_| capture::capture(env, subject.profile, &name, &path, runner, http))
                .map(|observation| events(&observation.trace))
                .with_context(|| format!("run {run}, {}", subject.id))
        })
        .collect()
}

/// The first event at which two runs of one subject differ, described for the command output.
pub(crate) fn divergence(subject: &Subject, first: &[Event], second: &[Event]) -> Option<String> {
    let index = first
        .iter()
        .zip(second)
        .position(|(a, b)| a != b)
        .or_else(|| (first.len() != second.len()).then(|| first.len().min(second.len())))?;
    let at = |events: &[Event]| {
        events.get(index).map_or_else(
            || format!("<end after {} events>", events.len()),
            ToString::to_string,
        )
    };
    Some(format!(
        "{}: runs differ at event {index}: {} vs {}",
        subject.id,
        at(first),
        at(second)
    ))
}

/// Writes one file per subject into `dir`, replacing earlier ones.
pub(crate) fn write(lifecycles: &[Lifecycle], dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for lifecycle in lifecycles {
        let path = dir.join(format!("{}.jsonl", lifecycle.header.id));
        fs::write(&path, lifecycle.render()?)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::HttpResponse;
    use crate::process::testing::FakeRunner;
    use test_harness::{FileLoad, HookEvent};

    fn hook(kind: HookKind, name: &str, depth: u32) -> HookEvent {
        HookEvent {
            kind,
            name: name.to_owned(),
            args: 0,
            depth,
        }
    }

    fn file(path: &str, before_hook: u32) -> FileLoad {
        FileLoad {
            path: path.to_owned(),
            before_hook,
        }
    }

    fn trace(hooks: Vec<HookEvent>, files: Vec<FileLoad>) -> Trace {
        Trace {
            schema: Trace::SCHEMA,
            hooks,
            files,
            queries: Vec::new(),
            mail: Vec::new(),
            cron: Vec::new(),
        }
    }

    fn action(name: &str, depth: u32) -> Event {
        Event::Action {
            name: name.to_owned(),
            args: 0,
            depth,
        }
    }

    fn load(path: &str) -> Event {
        Event::Load {
            path: path.to_owned(),
        }
    }

    fn lifecycle(events: Vec<Event>) -> Lifecycle {
        Lifecycle {
            header: Header {
                schema: SCHEMA,
                platform_version: "7.1.3".to_owned(),
                id: "single-front-page".to_owned(),
                profile: Profile::Single,
                path: "/".to_owned(),
            },
            events,
        }
    }

    #[test]
    fn events_interleave_loads_with_actions_and_drop_filters() {
        let trace = trace(
            vec![
                hook(HookKind::Filter, "early", 1),
                hook(HookKind::Action, "init", 1),
                hook(HookKind::Action, "nested", 2),
                hook(HookKind::Filter, "late", 1),
            ],
            vec![
                file("index.php", 0),
                file("a.php", 1),
                file("b.php", 3),
                file("c.php", 4),
            ],
        );
        assert_eq!(
            events(&trace),
            [
                load("index.php"),
                load("a.php"),
                action("init", 1),
                action("nested", 2),
                load("b.php"),
                load("c.php"),
            ]
        );
        assert!(events(&self::trace(Vec::new(), Vec::new())).is_empty());
    }

    #[test]
    fn lifecycles_render_and_parse_back() {
        let recorded = lifecycle(vec![load("index.php"), action("init", 1)]);
        let text = recorded.render().unwrap();
        assert_eq!(
            text,
            "{\"schema\":1,\"platform_version\":\"7.1.3\",\"id\":\"single-front-page\",\
             \"profile\":\"single\",\"path\":\"/\"}\n\
             {\"event\":\"load\",\"path\":\"index.php\"}\n\
             {\"event\":\"action\",\"name\":\"init\",\"args\":0,\"depth\":1}\n"
        );
        assert_eq!(Lifecycle::parse(&text).unwrap(), recorded);
        assert_eq!(recorded.summary(), "single-front-page: 1 actions, 1 files");
    }

    #[test]
    fn parse_rejects_bad_headers_schemas_and_events() {
        let text = lifecycle(vec![load("index.php")]).render().unwrap();
        for (bad, message) in [
            (String::new(), "parsing the lifecycle header"),
            (
                text.replacen("\"schema\":1", "\"schema\":2", 1),
                "lifecycle schema 2 is not supported (expected 1)",
            ),
            (
                text.replacen("\"profile\":\"single\"", "\"profile\":\"cluster\"", 1),
                "parsing the lifecycle header",
            ),
            (
                text.replacen("\"event\":\"load\"", "\"event\":\"include\"", 1),
                "parsing lifecycle event 0",
            ),
            (
                text.replacen(
                    "\"path\":\"index.php\"",
                    "\"path\":\"index.php\",\"x\":1",
                    1,
                ),
                "parsing lifecycle event 0",
            ),
        ] {
            let error = Lifecycle::parse(&bad).unwrap_err();
            assert_eq!(error.to_string(), message, "{bad}");
        }
    }

    #[test]
    fn divergence_reports_the_first_differing_event() {
        let subject = SUBJECTS[0];
        let first = [load("a.php"), action("init", 1)];
        assert_eq!(divergence(&subject, &first, &first), None);
        assert_eq!(
            divergence(&subject, &first, &[load("a.php"), action("wp", 1)]).unwrap(),
            "single-front-page: runs differ at event 1: action init (args 0, depth 1) vs \
             action wp (args 0, depth 1)"
        );
        assert_eq!(
            divergence(&subject, &first, &first[..1]).unwrap(),
            "single-front-page: runs differ at event 1: action init (args 0, depth 1) vs \
             <end after 1 events>"
        );
    }

    struct OkHttp(Vec<String>);

    impl HttpClient for OkHttp {
        fn get(&mut self, url: &str, _: &[(&str, &str)]) -> anyhow::Result<HttpResponse> {
            self.0.push(url.to_owned());
            Ok(HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: Vec::new(),
            })
        }
    }

    const TRACE: &str = r#"{"schema":2,"hooks":[{"kind":"action","name":"init","args":0,"depth":1}],
        "files":[{"path":"index.php","before_hook":0}],"queries":[],"mail":[],"cron":[]}"#;

    #[test]
    fn record_resets_then_captures_each_subject_after_a_warm_up() {
        let mut runner = FakeRunner::new(|invocation: &Invocation| {
            let args = invocation.args.join(" ");
            Ok(Some(if args.contains(" cat ") {
                TRACE.to_owned()
            } else {
                String::new()
            }))
        });
        let mut http = OkHttp(Vec::new());
        let reset = [Invocation::new("docker", ["reset"])];
        let recorded = record(&[], &reset, 'a', &mut runner, &mut http).unwrap();
        assert_eq!(runner.seen[0], "docker reset");
        assert!(
            runner
                .seen
                .iter()
                .any(|c| c.ends_with("lifecycle-a-2-warm.json"))
        );
        assert!(
            runner
                .seen
                .iter()
                .any(|c| c.ends_with("lifecycle-a-2.json"))
        );
        assert_eq!(
            http.0,
            [
                "http://127.0.0.1:8080/",
                "http://127.0.0.1:8080/",
                "http://127.0.0.1:8081/",
                "http://127.0.0.1:8081/",
                "http://127.0.0.1:8081/second/",
                "http://127.0.0.1:8081/second/",
            ]
        );
        assert_eq!(
            recorded,
            vec![vec![load("index.php"), action("init", 1)]; 3]
        );
    }

    #[test]
    fn record_names_the_failing_subject() {
        let mut runner = FakeRunner::new({
            let mut calls = 0;
            move |_: &Invocation| {
                calls += 1;
                Ok((calls == 1).then(String::new))
            }
        });
        let error = record(
            &[],
            &[Invocation::new("docker", ["reset"])],
            'b',
            &mut runner,
            &mut OkHttp(Vec::new()),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "run b, single-front-page");
    }

    #[test]
    fn write_creates_one_file_per_subject_and_reports_failures() {
        let dir = std::env::temp_dir().join(format!("xtask-lifecycle-{}", std::process::id()));
        let recorded = lifecycle(vec![load("index.php")]);
        write(std::slice::from_ref(&recorded), &dir).unwrap();
        let text = fs::read_to_string(dir.join("single-front-page.jsonl")).unwrap();
        assert_eq!(Lifecycle::parse(&text).unwrap(), recorded);

        let blocked = dir.join("blocked");
        fs::write(&blocked, b"not a directory").unwrap();
        let error = write(&[recorded], &blocked).unwrap_err();
        assert!(error.to_string().starts_with("creating "), "{error}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn committed_lifecycles_cover_every_subject_and_are_canonical() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(LIFECYCLE_DIR);
        let manifest =
            crate::manifest::parse(include_str!("../../contracts/reference-manifest.toml"))
                .unwrap();
        let version = crate::lab::platform_version(&manifest).unwrap();
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort_unstable();
        let mut expected: Vec<String> =
            SUBJECTS.iter().map(|s| format!("{}.jsonl", s.id)).collect();
        expected.sort_unstable();
        assert_eq!(names, expected);

        for subject in SUBJECTS {
            let text = fs::read_to_string(dir.join(format!("{}.jsonl", subject.id))).unwrap();
            let lifecycle = Lifecycle::parse(&text).unwrap();
            assert_eq!(
                lifecycle.header,
                Header {
                    schema: SCHEMA,
                    platform_version: version.to_owned(),
                    id: subject.id.to_owned(),
                    profile: subject.profile,
                    path: subject.path.to_owned(),
                }
            );
            assert_eq!(lifecycle.render().unwrap(), text, "{}", subject.id);
            assert_eq!(lifecycle.events.first(), Some(&load("index.php")));
            let last_action = lifecycle
                .events
                .iter()
                .rev()
                .find(|event| matches!(event, Event::Action { .. }));
            assert_eq!(last_action, Some(&action("shutdown", 1)), "{}", subject.id);
        }
    }
}
