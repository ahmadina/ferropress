//! `dashboard`: how much of the reference inventory is documented, implemented, tested and certified,
//! computed from contract entries and written to `docs/COMPLETENESS.md`.
//!
//! A contract entry (`contracts/entries/<id>.toml`) describes one behavior in our own words, names the
//! inventory items it covers and its upstream provenance, and lists its evidence: the crates that
//! implement it and the tests that prove it. Every claim is checked against the repository, so the
//! dashboard never counts what cannot be shown.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::Context as _;
use serde::Deserialize;
use test_harness::Slug;

use crate::inventory::Inventory;

/// Where contract entries live, relative to the repository root; each is named `<id>.toml`.
pub(crate) const ENTRY_DIR: &str = "contracts/entries";
/// The generated dashboard, relative to the repository root.
pub(crate) const DASHBOARD_PATH: &str = "docs/COMPLETENESS.md";
/// The entry format version.
const SCHEMA: u32 = 1;

/// One inventory section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Section {
    Functions,
    Classes,
    Hooks,
    Constants,
    Globals,
    RestRoutes,
    Options,
    Tables,
}

impl Section {
    const ALL: [Self; 8] = [
        Self::Functions,
        Self::Classes,
        Self::Hooks,
        Self::Constants,
        Self::Globals,
        Self::RestRoutes,
        Self::Options,
        Self::Tables,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Functions => "functions",
            Self::Classes => "classes",
            Self::Hooks => "hooks",
            Self::Constants => "constants",
            Self::Globals => "globals",
            Self::RestRoutes => "rest-routes",
            Self::Options => "options",
            Self::Tables => "tables",
        }
    }

    /// The identity of an item within its section: function and class names are case-insensitive, and
    /// hooks are written `action:<name>` or `filter:<name>`.
    fn key(self, name: &str) -> String {
        match self {
            Self::Functions | Self::Classes => name.to_ascii_lowercase(),
            _ => name.to_owned(),
        }
    }
}

/// Every item key per section, plus every upstream source file the inventory mentions.
struct Items {
    keys: BTreeMap<Section, BTreeSet<String>>,
    sources: BTreeSet<String>,
}

impl Items {
    fn new(inventory: &Inventory) -> Self {
        let mut keys: BTreeMap<Section, BTreeSet<String>> = BTreeMap::new();
        let mut add = |section: Section, names: Vec<String>| {
            keys.entry(section)
                .or_default()
                .extend(names.iter().map(|name| section.key(name)));
        };
        add(
            Section::Functions,
            inventory.functions.iter().map(|f| f.name.clone()).collect(),
        );
        add(
            Section::Classes,
            inventory.classes.iter().map(|c| c.name.clone()).collect(),
        );
        add(
            Section::Hooks,
            inventory
                .hooks
                .iter()
                .map(|h| format!("{}:{}", h.kind.as_str(), h.name))
                .collect(),
        );
        add(
            Section::Constants,
            inventory.constants.iter().map(|c| c.name.clone()).collect(),
        );
        add(
            Section::Globals,
            inventory.globals.iter().map(|g| g.name.clone()).collect(),
        );
        add(
            Section::RestRoutes,
            inventory
                .rest_routes
                .iter()
                .map(|r| r.route.clone())
                .collect(),
        );
        add(
            Section::Options,
            inventory.options.iter().map(|o| o.name.clone()).collect(),
        );
        add(
            Section::Tables,
            inventory.tables.iter().map(|t| t.name.clone()).collect(),
        );

        let path = |location: &String| {
            location
                .rsplit_once(':')
                .map_or(location.as_str(), |(path, _)| path)
                .to_owned()
        };
        let mut sources = BTreeSet::new();
        for function in &inventory.functions {
            sources.extend(function.declarations.iter().map(|d| path(&d.location)));
        }
        for class in &inventory.classes {
            sources.extend(class.declarations.iter().map(path));
        }
        for hook in &inventory.hooks {
            sources.extend(hook.sites.iter().map(|s| path(&s.location)));
        }
        for constant in &inventory.constants {
            sources.extend(constant.declarations.iter().map(path));
        }
        for global in &inventory.globals {
            sources.extend(global.files.iter().cloned());
        }
        Self { keys, sources }
    }

    fn count(&self, section: Section) -> usize {
        self.keys.get(&section).map_or(0, BTreeSet::len)
    }
}

/// The inventory items an entry covers, per section.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Covers {
    #[serde(default)]
    functions: Vec<String>,
    #[serde(default)]
    classes: Vec<String>,
    #[serde(default)]
    hooks: Vec<String>,
    #[serde(default)]
    constants: Vec<String>,
    #[serde(default)]
    globals: Vec<String>,
    #[serde(default, rename = "rest-routes")]
    rest_routes: Vec<String>,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    tables: Vec<String>,
}

impl Covers {
    fn by_section(&self) -> [(Section, &[String]); 8] {
        [
            (Section::Functions, &self.functions),
            (Section::Classes, &self.classes),
            (Section::Hooks, &self.hooks),
            (Section::Constants, &self.constants),
            (Section::Globals, &self.globals),
            (Section::RestRoutes, &self.rest_routes),
            (Section::Options, &self.options),
            (Section::Tables, &self.tables),
        ]
    }
}

/// A contract entry as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    schema: u32,
    id: Slug,
    title: String,
    summary: String,
    /// Upstream source files the behavior was studied in, relative to the installation root.
    upstream: Vec<String>,
    /// Repository files that show the behavior, such as recorded lifecycles.
    #[serde(default)]
    evidence: Vec<String>,
    covers: Covers,
    /// Crate directories that implement the behavior natively.
    #[serde(default)]
    implemented_in: Vec<String>,
    /// Tests that prove the native implementation, as `<file>::<test function>`.
    #[serde(default)]
    tests: Vec<String>,
    /// Scenario ids whose Ferropress run matches the recorded reference.
    #[serde(default)]
    certified_by: Vec<String>,
}

/// A validated contract entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) id: Slug,
    pub(crate) title: String,
    covered: BTreeMap<Section, BTreeSet<String>>,
    implemented: bool,
    tested: bool,
    certified: bool,
}

impl Entry {
    fn item_count(&self) -> usize {
        self.covered.values().map(BTreeSet::len).sum()
    }
}

/// Checks one entry against the inventory and the repository; returns the entry or its problems.
fn validate(
    file_stem: &str,
    raw: RawEntry,
    items: &Items,
    repo_root: &Path,
) -> Result<Entry, Vec<String>> {
    let mut problems = Vec::new();
    let mut problem = |message: String| problems.push(format!("{file_stem}: {message}"));
    if raw.schema != SCHEMA {
        problem(format!(
            "schema {} is not supported (expected {SCHEMA})",
            raw.schema
        ));
    }
    if raw.id.as_str() != file_stem {
        problem(format!("id `{}` does not match the file name", raw.id));
    }
    if raw.title.trim().is_empty() {
        problem("the title is empty".to_owned());
    }
    if raw.summary.trim().is_empty() {
        problem("the summary is empty".to_owned());
    }
    if raw.upstream.is_empty() {
        problem("no upstream provenance is given".to_owned());
    }
    for source in &raw.upstream {
        if !items.sources.contains(source) {
            problem(format!(
                "upstream `{source}` is not a source file of the inventory"
            ));
        }
    }
    let inside = |path: &str| !path.starts_with('/') && !path.split('/').any(|part| part == "..");
    for path in &raw.evidence {
        if !inside(path) || !repo_root.join(path).is_file() {
            problem(format!("evidence `{path}` is not a repository file"));
        }
    }
    for path in &raw.implemented_in {
        if !inside(path)
            || !path.starts_with("crates/")
            || !repo_root.join(path).join("Cargo.toml").is_file()
        {
            problem(format!("implementation `{path}` is not a workspace crate"));
        }
    }
    for test in &raw.tests {
        if !test_exists(repo_root, test, &inside) {
            problem(format!(
                "test `{test}` is not a test function in a repository file"
            ));
        }
    }
    for scenario in &raw.certified_by {
        let path = format!("contracts/scenarios/{scenario}.toml");
        if !inside(&path) || !repo_root.join(&path).is_file() {
            problem(format!(
                "certified-by `{scenario}` is not a scenario in contracts/scenarios"
            ));
        }
    }

    let mut covered: BTreeMap<Section, BTreeSet<String>> = BTreeMap::new();
    let mut listed = false;
    for (section, names) in raw.covers.by_section() {
        for name in names {
            listed = true;
            let key = section.key(name);
            if !items
                .keys
                .get(&section)
                .is_some_and(|keys| keys.contains(&key))
            {
                problem(format!(
                    "{} `{name}` is not in the inventory",
                    section.name()
                ));
            } else if !covered.entry(section).or_default().insert(key) {
                problem(format!("{} `{name}` is listed twice", section.name()));
            }
        }
    }
    if !listed {
        problem("covers no inventory items".to_owned());
    }

    if problems.is_empty() {
        Ok(Entry {
            id: raw.id,
            title: raw.title.trim().to_owned(),
            covered,
            implemented: !raw.implemented_in.is_empty(),
            tested: !raw.tests.is_empty(),
            certified: !raw.certified_by.is_empty(),
        })
    } else {
        Err(problems)
    }
}

/// Whether `reference` (`<file>::<function>`) names a `#[test]` function in a repository file.
fn test_exists(repo_root: &Path, reference: &str, inside: &impl Fn(&str) -> bool) -> bool {
    let Some((file, function)) = reference.rsplit_once("::") else {
        return false;
    };
    if !inside(file) || function.is_empty() {
        return false;
    }
    let Ok(text) = fs::read_to_string(repo_root.join(file)) else {
        return false;
    };
    let signature = format!("fn {function}(");
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    lines.iter().enumerate().any(|(index, line)| {
        line.contains(&signature)
            && lines[..index]
                .iter()
                .rev()
                .take_while(|before| before.starts_with("#["))
                .any(|attribute| attribute.starts_with("#[test]"))
    })
}

/// Reads and validates every entry in `dir`, in file name order.
pub(crate) fn load_entries(
    dir: &Path,
    inventory: &Inventory,
    repo_root: &Path,
) -> anyhow::Result<Result<Vec<Entry>, Vec<String>>> {
    let items = Items::new(inventory);
    let mut paths: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .with_context(|| format!("reading {}", dir.display()))?;
    paths.sort();
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    for path in paths {
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.extension().is_none_or(|extension| extension != "toml") {
            problems.push(format!(
                "{stem}: only `.toml` entries belong in {ENTRY_DIR}"
            ));
            continue;
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        match toml::from_str::<RawEntry>(&text) {
            Err(error) => problems.push(format!("{stem}: {}", error.message())),
            Ok(raw) => match validate(&stem, raw, &items, repo_root) {
                Ok(entry) => entries.push(entry),
                Err(found) => problems.extend(found),
            },
        }
    }
    Ok(if problems.is_empty() {
        Ok(entries)
    } else {
        Err(problems)
    })
}

/// Item counts for one section.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counts {
    items: usize,
    documented: usize,
    implemented: usize,
    tested: usize,
    certified: usize,
}

impl Counts {
    fn add(&mut self, other: Self) {
        self.items += other.items;
        self.documented += other.documented;
        self.implemented += other.implemented;
        self.tested += other.tested;
        self.certified += other.certified;
    }
}

fn counts(inventory: &Inventory, entries: &[Entry]) -> Vec<(Section, Counts)> {
    let items = Items::new(inventory);
    Section::ALL
        .into_iter()
        .map(|section| {
            let mut documented = BTreeSet::new();
            let mut implemented = BTreeSet::new();
            let mut tested = BTreeSet::new();
            let mut certified = BTreeSet::new();
            for entry in entries {
                for key in entry.covered.get(&section).into_iter().flatten() {
                    documented.insert(key);
                    if entry.implemented {
                        implemented.insert(key);
                    }
                    if entry.tested {
                        tested.insert(key);
                    }
                    if entry.certified {
                        certified.insert(key);
                    }
                }
            }
            (
                section,
                Counts {
                    items: items.count(section),
                    documented: documented.len(),
                    implemented: implemented.len(),
                    tested: tested.len(),
                    certified: certified.len(),
                },
            )
        })
        .collect()
}

fn share(part: usize, whole: usize) -> String {
    if whole == 0 {
        return format!("{part}");
    }
    let tenths = (part * 1000 + whole / 2) / whole;
    format!("{part} ({}.{}%)", tenths / 10, tenths % 10)
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// The dashboard document.
pub(crate) fn render(inventory: &Inventory, entries: &[Entry]) -> String {
    let rows = counts(inventory, entries);
    let mut total = Counts::default();
    let mut text = format!(
        "# Completeness\n\n\
         Generated by `cargo xtask dashboard` from `contracts/inventory/` (reference platform {}) and \
         `contracts/entries/`. Do not edit by hand.\n\n\
         Each inventory item has four independent states:\n\n\
         - **Documented**: a contract entry describes the item's behavior in our own words, with \
         upstream provenance.\n\
         - **Implemented**: an entry covering the item names a workspace crate that implements it \
         natively.\n\
         - **Tested**: an entry covering the item names test functions that prove the native behavior.\n\
         - **Certified**: a contract entry covering the item names a scenario that \
         `apps/test-orchestrator` has compared to the recorded reference.\n\n\
         **Unknown** items are not documented yet.\n\n\
         | Section | Items | Documented | Implemented | Tested | Certified | Unknown |\n\
         |---|---:|---:|---:|---:|---:|---:|\n",
        inventory.platform_version
    );
    let mut row = |name: &str, counts: Counts| {
        let _ = writeln!(
            text,
            "| {name} | {} | {} | {} | {} | {} | {} |",
            counts.items,
            share(counts.documented, counts.items),
            share(counts.implemented, counts.items),
            share(counts.tested, counts.items),
            share(counts.certified, counts.items),
            share(counts.items - counts.documented, counts.items),
        );
    };
    for (section, counts) in &rows {
        row(section.name(), *counts);
        total.add(*counts);
    }
    row("**total**", total);

    text.push_str("\n## Contract entries\n\n");
    if entries.is_empty() {
        text.push_str("None yet.\n");
        return text;
    }
    text.push_str(
        "| Entry | Title | Items | Implemented | Tested | Certified |\n|---|---|---:|---|---|---|\n",
    );
    for entry in entries {
        let _ = writeln!(
            text,
            "| `{}` | {} | {} | {} | {} | {} |",
            entry.id,
            entry.title,
            entry.item_count(),
            yes_no(entry.implemented),
            yes_no(entry.tested),
            yes_no(entry.certified)
        );
    }
    text
}

/// One-line summary of documented coverage, e.g. for command output.
pub(crate) fn summary(inventory: &Inventory, entries: &[Entry]) -> String {
    let mut total = Counts::default();
    for (_, counts) in counts(inventory, entries) {
        total.add(counts);
    }
    format!(
        "{} entries; {} of {} items documented, {} implemented, {} tested, {} certified",
        entries.len(),
        total.documented,
        total.items,
        total.implemented,
        total.tested,
        total.certified
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory;

    const INVENTORY: &str = r#"{
        "schema": 1,
        "platform_version": "7.1.3",
        "functions": [{"name": "Load_Things", "declarations": [{"location": "boot.php:3", "params": [], "return_type": null, "by_ref": false}]}],
        "classes": [{"name": "Widget", "kind": "class", "abstract": false, "final": false, "extends": [], "implements": [], "traits": [], "constants": [], "properties": [], "methods": [], "declarations": ["lib/widget.php:1"]}],
        "hooks": [
            {"kind": "action", "name": "boot", "dynamic": false, "deprecated": false, "sites": [{"location": "boot.php:9", "args": 0}]},
            {"kind": "filter", "name": "boot", "dynamic": false, "deprecated": false, "sites": [{"location": "boot.php:10", "args": 1}]}
        ],
        "constants": [{"name": "ROOT", "declarations": ["config.php:2"], "defined_at_boot": true}],
        "globals": [{"name": "registry", "files": ["lib/registry.php"], "set_at_boot": true}],
        "rest_routes": [{"route": "/", "namespace": "", "methods": ["GET"]}],
        "options": [{"name": "title", "autoload": "on"}],
        "tables": [{"name": "items", "scope": "site", "created": true, "columns": [], "indexes": []}]
    }"#;

    const ENTRY: &str = r#"schema = 1
id = "boot-order"
title = "Boot order"
summary = "What loads when."
upstream = ["boot.php"]

[covers]
functions = ["load_things"]
hooks = ["action:boot"]
"#;

    fn inventory() -> Inventory {
        inventory::parse(INVENTORY).unwrap()
    }

    /// A temporary repository with `entries` (file name, text) and the given extra files.
    struct Repo(std::path::PathBuf);

    impl Repo {
        fn new(name: &str, entries: &[(&str, &str)], files: &[(&str, &str)]) -> Self {
            let root =
                std::env::temp_dir().join(format!("xtask-dashboard-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join(ENTRY_DIR)).unwrap();
            for (file, text) in entries {
                fs::write(root.join(ENTRY_DIR).join(file), text).unwrap();
            }
            for (file, text) in files {
                let path = root.join(file);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, text).unwrap();
            }
            Self(root)
        }

        fn load(&self) -> Result<Vec<Entry>, Vec<String>> {
            load_entries(&self.0.join(ENTRY_DIR), &inventory(), &self.0).unwrap()
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn valid_entries_load_with_case_insensitive_function_names() {
        let repo = Repo::new("valid", &[("boot-order.toml", ENTRY)], &[]);
        let entries = repo.load().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id.as_str(), "boot-order");
        assert_eq!(entries[0].item_count(), 2);
        assert!(!entries[0].implemented);
        assert!(!entries[0].tested);
        assert!(!entries[0].certified);
    }

    #[test]
    fn evidence_implementations_and_tests_are_checked_against_the_repository() {
        let entry = ENTRY.replace(
            "upstream = [\"boot.php\"]",
            "upstream = [\"boot.php\"]\nevidence = [\"contracts/lifecycle/a.jsonl\"]\n\
             implemented_in = [\"crates/cms-boot\"]\n\
             tests = [\"crates/cms-boot/src/lib.rs::boots_in_order\"]",
        );
        let files = [
            ("contracts/lifecycle/a.jsonl", "{}"),
            ("crates/cms-boot/Cargo.toml", "[package]"),
            (
                "crates/cms-boot/src/lib.rs",
                "#[cfg(test)]\nmod tests {\n    #[test]\n    #[ignore]\n    fn boots_in_order() {}\n    fn helper() {}\n}\n",
            ),
        ];
        let repo = Repo::new("evidence", &[("boot-order.toml", &entry)], &files);
        let entries = repo.load().unwrap();
        assert!(entries[0].implemented);
        assert!(entries[0].tested);
        assert!(!entries[0].certified);

        let certified = entry.replace(
            "tests = [\"crates/cms-boot/src/lib.rs::boots_in_order\"]",
            "tests = [\"crates/cms-boot/src/lib.rs::boots_in_order\"]\n\
             certified_by = [\"plugin-hook-trace\"]",
        );
        let mut certified_files = files.to_vec();
        certified_files.push(("contracts/scenarios/plugin-hook-trace.toml", "schema = 1\n"));
        let repo = Repo::new(
            "certified",
            &[("boot-order.toml", &certified)],
            &certified_files,
        );
        assert!(repo.load().unwrap()[0].certified);

        let missing_scenario = Repo::new(
            "missing-scenario",
            &[("boot-order.toml", &certified)],
            &files,
        );
        assert_eq!(
            missing_scenario.load().unwrap_err(),
            [
                "boot-order: certified-by `plugin-hook-trace` is not a scenario in contracts/scenarios"
            ]
        );

        let missing = Repo::new(
            "missing-evidence",
            &[("boot-order.toml", &entry)],
            &files[2..],
        );
        let problems = missing.load().unwrap_err();
        assert_eq!(
            problems,
            [
                "boot-order: evidence `contracts/lifecycle/a.jsonl` is not a repository file",
                "boot-order: implementation `crates/cms-boot` is not a workspace crate",
            ]
        );

        for bad in [
            "crates/cms-boot/src/lib.rs::helper",
            "crates/cms-boot/src/lib.rs::absent",
            "crates/cms-boot/src/missing.rs::boots_in_order",
            "../outside.rs::boots_in_order",
            "crates/cms-boot/src/lib.rs::",
            "no-separator",
        ] {
            let text = entry.replace("crates/cms-boot/src/lib.rs::boots_in_order", bad);
            let repo = Repo::new("bad-test", &[("boot-order.toml", &text)], &files);
            assert_eq!(
                repo.load().unwrap_err(),
                [format!(
                    "boot-order: test `{bad}` is not a test function in a repository file"
                )],
                "{bad}"
            );
        }

        for bad in [
            "/etc/passwd",
            "../x",
            "crates/../crates/cms-boot",
            "lab/cms-boot",
        ] {
            let text = entry.replace(
                "implemented_in = [\"crates/cms-boot\"]",
                &format!("implemented_in = [\"{bad}\"]"),
            );
            let repo = Repo::new("bad-crate", &[("boot-order.toml", &text)], &files);
            assert_eq!(
                repo.load().unwrap_err(),
                [format!(
                    "boot-order: implementation `{bad}` is not a workspace crate"
                )],
                "{bad}"
            );
        }
    }

    #[test]
    fn invalid_entries_report_every_problem() {
        let broken = ENTRY
            .replace("schema = 1", "schema = 2")
            .replace("id = \"boot-order\"", "id = \"other\"")
            .replace("title = \"Boot order\"", "title = \" \"")
            .replace("summary = \"What loads when.\"", "summary = \"\"")
            .replace("upstream = [\"boot.php\"]", "upstream = [\"elsewhere.php\"]")
            .replace(
                "hooks = [\"action:boot\"]",
                "hooks = [\"action:boot\", \"action:boot\", \"filter:missing\"]\noptions = [\"absent\"]",
            );
        let repo = Repo::new("invalid", &[("boot-order.toml", &broken)], &[]);
        assert_eq!(
            repo.load().unwrap_err(),
            [
                "boot-order: schema 2 is not supported (expected 1)",
                "boot-order: id `other` does not match the file name",
                "boot-order: the title is empty",
                "boot-order: the summary is empty",
                "boot-order: upstream `elsewhere.php` is not a source file of the inventory",
                "boot-order: hooks `action:boot` is listed twice",
                "boot-order: hooks `filter:missing` is not in the inventory",
                "boot-order: options `absent` is not in the inventory",
            ]
        );

        let no_upstream = ENTRY.replace("upstream = [\"boot.php\"]", "upstream = []");
        let empty = ENTRY
            .replace("functions = [\"load_things\"]\n", "")
            .replace("hooks = [\"action:boot\"]\n", "");
        let repo = Repo::new(
            "more-invalid",
            &[
                ("a-syntax.toml", "schema = "),
                ("b-unknown.toml", &format!("{ENTRY}extra = 1\n")),
                ("boot-order.toml", &no_upstream),
                ("empty.toml", &empty.replace("boot-order", "empty")),
                ("notes.md", "not an entry"),
            ],
            &[],
        );
        let problems = repo.load().unwrap_err();
        assert_eq!(problems.len(), 5, "{problems:?}");
        assert!(problems[0].starts_with("a-syntax: "), "{problems:?}");
        assert!(
            problems[1].starts_with("b-unknown: unknown field `extra`"),
            "{problems:?}"
        );
        assert_eq!(problems[2], "boot-order: no upstream provenance is given");
        assert_eq!(problems[3], "empty: covers no inventory items");
        assert_eq!(
            problems[4],
            "notes: only `.toml` entries belong in contracts/entries"
        );
    }

    #[test]
    fn missing_entry_directories_are_errors() {
        let error = load_entries(
            Path::new("/nonexistent/entries"),
            &inventory(),
            Path::new("/"),
        )
        .unwrap_err();
        assert!(error.to_string().starts_with("reading "), "{error}");
    }

    #[test]
    fn every_upstream_location_kind_counts_as_a_source() {
        let items = Items::new(&inventory());
        assert_eq!(
            items.sources.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "boot.php",
                "config.php",
                "lib/registry.php",
                "lib/widget.php"
            ]
        );
        assert_eq!(
            Section::ALL.map(|section| items.count(section)),
            [1, 1, 2, 1, 1, 1, 1, 1]
        );
    }

    #[test]
    fn render_counts_each_state_independently() {
        let repo = Repo::new(
            "render",
            &[("boot-order.toml", ENTRY)],
            &[("crates/cms-boot/Cargo.toml", "[package]")],
        );
        let mut entries = repo.load().unwrap();
        let mut implemented = entries[0].clone();
        implemented.id = "boot-impl".parse().unwrap();
        implemented.title = "Boot implementation".to_owned();
        implemented.covered =
            BTreeMap::from([(Section::Hooks, BTreeSet::from(["action:boot".to_owned()]))]);
        implemented.implemented = true;
        entries.push(implemented);

        let text = render(&inventory(), &entries);
        assert!(text.starts_with("# Completeness\n"), "{text}");
        assert!(text.contains("(reference platform 7.1.3)"), "{text}");
        assert!(
            text.contains(
                "| functions | 1 | 1 (100.0%) | 0 (0.0%) | 0 (0.0%) | 0 (0.0%) | 0 (0.0%) |"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "| hooks | 2 | 1 (50.0%) | 1 (50.0%) | 0 (0.0%) | 0 (0.0%) | 1 (50.0%) |"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "| **total** | 9 | 2 (22.2%) | 1 (11.1%) | 0 (0.0%) | 0 (0.0%) | 7 (77.8%) |"
            ),
            "{text}"
        );
        assert!(
            text.contains("| `boot-order` | Boot order | 2 | no | no | no |"),
            "{text}"
        );
        assert!(
            text.contains("| `boot-impl` | Boot implementation | 1 | yes | no | no |"),
            "{text}"
        );
        assert_eq!(
            summary(&inventory(), &entries),
            "2 entries; 2 of 9 items documented, 1 implemented, 0 tested, 0 certified"
        );

        let empty = render(&inventory(), &[]);
        assert!(
            empty.ends_with("## Contract entries\n\nNone yet.\n"),
            "{empty}"
        );
        assert_eq!(share(0, 0), "0");
        assert_eq!(share(1, 3), "1 (33.3%)");
        assert_eq!(share(2, 3), "2 (66.7%)");
    }

    #[test]
    fn committed_dashboard_is_current() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let inventory = Inventory::load(&root.join(inventory::INVENTORY_DIR)).unwrap();
        let entries = load_entries(&root.join(ENTRY_DIR), &inventory, &root)
            .unwrap()
            .unwrap();
        let committed = fs::read_to_string(root.join(DASHBOARD_PATH)).unwrap();
        assert_eq!(
            committed,
            render(&inventory, &entries),
            "run `cargo xtask dashboard` and commit the result"
        );
    }
}
