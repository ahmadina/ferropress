//! Aspect comparators between a reference and a candidate observation.
//!
//! Order is significant wherever the platform makes it observable: hooks, loaded files, SQL statements,
//! mail and the values of one header name are compared as sequences, and each sequence reports only its first
//! divergence because later ones usually follow from it. Header fields with different names are
//! compared without regard to their relative order, which carries no meaning in HTTP (RFC 9110 §5.3).
//! Bodies are compared byte for byte through their SHA-256.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

use crate::observation::{Observation, TableDiff};
use crate::scenario::Aspect;

const EXCERPT_CHARS: usize = 48;

/// One difference between the reference and the candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// The aspect that differs.
    pub aspect: Aspect,
    /// Where inside the aspect, e.g. `hooks[42]` or `headers.content-type`.
    pub location: String,
    /// The reference value at that location.
    pub expected: String,
    /// The candidate value at that location.
    pub actual: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}: expected {}, got {}",
            self.aspect, self.location, self.expected, self.actual
        )
    }
}

/// Compares the given aspects, in order, and returns every mismatch found.
pub fn compare(
    aspects: &[Aspect],
    reference: &Observation,
    candidate: &Observation,
) -> Vec<Mismatch> {
    let mut mismatches = Vec::new();
    for &aspect in aspects {
        let mut found = Found {
            aspect,
            mismatches: &mut mismatches,
        };
        match aspect {
            Aspect::ResponseStatus => {
                if reference.response.status != candidate.response.status {
                    found.push(
                        "status",
                        reference.response.status.to_string(),
                        candidate.response.status.to_string(),
                    );
                }
            }
            Aspect::ResponseHeaders => {
                compare_headers(
                    &reference.response.headers,
                    &candidate.response.headers,
                    &mut found,
                );
            }
            Aspect::ResponseBody => compare_bodies(reference, candidate, &mut found),
            Aspect::Hooks => compare_sequences(
                "hooks",
                &reference.trace.hooks,
                &candidate.trace.hooks,
                ToString::to_string,
                &mut found,
            ),
            Aspect::Files => compare_sequences(
                "files",
                &reference.trace.files,
                &candidate.trace.files,
                ToString::to_string,
                &mut found,
            ),
            Aspect::Queries => compare_sequences(
                "queries",
                &reference.trace.queries,
                &candidate.trace.queries,
                |query| format!("{query:?}"),
                &mut found,
            ),
            Aspect::Mail => compare_sequences(
                "mail",
                &reference.trace.mail,
                &candidate.trace.mail,
                json,
                &mut found,
            ),
            Aspect::Cron => compare_sequences(
                "cron",
                &reference.trace.cron,
                &candidate.trace.cron,
                json,
                &mut found,
            ),
            Aspect::Database => compare_databases(reference, candidate, &mut found),
        }
    }
    mismatches
}

struct Found<'a> {
    aspect: Aspect,
    mismatches: &'a mut Vec<Mismatch>,
}

impl Found<'_> {
    fn push(&mut self, location: impl Into<String>, expected: String, actual: String) {
        self.mismatches.push(Mismatch {
            aspect: self.aspect,
            location: location.into(),
            expected,
            actual,
        });
    }
}

fn json(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| format!("<unserializable: {error}>"))
}

/// Reports the first index at which two sequences differ, including one ending before the other.
fn compare_sequences<T: PartialEq>(
    label: &str,
    reference: &[T],
    candidate: &[T],
    render: impl Fn(&T) -> String,
    found: &mut Found<'_>,
) {
    let Some(index) = first_divergence(reference, candidate) else {
        return;
    };
    let item = |items: &[T]| {
        items
            .get(index)
            .map_or_else(|| format!("<end after {} entries>", items.len()), &render)
    };
    found.push(
        format!("{label}[{index}]"),
        item(reference),
        item(candidate),
    );
}

fn first_divergence<T: PartialEq>(reference: &[T], candidate: &[T]) -> Option<usize> {
    reference
        .iter()
        .zip(candidate)
        .position(|(a, b)| a != b)
        .or_else(|| {
            (reference.len() != candidate.len()).then(|| reference.len().min(candidate.len()))
        })
}

fn headers_by_name(headers: &[(String, String)]) -> BTreeMap<&str, Vec<&str>> {
    let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, value) in headers {
        by_name.entry(name).or_default().push(value);
    }
    by_name
}

fn compare_headers(
    reference: &[(String, String)],
    candidate: &[(String, String)],
    found: &mut Found<'_>,
) {
    let expected = headers_by_name(reference);
    let actual = headers_by_name(candidate);
    let mut names: Vec<&str> = expected.keys().chain(actual.keys()).copied().collect();
    names.sort_unstable();
    names.dedup();
    let render = |values: Option<&Vec<&str>>| {
        values.map_or_else(|| "<absent>".to_owned(), |v| format!("{v:?}"))
    };
    for name in names {
        let (a, b) = (expected.get(name), actual.get(name));
        if a != b {
            found.push(format!("headers.{name}"), render(a), render(b));
        }
    }
}

fn excerpt(text: &str, from: usize) -> String {
    let rest: String = text
        .get(from..)
        .unwrap_or_default()
        .chars()
        .take(EXCERPT_CHARS)
        .collect();
    format!("{rest:?}")
}

fn compare_bodies(reference: &Observation, candidate: &Observation, found: &mut Found<'_>) {
    let (a, b) = (&reference.response, &candidate.response);
    if a.body_sha256 == b.body_sha256 {
        return;
    }
    let offset = a
        .body
        .char_indices()
        .zip(b.body.chars())
        .find(|((_, x), y)| x != y)
        .map(|((offset, _), _)| offset)
        .or_else(|| (a.body.len() != b.body.len()).then(|| a.body.len().min(b.body.len())));
    match offset {
        Some(offset) => found.push(
            format!("body byte {offset}"),
            excerpt(&a.body, offset),
            excerpt(&b.body, offset),
        ),
        None => found.push(
            "body sha256 (texts equal after UTF-8 replacement)",
            a.body_sha256.clone(),
            b.body_sha256.clone(),
        ),
    }
}

fn compare_databases(reference: &Observation, candidate: &Observation, found: &mut Found<'_>) {
    let empty = TableDiff::default();
    let mut tables: Vec<&String> = reference
        .database
        .keys()
        .chain(candidate.database.keys())
        .collect();
    tables.sort_unstable();
    tables.dedup();
    for table in tables {
        let a = reference.database.get(table).unwrap_or(&empty);
        let b = candidate.database.get(table).unwrap_or(&empty);
        let render = |row: &String| row.clone();
        compare_sequences(&format!("{table}.added"), &a.added, &b.added, render, found);
        compare_sequences(
            &format!("{table}.removed"),
            &a.removed,
            &b.removed,
            render,
            found,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakes;
    use crate::observation::{CronEvent, FileLoad, HookEvent, HookKind, Mail};
    use proptest::prelude::*;

    const ALL: [Aspect; 9] = [
        Aspect::ResponseStatus,
        Aspect::ResponseHeaders,
        Aspect::ResponseBody,
        Aspect::Hooks,
        Aspect::Files,
        Aspect::Queries,
        Aspect::Mail,
        Aspect::Cron,
        Aspect::Database,
    ];

    fn base() -> Observation {
        fakes::observation("probe".parse().unwrap(), "/".parse().unwrap())
    }

    fn hook(name: &str) -> HookEvent {
        HookEvent {
            kind: HookKind::Action,
            name: name.to_owned(),
            args: 0,
            depth: 1,
        }
    }

    fn with_body(body: &str, sha: &str) -> Observation {
        let mut observation = base();
        body.clone_into(&mut observation.response.body);
        sha.clone_into(&mut observation.response.body_sha256);
        observation
    }

    fn headers(pairs: &[(&str, &str)]) -> Observation {
        let mut observation = base();
        observation.response.headers = pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect();
        observation
    }

    fn owned_headers(pairs: Vec<(String, String)>) -> Observation {
        let mut observation = base();
        observation.response.headers = pairs;
        observation
    }

    fn mismatch(aspect: Aspect, location: &str, expected: &str, actual: &str) -> Mismatch {
        Mismatch {
            aspect,
            location: location.to_owned(),
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        }
    }

    #[test]
    fn identical_observations_match_on_every_aspect() {
        assert!(compare(&ALL, &base(), &base()).is_empty());
    }

    #[test]
    fn only_requested_aspects_are_compared() {
        let mut other = base();
        other.response.status = 404;
        assert!(compare(&[Aspect::Hooks], &base(), &other).is_empty());
        assert_eq!(
            compare(&[Aspect::ResponseStatus], &base(), &other),
            [mismatch(Aspect::ResponseStatus, "status", "200", "404")]
        );
    }

    #[test]
    fn header_order_across_names_is_ignored_but_values_per_name_are_ordered() {
        let reference = headers(&[("a", "1"), ("set-cookie", "x"), ("set-cookie", "y")]);
        let reordered = headers(&[("set-cookie", "x"), ("a", "1"), ("set-cookie", "y")]);
        assert!(compare(&[Aspect::ResponseHeaders], &reference, &reordered).is_empty());

        let swapped = headers(&[("a", "1"), ("set-cookie", "y"), ("set-cookie", "x")]);
        assert_eq!(
            compare(&[Aspect::ResponseHeaders], &reference, &swapped),
            [mismatch(
                Aspect::ResponseHeaders,
                "headers.set-cookie",
                r#"["x", "y"]"#,
                r#"["y", "x"]"#
            )]
        );
    }

    #[test]
    fn missing_and_extra_headers_are_reported_by_name() {
        let reference = headers(&[("server", "a"), ("vary", "x")]);
        let candidate = headers(&[("vary", "x"), ("etag", "1")]);
        assert_eq!(
            compare(&[Aspect::ResponseHeaders], &reference, &candidate),
            [
                mismatch(
                    Aspect::ResponseHeaders,
                    "headers.etag",
                    "<absent>",
                    r#"["1"]"#
                ),
                mismatch(
                    Aspect::ResponseHeaders,
                    "headers.server",
                    r#"["a"]"#,
                    "<absent>"
                ),
            ]
        );
    }

    #[test]
    fn bodies_report_the_first_differing_byte_with_excerpts() {
        let reference = with_body("<p>héllo\nworld</p>", "1");
        let candidate = with_body("<p>héllo\nWorld</p>", "2");
        assert_eq!(
            compare(&[Aspect::ResponseBody], &reference, &candidate),
            [mismatch(
                Aspect::ResponseBody,
                "body byte 10",
                r#""world</p>""#,
                r#""World</p>""#
            )]
        );
        let truncated = with_body("<p>héllo", "3");
        assert_eq!(
            compare(&[Aspect::ResponseBody], &reference, &truncated),
            [mismatch(
                Aspect::ResponseBody,
                "body byte 9",
                r#""\nworld</p>""#,
                r#""""#
            )]
        );
    }

    #[test]
    fn bodies_with_equal_hashes_match_and_lossy_equal_texts_fall_back_to_hashes() {
        assert!(
            compare(
                &[Aspect::ResponseBody],
                &with_body("a", "1"),
                &with_body("b", "1")
            )
            .is_empty()
        );
        assert_eq!(
            compare(
                &[Aspect::ResponseBody],
                &with_body("\u{fffd}", "1"),
                &with_body("\u{fffd}", "2")
            ),
            [mismatch(
                Aspect::ResponseBody,
                "body sha256 (texts equal after UTF-8 replacement)",
                "1",
                "2"
            )]
        );
    }

    #[test]
    fn long_body_excerpts_are_truncated() {
        let long = "x".repeat(EXCERPT_CHARS * 2);
        let found = compare(
            &[Aspect::ResponseBody],
            &with_body(&long, "1"),
            &with_body("", "2"),
        );
        assert_eq!(
            found[0].expected,
            format!("{:?}", "x".repeat(EXCERPT_CHARS))
        );
    }

    #[test]
    fn hook_traces_report_the_first_divergence_only() {
        let mut reference = base();
        reference.trace.hooks = vec![hook("init"), hook("wp_loaded"), hook("shutdown")];
        let mut candidate = reference.clone();
        candidate.trace.hooks[1] = hook("loaded");
        candidate.trace.hooks[2] = hook("end");
        assert_eq!(
            compare(&[Aspect::Hooks], &reference, &candidate),
            [mismatch(
                Aspect::Hooks,
                "hooks[1]",
                "action wp_loaded (args 0, depth 1)",
                "action loaded (args 0, depth 1)"
            )]
        );
        candidate.trace.hooks = reference.trace.hooks[..1].to_vec();
        assert_eq!(
            compare(&[Aspect::Hooks], &reference, &candidate),
            [mismatch(
                Aspect::Hooks,
                "hooks[1]",
                "action wp_loaded (args 0, depth 1)",
                "<end after 1 entries>"
            )]
        );
    }

    #[test]
    fn file_loads_compare_paths_and_positions_in_order() {
        let load = |path: &str, before_hook| FileLoad {
            path: path.to_owned(),
            before_hook,
        };
        let mut reference = base();
        reference.trace.files = vec![load("index.php", 0), load("a.php", 3)];
        let mut candidate = reference.clone();
        candidate.trace.files[1] = load("a.php", 4);
        assert_eq!(
            compare(&[Aspect::Files], &reference, &candidate),
            [mismatch(
                Aspect::Files,
                "files[1]",
                "a.php (before hook 3)",
                "a.php (before hook 4)"
            )]
        );
    }

    #[test]
    fn queries_mail_and_cron_are_ordered_sequences() {
        let mut reference = base();
        reference.trace.queries = vec!["SELECT 1".to_owned(), "SELECT 2".to_owned()];
        reference.trace.mail = vec![Mail {
            to: vec!["a@example.test".to_owned()],
            subject: "Hi".to_owned(),
            message: String::new(),
            headers: Vec::new(),
            attachments: Vec::new(),
        }];
        reference.trace.cron = vec![CronEvent {
            hook: "daily".to_owned(),
            schedule: Some("daily".to_owned()),
            interval: Some(86_400),
            args: Vec::new(),
        }];
        let mut candidate = reference.clone();
        candidate.trace.queries.reverse();
        candidate.trace.mail.clear();
        candidate.trace.cron[0].interval = Some(3_600);
        assert_eq!(
            compare(
                &[Aspect::Queries, Aspect::Mail, Aspect::Cron],
                &reference,
                &candidate
            ),
            [
                mismatch(
                    Aspect::Queries,
                    "queries[0]",
                    r#""SELECT 1""#,
                    r#""SELECT 2""#
                ),
                mismatch(
                    Aspect::Mail,
                    "mail[0]",
                    r#"{"to":["a@example.test"],"subject":"Hi","message":"","headers":[],"attachments":[]}"#,
                    "<end after 0 entries>"
                ),
                mismatch(
                    Aspect::Cron,
                    "cron[0]",
                    r#"{"hook":"daily","schedule":"daily","interval":86400,"args":[]}"#,
                    r#"{"hook":"daily","schedule":"daily","interval":3600,"args":[]}"#
                ),
            ]
        );
    }

    #[test]
    fn database_changes_are_compared_per_table() {
        let mut reference = base();
        reference.database.insert(
            "options".to_owned(),
            TableDiff {
                added: vec!["(1,'new')".to_owned()],
                removed: vec!["(1,'old')".to_owned()],
            },
        );
        let mut candidate = base();
        candidate.database.insert(
            "posts".to_owned(),
            TableDiff {
                added: vec!["(9)".to_owned()],
                removed: Vec::new(),
            },
        );
        assert_eq!(
            compare(&[Aspect::Database], &reference, &candidate),
            [
                mismatch(
                    Aspect::Database,
                    "options.added[0]",
                    "(1,'new')",
                    "<end after 0 entries>"
                ),
                mismatch(
                    Aspect::Database,
                    "options.removed[0]",
                    "(1,'old')",
                    "<end after 0 entries>"
                ),
                mismatch(
                    Aspect::Database,
                    "posts.added[0]",
                    "<end after 0 entries>",
                    "(9)"
                ),
            ]
        );
    }

    #[test]
    fn mismatches_display_as_one_line() {
        assert_eq!(
            mismatch(Aspect::ResponseStatus, "status", "200", "404").to_string(),
            "response.status at status: expected 200, got 404"
        );
    }

    proptest! {
        #[test]
        fn first_divergence_finds_the_earliest_difference(
            a in proptest::collection::vec(0u8..3, 0..12),
            b in proptest::collection::vec(0u8..3, 0..12),
        ) {
            match first_divergence(&a, &b) {
                None => prop_assert_eq!(&a, &b),
                Some(index) => {
                    prop_assert_eq!(&a[..index], &b[..index]);
                    prop_assert!(a.get(index) != b.get(index));
                }
            }
        }

        #[test]
        fn header_comparison_matches_per_name_value_lists(
            a in proptest::collection::vec(("(a|b|c)", "(1|2)"), 0..5),
            b in proptest::collection::vec(("(a|b|c)", "(1|2)"), 0..5),
        ) {
            let (x, y) = (owned_headers(a.clone()), owned_headers(b.clone()));
            let forward = compare(&[Aspect::ResponseHeaders], &x, &y);
            let backward = compare(&[Aspect::ResponseHeaders], &y, &x);
            prop_assert_eq!(forward.len(), backward.len());
            prop_assert_eq!(forward.is_empty(), headers_by_name(&a) == headers_by_name(&b));
        }
    }
}
